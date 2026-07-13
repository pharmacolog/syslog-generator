//! N10 (v8.8.0): реализация RFC 3164 (BSD) в слое format.

use super::{sanitize_header, Header, NILVALUE};
use chrono::Local;

/// Собрать сообщение RFC 3164 (BSD):
/// `<PRI>Mmm dd hh:mm:ssHOSTNAME TAG: MSG`
/// TIMESTAMP — локальное время; день с ведущим пробелом для 1..9.
/// TAG формируется из app_name (+ [procid], если не NILVALUE).
pub fn build(h: &Header, msg: &[u8]) -> Vec<u8> {
    let pri = super::prival(h.facility, h.severity);
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
