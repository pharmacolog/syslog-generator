//! N10 (v8.8.0): слой форматов (RFC 5424, RFC 3164, raw, protobuf, cef, leef, json-lines).
//!
//! Это абстракция над типами syslog-сообщений, которые мы умеем генерировать.
//! До N10 вся логика была в одном `src/syslog.rs` (RFC 5424/3164) и
//! `src/protobuf.rs` (wire-format). После рефакторинга:
//!
//! - `mod.rs` — общие утилиты: `Header`, `prival`, `escape_sd_value`,
//!   `BOM`, `NILVALUE`, `sanitize_header` (private), `FormatContext` (F15).
//! - `rfc5424` — `build(&Header, &[u8]) -> Vec<u8>` (RFC 5424 §6.4).
//! - `rfc3164` — `build(&Header, &[u8]) -> Vec<u8>` (RFC 3164).
//! - `raw` — passthrough: `build(&Header, &[u8]) -> Vec<u8>` (копия).
//! - `protobuf` — `apply_protobuf_schema`, `serialize_protobuf`,
//!   `serialize_protobuf_like` (wire-format varint + length-delimited).
//! - `cef` (F15) — ArcSight Common Event Format.
//! - `leef` (F15) — IBM QRadar LEEF.
//! - `json_lines` (F15) — newline-delimited JSON.
//!
//! Старые пути `syslog_generator::build_rfc5424` и
//! `syslog_generator::serialize_protobuf` сохранены как backward-compat
//! re-exports в `src/syslog.rs` и `src/protobuf.rs`.

use crate::generator::config::{CefConfig, LeefConfig};
use chrono::Utc;
use std::fmt;

/// Параметры заголовка, уже с подставленными значениями шаблона.
pub struct Header {
    pub facility: u8,
    pub severity: u8,
    pub hostname: String,
    pub app_name: String,
    pub procid: String,
    pub msgid: String,
    pub structured_data: String,
    pub bom: bool,
}

pub(crate) const NILVALUE: &str = "-";
pub(crate) const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// PRIVAL = facility*8 + severity (RFC 5424 §6.2.1).
/// facility зажимается в 0..=23, severity в 0..=7.
pub fn prival(facility: u8, severity: u8) -> u16 {
    let f = facility.min(23) as u16;
    let s = severity.min(7) as u16;
    f * 8 + s
}

/// Санитизация printable-US-ASCII поля заголовка: пустое → NILVALUE, пробелы и
/// непечатаемые символы заменяются на '_', длина обрезается до `max` октетов.
/// Пустое значение или явный "-" даёт NILVALUE.
pub(crate) fn sanitize_header(value: &str, max: usize) -> String {
    if value.is_empty() || value == NILVALUE {
        return NILVALUE.to_string();
    }
    let cleaned: String = value
        .chars()
        .map(|c| if ('!'..='~').contains(&c) { c } else { '_' })
        .take(max)
        .collect();
    if cleaned.is_empty() {
        NILVALUE.to_string()
    } else {
        cleaned
    }
}

/// TIMESTAMP по RFC 5424: RFC3339, UTC, миллисекунды, суффикс Z.
/// Пример: 2026-07-11T14:30:00.123Z
pub(crate) fn rfc5424_timestamp() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Экранирование PARAM-VALUE для STRUCTURED-DATA (RFC 5424 §6.3.3):
/// символы `"`, `\`, `]` экранируются обратным слэшем.
pub fn escape_sd_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' | '\\' | ']' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

// Подмодули реализации конкретных форматов. Каждый предоставляет функцию
// `build(&Header|Config, &[u8]) -> Vec<u8>` (для cef/leef — своя сигнатура).
pub mod cef;
pub mod json_lines;
pub mod leef;
pub mod protobuf;
pub mod raw;
pub mod rfc3164;
pub mod rfc5424;

/// Контекст рендеринга сообщения (F15, v9.2.0).
///
/// Содержит всё, что может понадобиться разным форматам:
/// - `header` — общий syslog-заголовок (facility/severity/hostname/...)
///   для rfc5424, rfc3164, raw и как источник severity для json_lines.
/// - `cef` / `leef` / `json_lines_fields` — конфигурации соответствующих
///   форматов; `None` для форматов, которым они не нужны.
///
/// Поля сделаны `Option` чтобы не плодить разные trait-методы под каждый
/// формат. Использование единого `FormatContext` даёт static dispatch
/// через enum без vtable lookups и без аллокаций на сообщение.
pub struct FormatContext<'a> {
    pub header: &'a Header,
    pub cef: Option<&'a CefConfig>,
    pub leef: Option<&'a LeefConfig>,
    /// Доп. поля верхнего уровня для json_lines: ключ→значение (значения
    /// могут содержать `{{...}}` для подстановки шаблона).
    pub json_lines_fields: Option<&'a std::collections::BTreeMap<String, String>>,
}

/// Trait `Format` (N10, v9.1.0; расширен F15 в v9.2.0) — абстракция для
/// динамического выбора формата. Реализуется в [`FormatKind`] для
/// static dispatch через enum (вместо `Box<dyn Format>` — экономия
/// heap-аллокаций на горячем пути).
///
/// До v9.2.0 сигнатура была `fn render(&self, &Header, &[u8]) -> Vec<u8>`.
/// В v9.2.0 добавлены CEF/LEEF/JSON-lines форматы — для них нужны
/// дополнительные поля (extensions, vendor/product, ...). Вместо
/// раздувания сигнатуры (или введения отдельных методов) введён
/// `FormatContext` — каждая реализация берёт из него только то, что
/// ей нужно. Existing форматы (rfc5424/rfc3164/raw/protobuf) используют
/// только `ctx.header` — обратная совместимость сохранена.
pub trait Format {
    /// Собрать полное сообщение в данном формате.
    fn render(&self, ctx: &FormatContext<'_>, msg: &[u8]) -> Vec<u8>;
}

/// Конкретный выбор формата для фазы (N10, v9.1.0; расширен F15 в v9.2.0).
///
/// Используется в `run_phase_multi` для dyn-dispatch — `match self { ... }`
/// в `render` компилируется в branchless jumps. Стоимость: 0 heap-аллокаций
/// на сообщение, 0 vtable lookups (в отличие от `Box<dyn Format>`).
///
/// В v9.2.0 добавлены варианты `Cef`, `Leef`, `JsonLines`. Конфигурации
/// этих форматов (`CefConfig`/`LeefConfig`/`json_lines_fields`) живут в
/// `Phase` и передаются через `FormatContext` при вызове `render` —
/// здесь они не нужны (варианты без параметров — данные идут в ctx).
pub enum FormatKind {
    Rfc5424,
    Rfc3164,
    Raw,
    /// Protobuf с опциональной схемой. `None` — пустое сообщение
    /// (валидный пустой protobuf-блоб).
    Protobuf(Option<crate::generator::config::ProtobufSchemaFieldMap>),
    /// F15: ArcSight CEF. Конфигурация — в `Phase::cef`, передаётся через `FormatContext`.
    Cef,
    /// F15: IBM QRadar LEEF. Конфигурация — в `Phase::leef`, передаётся через `FormatContext`.
    Leef,
    /// F15: newline-delimited JSON. Доп. поля — в `Phase::json_lines_fields`,
    /// передаются через `FormatContext`.
    JsonLines,
}

impl Format for FormatKind {
    fn render(&self, ctx: &FormatContext<'_>, msg: &[u8]) -> Vec<u8> {
        match self {
            Self::Rfc5424 => rfc5424::build(ctx.header, msg),
            Self::Rfc3164 => rfc3164::build(ctx.header, msg),
            Self::Raw => msg.to_vec(),
            Self::Protobuf(map) => {
                // Преобразуем текущий `Header` в `HashMap<String, String>` для
                // совместимости с существующим API `serialize_protobuf`.
                let values: std::collections::HashMap<String, String> = [
                    ("hostname", ctx.header.hostname.clone()),
                    ("app_name", ctx.header.app_name.clone()),
                    ("procid", ctx.header.procid.clone()),
                    ("msgid", ctx.header.msgid.clone()),
                ]
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
                protobuf::serialize_protobuf(map.as_ref(), &values)
            }
            Self::Cef => {
                // ctx.cef должен быть Some для FormatKind::Cef (валидируется в F13).
                // Если None — это баг валидатора, fallback на raw.
                match ctx.cef {
                    Some(cef) => cef::build(cef, msg),
                    None => msg.to_vec(),
                }
            }
            Self::Leef => {
                // Аналогично Cef.
                match ctx.leef {
                    Some(leef) => leef::build(leef, msg),
                    None => msg.to_vec(),
                }
            }
            Self::JsonLines => json_lines::build(ctx.header, ctx.json_lines_fields, msg),
        }
    }
}

/// B7 (v10.0.0): `Display` вместо `Format::name() -> &'static str`.
/// Используется для логирования и метрик (`syslog_messages_by_format_total`).
impl fmt::Display for FormatKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Rfc5424 => "rfc5424",
            Self::Rfc3164 => "rfc3164",
            Self::Raw => "raw",
            Self::Protobuf(_) => "protobuf",
            Self::Cef => "cef",
            Self::Leef => "leef",
            Self::JsonLines => "json_lines",
        };
        f.write_str(s)
    }
}

impl FormatKind {
    /// Конвертация из строки имени формата (phase.format) в [`FormatKind`].
    /// Возвращает `None` для неизвестных имён (F13 валидация должна была их отклонить).
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "rfc5424" => Some(Self::Rfc5424),
            "rfc3164" => Some(Self::Rfc3164),
            "raw" => Some(Self::Raw),
            "protobuf" => Some(Self::Protobuf(None)),
            "cef" => Some(Self::Cef),
            "leef" => Some(Self::Leef),
            "json_lines" => Some(Self::JsonLines),
            _ => None,
        }
    }
}

pub mod local_time {
    //! Local time helper for RFC 3164 timestamps.
    pub use chrono::Local;
}

/// Local helper — алиас для тестов и обратной совместимости.
pub fn build_rfc5424(h: &Header, msg: &[u8]) -> Vec<u8> {
    rfc5424::build(h, msg)
}

pub fn build_rfc3164(h: &Header, msg: &[u8]) -> Vec<u8> {
    rfc3164::build(h, msg)
}

pub fn build_raw(_h: &Header, msg: &[u8]) -> Vec<u8> {
    // Raw: сообщение передаётся как есть, без обёртки. Удобно для
    // интеграций где syslog-фрейм уже есть (например, передача через
    // syslog-фронтенд).
    msg.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-export основного приватного helper-а для тестов. Реальные тесты
    /// остались в `src/syslog.rs::tests` для backward-compat.
    #[test]
    fn prival_formula_correct() {
        // 0*8+0 = 0; 23*8+7 = 191; 16*8+6 = 134.
        assert_eq!(prival(0, 0), 0);
        assert_eq!(prival(23, 7), 191);
        assert_eq!(prival(16, 6), 134);
        // Clamping при out-of-range значениях (F13 валидация не пропустит
        // эти значения в проде, но защита в prival остаётся).
        assert_eq!(prival(100, 100), (23u16 * 8) + 7);
    }

    // ===== N10 (v9.1.0): тесты trait Format =====

    /// N10: `FormatKind::Rfc5424` рендерит валидный RFC 5424.
    #[test]
    fn n10_formatkind_rfc5424_renders_valid() {
        let h = Header {
            facility: 1,
            severity: 4,
            hostname: "host".into(),
            app_name: "app".into(),
            procid: "1".into(),
            msgid: "X".into(),
            structured_data: "-".into(),
            bom: false,
        };
        let ctx = FormatContext {
            header: &h,
            cef: None,
            leef: None,
            json_lines_fields: None,
        };
        let out = FormatKind::Rfc5424.render(&ctx, b"hello");
        assert!(out.starts_with(b"<12>1 "));
        assert!(out.ends_with(b" hello"));
        assert_eq!(out[out.len() - 1], b'o');
    }

    /// N10: `FormatKind::Raw` = passthrough (копия msg).
    #[test]
    fn n10_formatkind_raw_is_passthrough() {
        let h = Header {
            facility: 0,
            severity: 0,
            hostname: "h".into(),
            app_name: "a".into(),
            procid: "".into(),
            msgid: "".into(),
            structured_data: "-".into(),
            bom: false,
        };
        let ctx = FormatContext {
            header: &h,
            cef: None,
            leef: None,
            json_lines_fields: None,
        };
        let out = FormatKind::Raw.render(&ctx, b"hello world");
        assert_eq!(out, b"hello world");
    }

    /// N10: имя формата через `name()`.
    #[test]
    fn n10_formatkind_name() {
        assert_eq!(format!("{}", FormatKind::Rfc5424), "rfc5424");
        assert_eq!(format!("{}", FormatKind::Rfc3164), "rfc3164");
        assert_eq!(format!("{}", FormatKind::Raw), "raw");
        assert_eq!(format!("{}", FormatKind::Protobuf(None)), "protobuf");
        assert_eq!(format!("{}", FormatKind::Cef), "cef");
        assert_eq!(format!("{}", FormatKind::Leef), "leef");
        assert_eq!(format!("{}", FormatKind::JsonLines), "json_lines");
    }

    /// N10: `parse("rfc5424")` → `Some(Rfc5424)`, `parse("unknown")` → `None`.
    #[test]
    fn n10_formatkind_parse() {
        assert!(matches!(
            FormatKind::parse("rfc5424"),
            Some(FormatKind::Rfc5424)
        ));
        assert!(matches!(
            FormatKind::parse("rfc3164"),
            Some(FormatKind::Rfc3164)
        ));
        assert!(matches!(FormatKind::parse("raw"), Some(FormatKind::Raw)));
        assert!(matches!(
            FormatKind::parse("protobuf"),
            Some(FormatKind::Protobuf(_))
        ));
        assert!(matches!(FormatKind::parse("cef"), Some(FormatKind::Cef)));
        assert!(matches!(FormatKind::parse("leef"), Some(FormatKind::Leef)));
        assert!(matches!(
            FormatKind::parse("json_lines"),
            Some(FormatKind::JsonLines)
        ));
        assert!(FormatKind::parse("unknown").is_none());
    }
}
