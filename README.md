
# syslog-generator

[![CI](https://github.com/pharmacolog/syslog-generator/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/pharmacolog/syslog-generator/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-v9.0.0-blue)]()
[![Rust](https://img.shields.io/badge/rust-1.97%2B-orange)]()

**Milestone `v9.0.0` — веха D «Продакшн-готовность» ЗАКРЫТА.** Все P0/P1 задачи
выполнены. Публичный API полностью backward-compatible с v8.x (только
добавлены новые типы, ничего не удалено). Следующая веха — E (P2 «Зрелость»):
F15 (CEF/LEEF/JSON-lines), F16 (Kafka/Redpanda), F17 (сценарии аномалий),
N10 (transport trait), N12 (Docker/musl). Модульная архитектура с реальным multi-target
runtime (`file`, `tcp`, `udp`, `tls`), настоящим TLS client handshake через
`native-tls` / `tokio-native-tls`, mixed end-to-end тестами для `file + tcp + udp + tls`
по всем режимам диспетчеризации (`broadcast`, `round-robin`, `weighted`), negative-path
тестами и бенчмарками на Criterion. Вся сборка и тесты проверены реальной компиляцией
(`cargo build`, `cargo test`, `cargo bench`, `cargo clippy`) и автоматизированы через
GitHub Actions на ubuntu-latest + macos-latest.

**v9.0.0:** milestone-релиз — веха D «Продакшн-готовность» ЗАКРЫТА.
Все P0+P1 задачи AUDIT.md §4 выполнены (F1-F10, N1-N11, D3). Major-бамп
без breaking changes. Публичный API полностью backward-compatible с v8.x.

**v8.8.1:** патч-долг — правки `AUDIT.md` (поставлены ✅ на F7/F8/F9,
убраны устаревшие пометки "Отложено" из F13 и N4). Код без изменений.

**v8.7.0 (N6):** zero-copy/буферизация — `BytesMut` для TCP/TLS батчинга,
`BufWriter` (8 KiB) для файла. Уменьшение syscall'ов в ~50-100 раз для
типичной нагрузки (10k msg/s) и 0 аллокаций в горячем пути send-loop.

**v8.6.0 (N2):** синхронизация Grafana-дашборда с реальными метриками.
Добавлена `syslog_messages_by_format_total{format}` (счётчик по формату)
и 6 panels в дашборд (rate/latency/active workers/errors/messages by format).
Удалены фейковые gauge-ы `cpu_usage_percent`/`memory_usage_bytes` —
они были объявлены, но никогда не обновлялись. Дашборд теперь покрывает
все ключевые метрики нагрузки.

**v8.5.0 (D3):** формальная JSON Schema (`schemas/profile.schema.json`) +
YAML-ввод профиля. Профили можно загружать как из JSON (`.json`), так и
из YAML (`.yaml`/`.yml`) с автоопределением формата по расширению.
Новый флаг `--schema-strict` для runtime-валидации через `jsonschema`;
CI-стадия прогоняет его на всех примерах.

**v8.4.1:** patch-релиз — починка регрессии `sender_throughput` бенчмарков
(сломались ещё в v8.1.0 после введения F13-валидации профиля):
`make_profile` теперь выставляет явный `total_messages`, чтобы валидатор
не отвергал фазу как `UnboundedPhase`. Все 9 бенчей (3 + 6) теперь проходят.

**v8.4.0 (N9):** CI-пайплайн на GitHub Actions (`.github/workflows/ci.yml`).
Все PR и push в `main`/`dev` проходят через `fmt --check` →
`clippy --all-targets -- -D warnings` → `build --release` →
`test` → `bench --no-run`. Матрица ubuntu-latest + macos-latest
покрывает оба бэкенда native-tls (openssl-sys / Security.framework).
Актуализирован `.gitignore` (тестовые логи, TLS-PEM, zip-архивы,
IDE/editor). Применён `cargo fmt --all` для соответствия CI-гейту.

**v8.3.1:** patch-релиз — починка 3 упавших TLS-интеграционных тестов
(`test_mixed_multi_target_*_end_to_end`), которые падали из-за несовместимости
`rcgen 0.13` с системным OpenSSL и превышения лимита validity period на macOS.
Сертификаты теперь генерируются через `openssl req -config openssl-server.cnf`.
Все 49 интеграционных + 11 N7 + 88 unit-тестов зелёные.

**v8.3.0 (N7):** типизированные ошибки рантайма через `thiserror`. В рантайм-коде
больше нет `.unwrap()`/`.expect()`: `MetricsError`, `ConfigError`, `DrainError`
и общий `RuntimeError` пробрасываются через `?` и всплывают в `eprintln` как
структурированное сообщение с человекочитаемым контекстом.

## Quick start

```bash
cargo build --release
cargo test
./target/release/syslog-generator --profile examples/single_target.json
```

## CLI (F11)

Генератор можно запускать как из JSON-профиля, так и целиком из флагов
командной строки. Флаги-оверрайды применяются к загруженному профилю
(или к пустому в быстром режиме) **перед валидацией и запуском**.

```bash
# Полный список флагов
./target/release/syslog-generator --help
./target/release/syslog-generator --version

# Быстрый режим без файла-профиля: одна фаза из --message
./target/release/syslog-generator -t 127.0.0.1:514:udp -m 'evt {{sequence}}' --total 100 --seed 42

# Запись в файл (транспорт file, адрес = путь)
./target/release/syslog-generator -t /tmp/out.log:file -m 'line {{sequence}}' --total 10 --format raw

# Профиль из файла + оверрайды скорости/длительности во ВСЕХ фазах
./target/release/syslog-generator --profile examples/single_target.json --rate 500 --duration 60
```

Флаги-оверрайды:

| Флаг | Действие |
|------|----------|
| `-p, --profile <FILE>` | JSON-профиль нагрузки |
| `-t, --target <ADDR[:TRANSPORT]>` | Цель (повторяемый); заменяет `targets`. TRANSPORT: `tcp`\|`udp`\|`tls`\|`file` (по умолчанию `tcp`) |
| `--distribution <D>` | `round-robin`\|`broadcast`\|`weighted` |
| `--rate <N>` | `messages_per_second` во всех фазах |
| `--duration <SEC>` | `duration_secs` во всех фазах |
| `--total <N>` | `total_messages` во всех фазах |
| `--format <F>` | `rfc5424`\|`rfc3164`\|`raw`\|`protobuf` во всех фазах |
| `--seed <N>` | seed ГПСЧ во всех фазах |
| `-m, --message <TPL>` | Шаблон(ы) для быстрого режима без профиля |
| `--validate` | Только проверить профиль (dry-run) и выйти |
| `--print-config` | Вывести итоговый профиль (JSON) и выйти |

Коды возврата: `0` — успех/профиль валиден; `1` — ошибка чтения/парсинга
или профиль невалиден.

## Валидация профиля (F13)

Перед запуском профиль проходит структурную и семантическую валидацию
(**fail-fast**). Валидатор собирает **все** проблемы за один проход и
выводит их списком, а не падает на первой. Проверяются:

- допустимость `transport`, `format`, `distribution`, `framing`, `shutdown.mode`;
- диапазоны `syslog.facility` (0..=23) и `syslog.severity` (0..=7) — только для `rfc5424`/`rfc3164`;
- согласованность `template_weights` с числом шаблонов, отсутствие отрицательных/NaN-весов;
- непустые `targets`/`phases`, `connections >= 1`, наличие источника контента (templates/templates_file/schema_file);
- условие остановки фазы (иначе фаза работала бы бесконечно);
- корректность `load_shape` (неотрицательные rate, положительные периоды).

Пример вывода на невалидном профиле:

```
профиль невалиден: найдено 2 проблем(ы):
  1. target[0] (address="127.0.0.1:514"): недопустимый transport "sctp"; допустимо: tcp, udp, tls, file
  2. phase[0] ("burst"): syslog.severity=12 вне диапазона 0..=7 (RFC 5424 §6.2.1)
```

Программно: `validate_profile(&profile) -> Vec<ValidationError>` (пустой
вектор = профиль валиден); `run_profile()` вызывает валидацию сам.

### Формальная JSON Schema и YAML-ввод (D3, v8.5.0)

Профили можно загружать как из JSON, так и из YAML:

```bash
# JSON
./target/release/syslog-generator --profile examples/multi_target_roundrobin.json

# YAML
./target/release/syslog-generator --profile examples/multi_target_roundrobin.yaml
./target/release/syslog-generator --profile examples/multi_target_roundrobin.yml
```

Формат определяется по расширению файла в `load_profile_from_path`
(`.json` / `.yaml` / `.yml`). Неподдерживаемое расширение даёт явную
ошибку `ConfigError::UnsupportedFormat`.

Дополнительно к F13-валидации доступна структурная проверка против
формальной JSON Schema (`schemas/profile.schema.json`) — встроена в
бинарник через `include_str!`:

```bash
./target/release/syslog-generator --validate --schema-strict --profile my.yaml
```

Schema-strict ловит структурные ошибки (неправильные типы, неизвестные
ключи, значения вне диапазонов 0..=23/0..=7) с более точными сообщениями
от `jsonschema`, чем общий serde-парсер. В CI (`validate examples` job)
schema-strict прогоняется на всех примерах из `examples/` — это регрессионный
тест на изменения в схеме.

Семантические правила (диапазоны facility/severity в зависимости от формата,
веса шаблонов, условия остановки фазы) остаются на F13-валидаторе — JSON Schema
дополняет, не дублирует.

## Бенчмарки

Бенчмарки используют Criterion (harness = false) и работают поверх публичного API:

```bash
# генерация сообщений: рендеринг шаблонов, generate_message, диспетчер
cargo bench --bench message_generation

# пропускная способность отправки: реальные TCP/UDP endpoint'ы + run_profile
cargo bench --bench sender_throughput
```

## Управление нагрузкой (Веха A)

Интенсивность и объём генерации задаются на уровне фазы:

- `messages_per_second` — целевая интенсивность (сообщений в секунду). Ограничение
  скорости реализовано токен-бакетом на базе крейта `governor`. Значение `0` означает
  «без ограничения скорости» (максимальная скорость).
- `duration_secs` — условие остановки фазы по времени (сек). `0` — не ограничивать по
  времени.
- `total_messages` — условие остановки по общему числу сообщений. Не задано — не
  ограничивать по количеству.

Фаза завершается по первому наступившему условию (`duration_secs`, `total_messages`
 или сигнал завершения). Если не заданы ни `duration_secs`, ни `total_messages`,
 отправляется одно сообщение (режим smoke/демо), чтобы прогон не длился бесконечно.
Жёсткий потолок в 100 сообщений на фазу, присутствовавший в прежних версиях, снят.

Пример профиля постоянной нагрузки 200 msg/s, всего 500 сообщений:

```json
{
  "targets": [{"address": "/tmp/out.log", "transport": "file"}],
  "distribution": "round-robin",
  "phases": [
    {"name": "steady", "messages_per_second": 200, "total_messages": 500,
     "templates": ["load seq={{sequence}}"]}
  ]
}
```

Метрики нагрузки Prometheus: `syslog_messages_generated_total{phase}` (счётчик
сгенерированных сообщений), `syslog_target_rate_messages_per_second` (целевая скорость),
`syslog_achieved_rate_messages_per_second` (достигнутая скорость последней фазы),
`syslog_active_workers` (число активных воркеров во всех target'ах фазы).

### Профили нагрузки во времени (F3)

Помимо постоянного rate фаза может задавать **кривую интенсивности** через
поле `load_shape`. Когда оно задано, планировщик вычисляет мгновенный
target rate `r(t)` в каждый момент и выдерживает соответствующий интервал
(`messages_per_second`/`governor` в этом режиме не используется). Формы:

| `type`     | Параметры | Описание |
|------------|-----------|----------|
| `constant` | `rate?` | Постоянный rate (без `rate` — берётся `messages_per_second`) |
| `linear`   | `start_rate`, `end_rate` | Линейный ramp за `duration_secs` фазы |
| `sine`     | `min_rate`, `max_rate`, `period_secs` | Синусоида, старт в минимуме |
| `burst`    | `base_rate`, `burst_rate`, `every_secs`, `burst_secs` | База + периодические всплески |

Сценарий ramp-up → ramp-down через две фазы:

```json
{
  "phases": [
    { "name": "ramp-up", "duration_secs": 60,
      "templates": ["seq={{sequence}}"],
      "load_shape": { "type": "linear", "start_rate": 100, "end_rate": 5000 } },
    { "name": "ramp-down", "duration_secs": 30,
      "templates": ["seq={{sequence}}"],
      "load_shape": { "type": "linear", "start_rate": 5000, "end_rate": 0 } }
  ]
}
```

Всплески каждые 10с по 2с:

```json
{ "type": "burst", "base_rate": 100, "burst_rate": 8000, "every_secs": 10, "burst_secs": 2 }
```

Обратная совместимость: без `load_shape` фаза работает как раньше — постоянный
rate через `messages_per_second`. Готовые примеры: `examples/load_shape_ramp.json`,
`examples/load_shape_sine.json`, `examples/load_shape_burst.json`.

## Конкурентность соединений (пул воркеров)

Поле `connections` у target'а задаёт размер пула воркеров (параллельных
соединений/сокетов) на этот target. Все воркеры target'а конкурентно
читают из его общей очереди: каждое сообщение обрабатывает ровно один
воркер. Для `tcp`/`tls` это даёт N независимых соединений (горизонтальное
масштабирование потоков на один приёмник).

```json
{
  "targets": [
    { "address": "127.0.0.1:6514", "transport": "tcp", "connections": 8, "weight": 1 }
  ],
  "distribution": "round-robin",
  "phases": [
    { "name": "pool", "messages_per_second": 5000, "duration_secs": 30,
      "templates": ["load seq={{sequence}}"] }
  ]
}
```

Запись каждого сообщения выполняется одним `write_all` (пейлоад + `\n` в одном
буфере); для файла с O_APPEND это гарантирует атомарную дозапись без
перемешивания строк между воркерами.

## Форматы syslog (Веха B)

Поле `format` фазы выбирает формат сообщения:

- `rfc5424` (по умолчанию) — [RFC 5424](https://www.rfc-editor.org/rfc/rfc5424.html):
  `<PRI>1 TIMESTAMP HOSTNAME APP-NAME PROCID MSGID STRUCTURED-DATA MSG`.
  PRIVAL = facility·8 + severity; TIMESTAMP — RFC3339 UTC с миллисекундами и `Z`;
  пустые поля → NILVALUE (`-`); опционально UTF-8 BOM перед MSG.
- `rfc3164` — классический BSD ([Graylog reference](https://graylog.org/post/syslog-protocol-a-reference-guide/)):
  `<PRI>Mmm dd hh:mm:ss HOSTNAME TAG: MSG`.
- `protobuf` — protobuf-подобная сериализация по схеме.
- любое другое значение (например `raw`) — сырой рендер шаблона без обёртки.

Тело шаблона (`templates`) — это MSG; заголовок задаётся блоком `syslog`:

```json
{
  "name": "auth",
  "format": "rfc5424",
  "total_messages": 100,
  "templates": ["User {{sequence}} logged in from {{faker.ipv4}}"],
  "syslog": {
    "facility": 4, "severity": 5,
    "hostname": "web-01", "app_name": "authsvc",
    "procid": "8421", "msgid": "AUTH",
    "structured_data": "[origin@32473 ip=\"192.0.2.1\"]",
    "bom": false
  }
}
```

Поля заголовка: `facility` (0..23), `severity` (0..7), `hostname`, `app_name`,
`procid`, `msgid`, `structured_data`, `bom`. Строковые поля проходят подстановку
шаблона, поэтому в них можно использовать `{{hostname}}`, `{{sequence}}` и т.п.
Пустые/`-` поля дают NILVALUE. Значения в STRUCTURED-DATA (`"`, `\`, `]`) нужно
экранировать (см. `syslog::escape_sd_value`).

## Фрейминг потоковых транспортов (RFC 6587)

Для `tcp`/`tls` поле `framing` у target'а выбирает способ разграничения сообщений:

- `non-transparent` (по умолчанию) — `SYSLOG-MSG` + LF (`\n`).
- `octet-counting` — `MSG-LEN SP SYSLOG-MSG`, где MSG-LEN — число октетов
  сообщения ([RFC 6587](https://datatracker.ietf.org/doc/html/rfc6587)).
  Рекомендуется для syslog-over-TLS ([RFC 5425](https://www.rfc-editor.org/rfc/rfc5425.txt), порт 6514).

```json
{ "address": "127.0.0.1:6514", "transport": "tls", "framing": "octet-counting" }
```

Для `file`/`udp` поле игнорируется (строка/датаграмма — самостоятельная единица).

## Вариативный пейлоад (Веха C)

Пейлоад генерируется вариативно; при заданном `seed` фазы вывод полностью
воспроизводим (в т.ч. между процессами) — один и тот же `seed` + порядковый
номер сообщения дают идентичный результат. Без `seed` берётся энтропия ОС.

### Faker-токены `{{faker.*}}`

Доступны прямо в шаблонах: `ipv4`, `ipv6`, `mac`, `uuid` (валидный v4),
`hostname`, `username`, `user_agent`, `url`, `http_status`.

```json
"templates": ["login user={{faker.username}} ip={{faker.ipv4}} sid={{faker.uuid}}"]
```

### Поля schema

В `schema_file` поле описывается типом и параметрами:

- `int` — случайное целое `min..=max`;
- `string` — случайная строка длины `len`;
- `datetime` — реальное «сейчас» (RFC3339 UTC) ± `jitter_secs`;
- `faker` — любой faker-вид через `"faker": "ipv4"` и т.п.;
- `enum` — выбор из `values` с `distribution`: `uniform` (дефолт),
  `weighted` (по `weights`) или `zipf` (по `zipf_exponent` — «горячие» ключи чаще);
- `regex` — строка, соответствующая паттерну из `regex` (см. ниже).

```json
{
  "template": "status={{status}} rid={{rid}} bytes={{bytes}}",
  "fields": {
    "status": {"type":"enum","values":["200","404","500"],"distribution":"weighted","weights":[8,1,1]},
    "rid": {"type":"string","len":10},
    "bytes": {"type":"int","min":100,"max":9999}
  }
}
```

### Regex-генерация строк (F5)

Поле `"type": "regex"` генерирует строку, соответствующую заданному паттерну.
Паттерн разбирается в HIR (`regex-syntax`) и обходится проектным ГПСЧ,
поэтому вывод детерминирован по `seed` (F4). Поддержаны литералы, классы
символов, повторы, альтернация, группы. Неограниченные повторы (`+`,
`*`) ограничены `REGEX_MAX_REPEAT = 16`; некорректный паттерн даёт пустую
строку.

```json
{
  "template": "txn={{txn}} sess={{sess}}",
  "fields": {
    "txn":  {"type":"regex","regex":"[A-Z]{2}-[0-9]{4}-[a-f0-9]{6}"},
    "sess": {"type":"regex","regex":"(alpha|beta|gamma)_[0-9]{3}"}
  }
}
```

### Межполевые корреляции (F6)

Поле может зависеть от другого: `"depends_on": "<имя родительского поля>"`
вместе с `"mapping"` (значение родителя → значение этого поля) и
`"mapping_default"` (значение, если значения родителя нет в `mapping`).
Генерация двухпроходная: сначала независимые поля, затем зависимые.
Детерминизм по `seed` сохранён.

```json
{
  "template": "status={{status}} sev={{sev}}",
  "fields": {
    "status": {"type":"enum","values":["200","301","404","500"]},
    "sev":    {"type":"string","len":5,"depends_on":"status",
               "mapping":{"200":"INFO","301":"INFO","404":"WARN","500":"ERROR"},
               "mapping_default":"NA"}
  }
}
```

Строка `status=404 sev=WARN` — `sev` всегда согласован со `status`.

### Мультишаблоны (F14) и паддинг

Если задано несколько `templates`, на каждое сообщение выбирается случайный
шаблон — равновероятно или по `template_weights` (длина = числу шаблонов).
Поле фазы `pad_to_bytes` добивает тело сообщения случайными символами до
целевого размера в байтах.

```json
{
  "name": "varied", "seed": 777,
  "templates": ["kind=login user={{faker.username}}", "kind=http url={{faker.url}}"],
  "template_weights": [1, 2],
  "pad_to_bytes": 256
}
```

## Формат protobuf (F10)

Фаза с `"format": "protobuf"` выдаёт настоящий Protobuf wire-format
(varint + length-delimited), а не JSON. Поля задаются в `protobuf_schema`
строкой `"номер:тип:шаблон"` (или `"номер:шаблон"`, или просто
`"шаблон"` — тогда номера назначаются по алфавиту имён). Типы: `str`
(дефолт), `bytes`, `int`, `uint`, `sint`, `bool`, `double`, `float`.
Шаблон значения рендерится (faker-токены доступны). Поля
сортируются по номеру — вывод каноничен и детерминирован.

```json
{
  "name": "pb", "seed": 42, "format": "protobuf",
  "protobuf_schema": {
    "1_host": "1:{{faker.hostname}}",
    "2_pid":  "2:int:{{pid}}",
    "3_code": "3:uint:{{faker.http_status}}",
    "4_msg":  "4:user {{faker.username}} from {{faker.ipv4}}"
  }
}
```

См. готовый пример `examples/protobuf_message.json`.

> ⚠️ Файловый транспорт использует `\n`-фрейминг и небезопасен
> для бинарного protobuf (тело может содержать байт `0x0a`). Для
> бинарного вывода по TCP/TLS используйте octet-counting фрейминг.

## Метрики (N3)

Помимо базовых счётчиков собираются:

- `syslog_send_duration_seconds` — histogram латентности отправки
  (корзины 5µs–1s) — основа для p50/p95/p99;
- `syslog_message_size_bytes` — histogram размера сообщений (16B–64KB);
- `syslog_reconnects_total` — счётчик попыток реконнекта с метками
  `transport`, `target` (инкрементируется при восстановлении TCP/TLS).

## HTTP-эндпоинт /metrics (F12)

Если задан `metrics_addr` (в профиле) или флаг `--metrics-addr`, генератор
поднимает лёгкий HTTP-сервер на всё время прогона:

```bash
syslog-generator -p profile.json --metrics-addr 127.0.0.1:9090
curl -s http://127.0.0.1:9090/metrics   # Prometheus text exposition (v0.0.4)
```

- `GET /metrics` (и `GET /` как алиас) — 200, тело в формате Prometheus;
- любой другой путь — 404; не-GET — 405.
- Сервер гасится по завершении всех фаз. Недоступность привязки
  логируется, но не прерывает генерацию (метрики — вспомогательный канал).

Метрики также доступны программно через `gather_metrics`.

## Транспорты

- `file` — дозапись в файл;
- `tcp` — потоковая отправка с фреймингом (`non-transparent`/`octet-counting`);
- `udp` — датаграммы;
- `tls` — настоящий TLS client handshake (`native-tls` / `tokio-native-tls`).

### Безопасный TLS (N4)

По умолчанию TLS-транспорт **проверяет сертификат** сервера и его имя.
Поля `TargetConfig`:

| Поле | Назначение |
|------|-----------|
| `tls_domain` | Имя для SNI и проверки имени (по умолчанию — хост-часть `address`) |
| `tls_ca_file` | PEM доверенного CA (для self-signed/приватного CA); добавляется к системным корням |
| `tls_insecure` | `true` — ОТКЛЮЧИТЬ проверку (явный opt-in, небезопасно; по умолчанию `false`) |

Пример (self-signed через доверенный CA):

```json
{ "address": "10.0.0.5:6514", "transport": "tls",
  "tls_domain": "syslog.internal", "tls_ca_file": "/etc/ssl/my-ca.pem" }
```

Несуществующий `tls_ca_file` отклоняется валидацией (F13). При
`tls_insecure: true` в stderr печатается предупреждение.

При ошибке записи TCP/TLS-транспорт выполняет одну попытку реконнекта
(с учётом в `syslog_reconnects_total`) и повторно отправляет сообщение.
При ошибке подключения или TLS-handshake транспорт фиксирует ошибку в метрике
`syslog_errors_total` и дренирует очередь, не блокируя генератор.

## Документы

- `docs/USER_GUIDE.md`
- `docs/DEVELOPER_GUIDE.md`
- `CHANGELOG.md`
- `REVIEW.md`
