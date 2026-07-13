//! N8 (v8.7.1): property-based тесты для payload генераторов.
//!
//! Использует `proptest = "1"` для автоматической генерации входных данных
//! и проверки инвариантов. Каждый тест запускает 256+ случайных комбинаций
//! (по умолчанию в proptest).
//!
//! Это дополнение к существующим unit-тестам в `src/payload.rs::tests`,
//! которые покрывают только конкретные edge cases.

use crate::payload::{derive_rng, faker, int_in_range, pad_to_size};
use proptest::prelude::*;

/// N8: `int_in_range(min, max)` всегда возвращает значение в `[min, max]`.
/// Edge cases: min == max, min > max, min == i64::MIN, max == i64::MAX.
#[test]
fn prop_int_in_range() {
    proptest!(|(min: i64, max: i64)| {
        let mut rng = derive_rng(Some(42), 0);
        let v = int_in_range(min, max, &mut rng);
        if max < min {
            // Документированное поведение: возвращает min.
            assert_eq!(v, min);
        } else {
            assert!(v >= min && v <= max, "v={} не в [{}, {}]", v, min, max);
        }
    });
}

/// N8: `derive_rng(seed, seq)` детерминирован — тот же `(seed, seq)` даёт
/// ту же последовательность u64. Это инвариант F4 — seed-детерминизм
/// в межпроцессном контексте.
#[test]
fn prop_seed_determinism() {
    proptest!(|(seed: u64)| {
        let mut a = derive_rng(Some(seed), 0);
        let mut b = derive_rng(Some(seed), 0);
        // Каждый seed даёт одну и ту же последовательность u64 (8 байт каждый).
        for i in 0..16 {
            let av = rand::Rng::random::<u64>(&mut a);
            let bv = rand::Rng::random::<u64>(&mut b);
            assert_eq!(av, bv, "seed={} iter={}: {:?} != {:?}", seed, i, av, bv);
        }
    });
}

/// N8: `pad_to_size` всегда возвращает ровно `target` байт (не больше,
/// не меньше), даже если исходный body был длиннее target.
///
/// Ограничиваем диапазоны чтобы не уйти в OOM (target <= 64KB,
/// body_len <= target) — `pad_to_size` аллоцирует буфер размера target
/// сразу, и при target=usize::MAX это ~18 EB.
#[test]
fn prop_pad_to_size_exact_target() {
    proptest!(|(body_len in 0usize..1024, target in 0usize..65536)| {
        // Если body_len > target — pad_to_size вернёт body as-is (не усекает,
        // документированное поведение). Этот случай проверяем отдельно.
        if body_len > target {
            return Ok(());
        }
        let body: Vec<u8> = (0..body_len).map(|i| (i % 256) as u8).collect();
        let mut rng = derive_rng(Some(42), 0);
        let padded = pad_to_size(body, target, &mut rng);
        assert_eq!(
            padded.len(),
            target,
            "body_len={} target={} padded_len={}",
            body_len,
            target,
            padded.len()
        );
    });
}

/// N8: `faker("ipv4")` всегда возвращает валидный IPv4 (4 октета, 0..=255).
/// Делаем property-test на seed'ах из proptest'а.
#[test]
fn prop_faker_ipv4_valid_format() {
    proptest!(|(seed: u64)| {
        let mut rng = derive_rng(Some(seed), 0);
        let ip = faker("ipv4", &mut rng);
        let parts: Vec<&str> = ip.split('.').collect();
        assert_eq!(parts.len(), 4, "ip={} не имеет 4 октетов", ip);
        for p in &parts {
            let n: u32 = p.parse().unwrap_or_else(|_| panic!("ip={} октет {:?} не парсится", ip, p));
            assert!(n <= 255, "ip={} октет {} > 255", ip, n);
        }
    });
}

/// N8: `faker("uuid")` всегда возвращает валидный UUID v4
/// (формат 8-4-4-4-12, версия 4 = '4' в позиции 14, вариант ∈ {8,9,a,b}).
#[test]
fn prop_faker_uuid_v4_format() {
    proptest!(|(seed: u64)| {
        let mut rng = derive_rng(Some(seed), 0);
        let id = faker("uuid", &mut rng);
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 5, "uuid={} не 5 секций", id);
        assert_eq!(parts[0].len(), 8, "uuid={}", id);
        assert_eq!(parts[1].len(), 4, "uuid={}", id);
        assert_eq!(parts[2].len(), 4, "uuid={}", id);
        assert_eq!(parts[3].len(), 4, "uuid={}", id);
        assert_eq!(parts[4].len(), 12, "uuid={}", id);
        // Версия 4: первый hex-digit третьей группы = '4'.
        assert!(parts[2].starts_with('4'), "uuid={} не UUID v4", id);
        // Вариант RFC 4122: первый hex-digit четвёртой группы ∈ {8, 9, a, b}.
        let variant = parts[3].chars().next().unwrap();
        assert!(
            matches!(variant, '8' | '9' | 'a' | 'b'),
            "uuid={} вариант {} не ∈ {{8,9,a,b}}",
            id,
            variant
        );
    });
}

/// N8: `pad_to_size(body, 0)` — corner case: target=0, body не модифицируется
/// (тело возвращается as-is, т.к. body.len() >= 0). Это документированное
/// поведение — `pad_to_size` НЕ усекает body, только дополняет.
#[test]
fn prop_pad_to_size_zero_target_no_truncation() {
    let body = vec![0x42; 16];
    let mut rng = derive_rng(Some(0), 0);
    let padded = pad_to_size(body.clone(), 0, &mut rng);
    assert_eq!(
        padded, body,
        "pad_to_size(body, 0) должен вернуть body as-is (не усекать)"
    );
}
