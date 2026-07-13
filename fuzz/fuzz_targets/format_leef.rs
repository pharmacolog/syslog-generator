#![no_main]
use libfuzzer_sys::fuzz_target;
use syslog_generator::LeefConfig;

/// Fuzz format_leef: рендеринг LEEF v2.0.
fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let vendor_len = (data[0] % 32) as usize;
    let product_len = (data[1] % 32) as usize;
    let version_len = (data[2] % 16) as usize;
    let event_id_len = (data[3] % 32) as usize;

    let mut offset = 4;
    if offset + vendor_len + product_len + version_len + event_id_len > data.len() {
        return;
    }
    let vendor = String::from_utf8_lossy(&data[offset..offset + vendor_len]).into_owned();
    offset += vendor_len;
    let product = String::from_utf8_lossy(&data[offset..offset + product_len]).into_owned();
    offset += product_len;
    let version = String::from_utf8_lossy(&data[offset..offset + version_len]).into_owned();
    offset += version_len;
    let event_id = String::from_utf8_lossy(&data[offset..offset + event_id_len]).into_owned();
    offset += event_id_len;
    let extension = String::from_utf8_lossy(&data[offset.min(data.len())..]).into_owned();

    let cfg = LeefConfig {
        vendor,
        product,
        version,
        event_id,
        attributes: Some(std::collections::BTreeMap::from([("msg".to_string(), extension)])),
    };
    let _ = syslog_generator::format::leef::build(&cfg, b"test payload");
});
