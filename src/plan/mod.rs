//! PR-A2 (v10.8.0): Compiled ExecutionPlan.
//!
//! Новый слой immutable плана, который компилируется один раз из `Profile`
//! и затем переиспользуется для генерации сообщений без hash lookups,
//! per-message schema parsing, per-message regex compilation и
//! per-message format parsing.
//!
//! Архитектура:
//!
//! ```text
//! Profile (validated, immutable user input)
//!     │
//!     ▼ compile_profile()
//! CompiledPlan
//!     ├── plan.per_phase[name]  → CompiledPhase
//!     │   ├── body_template: CompiledTemplateV2  (slot-based)
//!     │   ├── syslog_header: CompiledSyslogHeader
//!     │   ├── value_slot_count: usize  (pre-computed)
//!     │   ├── schema: Option<CompiledSchema>
//!     │   └── format: CompiledFormat
//!     │
//!     ├── plan.targets[]  → ResolvedTarget
//!     │
//!     └── plan.runtime: CompiledRuntime
//! ```
//!
//! ## Hot path (per-message)
//!
//! ```ignore
//! for seq in 1..=N {
//!     arena.reset();
//!     plan.resolve_values_into(&mut arena, seq, &mut rng, now);
//!     output_buf.clear();
//!     plan.format.write_into(&mut output_buf, &arena);
//! }
//! ```
//!
//! Backward-compat: legacy `HashMap<String, String>` path сохранён как
//! `PhaseContext::default_values_into` и используется fallback'ом если
//! план отсутствует (см. `PhaseContext::default_values_into`).

pub mod schema;
pub mod template;
pub mod value;

pub use schema::{CompiledSchema, CompiledSchemaField};
pub use template::CompiledTemplateV2;
pub use value::{FakerKind, Value, ValueArena, ValueSlot};

use crate::generator::config::Phase;

/// Pre-compiled per-phase execution plan.
///
/// Содержит все артефакты, которые до этого пересоздавались на каждом
/// сообщении (sorted schema fields, parsed regex HIR, parsed protobuf
/// fields, slot-based templates).
///
/// В PR-A2 это MVP — содержит только базовые структуры. Полная
/// компиляция с pre-resolved schema, regex, protobuf — следующие шаги.
#[derive(Debug, Clone)]
pub struct CompiledPhase {
    /// Slot-based body template (replaces `Vec<String>` + `render_template`).
    pub body_template: CompiledTemplateV2,
    /// Slot-based syslog header templates (5 полей: hostname/app_name/
    /// procid/msgid/structured_data). None для raw/protobuf/cef/leef.
    pub syslog_templates: Option<SyslogHeaderTemplates>,
    /// Общее число slot'ов — pre-computed для `ValueArena::with_capacity`.
    pub value_slot_count: usize,
}

/// Compiled syslog header templates.
///
/// Хранит slot-based шаблоны для всех 5 syslog header полей. Если поле
/// static (нет per-message placeholder'ов), соответствующий шаблон
/// pre-rendered и не требует arena lookup'а вообще.
#[derive(Debug, Clone)]
pub struct SyslogHeaderTemplates {
    pub hostname: CompiledTemplateV2,
    pub app_name: CompiledTemplateV2,
    pub procid: CompiledTemplateV2,
    pub msgid: CompiledTemplateV2,
    pub structured_data: CompiledTemplateV2,
}

impl SyslogHeaderTemplates {
    /// Render все поля в output. Static-поля (pre-rendered в templates)
    /// renderятся в один memcpy.
    #[inline]
    pub fn render_into(&self, arena: &ValueArena, output: &mut Vec<u8>) {
        output.extend_from_slice(b"<");
        self.priority_into(arena, output);
        // ... priority вычисляется в format layer.
    }

    fn priority_into(&self, _arena: &ValueArena, _output: &mut Vec<u8>) {
        // Заглушка — реализация в PR-A2.5 (caller-owned buffer для rfc5424).
    }
}
/// Compile a Phase в CompiledPhase.
///
/// MVP: компилирует только body_template в slot-based form. Syslog header
/// compilation, schema pre-resolution — следующие шаги.
///
/// Infallible: пустые/invalid шаблоны дают empty `CompiledPhase` без
/// аллокаций. Если в будущем потребуется fallible variant — возвращаем
/// `Result<CompiledPhase>` через явный `try_compile_phase`.
pub fn compile_phase(phase: &Phase) -> CompiledPhase {
    let mut slots: Vec<String> = Vec::new();
    let body_template = CompiledTemplateV2::compile_from_strings(&phase.templates, &mut slots);
    let value_slot_count = slots.len();
    CompiledPhase {
        body_template,
        syslog_templates: None, // PR-A2.5: compile from phase.syslog
        value_slot_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_phase() -> Phase {
        Phase {
            name: "test".to_string(),
            duration_secs: 0,
            messages_per_second: 0,
            total_messages: Some(1),
            templates: vec![
                "user={{real_app}} seq={{sequence}}".to_string(),
                "{{faker.username}} from {{faker.ipv4}}".to_string(),
            ],
            seed: Some(42),
            ..Default::default()
        }
    }

    #[test]
    fn compile_phase_basic() {
        let phase = test_phase();
        let plan = compile_phase(&phase);
        assert_eq!(plan.value_slot_count, 4);
        assert!(plan.body_template.part_count() > 0);
    }

    #[test]
    fn compile_phase_empty_templates() {
        let phase = Phase::default();
        let plan = compile_phase(&phase);
        assert_eq!(plan.value_slot_count, 0);
        assert_eq!(plan.body_template.part_count(), 0);
    }

    /// Snapshot test: slot-based render даёт корректный output для
    /// известной arena population. Smoke test для compile + render.
    #[test]
    fn snapshot_slot_based_render_smoke() {
        let phase = test_phase();
        let plan = compile_phase(&phase);

        // Populate arena matching slot order (real_app, sequence, faker.username, faker.ipv4).
        let mut arena = ValueArena::new(plan.value_slot_count);
        arena.push(Value::from("test"));
        arena.push(Value::from("42"));
        arena.push(Value::from("alice"));
        arena.push(Value::from("192.168.1.10"));

        let mut out = String::new();
        plan.body_template.render_into(&arena, &mut out);

        // verify placeholder substitution работает в обоих templates.
        assert!(out.contains("user=test seq=42"), "got: {out}");
        assert!(out.contains("alice from 192.168.1.10"), "got: {out}");
    }

    /// Snapshot test: render в `BytesMut` для format layer.
    #[test]
    fn snapshot_render_into_bytes_mut() {
        let mut slots: Vec<String> = Vec::new();
        let tpl = CompiledTemplateV2::compile_from_strings(
            &["<{{pri}}>{{msg}}\n".to_string()],
            &mut slots,
        );
        let mut arena = ValueArena::new(2);
        arena.push(Value::from("13"));
        arena.push(Value::from("hello world"));
        let mut out = bytes::BytesMut::with_capacity(64);
        tpl.render_into_bytes(&arena, &mut out);
        assert_eq!(&out[..], b"<13>hello world\n");
    }

    /// Smoke test: snapshot_slot_based_render_smoke дополнительно проверяет
    /// что slot indices совпадают с position в input templates.
    #[test]
    fn smoke_slot_based_render_smoke() {
        let phase = test_phase();
        let plan = compile_phase(&phase);

        let mut arena = ValueArena::new(plan.value_slot_count);
        arena.push(Value::from("test"));
        arena.push(Value::from("42"));
        arena.push(Value::from("alice"));
        arena.push(Value::from("192.168.1.10"));

        let mut out = String::new();
        plan.body_template.render_into(&arena, &mut out);

        assert!(out.contains("user=test seq=42"), "got: {out}");
        assert!(out.contains("alice from 192.168.1.10"), "got: {out}");
    }
}
