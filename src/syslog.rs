//! Сборка синтаксически валидных syslog-сообщений по RFC 5424 и RFC 3164.
//!
//! Источники:
//! - RFC 5424 (The Syslog Protocol): формат HEADER = PRI VERSION SP TIMESTAMP SP
//!   HOSTNAME SP APP-NAME SP PROCID SP MSGID; PRIVAL = facility*8 + severity;
//!   TIMESTAMP — RFC3339 (T и Z в верхнем регистре, TIME-SECFRAC 1..6 цифр);
//!   STRUCTURED-DATA = NILVALUE / 1*SD-ELEMENT; в PARAM-VALUE экранируются `"` `\` `]`;
//!   MSG может начинаться с UTF-8 BOM (%xEF.BB.BF).
//! - RFC 3164 (BSD syslog): `<PRI>Mmm dd hh:mm:ss HOSTNAME TAG: MSG`, день с ведущим
//!   пробелом для 1..9, максимум 1024 октета.

use chrono::{Local, Utc};

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

const NILVALUE: &str = "-";
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

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
fn sanitize_header(value: &str, max: usize) -> String {
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
fn rfc5424_timestamp() -> String {
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

/// Собрать полное сообщение RFC 5424:
/// `<PRI>1 TIMESTAMP HOSTNAME APP-NAME PROCID MSGID SD [BOM]MSG`
pub fn build_rfc5424(h: &Header, msg: &[u8]) -> Vec<u8> {
    let pri = prival(h.facility, h.severity);
    let hostname = sanitize_header(&h.hostname, 255);
    let app_name = sanitize_header(&h.app_name, 48);
    let procid = sanitize_header(&h.procid, 128);
    let msgid = sanitize_header(&h.msgid, 32);
    let sd = if h.structured_data.is_empty() {
        NILVALUE.to_string()
    } else {
        h.structured_data.clone()
    };

    let header = format!(
        "<{}>1 {} {} {} {} {} {}",
        pri,
        rfc5424_timestamp(),
        hostname,
        app_name,
        procid,
        msgid,
        sd,
    );

    let mut out = Vec::with_capacity(header.len() + msg.len() + 4);
    out.extend_from_slice(header.as_bytes());
    out.push(b' ');
    if h.bom {
        out.extend_from_slice(BOM);
    }
    out.extend_from_slice(msg);
    out
}

/// Собрать сообщение RFC 3164 (BSD):
/// `<PRI>Mmm dd hh:mm:ss HOSTNAME TAG: MSG`
/// TIMESTAMP — локальное время; день с ведущим пробелом для 1..9.
/// TAG формируется из app_name (+ [procid], если не NILVALUE).
pub fn build_rfc3164(h: &Header, msg: &[u8]) -> Vec<u8> {
    let pri = prival(h.facility, h.severity);
    let ts = Local::now().format("%b %e %H:%M:%S").to_string();
    let hostname = {
        // RFC3164 HOSTNAME без пробелов; NILVALUE неуместен — берём "localhost".
        let s = sanitize_header(&h.hostname, 255);
        if s == NILVALUE {
            "localhost".to_string()
        } else {
            s
        }
    };
    let app = sanitize_header(&h.app_name, 32);
    let app = if app == NILVALUE {
        "app".to_string()
    } else {
        app
    };
    let tag = if h.procid.is_empty() || h.procid == NILVALUE {
        format!("{}:", app)
    } else {
        let pid = sanitize_header(&h.procid, 128);
        format!("{}[{}]:", app, pid)
    };

    let header = format!("<{}>{} {} {}", pri, ts, hostname, tag);
    let mut out = Vec::with_capacity(header.len() + msg.len() + 2);
    out.extend_from_slice(header.as_bytes());
    out.push(b' ');
    out.extend_from_slice(msg);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prival_examples() {
        // RFC 5424 §6.2.1: facility=0 severity=0 → 0; facility=20 severity=5 → 165.
        assert_eq!(prival(0, 0), 0);
        assert_eq!(prival(20, 5), 165);
        assert_eq!(prival(1, 6), 14); // user.info
                                      // Зажим диапазонов.
        assert_eq!(prival(255, 255), 23 * 8 + 7);
    }

    #[test]
    fn test_escape_sd_value() {
        assert_eq!(escape_sd_value(r#"a"b\c]d"#), r#"a\"b\\c\]d"#);
        assert_eq!(escape_sd_value("plain"), "plain");
    }

    #[test]
    fn test_rfc5424_structure() {
        let h = Header {
            facility: 1,
            severity: 6,
            hostname: "host1".into(),
            app_name: "app1".into(),
            procid: "123".into(),
            msgid: "ID47".into(),
            structured_data: "-".into(),
            bom: false,
        };
        let out = String::from_utf8(build_rfc5424(&h, b"hello")).unwrap();
        assert_eq!(out, "<14>1 ".to_string() + &out[6..]);
        assert!(out.starts_with("<14>1 "));
        assert!(out.ends_with(" host1 app1 123 ID47 - hello"));
    }

    #[test]
    fn test_rfc5424_bom() {
        let h = Header {
            facility: 1,
            severity: 6,
            hostname: "h".into(),
            app_name: "a".into(),
            procid: "-".into(),
            msgid: "-".into(),
            structured_data: "-".into(),
            bom: true,
        };
        let out = build_rfc5424(&h, b"x");
        // BOM должен стоять непосредственно перед MSG.
        assert!(out.ends_with(&[0xEF, 0xBB, 0xBF, b'x']));
    }

    #[test]
    fn test_rfc3164_structure() {
        let h = Header {
            facility: 1,
            severity: 6,
            hostname: "srv".into(),
            app_name: "sshd".into(),
            procid: "42".into(),
            msgid: "-".into(),
            structured_data: "-".into(),
            bom: false,
        };
        let out = String::from_utf8(build_rfc3164(&h, b"login")).unwrap();
        assert!(out.starts_with("<14>"));
        assert!(out.contains(" srv sshd[42]: login"));
    }

    #[test]
    fn test_header_sanitization() {
        // Пробелы заменяются на '_', пустое поле → NILVALUE, обрезка по длине.
        let h = Header {
            facility: 1,
            severity: 6,
            hostname: "has space".into(),
            app_name: "".into(),
            procid: "-".into(),
            msgid: "-".into(),
            structured_data: "-".into(),
            bom: false,
        };
        let out = String::from_utf8(build_rfc5424(&h, b"m")).unwrap();
        assert!(out.contains(" has_space "));
        // app_name пустой → NILVALUE.
        assert!(out.contains(" has_space - - - "));
    }

    /// N8 (v8.6.1): round-trip парсер RFC 5424. Берём вывод `build_rfc5424`,
    /// парсим обратно через `parse_rfc5424_for_test`, проверяем что поля
    /// совпадают с исходными. Парсер намеренно минимальный — для целей
    /// тестирования, не предназначен для общего использования.
    #[test]
    fn rfc5424_round_trip() {
        let header = Header {
            facility: 1,
            severity: 4,
            hostname: "host-a".into(),
            app_name: "app-b".into(),
            procid: "1234".into(),
            msgid: "MSGID-X".into(),
            structured_data: "-".into(),
            bom: false,
        };
        let msg = b"hello world \x80\x81";
        let encoded = build_rfc5424(&header, msg);
        let parsed = parse_rfc5424_for_test(&encoded).expect("должно парситься");

        // PRI: facility*8 + severity = 1*8+4 = 12.
        assert_eq!(parsed.pri, 12);
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.hostname, "host-a");
        assert_eq!(parsed.app_name, "app-b");
        assert_eq!(parsed.procid, "1234");
        assert_eq!(parsed.msgid, "MSGID-X");
        assert_eq!(parsed.structured_data, "-");
        // MSG — последний "поле", после SD. Бинарные байты сохраняются as-is.
        assert_eq!(parsed.msg, b"hello world \x80\x81");
    }

    /// N8: round-trip с NILVALUE-полями и BOM.
    #[test]
    fn rfc5424_round_trip_nilvalues_and_bom() {
        let header = Header {
            facility: 16,
            severity: 6,
            hostname: "-".into(), // NILVALUE
            app_name: "-".into(),
            procid: "-".into(),
            msgid: "-".into(),
            structured_data: "-".into(),
            bom: true,
        };
        let msg = b"with BOM";
        let encoded = build_rfc5424(&header, msg);
        let parsed = parse_rfc5424_for_test(&encoded).expect("должно парситься");
        assert_eq!(parsed.pri, 16 * 8 + 6);
        assert_eq!(parsed.hostname, "-");
        assert_eq!(parsed.app_name, "-");
        assert_eq!(parsed.msg, b"with BOM");
    }

    /// N8: round-trip с непустой structured_data.
    #[test]
    fn rfc5424_round_trip_structured_data() {
        let header = Header {
            facility: 10,
            severity: 4,
            hostname: "fw-1".into(),
            app_name: "firewall".into(),
            procid: "9999".into(),
            msgid: "TRAFFIC".into(),
            structured_data:
                "[example@32473 iut=\"3\" eventSource=\"Application\" eventID=\"1011\"]".into(),
            bom: false,
        };
        let msg = b"connection established";
        let encoded = build_rfc5424(&header, msg);
        let parsed = parse_rfc5424_for_test(&encoded).expect("должно парситься");
        assert_eq!(
            parsed.structured_data,
            "[example@32473 iut=\"3\" eventSource=\"Application\" eventID=\"1011\"]"
        );
        assert_eq!(parsed.msg, b"connection established");
    }
}

/// Распарсенное RFC 5424 сообщение. Используется только в round-trip тестах.
#[cfg(test)]
#[derive(Debug, PartialEq)]
struct Rfc5424Parsed {
    pri: u8,
    version: u8,
    /// TIMESTAMP оставлен как &str из исходного байтов — мы только проверяем
    /// что парсер не паникует, формат timestamp не валидируем (наша реализация
    /// использует системное время в формате RFC3339, но это не часть контракта парсинга).
    _timestamp: String,
    hostname: String,
    app_name: String,
    procid: String,
    msgid: String,
    structured_data: String,
    msg: Vec<u8>,
}

/// Минимальный парсер RFC 5424 для round-trip тестов. Ожидает формат
/// `<PRI>VERSION SP TIMESTAMP SP HOSTNAME SP APP-NAME SP PROCID SP MSGID SP SD SP BOM? MSG`.
/// SP — пробел (ASCII 0x20). TIMESTAMP/HOSTNAME/.../SD могут быть NILVALUE (-).
/// MSG — всё после SD и пробела, опционально с BOM (UTF-8 BOM: EF BB BF).
///
/// Работает с `&[u8]` напрямую (без UTF-8 декодирования), потому что MSG может
/// содержать бинарные данные (например, syslog-сообщения с не-UTF-8 payload).
///
/// Не публичный API — доступен только через `#[cfg(test)]` crate-wide.
#[cfg(test)]
fn parse_rfc5424_for_test(bytes: &[u8]) -> Option<Rfc5424Parsed> {
    // 1. <PRI>: первый байт '<' и найти '>'.
    let bytes = bytes.strip_prefix(b"<")?;
    let gt = bytes.iter().position(|&b| b == b'>')?;
    let pri: u8 = std::str::from_utf8(&bytes[..gt]).ok()?.parse().ok()?;
    let rest = &bytes[gt + 1..];

    // 2. VERSION + 6 HEADER-полей. После — `SD SP BOM? MSG`.
    //
    // Работаем с байтами через splitn(7, b' ') — аналогично Python split(maxsplit=6).
    let mut parts = rest.splitn(7, |&b| b == b' ');
    // parts.next() возвращает &[u8]. До 6 итераций для 6 фиксированных полей;
    // 7-я — остаток `SD SP BOM? MSG`.
    let version_str = std::str::from_utf8(parts.next()?).ok()?;
    let version: u8 = version_str.parse().ok()?;
    if version != 1 {
        return None;
    }
    let _timestamp = std::str::from_utf8(parts.next()?).ok()?.to_string();
    let hostname = std::str::from_utf8(parts.next()?).ok()?.to_string();
    let app_name = std::str::from_utf8(parts.next()?).ok()?.to_string();
    let procid = std::str::from_utf8(parts.next()?).ok()?.to_string();
    let msgid = std::str::from_utf8(parts.next()?).ok()?.to_string();
    // 7-й фрагмент — остаток `SD SP BOM? MSG`.
    let rest = parts.next()?;

    // 3. SD: первый байт rest определяет тип.
    let (structured_data, after_sd) = match rest.first() {
        Some(b'[') => {
            // SD — это всё до первого `]` (без вложенных скобок в нашем простом парсере).
            let after_bracket = &rest[1..];
            let end = after_bracket.iter().position(|&b| b == b']')?;
            // `after_bracket[..end]` — без закрывающего `]`, добавляем его через format.
            let sd = format!("[{}]", std::str::from_utf8(&after_bracket[..end]).ok()?);
            (sd, &after_bracket[end + 1..])
        }
        Some(b'-') => {
            // NILVALUE; после "-" может быть SP + MSG или сразу конец строки.
            ("-".to_string(), &rest[1..])
        }
        _ => return None,
    };

    // 4. После SD и SP — опционально BOM, потом MSG до конца строки.
    let after_sd_sp = after_sd.strip_prefix(b" ")?;
    // BOM — UTF-8: EF BB BF.
    let msg = if after_sd_sp.starts_with(BOM) {
        after_sd_sp[BOM.len()..].to_vec()
    } else {
        after_sd_sp.to_vec()
    };

    Some(Rfc5424Parsed {
        pri,
        version,
        _timestamp,
        hostname,
        app_name,
        procid,
        msgid,
        structured_data,
        msg,
    })
}
