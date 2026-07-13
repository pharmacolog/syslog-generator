//! N10 (v8.8.0): реализация RFC 5424 в слое format.

use super::{sanitize_header, Header, BOM, NILVALUE};

/// Собрать полное сообщение RFC 5424:
/// `<PRI>1 TIMESTAMP HOSTNAME APP-NAME PROCID MSGID SD [BOM]MSG`
pub fn build(h: &Header, msg: &[u8]) -> Vec<u8> {
    let pri = super::prival(h.facility, h.severity);
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
        super::rfc5424_timestamp(),
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
