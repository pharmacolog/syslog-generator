#![no_main]
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;
use syslog_generator::Header;

/// Fuzz format_json_lines: сериализация в NDJSON.
fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let hostname_len = (data[0] % 32) as usize;
    let app_name_len = (data[1] % 32) as usize;
    let mut offset = 2;
    if offset + hostname_len + app_name_len > data.len() {
        return;
    }
    let hostname = String::from_utf8_lossy(&data[offset..offset + hostname_len]).into_owned();
    offset += hostname_len;
    let app_name = String::from_utf8_lossy(&data[offset..offset + app_name_len]).into_owned();
    offset += app_name_len;

    let header = Header {
        facility: data[offset % data.len()] % 24,
        severity: data[(offset + 1) % data.len()] % 8,
        hostname,
        app_name,
        procid: "proc".to_string(),
        msgid: "ID".to_string(),
        structured_data: "-".to_string(),
        bom: false,
    };

    let mut fields = BTreeMap::new();
    fields.insert("env".to_string(), "test".to_string());
    fields.insert("payload".to_string(), String::from_utf8_lossy(&data[offset.min(data.len())..]).into_owned());

    let _ = syslog_generator::format::json_lines::build(&header, Some(&fields), b"msg");
});
