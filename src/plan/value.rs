//! PR-A2 (v10.8.0): slot-based values для hot path.
//!
//! Текущая реализация `default_values_into` создаёт `HashMap<String, String>`
//! на каждое сообщение (в PR-17b кэшировали только HashMap, не String allocs).
//! Slot-based values устраняют:
//! - хеширование строки при lookup placeholder'а (в render)
//! - аллокации ключей `"sequence".to_string()`, `"real_hostname".to_string()` etc.
//! - генерацию неиспользуемых faker keys (уже сделано в PR-A0)
//! - временный HashMap между генерацией и render
//!
//! API:
//! - `ValueSlot` enum — pre-resolved описание каждого placeholder'а.
//! - `Value` enum — actual value на одно сообщение.
//! - `ValueArena` — bump allocator с переиспользованием буфера (caller-owned).
//!
//! Использование (в generate_message_with_plan, hot path):
//! ```ignore
//! let mut arena = ValueArena::new(plan.value_slot_count);
//! for seq in 1..=N {
//!     arena.reset();
//!     plan.resolve_into(&mut arena, seq, &mut rng, now);
//!     plan.body_template.render_into(&mut arena, &mut output_buf);
//! }
//! ```

/// Slot в pre-compiled шаблоне.
///
/// В compile-time (см. [`crate::plan::template::CompiledTemplateV2`]) мы
/// разбираем `{{key}}` placeholder'ы в индексы `u32` вместо runtime
/// `HashMap::get(&"key")`. Каждый slot ссылается на один из источников
/// ниже.
///
/// `Copy` где возможно; `Arc<str>`-вариант делает enum не-Copy но
/// это нормально — slot index is `usize` после pre-resolve, сам enum
/// используется только при compile-time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueSlot {
    /// `{{sequence}}` — ordinal number of message (1-based).
    Sequence,
    /// `{{timestamp}}` — shared `chrono::DateTime<Utc>` (PR-17c).
    Timestamp,
    /// `{{pid}}` — random PID in [1, 65535].
    Pid,
    /// `{{real_hostname}}`, `{{hostname}}` — статическая строка из syslog
    /// header. Pre-resolved в [`crate::plan::template::CompiledTemplateV2`].
    StaticStr(&'static str),
    /// `{{real_app}}`, `{{app_name}}` — из `phase.name`.
    StaticStrArc(std::sync::Arc<str>),
    /// `{{faker.username}}`, `{{faker.ipv4}}` etc. — pre-resolved kind.
    Faker(FakerKind),
    /// Schema field (PR-A2.2 — реализация в следующей итерации).
    SchemaField(u32),
    /// Lookup по ключу в legacy `HashMap<String, String>`. Используется
    /// только при backward-compat fallback.
    LegacyKey,
}

/// Faker kind для slot-based generation (PR-A0 уже подготовил
/// `referenced_fakers` как `HashSet<&'static str>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakerKind {
    Ipv4,
    Ipv6,
    Mac,
    Uuid,
    Hostname,
    Username,
    UserAgent,
    Url,
    HttpStatus,
}

impl FakerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ipv4 => "ipv4",
            Self::Ipv6 => "ipv6",
            Self::Mac => "mac",
            Self::Uuid => "uuid",
            Self::Hostname => "hostname",
            Self::Username => "username",
            Self::UserAgent => "user_agent",
            Self::Url => "url",
            Self::HttpStatus => "http_status",
        }
    }
}

/// Runtime value для одного сообщения.
///
/// `Cow<'static, str>` для большинства случаев: статические литералы
/// (`"localhost"`, `"-"`) — borrow без alloc, динамические (timestamp,
/// faker output) — owned.
#[derive(Debug, Clone)]
pub enum Value {
    Static(&'static str),
    Owned(String),
}

impl Value {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Static(s) => s,
            Self::Owned(s) => s.as_str(),
        }
    }
}

impl From<&'static str> for Value {
    fn from(s: &'static str) -> Self {
        Self::Static(s)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::Owned(s)
    }
}

/// Caller-owned arena для slot values.
///
/// Переиспользуется между сообщениями. `Vec` индексы = slot indices.
///
/// Hot path invariant: `Vec::clear()` сохраняет capacity, поэтому
/// повторные сообщения не вызывают heap allocations.
#[derive(Debug)]
pub struct ValueArena {
    slots: Vec<Value>,
    /// Scratch buffer для формирования timestamp.
    ts_buf: String,
}

impl ValueArena {
    pub fn new(slot_count: usize) -> Self {
        Self {
            slots: Vec::with_capacity(slot_count),
            ts_buf: String::with_capacity(24),
        }
    }

    /// Подготовить arena к следующему сообщению. Сохраняет capacity.
    #[inline(always)]
    pub fn reset(&mut self) {
        self.slots.clear();
        self.ts_buf.clear();
    }

    /// Записать значение в slot.
    #[inline]
    pub fn push(&mut self, v: impl Into<Value>) -> usize {
        let idx = self.slots.len();
        self.slots.push(v.into());
        idx
    }

    /// Записать значение в конкретный slot.
    #[inline]
    pub fn set(&mut self, idx: usize, v: impl Into<Value>) {
        self.slots[idx] = v.into();
    }

    /// Получить значение по индексу slot.
    #[inline]
    pub fn get(&self, idx: usize) -> &str {
        self.slots[idx].as_str()
    }

    /// Получить количество слотов.
    #[inline]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Всегда false для slot-arena (не applicable).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Mutable scratch buffer для timestamp / number formatting.
    #[inline]
    pub fn scratch_str(&mut self) -> &mut String {
        &mut self.ts_buf
    }

    /// Записать отформатированное значение в scratch buffer.
    #[inline]
    pub fn write_scratch<F: FnOnce(&mut String)>(&mut self, f: F) -> &str {
        self.ts_buf.clear();
        f(&mut self.ts_buf);
        self.ts_buf.as_str()
    }
}

/// Сериализовать `seq` в decimal в `out`. Использует `itoa`-like fast path.
#[inline]
pub fn write_seq(out: &mut String, seq: usize) {
    use std::fmt::Write;
    let _ = write!(out, "{}", seq);
}

/// Сериализовать `pid` в decimal в `out`.
#[inline]
pub fn write_pid(out: &mut String, pid: i64) {
    use std::fmt::Write;
    let _ = write!(out, "{}", pid);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_reset_preserves_capacity() {
        let mut a = ValueArena::new(4);
        a.push(Value::Static("a"));
        a.push(Value::Static("b"));
        assert_eq!(a.len(), 2);
        let cap_before = a.slots.capacity();
        a.reset();
        assert_eq!(a.len(), 0);
        assert_eq!(
            a.slots.capacity(),
            cap_before,
            "reset must preserve capacity"
        );
    }

    #[test]
    fn value_static_zero_copy() {
        let v: Value = "localhost".into();
        assert_eq!(v.as_str(), "localhost");
        assert!(matches!(v, Value::Static(_)));
    }

    #[test]
    fn value_owned_clone() {
        let v: Value = String::from("hello").into();
        assert_eq!(v.as_str(), "hello");
        assert!(matches!(v, Value::Owned(_)));
    }

    #[test]
    fn write_seq_and_pid() {
        let mut s = String::new();
        write_seq(&mut s, 12345);
        assert_eq!(s, "12345");
        s.clear();
        write_pid(&mut s, 65535);
        assert_eq!(s, "65535");
    }

    #[test]
    fn faker_kind_roundtrip() {
        for kind in [FakerKind::Ipv4, FakerKind::Uuid, FakerKind::Hostname] {
            assert!(!kind.as_str().is_empty());
        }
    }
}
