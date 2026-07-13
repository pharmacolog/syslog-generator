#![no_main]
use libfuzzer_sys::fuzz_target;
use syslog_generator::build_rfc5424;

/// Fuzz format_rfc5424: рендеринг syslog-сообщений из произвольных полей.
///
/// Цель: найти panics в `write!`/`format_args!` для pathological inputs.
fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }
    // Извлекаем 4 поля Header из первых 8 байт (по 1 байту на facility,
    // severity, hostname_len, app_name_len — упрощённо).
    let facility = data[0] % 24; // 0..=23
    let severity = data[1] % 8;  // 0..=7
    let hostname_len = (data[2] % 32) as usize;
    let app_name_len = (data[3] % 49) as usize;
    if 4 + hostname_len + app_name_len > data.len() {
        return;
    }
    let hostname = String::from_utf8_lossy(&data[4..4 + hostname_len]).into_owned();
    let app_name = String::from_utf8_lossy(&data[4 + hostname_len..4 + hostname_len + app_name_len])
        .into_owned();
    let procid = "proc".to_string();
    let msgid = "ID".to_string();
    let sd = "-".to_string();
    let bom = false;

    let header = syslog_generator::Header {
        facility,
        severity,
        hostname,
        app_name,
        procid,
        msgid,
        structured_data: sd,
        bom,
    };
    let body = &data[4 + hostname_len + app_name_len..];
    let _ = build_rfc5424(&header, body);
});
