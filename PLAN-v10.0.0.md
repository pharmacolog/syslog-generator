# PLAN: v9.x — Веха E «Зрелость» (P2)

> Статус: **середина вехи E**. v9.1.0 (N10) **выпущен** ✅. v9.0.0 (закрытие вехи D)
> выпущен ранее. Все P0+P1 задачи AUDIT.md §4 выполнены. Реализуем P2:
> новые форматы (F15), транспорты (F16), сценарии аномалий (F17),
> cipher_policy (N4), Docker (N12).

Дата: 2026-07-13. Цель: v9.2.0 (F15) → v9.3.0 (F16) → v9.4.0 (F17) → v9.5.0 (N4) → v9.6.0 (N12).

## Зафиксированные стратегические решения

| # | Решение | Обоснование |
|---|---|---|
| D1 | Kafka = `rskafka` под `feature = "kafka"` (optional) | Pure Rust, нет C-deps, быстрая компиляция, дефолт-сборка не платит за неиспользуемый код. |
| D2 | TLS = миграция `native-tls → rustls` в рамках N4 | `set_cipher_list` доступен только в OpenSSL-бэкенде native-tls (Linux-only). rustls даёт кросс-платформенный cipher selection через `CryptoProvider`. |
| D3 | `AnomalyKind` — расширенные сигнатуры (3-4 параметра) | `BurstInjection{rate_multiplier, every_secs, burst_secs, jitter_pct?}` выражает периодические burst'ы с джиттером. 2 параметра из исходного плана недостаточны. |

---

## 1. Полная инвентаризация вехи E (P2)

| Задача | Где в коде | Статус | План |
|--------|------------|--------|------|
| **N10** (✅ выпущен в v9.1.0) | `src/format/mod.rs` (`Format` trait + `FormatKind`), `src/transport/mod.rs` (`Transport` trait + `TransportKind`) | **DONE** | Trait'ы + static dispatch через enum. 0 breaking changes. |
| **N10 (gap)** | `src/generator/core.rs:251` `wrap_syslog` — match на `phase.format_type()` обходит `FormatKind` | **GAP** | **Шаг 0 для v9.2.0**: перевести продьюсер на `FormatKind`-диспатч с кешированием (см. §3.2). |
| **F15** | Не реализован. | pending | `src/format/{cef,leef,json_lines}.rs` через расширенный trait `Format` (см. §3.2). |
| **F16** | `src/transport/{file,tcp,udp,tls}.rs` — sender'ы. Нет Kafka. | pending | `src/transport/kafka.rs` через `rskafka` (feature flag). Расширение `file` rotation. Reconnect: exponential backoff + jitter (см. §3.3). |
| **F17** | Не реализован. | pending | `phases.anomalies: Option<Vec<Anomaly>>` в `Phase` — BurstInjection/SlowDrip/PacketLoss (расширенные сигнатуры, см. §3.4). |
| **N4.cipher_policy** | Не реализован. Текущий TLS-стек — `native-tls` (Linux-only cipher selection). | pending | **D2**: миграция на `rustls` + `tls_cipher_suites: Option<Vec<String>>` в `TargetConfig` (см. §3.5). |
| **N12** | Не реализован. | pending | `Dockerfile` (multi-stage, distroless/cc-debian12) + `.dockerignore` + `docker-compose.yml` (см. §3.6). |

---

## 2. План релизов

| Релиз | Тип | Что | Зависит от |
|-------|-----|-----|------------|
| **v9.1.0** ✅ | minor | **N10**: trait `Format` + trait `Transport` (static dispatch через enum). Без breaking changes. | — |
| **v9.2.0** | minor | **Шаг 0 (N10 gap)**: перевести `wrap_syslog` на `FormatKind`. **F15**: CEF + LEEF + JSON-lines через расширенный trait `Format`. | v9.1.0 |
| **v9.3.0** | minor | **F16**: Kafka transport (`rskafka`, `feature = "kafka"`) + файловая ротация (расширение `file`, не новый вариант) + reconnect exponential backoff с jitter | v9.2.0 (FormatKind) |
| **v9.4.0** | minor | **F17**: сценарии аномалий (`phases.anomalies: BurstInjection/SlowDrip/PacketLoss`, расширенные сигнатуры) + интеграционные тесты | — |
| **v9.5.0** | minor | **N4.cipher_policy** + **миграция `native-tls → rustls`**: добавление `tls_cipher_suites: Option<Vec<String>>` через `rustls::ClientConfig::with_cipher_suites` | — |
| **v9.6.0** | minor | **N12 (Docker)**: `Dockerfile` (multi-stage, distroless/cc-debian12) + `.dockerignore` + multi-arch buildx + `docker-compose.yml` | — |

---

## 3. Детальный план каждого релиза

### 3.1 v9.1.0 (N10) — ✅ ВЫПУЩЕН 2026-07-13

Trait `Format` (static dispatch через `FormatKind { Rfc5424, Rfc3164, Raw, Protobuf(Option<Schema>) }`), trait `Transport` (`TransportKind { File, Tcp, Udp, Tls }`), 6 unit-тестов, 0 breaking changes. Полное описание — CHANGELOG.md v9.1.0.

### 3.2 v9.2.0 (F15 — CEF/LEEF/JSON-lines) + Шаг 0 N10-gap

#### Шаг 0: рефакторинг `wrap_syslog`

Текущий `src/generator/core.rs:251`:
```rust
match phase.format_type() {       // String-диспатч, обходит FormatKind
    "rfc5424" => build_rfc5424(&header, &body),
    "rfc3164" => build_rfc3164(&header, &body),
    _ => body,
}
```

**Проблема**: F15 должен добавить новые форматы, но match в `wrap_syslog` — это hardcoded список. N10 ввёл `FormatKind`, но горячий путь его не использует.

**Решение**:
1. Кешировать `FormatKind` один раз в начале `run_phase_multi`:
   ```rust
   let format_kind = FormatKind::parse(phase.format_type())
       .expect("F13 валидация уже проверила");
   ```
2. Заменить `wrap_syslog` на вызов `format_kind.render(&header, &body)`.
3. **Устранить парсинг `phase.format_type()` в горячем пути** — было `O(N)` строковых сравнений на каждое сообщение, станет 1 match.

#### Расширение trait `Format`

CEF/LEEF не влезают в существующий `Header`. Два варианта:

**Вариант A (рекомендую)**: расширить сигнатуру:
```rust
pub struct FormatContext<'a> {
    pub header: &'a Header,                  // для rfc5424/3164
    pub cef: Option<&'a CefContext>,         // для cef
    pub leef: Option<&'a LeefContext>,       // для leef
    pub json_lines: Option<&'a JsonLinesContext>, // для json_lines
}
pub trait Format: Send + Sync {
    fn render(&self, ctx: &FormatContext<'_>, msg: &[u8]) -> Vec<u8>;
    fn name(&self) -> &'static str;
}
```

**Вариант B (legacy-compat)**: сохранить `render(&Header, &[u8])` для rfc5424/3164/raw/protobuf; добавить `render_cef(...)`, `render_leef(...)`, `render_json_lines(...)` как отдельные методы.

Выбираем A — единая точка диспатча, проще расширять.

#### F15 детали

- `src/format/cef.rs`: ArcSight Common Event Format
  - `CEF:Version|Device Vendor|Device Product|Device Version|Signature ID|Name|Severity|Extension`
  - **Escaping**: `|` → `\|`, `=` → `\=`, `\` → `\\` в extension-значениях; `|` в header-полях.
  - Поля в `Phase`: `cef_vendor`, `cef_product`, `cef_version`, `cef_signature_id`, `cef_name`, `cef_severity` (1..=10), `cef_extensions: HashMap<String, String>`.
- `src/format/leef.rs`: IBM QRadar LEEF
  - `LEEF:Version|Vendor|Product|Version|EventID|...|key=value\n`
  - Атрибуты как `key=value` пары (escape `\`, `=`).
- `src/format/json_lines.rs`: `{"ts":"...","level":"...","msg":"..."}\n`
  - Использовать существующую `serde_json::to_string` для экранирования.

#### Обновления артефактов

- `schemas/profile.schema.json`: добавить в `format` enum `["cef","leef","json_lines"]`, добавить `cef_*` и `leef_*` поля в `Phase`.
- `src/validate.rs::VALID_FORMATS`: расширить.
- `examples/cef_*.{json,yaml}`, `examples/leef_*.{json,yaml}`, `examples/json_lines_*.{json,yaml}`.
- `dashboards/*.json`: добавить label values `cef/leef/json_lines` в `messages_by_format_total`.

#### Тесты

- Round-trip парсинг для каждого формата.
- Faker-поля (`{{faker.ipv4}}`, `{{faker.user_agent}}`) в extensions.
- Escaping edge cases: `|` `=` `\` в значениях.

### 3.3 v9.3.0 (F16 — Kafka/Redpanda/файловая ротация/reconnect)

#### D1: Kafka через `rskafka` (feature flag)

`Cargo.toml`:
```toml
[features]
default = []
kafka = ["dep:rskafka"]

[dependencies]
rskafka = { version = "0.5", optional = true, default-features = false, features = ["compression-snappy"] }
```

`src/transport/kafka.rs` (под `#[cfg(feature = "kafka")]`):
- `target_sender_kafka(bootstrap_servers, topic, rx, metrics, shutdown)` через `rskafka::client::ClientBuilder` + `PartitionClient::producer()`.
- **Batching**: `rskafka` батчит внутренне, дополнительная логика не нужна.
- Метрики: `kafka_produce_duration_seconds` (histogram), `kafka_produce_batch_bytes`, `kafka_queue_depth`, `kafka_produce_errors_total`.

**Новые поля в `TargetConfig`** (все `Option` для backward-compat):
- `kafka_topic: Option<String>` (обязательно если `transport: "kafka"`)
- `kafka_client_id: Option<String>` (default `"syslog-generator"`)
- `kafka_compression: Option<String>` (`"none"/"gzip"/"snappy"/"lz4"`)
- `kafka_acks: Option<String>` (`"0"/"1"/"all"`)

#### Файловая ротация — расширение `file`, НЕ новый вариант

`TargetConfig`:
- `file_rotation_size_mb: Option<u64>` (default: None = без ротации)
- `file_rotation_interval_secs: Option<u64>` (default: None)
- `file_rotation_max_files: Option<u32>` (default: 10)

Триггеры:
- При `BufWriter` ≥ `size_mb * 1024 * 1024` ИЛИ
- При `Instant::now() >= opened_at + interval_secs`.

Именование: `<path>.<timestamp>.log` (timestamp = unix seconds). При ротации: flush → rename current → create new. Старые файлы сверх `max_files` удаляются (LRU).

Метрика: `file_rotations_total{phase, target}`.

#### Reconnect — exponential backoff с jitter

`TargetConfig`:
- `reconnect_max_attempts: Option<u32>` (None → бесконечно)
- `reconnect_initial_backoff_ms: u64` (default: 100)
- `reconnect_max_backoff_ms: u64` (default: 30000)
- `reconnect_multiplier: f64` (default: 2.0)

Алгоритм:
```rust
let mut backoff = initial_ms;
for attempt in 1..=max_attempts {
    if shutdown.is_cancelled() { return None; }
    if let Ok(s) = connect().await { return Some(s); }
    let jitter = rand::thread_rng().gen_range(0.5..1.5);
    let sleep_ms = (backoff as f64 * jitter).min(max_ms as f64) as u64;
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(sleep_ms)) => {}
        _ = shutdown.cancelled() => return None,
    }
    backoff = ((backoff as f64) * multiplier) as u64;
}
None
```

**Критично**: `tokio::select!` на shutdown внутри backoff-loop, иначе graceful shutdown будет ждать полного таймаута.

Применить к `tcp.rs::reconnect_tcp` и `tls.rs::tls_connect`.

#### Обновления артефактов

- `schemas/profile.schema.json`: `transport += ["kafka"]`, новые поля `kafka_*`, `file_rotation_*`, `reconnect_*` в `TargetConfig`.
- `src/validate.rs::VALID_TRANSPORTS` += `"kafka"`. Добавить `ValidationError::KafkaTopicRequired`.
- При `transport == "kafka"` валидировать наличие `kafka_topic`.

### 3.4 v9.4.0 (F17 — сценарии аномалий)

#### D3: расширенные сигнатуры `AnomalyKind`

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Anomaly {
    /// x-rate на every_secs, длительностью burst_secs. Периодические burst'ы с джиттером.
    BurstInjection {
        rate_multiplier: f64,
        every_secs: f64,
        burst_secs: f64,
        #[serde(default)]
        jitter_pct: Option<f64>,
    },
    /// rate / divisor на duration_secs. Плавное снижение нагрузки.
    SlowDrip {
        rate_divisor: f64,
        duration_secs: f64,
        #[serde(default)]
        jitter_pct: Option<f64>,
    },
    /// Дроп loss_percent% сгенерированных сообщений (не отправляются в transport).
    PacketLoss {
        loss_percent: f64,  // 0.0..=100.0
    },
}
```

#### Применение в `run_phase_multi`

После `generate_message` и **перед** `tx.send(msg)`:

1. **BurstInjection**: каждые `every_secs` в течение `burst_secs` сообщений умножают rate на `rate_multiplier` (sleep уменьшается в `rate_multiplier` раз). Вне burst — обычный rate.
2. **SlowDrip**: на `duration_secs` с начала фазы rate делится на `rate_divisor`.
3. **PacketLoss**: каждый сгенерированный message с вероятностью `loss_percent%` дропается (инкремент `anomalies_dropped_total`).

Дропнутые/задержанные сообщения **не должны** инкрементить `syslog_errors_total` — это не transport error.

#### Новые метрики

- `syslog_anomalies_dropped_total{phase, anomaly_kind}` — counter.
- `syslog_anomalies_active{phase, anomaly_kind}` — gauge (0/1 во время аномалии).

#### Интеграционные тесты

- BurstInjection: `achieved_rate` > `target_rate * 1.2` во время burst'а, tolerance ±20%.
- SlowDrip: `achieved_rate` < `target_rate * 0.8` во время drip'а, tolerance ±20%.
- PacketLoss: `messages_total{result="dropped_anomaly"}` ≈ `generated * loss_percent / 100`, tolerance ±5%.
- Детерминированность: при заданном `seed` — тот же профиль даёт тот же результат.

### 3.5 v9.5.0 (N4.cipher_policy + миграция на rustls)

#### D2: миграция `native-tls → rustls`

**Обоснование**: `native-tls` использует SChannel (Windows) / Secure Transport (macOS) / OpenSSL (Linux). `set_cipher_list` доступен только через OpenSSL-бэкенд (Linux-only). `rustls` — pure Rust, кросс-платформенный, поддерживает `ClientConfig::with_cipher_suites()`.

**Breaking changes** (документировать в CHANGELOG):
- Формат сертификатов: native-tls принимает PEM/DER напрямую; rustls принимает PEM через `rustls_pemfile`.
- Клиент-сертификат + ключ: native-tls принимает отдельные файлы; rustls принимает chain в одном файле или раздельно.
- Коннектор API: `tokio_native_tls::TlsConnector::connect()` → `tokio_rustls::client::TlsConnector::connect()`.

**План миграции** (выполняется в v9.5.0):

1. Добавить зависимости:
   ```toml
   rustls = { version = "0.23", default-features = false, features = ["std", "logging", "tls12"] }
   tokio-rustls = "0.26"
   rustls-pemfile = "2"
   web-pki = { version = "...", features = ["std"] }  # для валидации
   ```
2. Удалить `native-tls` + `tokio-native-tls` + `rcgen` (если использовался только для тестов TLS — оставить).
3. Переписать `src/transport/tls.rs::build_tls_connector` под `rustls::ClientConfig`.
4. Тесты TLS round-trip — обновить под новый API.

**`tls_cipher_suites: Option<Vec<String>>` в `TargetConfig`**:

```rust
let suites: Vec<rustls::SupportedCipherSuite> = cipher_names
    .iter()
    .map(|n| match n.as_str() {
        "TLS_AES_256_GCM_SHA384" => rustls::cipher_suite::TLS13_AES_256_GCM_SHA384,
        "TLS_AES_128_GCM_SHA256" => rustls::cipher_suite::TLS13_AES_128_GCM_SHA256,
        "TLS_CHACHA20_POLY1305_SHA256" => rustls::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
        // TLS 1.2 suites через rustls 0.23 API
        _ => return Err(format!("unsupported cipher suite: {n}")),
    })
    .collect::<Result<_, _>>()?;
config.cipher_suites = suites;
```

**Валидация**: `src/validate.rs` — `ValidationError::InvalidCipherSuite { name, allowed }` (allowed = список поддерживаемых rustls suites).

#### Альтернативный вариант (если миграция слишком болезненна)

Вынести rustls-миграцию в отдельный промежуточный релиз `v9.5.0-pre`, а `v9.5.0` оставить "только cipher_policy на native-tls (Linux-only)". Решение — после оценки объёма миграции.

### 3.6 v9.6.0 (N12 — Docker)

#### Dockerfile (multi-stage)

```dockerfile
# syntax=docker/dockerfile:1.6
FROM rust:1.97-bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake build-essential pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches
COPY tests ./tests
COPY schemas ./schemas
COPY examples ./examples
RUN cargo build --release --bin syslog-generator

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /app/target/release/syslog-generator /usr/local/bin/
COPY --from=builder /app/examples /examples:ro
ENTRYPOINT ["/usr/local/bin/syslog-generator"]
```

**Замечания**:
- `cc-debian12` (а не `static`) — потому что `rskafka` может линковаться с системными библиотеками; `cc` содержит libc.
- Если `rskafka` окажется полностью static — можно перейти на `gcr.io/distroless/static-debian12`.

#### .dockerignore

```
target/
.git/
.archived-releases/
.github/
**/*.log
**/Cargo.lock.bak
```

#### Multi-arch build

```yaml
# .github/workflows/docker.yml
- uses: docker/setup-buildx-action@v3
- uses: docker/build-push-action@v5
  with:
    platforms: linux/amd64,linux/arm64
    push: true
    tags: ghcr.io/${{ github.repository }}:${{ github.ref_name }}
```

#### docker-compose.yml

```yaml
services:
  syslog-generator:
    build: .
    command: --profile /examples/profile-docker.yaml
    volumes:
      - ./examples:/examples:ro
    depends_on: [rsyslog, prometheus]

  rsyslog:
    image: rsyslog/rsyslog:latest
    ports: ["514:514/udp", "601:601/tcp"]
    volumes:
      - ./docker/rsyslog.conf:/etc/rsyslog.conf:ro

  prometheus:
    image: prom/prometheus:latest
    ports: ["9090:9090"]
    volumes:
      - ./docker/prometheus.yml:/etc/prometheus/prometheus.yml:ro

  grafana:
    image: grafana/grafana:latest
    ports: ["3000:3000"]
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
```

#### Артефакты

- `examples/profile-docker.yaml` — пример профиля для Docker-окружения (target = rsyslog, format = rfc5424).
- `docker/rsyslog.conf` — приём по UDP/TCP, опционально TLS.
- `docker/prometheus.yml` — scrape endpoint генератора.

---

## 4. Критерии приёмки (для каждого релиза)

1. ✅ Все ранее зелёные тесты остаются зелёными (никаких регрессий)
2. ✅ Новые тесты добавлены для новой функциональности
3. ✅ **Backward-compat прогон**: `load_profile_from_path` на все `examples/*.json` + `examples/*.yaml` без изменений.
4. ✅ `cargo public-api` diff не показывает breaking changes (или breaking changes явно документированы в CHANGELOG с migration guide).
5. ✅ `cargo fmt --all -- --check` clean
6. ✅ `cargo clippy --all-targets -- -D warnings` clean
7. ✅ `cargo build --release` успех (для фичей с feature flags — `cargo build --release --features kafka`)
8. ✅ `cargo test --locked` все зелёные (включая `--features kafka` если есть)
9. ✅ `cargo bench --no-run --locked` успех
10. ✅ `cargo bench --quick` 9/9 Success
11. ✅ **Bench regression check**: throughput message_generation и sender_throughput не просел > 10% относительно предыдущего релиза.
12. ✅ Live-проверка бинарника: `./target/release/syslog-generator --version` показывает корректную версию
13. ✅ Уборка: `target/` удалён, zip удалён
14. ✅ Gitflow: feature → dev → release → main → tag → push
15. ✅ CI: все job'ы зелёные на GitHub Actions
16. ✅ Документация: README, CHANGELOG, CLAUDE_HANDOFF, AUDIT обновлены
17. ✅ PLAN-v10.0.0.md обновлён (отметка ✅ для закрытых задач)
18. ✅ Schema: `schemas/profile.schema.json` синхронизирован с новыми полями
19. ✅ Архив в `.archived-releases/` сохранён (НЕ в git)
20. ✅ feature/release ветки НЕ удаляются (по требованию)
21. ✅ **Cargo.toml version bumped** в первом коммите feature-ветки

---

## 5. Roadmap (после v9.6.0)

После завершения вехи E (v9.6.0) следующий этап — **v10.0.0** major release
(полная milestone вехи E, может быть с breaking changes если потребуется
для новых архитектурных решений).

Кандидаты на v10.0.0:
- Hot-reload профиля без остановки генератора.
- Distributed mode (multi-node coordinator).
- gRPC/syslog-over-HTTP2 транспорты.
- OpenTelemetry exporter (помимо Prometheus).