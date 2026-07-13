#![no_main]
use libfuzzer_sys::fuzz_target;
use syslog_generator::load_profile_from_yaml_str;

/// Fuzz profile_parser: парсинг YAML-профиля из произвольных байт.
///
/// Цель: найти panics/undefined-behavior в:
/// - `serde_yaml::from_str` (внешняя зависимость, но мы её используем)
/// - структурах `Profile`, `Phase`, `TargetConfig`
///
/// Если fuzzer найдёт вход, который вызывает panic — он сохранится в
/// `fuzz/corpus/profile_parser/`.
fuzz_target!(|data: &[u8]| {
    // Ограничиваем длину чтобы не тратить ресурсы на мега-входы.
    if data.len() > 64 * 1024 {
        return;
    }
    // Преобразуем байты в UTF-8 (если не получится — пропускаем).
    let Ok(yaml) = std::str::from_utf8(data) else {
        return;
    };
    let _ = load_profile_from_yaml_str(yaml);
});
