//! PR-A2 (v10.8.0): slot-based template — render с pre-resolved slot indices.
//!
//! Текущая реализация (`crate::template::CompiledTemplate`) хранит parts
//! как `Vec<TemplatePart>` с `String` keys. При render для каждого
//! `Placeholder(key)` делается `HashMap::get(key)` (хеширование строки,
//! lookup, virtual dispatch).
//!
//! Новая реализация хранит `Vec<Part>` где каждый `Placeholder` ссылается
//! на индекс в `ValueArena` (см. [`crate::plan::value::ValueArena`]).
//! Render — линейный проход по частям без hash lookup, без аллокаций
//! строковых ключей.
//!
//! Backward-compat: legacy `CompiledTemplate` (String-keyed) сохранён как
//! `LegacyCompiledTemplate` и используется в backward-compat path. По
//! завершении миграции будет deprecated.

use crate::plan::value::ValueArena;

/// Pre-compiled template с slot indices.
///
/// Компилируется один раз при `Plan::compile_phase`, затем переиспользуется
/// per-message без heap allocations.
#[derive(Debug, Clone)]
pub struct CompiledTemplateV2 {
    parts: Vec<Part>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Static reserved for PR-A2.5 syslog header pre-rendering.
enum Part {
    /// Литеральный кусок текста — копируется напрямую в output.
    Literal(&'static str),
    /// Static placeholder (например `{{hostname}}` в syslog header).
    /// Value pre-resolved при компиляции, в render просто пушится в arena
    /// и берётся без lookup.
    Static(&'static str),
    /// Dynamic placeholder (sequence, pid, faker, schema).
    /// Render читает значение из `arena[idx]` (slot pre-populated).
    Slot(usize),
}

impl CompiledTemplateV2 {
    /// Empty template.
    pub fn empty() -> Self {
        Self { parts: Vec::new() }
    }

    /// Render template в `out`. Использует `arena` для dynamic slot values.
    ///
    /// Hot path: linear scan parts, push_str для литералов, arena lookup
    /// для slots. 0 hash lookups, 0 String allocs per render (output идёт
    /// в caller-owned `String`/`BytesMut`).
    #[inline]
    pub fn render_into(&self, arena: &ValueArena, out: &mut String) {
        for part in &self.parts {
            match part {
                Part::Literal(s) => out.push_str(s),
                Part::Static(s) => out.push_str(s),
                Part::Slot(idx) => out.push_str(arena.get(*idx)),
            }
        }
    }

    /// Render template в `BytesMut`. Используется в rfc5424 / rfc3164 hot path.
    #[inline]
    pub fn render_into_bytes(&self, arena: &ValueArena, out: &mut bytes::BytesMut) {
        for part in &self.parts {
            match part {
                Part::Literal(s) => out.extend_from_slice(s.as_bytes()),
                Part::Static(s) => out.extend_from_slice(s.as_bytes()),
                Part::Slot(idx) => out.extend_from_slice(arena.get(*idx).as_bytes()),
            }
        }
    }

    /// Compile legacy `Vec<String>` templates в slot-based form.
    /// Slot indices выделяются placeholder'ам в порядке появления.
    ///
    /// Returns compiled template. Caller отвечает за population slot'ов
    /// в arena перед render.
    pub fn compile_from_strings(templates: &[String], arena_slots: &mut Vec<String>) -> Self {
        let mut parts: Vec<Part> = Vec::new();
        for tmpl in templates {
            Self::parse_into(tmpl, arena_slots, &mut parts);
        }
        Self { parts }
    }

    /// Compile один legacy template в slot-based form. `arena_slots`
    /// получает placeholder names в порядке появления; caller затем
    /// заполняет arena в том же порядке.
    fn parse_into(template: &str, arena_slots: &mut Vec<String>, parts: &mut Vec<Part>) {
        let bytes = template.as_bytes();
        let mut i = 0;
        let mut lit_start = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'{' && bytes[i + 1] == b'{' {
                // Flush литерала до placeholder.
                if lit_start < i {
                    // SAFETY: bytes utf-8 — template валидный.
                    let lit = &template[lit_start..i];
                    parts.push(Part::Literal(Box::leak(lit.to_string().into_boxed_str())));
                }
                i += 2;
                let key_start = i;
                while i + 1 < bytes.len() && !(bytes[i] == b'}' && bytes[i + 1] == b'}') {
                    i += 1;
                }
                if i + 1 >= bytes.len() {
                    // No closing `}}` — остаток как литерал.
                    let lit = &template[key_start.saturating_sub(2)..];
                    parts.push(Part::Literal(Box::leak(lit.to_string().into_boxed_str())));
                    break;
                }
                let key = template[key_start..i].to_string();
                i += 2;
                lit_start = i;
                if key.is_empty() {
                    // Пустой placeholder `{{}}` — оставляем как литерал.
                    parts.push(Part::Literal("{{}}"));
                    continue;
                }
                parts.push(Part::Slot(arena_slots.len()));
                arena_slots.push(key);
                continue;
            }
            i += 1;
        }
        if lit_start < bytes.len() {
            let lit = &template[lit_start..];
            parts.push(Part::Literal(Box::leak(lit.to_string().into_boxed_str())));
        }
    }

    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    pub fn slot_count(&self) -> usize {
        self.parts
            .iter()
            .filter(|p| matches!(p, Part::Slot(_)))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::value::Value;

    #[test]
    fn compile_empty_template() {
        let mut slots = Vec::new();
        let tpl = CompiledTemplateV2::compile_from_strings(&["".to_string()], &mut slots);
        assert_eq!(tpl.part_count(), 0);
        assert_eq!(tpl.slot_count(), 0);
    }

    #[test]
    fn compile_literal_only() {
        let mut slots = Vec::new();
        let tpl =
            CompiledTemplateV2::compile_from_strings(&["hello world".to_string()], &mut slots);
        assert_eq!(tpl.part_count(), 1);
        assert_eq!(tpl.slot_count(), 0);

        let arena = ValueArena::new(0);
        let mut out = String::new();
        tpl.render_into(&arena, &mut out);
        assert_eq!(out, "hello world");
    }

    #[test]
    fn compile_with_placeholders() {
        let mut slots = Vec::new();
        let tpl = CompiledTemplateV2::compile_from_strings(
            &["seq={{sequence}} app={{real_app}}".to_string()],
            &mut slots,
        );
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0], "sequence");
        assert_eq!(slots[1], "real_app");

        let mut arena = ValueArena::new(2);
        arena.push(Value::from("42"));
        arena.push(Value::from("authsvc"));
        let mut out = String::new();
        tpl.render_into(&arena, &mut out);
        assert_eq!(out, "seq=42 app=authsvc");
    }

    #[test]
    fn render_into_bytes() {
        let mut slots = Vec::new();
        let tpl =
            CompiledTemplateV2::compile_from_strings(&["<{{pri}}>{{msg}}".to_string()], &mut slots);
        let mut arena = ValueArena::new(2);
        arena.push(Value::from("13"));
        arena.push(Value::from("hello"));
        let mut out = bytes::BytesMut::with_capacity(32);
        tpl.render_into_bytes(&arena, &mut out);
        assert_eq!(&out[..], b"<13>hello");
    }

    #[test]
    fn unclosed_placeholder_falls_back_to_literal() {
        let mut slots = Vec::new();
        let tpl = CompiledTemplateV2::compile_from_strings(
            &["hello {{unfinished".to_string()],
            &mut slots,
        );
        assert_eq!(slots.len(), 0, "unclosed placeholder must not be a slot");
        assert!(tpl.part_count() >= 1);
    }

    #[test]
    fn empty_placeholder_falls_back_to_literal() {
        let mut slots = Vec::new();
        let tpl =
            CompiledTemplateV2::compile_from_strings(&["hello {{}} world".to_string()], &mut slots);
        assert_eq!(slots.len(), 0);
        assert!(tpl.part_count() >= 1);
    }
}
