//! N10 (v8.8.0): "raw" формат — passthrough без обёртки.
//!
//! Используется когда syslog-фрейм уже есть (например, передача через
//! syslog-фронтенд) и наш слой format не должен оборачивать сообщение
//! в RFC-фрейм. `Header` параметр принимается для совместимости с trait
//! `Format` (планируется в вехе E), но не используется в этой реализации.

use super::Header;

/// Raw: сообщение передаётся как есть, без обёртки.
pub fn build(_h: &Header, msg: &[u8]) -> Vec<u8> {
    msg.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Header;

    fn test_header() -> Header {
        Header {
            facility: 16,
            severity: 6,
            hostname: "test".into(),
            app_name: "app".into(),
            procid: "1".into(),
            msgid: "ID".into(),
            structured_data: "-".into(),
            timestamp: "".into(),
            bom: false,
        }
    }

    /// Raw format возвращает message as-is (clone через to_vec).
    #[test]
    fn raw_format_passes_through_message_unchanged() {
        let h = test_header();
        let msg = b"hello world\n";
        let out = build(&h, msg);
        assert_eq!(out, msg);
    }

    /// Empty message возвращается как пустой Vec.
    #[test]
    fn raw_format_empty_message() {
        let h = test_header();
        let out = build(&h, b"");
        assert_eq!(out, b"");
    }

    /// Header параметр игнорируется (raw не оборачивает).
    #[test]
    fn raw_format_ignores_header() {
        let mut h1 = test_header();
        h1.facility = 0;
        h1.severity = 0;
        let mut h2 = test_header();
        h2.facility = 23;
        h2.severity = 7;
        let msg = b"test\n";
        // Разные header дают тот же output (raw passthrough).
        assert_eq!(build(&h1, msg), build(&h2, msg));
    }

    /// Binary data (с \0) проходит as-is.
    #[test]
    fn raw_format_binary_data() {
        let h = test_header();
        let msg: &[u8] = &[0u8, 1, 2, 0xff, 0x80, 0x7f, 0x00];
        let out = build(&h, msg);
        assert_eq!(out, msg);
    }
}
