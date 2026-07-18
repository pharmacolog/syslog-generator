# Coverage Policy — syslog-generator

**Версия:** v10.7.16+ (tier-based coverage enforcement)
**Coverage gate:** ≥ 92% (phased rollout), target 97% (Tier 1 per-module)
**CI workflow:** `.github/workflows/ci.yml::coverage` job (required check)

## Tier-Based Coverage Targets

`codecov.yml::component_management` определяет три tier'а с per-component
status check'ами. Каждый PR проверяется относительно base branch — если
новый код уменьшает coverage ниже target, status check падает (CI блокирует merge).

### Tier 1 (must be 97%+, threshold 1%)

Core library — блокирующий target для production binary. Любой новый
код в этих файлах должен сопровождаться unit/property тестами.

**Группы:**

- **`tier1_format`** — `src/format/{cef,json_lines,leef,mod,protobuf,raw,rfc3164,rfc5424}.rs`
- **`tier1_transport_core`** — `src/transport/{file,mod,reconnect,tcp,udp}.rs`
- **`tier1_core`** — `src/{anomaly,error,load_shape,payload,schema,schema_check,shutdown,template,validate}.rs`,
  `src/observability/{metrics,mod,server}.rs`, `src/generator/{config,core,mod}.rs`

### Tier 2 (target 85%, threshold 2%)

Complex/integration-heavy модули. Реальный TLS handshake / broker требует
инфраструктуры (cert, Kafka broker) — часть кода покрывается integration
тестами, но unit coverage остаётся ниже Tier 1.

- **`tier2_tls`** — `src/transport/tls.rs` (target 85%, threshold 2%)
- **`tier2_kafka`** — `src/transport/kafka.rs` (target 70%, threshold 5%)

### Default rules (Tier 1 fallback)

Любой файл вне explicit component → Tier 1 (97%, threshold 1%).

## History (phased rollout)

| Version | Lines | Functions | Regions | Tier 1 modules | Notes |
|---------|-------|-----------|---------|----------------|-------|
| v10.3.0 (Coverage ч.1) | 86.40% | 88.36% | 86.49% | n/a | baseline |
| v10.4.0 (Coverage ч.2) | 87.07% | 89.38% | 87.20% | n/a | +0.67pp |
| v10.7.9 | 87.88% | n/a | n/a | n/a | pre-PR-Q baseline |
| v10.7.14 | 89.65% | 90.42% | 89.53% | n/a | +25 tests (PR-16) |
| v10.7.15 (PR-Q) | 89.65% → 89.65% | — | — | n/a | 9 required CI checks + tier-based codecov.yml |
| v10.7.15 (PR-Q.1) | 91.51% | — | — | 95–100% | +25 tests (format, generator, transport) |
| v10.7.15 (PR-Q.2) | 92.17% | — | — | 95–100% | +20 tests (main.rs assert_cmd, shutdown, validate) |
| v10.7.15 (PR-Q.3) | 92.88% | — | — | 95–100% | +17 tests (transport/tcp reconnect, observability/server, transport/file rotation) |
| v10.7.15 (PR-Q.4) | TBD | — | — | 95–100% | **+13 proptest (anomaly, load_shape, validate)** |
| v10.7.17 | TBD | — | — | **97%** | Tier 1 enforcement per codecov.yml |

## Proptest coverage (PR-Q.4)

Phase 9 (PR-Q.4) добавляет **property-based тесты** с `proptest = "1"`
для трёх модулей с наибольшим coverage-приоритетом:

| Файл | Тестов | Invariants покрыты |
|------|--------|---------------------|
| `src/anomaly_proptests.rs` | 5 | `combined_rate_multiplier > 0` всегда; `BurstInjection` inactive вне окна → multiplier = 1.0; `PacketLoss` распределение дропов ±2% на 10k trials; граничные `loss_percent=0/100`; `SlowDrip` окно/после-окна |
| `src/load_shape_proptests.rs` | 4 | `Linear` монотонность + bounded; `Sine` rate ≥ 0 всегда; `Burst` среднее = (burst_rate × burst_secs + base × (cycle − burst_secs)) / cycle ±5%; `Constant` rate не зависит от t |
| `src/validate_proptests.rs` | 4 | валидный Profile → no errors; невалидный transport (long random) → `InvalidTransport` с value; невалидный distribution → `InvalidDistribution`; facility ∈ 0..=23 + severity ∈ 0..=7 → no facility/severity errors |

**Зачем proptest, если есть unit-тесты?**

Unit-тесты покрывают конкретные edge cases (t=0, t=duration, t=2*duration).
Property-тесты генерируют **256+ случайных комбинаций параметров** для
каждого теста — это страховка от regression, когда кто-то меняет
семантику `combined_rate_multiplier` и старые unit-тесты не ловят
новый edge case в непрерывном пространстве параметров.

## How to add new code

### 1. Написать код

Любой новый `pub fn` / `pub struct` в Tier 1 файле должен сопровождаться
unit/property тестом. Минимум: один happy-path тест + один edge-case тест.

### 2. Запустить локально coverage

```bash
cargo llvm-cov --features kafka,test-helpers --summary-only
```

Output показывает `lines/functions/regions` per file. Если новый файл
не в `codecov.yml::ignore`, его coverage пойдёт в Tier 1.

### 3. Проверить per-component targets

```bash
# Per-file HTML report:
cargo llvm-cov --features kafka,test-helpers --html --output-dir coverage/
open coverage/index.html
```

Coverage ≥ 97% для Tier 1, ≥ 85% для Tier 2 (tls), ≥ 70% для Tier 2 (kafka).

### 4. Если не достигли target — добавить тесты

Сначала — unit-тесты на конкретные edge cases. Если unit-тесты
становятся слишком гранулярными (>5 на одну функцию) — добавить
property-based тест через `proptest!`.

Шаблон нового proptests файла:

```rust
// src/<module>_proptests.rs
use crate::module::*;
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_<invariant_name>(param1 in 0u32..100, param2 in 0u32..100) {
        // arrange: создать структуру с произвольными параметрами
        // act: вызвать функцию
        // assert: проверить инвариант
    }
}
```

Зарегистрировать в `src/lib.rs`:

```rust
#[cfg(test)]
mod <module>_proptests;
```

### 5. CI проверит автоматически

`.github/workflows/ci.yml::coverage` job:
- запускает `cargo llvm-cov --features kafka --lcov`
- upload в codecov.io
- status check `codecov/project` падает если coverage < target

## Excluded from coverage

Список файлов в `codecov.yml::ignore` — не идут в coverage analysis:

```yaml
ignore:
  - "examples/**"     # примеры, не runtime код
  - "benches/**"      # benchmark'и (Criterion)
  - "fuzz/**"         # fuzz targets
  - "tests/**"        # integration tests
  - "**/tests.rs"     # test-only файлы
  - "src/main.rs"     # CLI entrypoint (тестируется через assert_cmd)
  - "src/payload_proptests.rs"     # PR-Q.0: test-only файл
  - "src/anomaly_proptests.rs"     # PR-Q.4: test-only файл
  - "src/load_shape_proptests.rs"  # PR-Q.4: test-only файл
  - "src/validate_proptests.rs"    # PR-Q.4: test-only файл
```

**Зачем exclude proptests файлы:** это `#[cfg(test)]` модули, не runtime
код. Они автоматически исключаются из release build, но для coverage
analysis мы исключаем явно (coverage инструментирует исходники по
маркеру `#[test]` / `#[cfg(test)]`).

## CI integration

Coverage job в `.github/workflows/ci.yml`:

```yaml
coverage:
  name: Coverage
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: taiki-e/install-action@v2
    - uses: dtolnay/rust-toolchain@stable
      with:
        components: rustfmt, clippy, llvm-tools-preview
    - run: cargo llvm-cov --features kafka --lcov --output-path lcov.info
    - uses: codecov/codecov-action@v4
      with:
        files: lcov.info
        fail_ci_if_error: true
```

codecov-action читает `codecov.yml` и применяет tier-based status checks.
PR блокируется если:
- project coverage < target (Tier 1: 97%, Tier 2: 85%/70%)
- patch coverage < 85% (новый код в PR не покрыт)

## Best practices

1. **Тесты — часть кода, не afterthought.** Coverage < 90% = сигнал что
   код плохо спроектирован (слишком много edge cases без тестов).
2. **Property-тесты для инвариантов.** Если функция имеет математический
   инвариант (монотонность, неотрицательность, conservation), property-тест
   ловит регрессии которые unit-тесты пропускают.
3. **Edge cases в unit-тестах, диапазоны в proptests.** Unit-тест для
   `f(0)`, `f(max)`, `f(NaN)`. Property-тест для всех `f(x) ∈ [min, max]`.
4. **Coverage ≠ качество.** 100% coverage без assert'ов = false positive.
   Каждый тест должен проверять конкретное поведение, а не просто «вызвать
   функцию».

## References

- `codecov.yml` — tier-based targets + ignore list
- `.github/workflows/ci.yml::coverage` — CI integration
- `docs/PERFORMANCE.md` — benchmark coverage (orthogonal dimension)
- `docs/FUZZING.md` — fuzz coverage (separate from unit/integration)
- `CHANGELOG.md` (v10.7.14..v10.7.17) — phased rollout history
