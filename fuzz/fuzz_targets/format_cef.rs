#![no_main]
use libfuzzer_sys::fuzz_target;
use syslog_generator::CefConfig;

/// Fuzz format_cef: рендеринг CEF-сообщений.
///
/// Цель: проверить escaping (|, =, \) на произвольных данных.
fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let device_vendor_len = (data[0] % 32) as usize;
    let device_product_len = (data[1] % 32) as usize;
    let version_len = (data[2] % 16) as usize;
    let name_len = (data[3] % 32) as usize;

    let mut offset = 4;
    if offset + device_vendor_len + device_product_len + version_len + name_len > data.len() {
        return;
    }
    let device_vendor = String::from_utf8_lossy(&data[offset..offset + device_vendor_len]).into_owned();
    offset += device_vendor_len;
    let device_product = String::from_utf8_lossy(&data[offset..offset + device_product_len]).into_owned();
    offset += device_product_len;
    let device_version = String::from_utf8_lossy(&data[offset..offset + version_len]).into_owned();
    offset += version_len;
    let name = String::from_utf8_lossy(&data[offset..offset + name_len]).into_owned();
    offset += name_len;
    let signature_id = "1001".to_string();
    // severity: 0..=10, проверяем границы (clamp).
    let severity: u8 = (data[offset % data.len()] % 11);
    offset += 1;
    let extension = String::from_utf8_lossy(&data[offset.min(data.len())..]).into_owned();

    let cfg = CefConfig {
        device_vendor,
        device_product,
        device_version,
        signature_id,
        name,
        severity: Some(severity),
        extensions: Some(std::collections::BTreeMap::from([("msg".to_string(), extension)])),
    };
    let _ = syslog_generator::format::cef::build(&cfg, b"test payload");
});
