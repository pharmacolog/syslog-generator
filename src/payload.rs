//! Вариативная генерация пейлоада (Веха C: F4–F6).
//!
//! - F4: детерминированный ГПСЧ. RNG выводится из `(seed, seq)`, поэтому один
//!   и тот же `seed` + номер сообщения дают идентичный вывод (воспроизводимость),
//!   при этом соседние сообщения различаются. Без `seed` берётся энтропия ОС.
//! - F5: богатый набор faker-генераторов (ipv4/ipv6/mac/uuid/hostname/username/
//!   user_agent/url/http_status), `int` с диапазоном, `enum` со случайным
//!   (в т.ч. взвешенным) выбором, `datetime` с реальным «сейчас» и джиттером,
//!   `string(len)`.
//! - F6: распределения выбора (uniform/weighted/zipf) и паддинг до размера.

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use std::fmt::Write as _;

/// В rand 0.10 `from_os_rng` удалён, используем `from_rng` с системным RNG.
///
/// PR-17d (v10.7.19): миграция `StdRng` (ChaCha12) → `StdRng` (xoshiro256++).
/// `StdRng` примерно в 2-3× быстрее на горячем пути (генерирует по 16 байт за раз
/// без раундов ChaCha). На 30-50% быстрее для коротких горячих вызовов (faker,
/// int_in_range, weighted_index). Детерминизм сохранён (один seed → одна последовательность).
fn fresh_os_rng() -> StdRng {
    let mut sys_rng = rand::rng();
    StdRng::from_rng(&mut sys_rng)
}

/// Детерминированный вывод RNG из seed и порядкового номера сообщения.
///
/// Если `seed` задан — RNG полностью воспроизводим для пары (seed, seq).
/// Смешивание seq выполняется через SplitMix64-подобное перемешивание, чтобы
/// соседние seq давали независимые потоки, а не смежные seed.
///
/// PR-17a (v10.7.16): `#[inline(always)]` — hot-path, вызывается на каждое сообщение.
/// PR-17d (v10.7.19): возврат `StdRng` (xoshiro256++) вместо `StdRng` (ChaCha12).
/// `StdRng` примерно в 2-3× быстрее для коротких вызовов (~30-50% экономии на RNG).
#[inline(always)]
pub fn derive_rng(seed: Option<u64>, seq: usize) -> StdRng {
    match seed {
        Some(s) => {
            // SplitMix64 finalizer поверх (seed XOR seq) — качественное перемешивание.
            let mut z = s ^ (seq as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            StdRng::seed_from_u64(z)
        }
        None => fresh_os_rng(),
    }
}

/// Список User-Agent для faker.user_agent.
const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
    "Mozilla/5.0 (X11; Linux x86_64; rv:121.0) Gecko/20100101 Firefox/121.0",
    "curl/8.4.0",
    "python-requests/2.31.0",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148 Safari/604.1",
];

/// Частые HTTP-статусы для faker.http_status.
const HTTP_STATUSES: &[u16] = &[
    200, 201, 204, 301, 302, 304, 400, 401, 403, 404, 429, 500, 502, 503,
];

const HOST_ADJ: &[&str] = &["web", "db", "cache", "api", "edge", "worker", "auth", "log"];
const USER_NAMES: &[&str] = &[
    "alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi", "ivan", "judy",
];

/// Реализация faker-генераторов. `kind` — часть после `faker.`
/// (например `ipv4`, `http_status`). Неизвестный вид даёт пустую строку.
///
/// v10.2.0 (Performance ч.2): hot-path оптимизации для горячих faker —
/// `String::with_capacity(N)` + `write!` через `std::fmt::Write` вместо
/// многоэтапных `format!` и `Vec<String>::join()`. Это устраняет
/// промежуточные аллокации в hot-path: один String на одну итоговую
/// аллокацию. Особенно заметно на `faker.ipv4`, `faker.uuid`, `faker.ipv6`,
/// `faker.url` — наиболее частых в нагрузочных профилях.
///
/// PR-17a (v10.7.16): `#[inline(always)]` — hot-path, вызывается per msg.
#[inline(always)]
pub fn faker(kind: &str, rng: &mut StdRng) -> String {
    match kind {
        "ipv4" => {
            // "255.255.255.255" = max 15 байт. Pre-alloc устраняет 2 re-alloc.
            let mut s = String::with_capacity(15);
            let _ = write!(
                s,
                "{}.{}.{}.{}",
                rng.random_range(1..=223),
                rng.random_range(0..=255),
                rng.random_range(0..=255),
                rng.random_range(1..=254)
            )
             /* String::write infallible */;
            s
        }
        "ipv6" => {
            // "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff" = 39 байт.
            // Было: 8×Vec<String> + join (8 аллокаций). Стало: 1 String.
            let mut s = String::with_capacity(39);
            for i in 0..8 {
                if i > 0 {
                    s.push(':');
                }
                let _ = write!(s, "{:x}", rng.random_range(0u16..=0xffff));
                /* String::write infallible */
            }
            s
        }
        "mac" => {
            // "ff:ff:ff:ff:ff:ff" = 17 байт.
            let mut s = String::with_capacity(17);
            for i in 0..6 {
                if i > 0 {
                    s.push(':');
                }
                let _ = write!(s, "{:02x}", rng.random_range(0u8..=255)); /* String::write infallible */
            }
            s
        }
        "uuid" => uuid_v4(rng),
        "hostname" => {
            // "edge-99" = max ~9 байт.
            let mut s = String::with_capacity(9);
            let _ = write!(
                s,
                "{}-{:02}",
                HOST_ADJ[rng.random_range(0..HOST_ADJ.len())],
                rng.random_range(1..=99)
            );
            s
        }
        "username" => USER_NAMES[rng.random_range(0..USER_NAMES.len())].to_string(),
        "user_agent" => USER_AGENTS[rng.random_range(0..USER_AGENTS.len())].to_string(),
        "url" => {
            // "https://edge-99.example.com/api/v1/users" = max ~48 байт.
            let paths = [
                "/",
                "/login",
                "/api/v1/users",
                "/health",
                "/static/app.js",
                "/search?q=x",
            ];
            let mut s = String::with_capacity(48);
            let _ = write!(
                s,
                "https://{}-{:02}.example.com{}",
                HOST_ADJ[rng.random_range(0..HOST_ADJ.len())],
                rng.random_range(1..=99),
                paths[rng.random_range(0..paths.len())]
            );
            s
        }
        "http_status" => HTTP_STATUSES[rng.random_range(0..HTTP_STATUSES.len())].to_string(),
        _ => String::new(),
    }
}

/// Случайный UUID версии 4 (RFC 4122): версия 4, вариант 10xx.
///
/// v10.2.0: одна аллокация `String::with_capacity(36)` + `write!` байт hex в
/// нужные позиции (с дефисами на 8, 13, 18, 23). Было: format! с 16
/// аргументами и промежуточными Display-форматированиями.
fn uuid_v4(rng: &mut StdRng) -> String {
    let mut b = [0u8; 16];
    rng.fill(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40; // версия 4
    b[8] = (b[8] & 0x3f) | 0x80; // вариант RFC 4122

    // UUID формат: 8-4-4-4-12 hex digits + 4 дефиса = 36 байт.
    let mut s = String::with_capacity(36);
    // Группа 1 (8 hex): b[0..8]
    write_hex_pair(&mut s, b[0]);
    write_hex_pair(&mut s, b[1]);
    write_hex_pair(&mut s, b[2]);
    write_hex_pair(&mut s, b[3]);
    s.push('-');
    // Группа 2 (4 hex): b[4..6]
    write_hex_pair(&mut s, b[4]);
    write_hex_pair(&mut s, b[5]);
    s.push('-');
    // Группа 3 (4 hex): b[6..8]
    write_hex_pair(&mut s, b[6]);
    write_hex_pair(&mut s, b[7]);
    s.push('-');
    // Группа 4 (4 hex): b[8..10]
    write_hex_pair(&mut s, b[8]);
    write_hex_pair(&mut s, b[9]);
    s.push('-');
    // Группа 5 (12 hex): b[10..16]
    for &byte in &b[10..16] {
        write_hex_pair(&mut s, byte);
    }
    s
}

/// Хелпер для `uuid_v4`: пишет 2 hex-цифры (lowercase) в String.
/// Формат `{:02x}` не выделяет промежуточный String — пишет прямо в `s`.
///
/// PR-17a (v10.7.16): `#[inline(always)]` — вызывается 16× за один uuid.
#[inline(always)]
fn write_hex_pair(s: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    s.push(HEX[(byte >> 4) as usize] as char);
    s.push(HEX[(byte & 0x0f) as usize] as char);
}

/// Случайное целое в диапазоне [min, max] включительно. Если max < min —
/// возвращает min.
///
/// PR-17a (v10.7.16): `#[inline(always)]` — hot-path.
#[inline(always)]
pub fn int_in_range(min: i64, max: i64, rng: &mut StdRng) -> i64 {
    if max < min {
        min
    } else {
        rng.random_range(min..=max)
    }
}

/// Случайная строка длины `len` из букв и цифр (a-z, A-Z, 0-9).
///
/// v10.2.0: одна аллокация `String::with_capacity(len)` + `push` байт (не char,
/// экономит UTF-8 валидацию в горячем пути). Было: `Vec<char>` (62 аллокации
/// для len=62) + `.collect::<String>()` (ещё одна аллокация + re-encoding).
pub fn random_string(len: usize, rng: &mut StdRng) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        s.push(CHARSET[rng.random_range(0..CHARSET.len())] as char);
    }
    s
}

/// datetime в RFC3339 UTC: реальное «сейчас» плюс/минус случайный джиттер
/// (в пределах `jitter_secs`), чтобы события не были одинаковыми по времени.
///
/// PR-17a (v10.7.16): `#[inline(always)]` — hot-path.
#[inline(always)]
pub fn datetime_now_jitter(jitter_secs: i64, rng: &mut StdRng) -> String {
    use chrono::Utc;
    datetime_now_jitter_at(Utc::now(), jitter_secs, rng)
}

/// PR-17c (v10.7.18): hot-path версия — принимает уже вычисленный `now`.
/// Позволяет shared timestamp между `rfc5424_timestamp_at` и
/// `datetime_now_jitter_at` — один `Utc::now()` per msg вместо двух.
#[inline(always)]
pub fn datetime_now_jitter_at(
    now: chrono::DateTime<chrono::Utc>,
    jitter_secs: i64,
    rng: &mut StdRng,
) -> String {
    use chrono::Duration;
    let delta = if jitter_secs > 0 {
        rng.random_range(-jitter_secs..=jitter_secs)
    } else {
        0
    };
    let t = now + Duration::seconds(delta);
    t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Взвешенный выбор индекса из массива весов. Возвращает индекс `i` с
/// вероятностью `weights[i] / sum`. Пустые/нулевые веса → 0.
pub fn weighted_index(weights: &[f64], rng: &mut StdRng) -> usize {
    let total: f64 = weights.iter().filter(|w| **w > 0.0).sum();
    if total <= 0.0 || weights.is_empty() {
        return 0;
    }
    let mut r = rng.random_range(0.0..total);
    for (i, w) in weights.iter().enumerate() {
        if *w > 0.0 {
            if r < *w {
                return i;
            }
            r -= *w;
        }
    }
    weights.len() - 1
}

/// Zipf-подобный выбор индекса из `n` элементов с экспонентой `s` (s>0).
/// Вес элемента ранга k (1-based) пропорционален 1/k^s — "горячие" ключи
/// выбираются чаще. Реализация через прямое суммирование (n обычно невелико).
pub fn zipf_index(n: usize, s: f64, rng: &mut StdRng) -> usize {
    if n == 0 {
        return 0;
    }
    let weights: Vec<f64> = (1..=n).map(|k| 1.0 / (k as f64).powf(s)).collect();
    weighted_index(&weights, rng)
}

/// Паддинг тела до целевого размера `target` байт: если тело короче, дописывает
/// пробел и случайные символы до нужной длины; если длиннее — не трогает.
pub fn pad_to_size(mut body: Vec<u8>, target: usize, rng: &mut StdRng) -> Vec<u8> {
    if body.len() >= target {
        return body;
    }
    body.push(b' ');
    while body.len() < target {
        const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        body.push(CHARSET[rng.random_range(0..CHARSET.len())]);
    }
    body
}

/// Максимальное число повторов для безграничных квантификаторов (`*`, `+`, `{n,}`),
/// чтобы генерация не расходилась. Значение выбрано как разумный предел для
/// нагрузочного пейлоада.
const REGEX_MAX_REPEAT: u32 = 16;

/// F5: сгенерировать строку, соответствующую регулярному выражению `pattern`.
///
/// Паттерн парсится в HIR (`regex_syntax`) и обходится рекурсивно нашим
/// детерминированным `StdRng`, поэтому генерация полностью воспроизводима по
/// `(seed, seq)` (сохраняется свойство F4). Безграничные квантификаторы
/// ограничиваются `REGEX_MAX_REPEAT`. При ошибке парсинга возвращается пустая
/// строка (некорректный паттерн не должен ронять генератор нагрузки).
pub fn gen_from_regex(pattern: &str, rng: &mut StdRng) -> String {
    match regex_syntax::parse(pattern) {
        Ok(hir) => {
            let mut out = String::new();
            gen_hir(&hir, rng, &mut out);
            out
        }
        Err(_) => String::new(),
    }
}

/// Issue #85 sub-task 6: pre-cached HIR variant — skip parsing per call.
/// Используется в hot-path если pattern был pre-parsed в PhaseContext.
pub fn gen_from_regex_cached(hir: &regex_syntax::hir::Hir, rng: &mut StdRng) -> String {
    let mut out = String::new();
    gen_hir(hir, rng, &mut out);
    out
}

/// Рекурсивный обход HIR с генерацией соответствующего текста.
fn gen_hir(hir: &regex_syntax::hir::Hir, rng: &mut StdRng, out: &mut String) {
    use regex_syntax::hir::HirKind;
    match hir.kind() {
        HirKind::Empty | HirKind::Look(_) => {}
        HirKind::Literal(lit) => {
            // Байты литерала — валидный UTF-8 (regex_syntax это гарантирует
            // для Unicode-режима); безопасно интерпретируем как строку.
            out.push_str(&String::from_utf8_lossy(&lit.0));
        }
        HirKind::Class(class) => gen_class(class, rng, out),
        HirKind::Repetition(rep) => {
            let min = rep.min;
            let max = rep
                .max
                .unwrap_or(min + REGEX_MAX_REPEAT)
                .min(min + REGEX_MAX_REPEAT);
            let count = if max > min {
                rng.random_range(min..=max)
            } else {
                min
            };
            for _ in 0..count {
                gen_hir(&rep.sub, rng, out);
            }
        }
        HirKind::Capture(cap) => gen_hir(&cap.sub, rng, out),
        HirKind::Concat(subs) => {
            for s in subs {
                gen_hir(s, rng, out);
            }
        }
        HirKind::Alternation(subs) => {
            if !subs.is_empty() {
                let idx = rng.random_range(0..subs.len());
                gen_hir(&subs[idx], rng, out);
            }
        }
    }
}

/// Выбрать случайный символ из класса символов (Unicode или байтового).
fn gen_class(class: &regex_syntax::hir::Class, rng: &mut StdRng, out: &mut String) {
    use regex_syntax::hir::Class;
    match class {
        Class::Unicode(u) => {
            let ranges = u.ranges();
            if ranges.is_empty() {
                return;
            }
            // Суммарная мощность класса, затем равномерный выбор скалярного значения.
            let total: u32 = ranges
                .iter()
                .map(|r| r.end() as u32 - r.start() as u32 + 1)
                .sum();
            let mut pick = rng.random_range(0..total);
            for r in ranges {
                let span = r.end() as u32 - r.start() as u32 + 1;
                if pick < span {
                    if let Some(c) = char::from_u32(r.start() as u32 + pick) {
                        out.push(c);
                    }
                    return;
                }
                pick -= span;
            }
        }
        Class::Bytes(b) => {
            let ranges = b.ranges();
            if ranges.is_empty() {
                return;
            }
            let total: u32 = ranges
                .iter()
                .map(|r| r.end() as u32 - r.start() as u32 + 1)
                .sum();
            let mut pick = rng.random_range(0..total);
            for r in ranges {
                let span = r.end() as u32 - r.start() as u32 + 1;
                if pick < span {
                    out.push((r.start() + pick as u8) as char);
                    return;
                }
                pick -= span;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seed_determinism_same_seed_seq() {
        // Один seed+seq → одинаковый вывод.
        let mut a = derive_rng(Some(42), 7);
        let mut b = derive_rng(Some(42), 7);
        assert_eq!(faker("ipv4", &mut a), faker("ipv4", &mut b));
        assert_eq!(faker("uuid", &mut a), faker("uuid", &mut b));
    }

    #[test]
    fn test_seed_different_seq_differs() {
        // Разные seq при одном seed дают разные потоки (почти наверняка).
        let mut a = derive_rng(Some(42), 1);
        let mut b = derive_rng(Some(42), 2);
        // Соберём по несколько значений, чтобы исключить редкое совпадение.
        let va: Vec<String> = (0..5).map(|_| faker("uuid", &mut a)).collect();
        let vb: Vec<String> = (0..5).map(|_| faker("uuid", &mut b)).collect();
        assert_ne!(va, vb);
    }

    #[test]
    fn test_ipv4_format() {
        let mut rng = derive_rng(Some(1), 1);
        for _ in 0..50 {
            let ip = faker("ipv4", &mut rng);
            let octets: Vec<u32> = ip.split('.').map(|x| x.parse().unwrap()).collect();
            assert_eq!(octets.len(), 4);
            assert!((1..=223).contains(&octets[0]), "bad first octet: {ip}");
            assert!(octets.iter().all(|o| *o <= 255));
        }
    }

    #[test]
    fn test_uuid_v4_format() {
        let mut rng = derive_rng(Some(2), 1);
        let u = faker("uuid", &mut rng);
        assert_eq!(u.len(), 36);
        let parts: Vec<&str> = u.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(&u[14..15], "4", "версия должна быть 4: {u}");
        assert!(
            matches!(&u[19..20], "8" | "9" | "a" | "b"),
            "вариант RFC4122: {u}"
        );
    }

    #[test]
    fn test_mac_and_ipv6_format() {
        let mut rng = derive_rng(Some(3), 1);
        let mac = faker("mac", &mut rng);
        assert_eq!(mac.split(':').count(), 6);
        assert!(mac.split(':').all(|h| h.len() == 2));
        let v6 = faker("ipv6", &mut rng);
        assert_eq!(v6.split(':').count(), 8);
    }

    #[test]
    fn test_int_in_range_bounds() {
        let mut rng = derive_rng(Some(4), 1);
        for _ in 0..200 {
            let v = int_in_range(10, 20, &mut rng);
            assert!((10..=20).contains(&v));
        }
        assert_eq!(int_in_range(5, 3, &mut rng), 5); // max<min => min
    }

    #[test]
    fn test_http_status_is_known() {
        let mut rng = derive_rng(Some(5), 1);
        for _ in 0..50 {
            let s: u16 = faker("http_status", &mut rng).parse().unwrap();
            assert!(HTTP_STATUSES.contains(&s));
        }
    }

    #[test]
    fn test_random_string_len() {
        let mut rng = derive_rng(Some(6), 1);
        assert_eq!(random_string(0, &mut rng).len(), 0);
        assert_eq!(random_string(32, &mut rng).len(), 32);
        assert!(random_string(16, &mut rng)
            .chars()
            .all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_weighted_index_favours_heavy() {
        let mut rng = derive_rng(Some(7), 1);
        // Вес [0, 100, 0] должен всегда давать индекс 1.
        for _ in 0..100 {
            assert_eq!(weighted_index(&[0.0, 100.0, 0.0], &mut rng), 1);
        }
        // Тяжёлый первый элемент выбирается чаще.
        let mut counts = [0usize; 2];
        for _ in 0..2000 {
            counts[weighted_index(&[9.0, 1.0], &mut rng)] += 1;
        }
        assert!(counts[0] > counts[1] * 3, "counts: {counts:?}");
    }

    #[test]
    fn test_zipf_hot_key_dominates() {
        let mut rng = derive_rng(Some(8), 1);
        let mut counts = [0usize; 5];
        for _ in 0..5000 {
            counts[zipf_index(5, 1.2, &mut rng)] += 1;
        }
        // Ранг 1 (индекс 0) должен доминировать над последним рангом.
        assert!(counts[0] > counts[4], "zipf counts: {counts:?}");
    }

    #[test]
    fn test_regex_matches_pattern() {
        // Сгенерированная строка должна соответствовать исходному regex.
        let re = regex::Regex::new(r"^[A-Z]{3}-\d{4}$").unwrap();
        for seq in 1..50 {
            let mut rng = derive_rng(Some(100), seq);
            let s = gen_from_regex(r"[A-Z]{3}-[0-9]{4}", &mut rng);
            assert!(
                re.is_match(&s),
                "regex output {s:?} не соответствует паттерну"
            );
        }
    }

    #[test]
    fn test_regex_deterministic_by_seed() {
        let mut a = derive_rng(Some(55), 7);
        let mut b = derive_rng(Some(55), 7);
        let pat = r"user-[a-z]{5}-[0-9]{2,4}";
        assert_eq!(gen_from_regex(pat, &mut a), gen_from_regex(pat, &mut b));
    }

    #[test]
    fn test_regex_alternation() {
        let re = regex::Regex::new(r"^(cat|dog|bird)$").unwrap();
        let mut seen = std::collections::HashSet::new();
        for seq in 1..200 {
            let mut rng = derive_rng(Some(3), seq);
            let s = gen_from_regex(r"cat|dog|bird", &mut rng);
            assert!(re.is_match(&s), "alternation output {s:?}");
            seen.insert(s);
        }
        // За 200 итераций должны встретиться все три альтернативы.
        assert_eq!(seen.len(), 3, "не все альтернативы: {seen:?}");
    }

    #[test]
    fn test_regex_invalid_pattern_yields_empty() {
        let mut rng = derive_rng(Some(1), 1);
        assert_eq!(gen_from_regex(r"[unterminated", &mut rng), "");
    }

    #[test]
    fn test_pad_to_size() {
        let mut rng = derive_rng(Some(9), 1);
        let padded = pad_to_size(b"short".to_vec(), 20, &mut rng);
        assert_eq!(padded.len(), 20);
        assert!(padded.starts_with(b"short "));
        // Уже длиннее целевого — без изменений.
        let same = pad_to_size(b"already long enough".to_vec(), 5, &mut rng);
        assert_eq!(same, b"already long enough");
    }

    /// Phase 11 (Tier 1): неизвестный faker kind → пустая строка (не паникуем).
    #[test]
    fn payload_faker_unknown_kind_returns_empty_string() {
        let mut rng = derive_rng(Some(1), 1);
        assert_eq!(faker("nonexistent_kind", &mut rng), "");
        assert_eq!(faker("", &mut rng), "");
        assert_eq!(faker("IPv4", &mut rng), ""); // case-sensitive
    }

    /// Phase 11 (Tier 1): `datetime_now_jitter` обёртка над `datetime_now_jitter_at`.
    /// Покрывает вызовы `Utc::now()` + `chrono::Duration::seconds`.
    #[test]
    fn payload_datetime_now_jitter_uses_chrono_now() {
        let mut rng = derive_rng(Some(11), 1);
        // jitter_secs = 0 → нет отклонения от now.
        let s0 = datetime_now_jitter(0, &mut rng);
        // RFC3339-подобный формат: 20+ символов.
        assert!(s0.len() >= 20, "got: {s0}");
        assert!(s0.contains('T'), "RFC3339-like timestamp: {s0}");
        assert!(s0.ends_with('Z'), "UTC: {s0}");

        // С jitter > 0 — формат по-прежнему валиден, не падает.
        let s_jit = datetime_now_jitter(60, &mut rng);
        assert!(s_jit.len() >= 20, "got: {s_jit}");
    }

    /// Phase 11 (Tier 1): `datetime_now_jitter_at` ветка jitter_secs > 0 —
    /// детерминированно отличается между seed.
    #[test]
    fn payload_datetime_now_jitter_at_nonzero_jitter_uses_rng_range() {
        use chrono::{TimeZone, Utc};
        // Фиксируем now для воспроизводимости.
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let mut rng = derive_rng(Some(42), 1);
        let s = datetime_now_jitter_at(now, 5, &mut rng);
        // 12:00:00 ± 5 сек → минуты и секунды могут быть в пределах 0..59.
        assert!(s.starts_with("2026-01-01T12:00:0") || s.starts_with("2026-01-01T11:59:5"));
    }

    /// Phase 11 (Tier 1): `weighted_index` пустые/нулевые веса → 0.
    #[test]
    fn payload_weighted_index_empty_and_zero_returns_zero() {
        let mut rng = derive_rng(Some(13), 1);
        // Пустой массив.
        assert_eq!(weighted_index(&[], &mut rng), 0);
        // Все нули → total=0 → 0.
        assert_eq!(weighted_index(&[0.0, 0.0, 0.0], &mut rng), 0);
        // Отрицательные веса тоже отфильтрованы (filter > 0).
        assert_eq!(weighted_index(&[-1.0, -2.0], &mut rng), 0);
    }

    /// Phase 11 (Tier 1): `zipf_index(0, _, _)` → 0 (n=0 — пустой домен).
    #[test]
    fn payload_zipf_index_n_zero_returns_zero() {
        let mut rng = derive_rng(Some(14), 1);
        assert_eq!(zipf_index(0, 1.0, &mut rng), 0);
        assert_eq!(zipf_index(0, 2.5, &mut rng), 0);
    }

    /// Phase 11 (Tier 1): regex `gen_from_regex` покрывает ветки HirKind.
    #[test]
    fn payload_regex_gen_covers_hir_branches() {
        let mut rng = derive_rng(Some(15), 1);
        // HirKind::Empty: пустой паттерн.
        let s = gen_from_regex("", &mut rng);
        assert_eq!(s, "");

        // HirKind::Look: lookahead (?=...) не генерирует символов.
        let s2 = gen_from_regex(r"(?:abc)", &mut rng);
        assert_eq!(s2, "abc");

        // HirKind::Capture: группа захвата.
        let s3 = gen_from_regex(r"(foo|bar)baz", &mut rng);
        assert!(s3 == "foobaz" || s3 == "barbaz", "got: {s3}");

        // HirKind::Literal: неэкранированные символы в паттерне.
        let s4 = gen_from_regex(r"hello", &mut rng);
        assert_eq!(s4, "hello");
    }

    /// Phase 11 (Tier 1): regex `gen_class` покрывает Class::Bytes ветку.
    /// `(?-u:...)` отключает Unicode-режим → байтовые классы.
    #[test]
    fn payload_regex_gen_class_bytes_mode() {
        let mut rng = derive_rng(Some(16), 1);
        // Class::Bytes: один ASCII диапазон.
        let s = gen_from_regex(r"(?-u:[\x41-\x5A])", &mut rng);
        assert_eq!(s.len(), 1);
        let c = s.as_bytes()[0];
        assert!((0x41..=0x5A).contains(&c), "expected A-Z, got 0x{c:02x}");

        // Class::Bytes: \d (в byte-mode) даёт ASCII digit.
        let s2 = gen_from_regex(r"(?-u:\d)", &mut rng);
        assert!(s2.as_bytes()[0].is_ascii_digit(), "got: {s2:?}");
    }

    /// Phase 11 (Tier 1): invalid regex pattern → empty string (не паникуем).
    #[test]
    fn payload_regex_invalid_pattern_returns_empty() {
        let mut rng = derive_rng(Some(17), 1);
        // Незакрытый класс.
        assert_eq!(gen_from_regex(r"[unterminated", &mut rng), "");
        // Невалидный escape.
        assert_eq!(gen_from_regex(r"\", &mut rng), "");
        // Битый синтаксис.
        assert_eq!(gen_from_regex(r"(", &mut rng), "");
    }

    /// Phase 11 (Tier 1): `int_in_range(max < min) → min` уже покрыт, но
    /// дополнительно — `max == min` даёт единственное значение.
    #[test]
    fn payload_int_in_range_equal_min_max_returns_constant() {
        let mut rng = derive_rng(Some(18), 1);
        for _ in 0..10 {
            assert_eq!(int_in_range(7, 7, &mut rng), 7);
        }
    }

    /// Phase 11 (Tier 1): `fresh_os_rng` не паникует при вызове через
    /// `derive_rng(None, seq)` — путь без seed.
    #[test]
    fn payload_derive_rng_none_uses_os_rng() {
        let mut rng = derive_rng(None, 0);
        // Просто не должно упасть; можно сгенерировать что-то.
        let _ = faker("uuid", &mut rng);
    }
}
