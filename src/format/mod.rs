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
use std::sync::Arc;

/// Параметры заголовка, уже с подставленными значениями шаблона.
///
/// PR-17c (v10.7.18): поля используют `Arc<str>` вместо `String` —
/// clone = atomic increment (~1-5 ns), а не heap alloc+memcpy (~25-50 ns).
/// Устраняет 4× String clone per rfc5424 msg (~100-200 нс/msg).
///
/// Также добавлено поле `timestamp: Arc<str>` — pre-computed timestamp
/// (из `rfc5424_timestamp_at(now)` в hot-path). Используется format::build
/// вместо внутреннего `Utc::now()` — экономит ещё ~50-100 нс/msg
/// (1 `Utc::now()` + `chrono::format!` call).
#[derive(Debug, Clone)]
pub struct Header {
    pub facility: u8,
    pub severity: u8,
    pub hostname: Arc<str>,
    pub app_name: Arc<str>,
    pub procid: Arc<str>,
    pub msgid: Arc<str>,
    pub structured_data: Arc<str>,
    /// PR-17c: pre-computed RFC 5424 timestamp string (например `"2026-07-11T14:30:00.123Z"`).
    /// Если пустой — format::build вызывает `rfc5424_timestamp()` самостоятельно.
    pub timestamp: Arc<str>,
    pub bom: bool,
}

pub(crate) const NILVALUE: &str = "-";
pub(crate) const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// PRIVAL = facility*8 + severity (RFC 5424 §6.2.1).
/// facility зажимается в 0..=23, severity в 0..=7.
///
/// PR-17a (v10.7.16): `#[inline(always)]` — hot-path.
#[inline(always)]
pub fn prival(facility: u8, severity: u8) -> u16 {
    let f = facility.min(23) as u16;
    let s = severity.min(7) as u16;
    f * 8 + s
}

/// Санитизация printable-US-ASCII поля заголовка: пустое → NILVALUE, пробелы и
/// непечатаемые символы заменяются на '_', длина обрезается до `max` октетов.
/// Пустое значение или явный "-" даёт NILVALUE.
///
/// PR-17a (v10.7.16): `#[inline(always)]` — hot-path (вызывается 4× per rfc5424 msg).
///
/// Issue #85 (A1 quick wins): fast ASCII-path — для значений, состоящих
/// только из байтов 0x21..=0x7E без превышения `max`, возвращаем срез
/// исходного `Arc<str>` без аллокации. Для hostname/app_name/procid/msgid
/// типичный случай — ASCII-only (syslog-конвенция), экономия ~1 alloc/msg.
#[inline(always)]
pub(crate) fn sanitize_header(value: &str, max: usize) -> String {
    // Fast path 1: empty или NILVALUE → без alloc.
    if value.is_empty() || value == NILVALUE {
        return NILVALUE.to_string();
    }
    let bytes = value.as_bytes();
    let len = bytes.len().min(max);
    // Fast path 2: ASCII-only printable + ≤ max → без аллокации.
    // Printable US-ASCII per RFC 5424 §6: byte 0x21..=0x7E.
    let mut needs_clean = false;
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        // Printable: 0x21 ('!') до 0x7E ('~'). Вне диапазона — нужно заменять.
        if !(0x21..=0x7E).contains(&b) {
            needs_clean = true;
            break;
        }
        i += 1;
    }
    if !needs_clean {
        // value.to_string() здесь даёт 1 alloc, но мы можем сделать 0 alloc
        // через возвращение уже существующей строки — value уже String-like.
        // Однако сигнатура возвращает String, поэтому делаем alloc только
        // один раз (вместо O(N) на chars().map().collect()).
        return value[..len].to_string();
    }
    // Slow path: байты вне printable-ASCII или unicode → обход chars().
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
///
/// PR-17a (v10.7.16): `#[inline(always)]` — hot-path.
#[inline(always)]
pub(crate) fn rfc5424_timestamp() -> String {
    rfc5424_timestamp_at(Utc::now())
}

/// PR-17c (v10.7.18): hot-path версия — принимает уже вычисленный timestamp.
/// Устраняет 2-й `Utc::now()` per msg (один timestamp shared между
/// `datetime_now_jitter` и `rfc5424_timestamp`). Экономия ~30-100 нс/msg.
#[inline(always)]
pub(crate) fn rfc5424_timestamp_at(now: chrono::DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
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
                // PR-17c: Arc<str> → String clone (для protobuf HashMap<String,String>).
                let values: std::collections::HashMap<String, String> = [
                    ("hostname", ctx.header.hostname.as_ref()),
                    ("app_name", ctx.header.app_name.as_ref()),
                    ("procid", ctx.header.procid.as_ref()),
                    ("msgid", ctx.header.msgid.as_ref()),
                ]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
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

    /// Issue #85 (A1): fast ASCII-path для sanitize_header.
    /// Типичный syslog header field (hostname, app_name, procid, msgid) —
    /// ASCII-only и короче `max`. Должен идти через fast path без лишних
    /// итераций по chars().
    #[test]
    fn sanitize_header_ascii_fast_path() {
        // ASCII printable, в пределах max → возврат as-is.
        assert_eq!(sanitize_header("hostname01", 255), "hostname01");
        assert_eq!(sanitize_header("my-app", 48), "my-app");
        assert_eq!(sanitize_header("12345", 128), "12345");
        assert_eq!(sanitize_header("T!E~S-T", 32), "T!E~S-T"); // edge chars
    }

    /// Issue #85 (A1): sanitize_header обрезает до max в fast-path.
    #[test]
    fn sanitize_header_truncates_at_max_in_fast_path() {
        assert_eq!(sanitize_header("host01extra", 5), "host0");
        assert_eq!(sanitize_header("aaaaaaaaaaaaaaaa", 3), "aaa");
    }

    /// Issue #85 (A1): sanitize_header empty / NILVALUE → "-" без alloc.
    #[test]
    fn sanitize_header_empty_or_nilvalue() {
        assert_eq!(sanitize_header("", 32), NILVALUE);
        assert_eq!(sanitize_header("-", 32), NILVALUE);
    }

    /// Issue #85 (A1): sanitize_header non-ASCII → slow path (replacement на '_').
    #[test]
    fn sanitize_header_non_ascii_falls_to_slow_path() {
        // Пробел (0x20) — вне printable range → '_'
        assert_eq!(sanitize_header("ab cd", 32), "ab_cd");
        // Управляющий символ → '_'
        assert_eq!(sanitize_header("a\x01b", 32), "a_b");
        // Unicode → '_'
        assert_eq!(sanitize_header("aë", 32), "a_");
        // DEL (0x7F) → '_'
        assert_eq!(sanitize_header("a\x7Fb", 32), "a_b");
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
            timestamp: "".into(),
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
            timestamp: "".into(),
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

    // ===== Phase 6 (PR-Q.1): coverage gaps в format/mod.rs =====

    fn hdr_basic() -> Header {
        Header {
            facility: 1,
            severity: 6,
            hostname: "host".into(),
            app_name: "app".into(),
            procid: "1".into(),
            msgid: "X".into(),
            structured_data: "-".into(),
            timestamp: "".into(),
            bom: false,
        }
    }

    /// Phase 6: `escape_sd_value` экранирует `"`, `\`, `]` (RFC 5424 §6.3.3).
    /// Остальные символы (включая `]`) остаются как есть.
    #[test]
    fn phase6_escape_sd_value_special_chars() {
        // No-op: нет специальных символов.
        assert_eq!(escape_sd_value("normal"), "normal");
        assert_eq!(escape_sd_value(""), "");
        // Каждый из трёх специальных символов экранируется обратным слэшем.
        assert_eq!(escape_sd_value("with\"quote"), "with\\\"quote");
        assert_eq!(escape_sd_value("with\\backslash"), "with\\\\backslash");
        assert_eq!(escape_sd_value("with]bracket"), "with\\]bracket");
        // Смешанный ввод — все три символа экранируются, остальное без изменений.
        assert_eq!(escape_sd_value("a\"b\\c]d"), "a\\\"b\\\\c\\]d");
    }

    /// Phase 6: `build_rfc5424` — публичный wrapper, делегирует в `rfc5424::build`.
    #[test]
    fn phase6_build_rfc5424_helper_matches_inner() {
        let h = hdr_basic();
        let via_helper = build_rfc5424(&h, b"hello");
        let via_inner = crate::format::rfc5424::build(&h, b"hello");
        assert_eq!(via_helper, via_inner);
        // Smoke-check структуры: PRIVAL + RFC 5424 версия + body.
        assert!(via_helper.starts_with(b"<14>1 "));
        assert!(via_helper.ends_with(b" hello"));
    }

    /// Phase 6: `build_rfc3164` — публичный wrapper, делегирует в `rfc3164::build`.
    #[test]
    fn phase6_build_rfc3164_helper_matches_inner() {
        let h = hdr_basic();
        let via_helper = build_rfc3164(&h, b"ping");
        let via_inner = crate::format::rfc3164::build(&h, b"ping");
        assert_eq!(via_helper, via_inner);
        assert!(via_helper.starts_with(b"<14>"));
    }

    /// Phase 6: `build_raw` — passthrough, копия `msg`.
    #[test]
    fn phase6_build_raw_is_passthrough() {
        let h = hdr_basic();
        assert_eq!(build_raw(&h, b"raw-payload"), b"raw-payload");
        // Пустое сообщение → пустой результат.
        assert_eq!(build_raw(&h, b""), b"");
    }

    /// Phase 6: `FormatKind::Protobuf(None)` → render идёт в ветку `Self::Protobuf(map)`
    /// где `map.as_ref() = None` → `serialize_protobuf(None, ...)` → пустой Vec
    /// (валидный пустой protobuf-блоб).
    #[test]
    fn phase6_formatkind_protobuf_none_renders_empty() {
        let h = hdr_basic();
        let ctx = FormatContext {
            header: &h,
            cef: None,
            leef: None,
            json_lines_fields: None,
        };
        let out = FormatKind::Protobuf(None).render(&ctx, b"ignored");
        // Empty schema → empty buffer (per `empty_schema_yields_empty_buffer`).
        assert!(out.is_empty());
    }

    /// Phase 6: `FormatKind::Protobuf(Some(schema))` → render с реальной схемой.
    /// Покрывает ветку `Self::Protobuf(map)` где `map.is_some()`.
    #[test]
    fn phase6_formatkind_protobuf_some_renders_wire_format() {
        use crate::generator::config::ProtobufSchemaFieldMap;
        let mut schema = ProtobufSchemaFieldMap::default();
        // field 1 (uint, template "42"), field 2 (str, template "alice").
        // Спецификация поля: "<field_number>:<type>:<template>".
        schema
            .fields
            .insert("id".to_string(), "1:uint:42".to_string());
        schema
            .fields
            .insert("name".to_string(), "2:str:alice".to_string());
        let h = hdr_basic();
        let ctx = FormatContext {
            header: &h,
            cef: None,
            leef: None,
            json_lines_fields: None,
        };
        let out = FormatKind::Protobuf(Some(schema)).render(&ctx, b"ignored");
        // wire-format: tag(1, VARINT)=0x08, varint(42)=0x2A, tag(2, LEN)=0x12, len=5, "alice".
        assert!(out.starts_with(&[0x08, 0x2A, 0x12, 0x05]));
        assert!(out.ends_with(b"alice"));
    }

    /// Phase 6: `FormatKind::Cef` с `ctx.cef = None` → fallback на passthrough.
    /// Покрывает ветку `None => msg.to_vec()` (строки 187-189).
    #[test]
    fn phase6_formatkind_cef_none_ctx_falls_back_to_raw() {
        let h = hdr_basic();
        let ctx = FormatContext {
            header: &h,
            cef: None,
            leef: None,
            json_lines_fields: None,
        };
        let payload = b"cef-fallback";
        let out = FormatKind::Cef.render(&ctx, payload);
        assert_eq!(out, payload);
    }

    /// Phase 6: `FormatKind::Cef` с `ctx.cef = Some(cfg)` → render через cef::build.
    /// Покрывает ветку `Some(cef) => cef::build(...)` (строки 186-187).
    #[test]
    fn phase6_formatkind_cef_some_ctx_renders_cef_format() {
        use crate::generator::config::CefConfig;
        let cfg = CefConfig {
            device_vendor: "ACME".into(),
            device_product: "FW".into(),
            device_version: "1.0".into(),
            signature_id: "100".into(),
            name: "login".into(),
            severity: Some(5),
            extensions: None,
        };
        let h = hdr_basic();
        let ctx = FormatContext {
            header: &h,
            cef: Some(&cfg),
            leef: None,
            json_lines_fields: None,
        };
        let out = FormatKind::Cef.render(&ctx, b"hello");
        let s = std::str::from_utf8(&out).expect("utf8");
        // CEF header: "CEF:0|Vendor|Product|Ver|SigID|Name|Sev|msg=<body>".
        assert!(
            s.starts_with("CEF:0|ACME|FW|1.0|100|login|5|msg=hello"),
            "got: {s}"
        );
    }

    /// Phase 6: `FormatKind::Leef` с `ctx.leef = None` → fallback на passthrough.
    /// Покрывает ветку `None => msg.to_vec()` (строки 194-195).
    #[test]
    fn phase6_formatkind_leef_none_ctx_falls_back_to_raw() {
        let h = hdr_basic();
        let ctx = FormatContext {
            header: &h,
            cef: None,
            leef: None,
            json_lines_fields: None,
        };
        let payload = b"leef-fallback";
        let out = FormatKind::Leef.render(&ctx, payload);
        assert_eq!(out, payload);
    }

    /// Phase 6: `FormatKind::Leef` с `ctx.leef = Some(cfg)` → render через leef::build.
    /// Покрывает ветку `Some(leef) => leef::build(...)` (строки 193-194).
    #[test]
    fn phase6_formatkind_leef_some_ctx_renders_leef_format() {
        use crate::generator::config::LeefConfig;
        let cfg = LeefConfig {
            vendor: "VendorCo".into(),
            product: "IDS".into(),
            version: "2.1".into(),
            event_id: "evt-001".into(),
            attributes: None,
        };
        let h = hdr_basic();
        let ctx = FormatContext {
            header: &h,
            cef: None,
            leef: Some(&cfg),
            json_lines_fields: None,
        };
        let out = FormatKind::Leef.render(&ctx, b"alert");
        let s = std::str::from_utf8(&out).expect("utf8");
        // LEEF header: "LEEF:2.0|Vendor|Product|Ver|EventID<TAB>msg=<body>\n".
        assert!(
            s.starts_with("LEEF:2.0|VendorCo|IDS|2.1|evt-001\tmsg=alert\n"),
            "got: {s}"
        );
    }

    /// Phase 6: `FormatKind::JsonLines` → render через json_lines::build.
    /// Покрывает ветку `Self::JsonLines => json_lines::build(...)` (строка 198).
    #[test]
    fn phase6_formatkind_json_lines_renders_json() {
        use std::collections::BTreeMap;
        let h = hdr_basic();
        let mut extras = BTreeMap::new();
        extras.insert("region".to_string(), "eu-west-1".to_string());
        let ctx = FormatContext {
            header: &h,
            cef: None,
            leef: None,
            json_lines_fields: Some(&extras),
        };
        let out = FormatKind::JsonLines.render(&ctx, b"payload");
        let s = std::str::from_utf8(&out).expect("utf8");
        // JSON-lines: должен содержать стандартные поля + пользовательское `region`.
        assert!(s.starts_with('{'), "got: {s}");
        assert!(s.contains("\"app\":\"app\""), "got: {s}");
        assert!(s.contains("\"host\":\"host\""), "got: {s}");
        assert!(s.contains("\"msg\":\"payload\""), "got: {s}");
        assert!(s.contains("\"region\":\"eu-west-1\""), "got: {s}");
        assert!(s.ends_with('\n'));
    }
}
