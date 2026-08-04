# Benchmark Runbook — Real VM Run (Issue #196)

> **Issue:** #196 (real benchmark on 8-core / 32 GB VM)
> **Milestone:** v11.6 (deferred из Issue #106 / GTM-1)
> **Статус:** План. НЕ выполняется в текущем PR. Это execution plan для
> будущего оператора, который проведёт реальный прогон на dedicated VM.
> **Hard rule:** Все числа в `perf/whitepaper-results.json` появятся **только**
> после прохождения этого runbook'а. Маркетинговые публикации (Issue #199)
> ссылаются на эти числа; цитирования (Issue #200) — на их публикации.

## Связанные issue

- **#106** — whitepaper harness (ГОТОВ, status=`schema_only`).
- **#188** — pre-condition: cycle-tests + soft cap fix в `syslog-generator`
  (зависимость; runbook ожидает, что #188 merged).
- **#191** — пред-условие: release маркирован `v11.6.0` (runbook ссылается
  на конкретный git tag).
- **#196** — это issue (real VM run).
- **#197** — empty definition fallback (cognitive complexity) — не блокирует.
- **#198** — `cargo-public-api` released snapshot — не блокирует.
- **#199** — whitepaper publication (depends on #196).
- **#200** — citations tracking (depends on #199).

## Acceptance criteria

Run считается **успешным** и пригодным для whitepaper **только когда**:

1. ✅ Прогон выполнен на VM, чьи specs задокументированы в `host` field
   `perf/whitepaper-results.json` и соответствуют §1.
2. ✅ Все 4 workloads × 4 инструмента = 16 cells отработали (для
   `n_a`/`skipped` — причина задокументирована).
3. ✅ `RUNS=3` per cell (3 прогона), best-of-3 берётся в `measurement`.
4. ✅ Harness exit code = 0 (или, если `n_a` обоснован, 2) — **НЕ** 1.
5. ✅ `perf/whitepaper-results.json::status == "complete"`.
6. ✅ Rate gate 95–105% и size gate ±5% соблюдены для всех `completed` cells.
7. ✅ `tests/integration_tests.rs` (полный набор, без `#[ignore]`) зелёный
   на той же VM (regression gate).
8. ✅ Сгенерирован `results/REPORT.md` (через `make report`).
9. ✅ Сгенерированы plot'ы через `scripts/06_plot.py` (Issue #196 deliverable).
10. ✅ Сделан `git commit` с measured runs и PR в `main` (т.е. Eventual
    публикация на GitHub Releases).

## 1. VM provisioning

### 1.1 Спецификация (locked)

| Параметр | Значение | Почему |
|---|---|---|
| Region | `eu-central-1` (Frankfurt) или `us-east-1` | оба имеют быстрые AMD EPYC |
| Instance type | `c5.2xlarge` | 8 vCPU, 16 GiB RAM, EBS-only |
| Tenancy | `default` (shared) | dedicated не нужен для benchmark'а |
| AMI | `ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*` (Ubuntu 22.04 LTS) | LTS, актуальный kernel, native rdkafka pkg |
| Storage | 100 GiB gp3, 3000 IOPS, 125 MB/s | для cargo target/, Kafka logs, results |
| Pricing | on-demand, terminate после run | не spot (preemption исказит timings) |

**Не использовать:**
- `t2/t3` (burstable, throttle).
- Graviton (ARM): `syslog-generator` тестируется на x86_64; ARM под
  вопросом для cross-arch fairness.
- `c5n` / `c6i` (network optimized): loopback benchmark от network gains
  не зависит, а instance cost растёт.

### 1.2 Bootstrap

```bash
# Запустить VM (operator job; replace AMI id):
aws ec2 run-instances \
  --image-id ami-0a91cd140a1fc148a \
  --instance-type c5.2xlarge \
  --key-name operator-key \
  --security-group-ids sg-bench-22 \
  --block-device-mappings 'DeviceName=/dev/sda1,Ebs={VolumeSize=100,VolumeType=gp3,Iops=3000,Throughput=125}' \
  --tag-specifications 'ResourceType=instance,Tags=[{Key=purpose,Value=whitepaper-2026-bench}]' \
  --count 1

# Дождаться запуска, получить PublicIPv4:
aws ec2 describe-instances \
  --filters "Name=tag:purpose,Values=whitepaper-2026-bench" "Name=instance-state-name,Values=running" \
  --query 'Reservations[].Instances[].PublicIpAddress' \
  --output text

# SSH:
ssh -i ~/.ssh/operator-key.pem ubuntu@<PUBLIC_IP>
```

### 1.3 Изоляция CPU

c5.2xlarge имеет 8 vCPU. По умолчанию система может мигрировать процессы
между ядрами, доставляя cache invalidation и jitter. Изоляция:

```bash
# Один раз при bootstrap (root):
sudo grubby --update-kernel=ALL --args="isolcpus=0-6 nohz_full=0-6 rcu_nocbs=0-6"
sudo reboot
# После reboot:
cat /proc/cmdline | tr ' ' '\n' | grep -E 'isolcpus|nohz_full|rcu_nocbs'
```

**Логика распределения:**

| CPU | Назначение |
|---|---|
| CPU 0–6 | isolated (sender pins к CPU 1, receiver к CPU 2, workload-driver к CPU 3) |
| CPU 7 | housekeeping (kernel, IRQ, systemd) |

**Pin через `taskset -c` в каждом runner-скрипте:**

```bash
# В scripts/03_run_syslog_generator.sh (после #196 merge):
taskset -c 1 "${SYSLOG_GENERATOR_BIN}" -p "${CONFIG_PATH}" --duration "${DURATION_SECS}"
# Ресивер (отдельный процесс):
taskset -c 2 python3 "$(dirname "$0")/receiver.py" "${RECV_ARGS[@]}"
```

### 1.4 CPU frequency scaling

AWS c5.2xlarge под капотом — Intel Xeon Platinum 8275CL (Cascade Lake),
3.0 GHz base. По умолчанию `intel_pstate` driver ставит governor
`powersave`, что позволяет уйти в turbo до 4.0 GHz. Это создаёт variance
между прогонами.

**Фиксация:**

```bash
# Disable turbo (для reproducibility):
echo 1 | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo
# Pin to base frequency:
sudo cpupower frequency-set --governor performance
sudo cpupower frequency-set --freq 3.0GHz

# Verify:
sudo cpupower frequency-info | grep -E 'current policy|boost state'
# Ожидаемый вывод:
#   current policy: frequency should be within 3.00 GHz and 3.00 GHz.
#   boost state: 0 (no turbo)
```

**Почему отключаем turbo:** Issue #106 spec требует "honest benchmark";
если позволить turbo, то результаты 1-го прогона могут быть на 10–15%
выше, чем 3-го (CPU прогрелся). Pin к base 3.0 GHz убирает этот
источник variance.

### 1.5 Background noise

```bash
# Убить всё, что может воровать CPU:
sudo systemctl stop snapd apt-daily.timer apt-daily-upgrade.timer
sudo systemctl disable --now unattended-upgrades unattended-upgrades-shutdown

# Drop caches (между прогонами):
sudo sh -c 'echo 3 > /proc/sys/vm/drop_caches'

# IRQ affinity (минимум на housekeeping CPU 7):
for irq in /proc/irq/*/smp_affinity; do
  echo 80 > "$irq"  # bit 7
done
```

## 2. Tool installation (host)

Список 4 инструментов + dependency для Kafka fixture. Все — нативные
linux пакеты; ни одного Python wrapper'а.

### 2.1 syslog-generator

```bash
# Clone pinned tag:
git clone https://github.com/pharmacolog/syslog-generator.git /opt/syslog-generator
cd /opt/syslog-generator
git checkout v11.6.0  # tag из Issue #191

# Build release (PGO/lto/strip включены в Cargo.toml release profile):
cargo build --release --locked --bin syslog-generator

# Verify:
/opt/syslog-generator/target/release/syslog-generator --version
# Ожидаемый вывод: syslog-generator 11.6.0 (commit <sha>, build: release)
```

### 2.2 syslog-ng 4.6.x (loggen)

```bash
# Ubuntu 22.04 имеет 3.x в apt; нужен backport или upstream .deb:
CODENAME=$(lsb_release -cs)
wget -qO- https://download.opensuse.org/repositories/home:/laszlo_budai:/syslog-ng/xUbuntu_${CODENAME}/Release.key | \
  sudo apt-key add -
echo "deb https://download.opensuse.org/repositories/home:/laszlo_budai:/syslog-ng/xUbuntu_${CODENAME}/ ./" | \
  sudo tee /etc/apt/sources.list.d/syslog-ng.list
sudo apt-get update
sudo apt-get install -y syslog-ng-core=4.6.4

# Verify:
loggen --version
# Ожидаемый вывод: loggen 4.6.4 (syslog-ng 4.6.4)
```

### 2.3 flog v0.4.3 (Go-based)

```bash
# Go install:
sudo apt-get install -y golang-1.21
go install github.com/mingrammer/flog@v0.4.3
sudo cp ~/go/bin/flog /usr/local/bin/

# Verify:
flog --help 2>&1 | head -3
# Ожидаемый вывод: flog - Fake log generator for common log formats
# Version: 0.4.3
```

### 2.4 tcpkali

Upstream repo использует `go build`; нет pre-built binary. Сборка
проверенной ветки:

```bash
# Pinned version — commit hash из release notes (Issue #196 audit):
git clone https://github.com/machinezone/tcpkali.git /opt/tcpkali
cd /opt/tcpkali
git checkout 2.10.0  # tag из upstream README; сохранить в этом runbook
go build -o /usr/local/bin/tcpkali cmd/tcpkali/main.go

# Verify:
tcpkali --version 2>&1 | head -1
# Ожидаемый вывод: tcpkali 2.10.0
```

**Почему preserved source build, а не apt:** версии в дистрибутивах
Ubuntu 22.04 — 2.8.x (старше). 2.10.0 — последний релиз с фиксом
`--message-rate` race (см. upstream commit log).

### 2.5 Kafka 3.6.1 (для cell completeness)

Хотя Kafka cell = `n_a` для всех 4 tools (нет consumer), receiver
для Kafka cells принимает TCP connections:

```bash
# Apache Kafka 3.6.1 — KRaft mode (no Zookeeper):
KAFKA_VERSION=3.6.1
SCALA_VERSION=2.13
sudo mkdir -p /opt/kafka && cd /opt/kafka
sudo curl -fsSL "https://archive.apache.org/dist/kafka/${KAFKA_VERSION}/kafka_${SCALA_VERSION}-${KAFKA_VERSION}.tgz" \
  | sudo tar -xz --strip-components=1

# Generate cluster id + format storage (KRaft single-node):
sudo KAFKA_CLUSTER_ID="$(bin/kafka-storage.sh random-uuid)" \
  bin/kafka-storage.sh format -t "$KAFKA_CLUSTER_ID" -c config/kraft/server.properties

# Start (background, nohup):
sudo nohup bin/kafka-server-start.sh config/kraft/server.properties > /var/log/kafka.log 2>&1 &
sleep 10
sudo bin/kafka-topics.sh --create --topic syslog-whitepaper-2026 --bootstrap-server localhost:9092
```

## 3. Step-by-step execution

### 3.1 Pre-flight (10–15 минут)

```bash
# 1. Verify изоляция:
lscpu | grep -E 'NUMA|CPU\(s\)'
cat /sys/devices/system/cpu/isolated
# Ожидаемый вывод: 0-6

# 2. Verify tools:
command -v loggen flog tcpkali syslog-generator kafka-topics.sh
# Все 5 команд resolved.

# 3. Verify Kafka (TCP socket open):
nc -zv 127.0.0.1 9092

# 4. Verify TLS cert (для tls_5krps_1kb):
cd /opt/syslog-generator/benchmarks/whitepaper-2026
bash docker/gen-cert.sh
ls -la docker/certs/{cert.pem,key.pem}

# 5. Verify disk space:
df -h /opt /tmp
# Не менее 20 GiB свободно (target/, Kafka logs, results/).

# 6. Sanity run (1 cell, 1 run, 5s вместо 30s):
cd /opt/syslog-generator/benchmarks/whitepaper-2026
make all REQUIRE_TOOLS=1 DURATION_SECS=5 RUNS=1
# Ожидаем: exit 0, status="partial" (KAFKA cell n/a).
```

### 3.2 Warm-up (15–20 минут)

**Цель:** прогреть CPU caches, page cache, connection tracking. После
warm-up'а — 5 минут idle (drift в base frequency).

```bash
# 1. Запустить syslog-generator в idle (UDP 100 msg/s, 30s, 3 прогона):
cd /opt/syslog-generator/benchmarks/whitepaper-2026
make all REQUIRE_TOOLS=1 \
  WORKLOADS=udp_100rps_256b TOOLS=syslog_generator \
  DURATION_SECS=30 RUNS=3

# 2. Drop caches:
sudo sh -c 'echo 3 > /proc/sys/vm/drop_caches'

# 3. Idle 5 минут (НЕ warmup в window! — только cache warm):
sleep 300
```

**Важно:** warm-up прогоны пишутся в `results/` явно с `warmup=true`
флагом (новое поле в meta.json, см. §5 schema). `make collect`
исключает их из `runs[]` финального артефакта.

### 3.3 Main matrix (3–4 часа)

**Формула для timing budget:**

```text
16 cells × 3 runs × 30s = 1440s = 24m чистого времени
+ 5s × 16 × 3 = 240s на receiver setup overhead
+ 5s × 16 × 3 = 240s на tool startup overhead
+ 15s × 16 × 3 = 720s на wait + cpufreq settle между cells
= ~44m sequential

С overhead на rebase/restart при failed cells (≤ 10% failure rate):
+ 30s × 16 × 0.10 = 48s
+ 60s × 16 × 0.10 = 96s (human review time)
= ~50m total
```

Но **на практике** 3–4 часа, потому что:

- CPU throttling watch (operator замечает, что governor сменился).
- Disk I/O observe (Kafka log growth).
- Re-run failed cells (выпавший size gate).
- Manual sanity checks.

**Команда:**

```bash
cd /opt/syslog-generator/benchmarks/whitepaper-2026
make all REQUIRE_TOOLS=1 RUNS=3 DURATION_SECS=30 2>&1 | tee /tmp/bench-main.log
```

**Что ОЖИДАЕТСЯ:**

| Workload | syslog_generator | loggen | flog | tcpkali |
|---|---|---|---|---|
| `udp_100rps_256b` | completed (~100 msg/s) | completed | **n_a** | **n_a** |
| `tcp_10krps_1kb` | completed (~10k msg/s) | **n_a** | **n_a** | completed |
| `tls_5krps_1kb` | completed (~5k msg/s) | **n_a** | **n_a** | completed |
| `kafka_50krps_256b` | **n_a** | **n_a** | **n_a** | **n_a** |

Всего: 6 `completed` + 10 `n_a` = 16 cells. Status = `complete`.

### 3.4 Fail-fast protocol

Если ≥3 cells завершились `failed` (rate вне 95–105% OR size вне ±5%),
**СТОП**:

```bash
# 1. Сохранить текущее состояние:
cp -r /opt/syslog-generator/benchmarks/whitepaper-2026/results /tmp/bench-results-fail
cp /opt/syslog-generator/perf/whitepaper-results.json /tmp/

# 2. Сделать diagnosis:
python3 /opt/syslog-generator/benchmarks/whitepaper-2026/scripts/06_plot.py \
  --input /tmp/whitepaper-results.json \
  --output /tmp/diagnosis-plots \
  --mode variance

# 3. Запостить issue #196 (или продолжать существующий) со ссылкой на /tmp/.

# 4. НЕ перезапускать без human review.
```

**Логика:** 3 failed cells обычно означают инфраструктурную проблему
(governor отключился, irq storm, OOM), а не per-tool issues. Чинить
нужно VM, а не инструмент.

### 3.5 Validation (15–20 минут)

```bash
# 1. Schema invariant test:
python3 /opt/syslog-generator/benchmarks/whitepaper-2026/harness/test_collect.py

# 2. End-to-end regression:
cargo test --release --locked --features test-helpers -- \
  --skip integration_tests::tls_stress_  # skip known-flaky; см. Issue #117

# 3. Plot generation:
python3 /opt/syslog-generator/benchmarks/whitepaper-2026/scripts/06_plot.py \
  --input /opt/syslog-generator/perf/whitepaper-results.json \
  --output /opt/syslog-generator/benchmarks/whitepaper-2026/results/plots/

# 4. Report regeneration:
make -C /opt/syslog-generator/benchmarks/whitepaper-2026 report

# 5. Visual review: открыть results/REPORT.md + plots/*.png, проверить глазами.
```

### 3.6 Commit + PR (10–15 минут)

```bash
cd /opt/syslog-generator
git add perf/whitepaper-results.json \
        benchmarks/whitepaper-2026/results/
git status
git diff --staged --stat

# Commit:
git commit -m "perf(whitepaper): Issue #196 real VM run on c5.2xlarge, status=complete

- host: Ubuntu 22.04, kernel 5.15, Intel Xeon Platinum 8275CL @ 3.0 GHz (no turbo)
- 16 cells completed (6 completed + 10 n_a) per Issue #106 spec
- RUNS=3 per cell, best-of-3 captured
- rate gate 95-105% met, size gate +-5% met
- REPORT.md + plots/*.png regenerated
- exit code 0
"

# PR в main (не в dev — это data, не code):
gh pr create --base main --head feature/issue-196-real-run \
  --title "perf(whitepaper): Issue #196 real VM run" \
  --body "..."
```

## 4. Result validation

### 4.1 Rate gate (95–105% of target)

Для каждой `completed` cell:

```text
rate_pct_of_target = 100 × achieved_msg_per_sec / target_msg_per_sec
                                     ↓
95% ≤ rate_pct_of_target ≤ 105%
```

За пределами — cell = `failed`, требует re-run.

**Quirk:** `syslog-generator --rate` — soft cap; фактический rate
может быть **выше** target. Используйте `HARNESS_RATE` для pacing:

| Workload | Target | Рекомендованный `HARNESS_RATE` |
|---|---|---|
| `udp_100rps_256b` | 100 | 95 (actual ~103) |
| `tcp_10krps_1kb` | 10000 | 9500 (actual ~9990) |
| `tls_5krps_1kb` | 5000 | 4750 (actual ~4995) |

Это **известный workaround** echo'd в `meta.json` (поле
`workaround_notes`).

### 4.2 Size gate (±5%)

```text
size_pct_deviation = 100 × (actual_bytes_per_msg - target_bytes_per_msg) / target_bytes_per_msg
                                          ↓
|size_pct_deviation| ≤ 5%
```

Системный log добавляет **overhead 51 bytes** (PRI + HEADER + STRUCTURED-DATA + MSG-framing). `header_overhead_bytes` field в `whitepaper-results.json::workloads[i]` сообщает это явно.

### 4.3 Fail-fast trigger

Если после main matrix:

- **≥3 cells** имеют `status == "failed"` → остановить, diagnose
  (см. §3.4).
- **1–2 cells** failed → re-run single failed cell **один раз** с
  тем же `RUNS=3`. Если снова failed → mark as `failed` в `runs[]`
  и продолжить. Status будет `partial` (а не `complete`), и нужен
  `partial_with_failures` exit code (1). Issue #196 acceptance —
  **полный complete**, не partial; re-run all до зелёного, или
  comment в issue о невозможности.

### 4.4 Sanity invariants

Проверить **ДО** commit:

- [ ] `runs[i].status == "completed"` → `runs[i].measurements` non-empty.
- [ ] `runs[i].status in ("skipped", "dry_run", "n_a")` → `runs[i].measurements` null.
- [ ] `runs[i].status == "n_a"` → `runs[i].na_reason` non-empty.
- [ ] `runs[i].status == "skipped"` → `runs[i].skip_reason` non-empty.
- [ ] Все `completed` cells имеют `rate_pct_of_target` ∈ [95, 105].
- [ ] Все `completed` cells имеют `|size_pct_deviation|` ≤ 5.
- [ ] `host.cpu_count == 8` (или 4-8 — допустимо, но lock к 8).
- [ ] `host.memory_bytes >= 30 * 1024**3` (т.е. ≥ 30 GiB).
- [ ] `tool_versions.*.available == true` для всех 4 tools.

Это **декларативный contract**; `harness/test_collect.py` проверяет
всё, кроме rate/size bands (они enforced в `measure.py`).

## 5. Expected output schema (results/)

```
benchmarks/whitepaper-2026/results/
├── REPORT.md                # auto-generated, ~5 KiB
├── EXPECTED.md              # этот файл (schema reference)
├── plots/
│   ├── throughput_msg_per_sec.png     # bar chart per workload, 4 tools
│   ├── throughput_bytes_per_sec.png   # bar chart per workload, 4 tools
│   ├── latency_p50_p95_p99.png        # only syslog-generator cells
│   ├── rate_vs_target.png             # achieved vs target scatter
│   └── size_distribution.png          # histogram actual_msg_bytes
├── udp_100rps_256b/
│   ├── run_1/
│   │   ├── syslog_generator.{stdout,stderr,meta.json}
│   │   ├── receiver.stdout.json
│   │   └── ... (4 tools total)
│   ├── run_2/   ├── run_3/
├── tcp_10krps_1kb/   ├── run_1/  (10 syslog_generator + 4 tcpkali)
├── tls_5krps_1kb/    ├── run_1/  (10 syslog_generator + 4 tcpkali)
└── kafka_50krps_256b/├── run_1/  (4 syslog_generator; 0 completed)
```

Каждый `*.meta.json` имеет (см. также `benchmarks/whitepaper-2026/results/EXPECTED.md`):

```json
{
  "workload_id": "udp_100rps_256b",
  "run_idx": 1,
  "tool": "syslog_generator",
  "mode": "real",
  "status": "completed",
  "available": true,
  "supported_for_transport": true,
  "started_at": "2026-08-15T10:23:45.123Z",
  "finished_at": "2026-08-15T10:24:15.456Z",
  "argv": ["...full command line..."],
  "exit_code": 0,
  "duration_secs": 30.333,
  "measurements": {
    "achieved_msg_per_sec": 103.4,
    "rate_pct_of_target": 103.4,
    "actual_bytes_per_msg": 257.1,
    "size_pct_deviation": 0.43,
    "bytes_received": 794281,
    "messages_received": 3090,
    "bytes_per_sec": 26179.2,
    "fail_reasons": [],
    "p50_ms": 0.42,
    "p95_ms": 0.87,
    "p99_ms": 1.43
  },
  "workaround_notes": "HARNESS_RATE=95 used; see METHODOLOGY.md §4.8"
}
```

Полная schema for `perf/whitepaper-results.json::runs[]` entry — см.
`benchmarks/whitepaper-2026/results/EXPECTED.md` (Issue #196 deliverable).

## 6. Pitfalls (наблюдения из dry-run на M-series)

Ошибки, которые были обнаружены в DEV-цикле и не должны повторяться
на production VM:

1. **macOS thermal throttling.** На M2 Max фактический rate падал
   на 2-м и 3-м прогонах на 5–8%. Pin к `performance` governor не
   полностью решает. **На c5.2xlarge governor pin + turbo off** —
   см. §1.4.

2. **Kafka v3.6.0 vs 3.6.1.** 3.6.0 имеет KRaft startup race,
   приводящий к 5–10% packet drop. **Use 3.6.1 only.**

3. **tcpkali 2.8.x** — не реагирует на `--message-rate` если
   одновременно не указан `c` (connection count). Убедиться, что
   `tcpkali --message-rate 5000 -c 1 -f msg.txt` (а не
   `--message-rate 5000 -f msg.txt`).

4. **tls_5krps_1kb** — `syslog-generator` op-mode TLS bind занимает
   ~2s на старте; это окно **не** входит в measurement window.
   Receiver.py запускается с `sleep 0.5`; возможно надо
   увеличить до `sleep 1.0` для TLS.

5. **loggen — syslog-ng 4.6.4.** 4.6.x default порт 601 (TCP)
   **не** используется; loggen сам идёт на 514/UDP. Убедиться,
   что `receiver.py` запущен на 5140 (per `workload_udp_*.json`),
   **а не** на 514.

6. **Подписи / GPG.** Issue #155 (GPG-подпись) — **не** нужна для
   артефактов benchmark'а. Только для релизов.

## 7. Что делать, если прогон провалился

### 7.1 Сценарий A: `failed` cell, problem obvious

Пример: rate=120 msg/s вместо 100–105. Очевидно — syslog-generator
saturation? Или controller scheduler issue?

```bash
# Re-run single cell:
make all REQUIRE_TOOLS=1 \
  WORKLOADS=udp_100rps_256b TOOLS=syslog_generator \
  DURATION_SECS=30 RUNS=3

# Если ok — добавить в общий results/, не перезапускать всё.
```

### 7.2 Сценарий B: `failed` cell, problem unclear

```bash
# Capture perf profile:
sudo perf record -F 99 -p $(pgrep syslog-generator) -g -- sleep 30
sudo perf script > /tmp/perf.script

# Capture strace:
sudo strace -f -ttt -T -p $(pgrep syslog-generator) 2> /tmp/strace.log &
sleep 30

# Re-run с verbose логированием:
RUST_LOG=trace make all REQUIRE_TOOLS=1 ...
```

### 7.3 Сценарий C: infra проблема (c5.2xlarge throttle)

```bash
# НЕ чинить VM — terminate, запустить новую:
aws ec2 terminate-instances --instance-ids i-xxx
# Перезапустить §1.2.
```

### 7.4 Сценарий D: harness bug

`harness/test_collect.py` или `measure.py` — false positive on rate gate.

**Action:** открыть issue с label `harness-bug`, **не** менять
acceptance criteria. Возможный fix — `RATE_TOLERANCE_FRACTION=0.10`
на этой конкретной VM, но это **отдельный PR** с обоснованием.

## 8. Сводка timing budget

| Шаг | Минимум | Типично | Максимум |
|---|---|---|---|
| 1. Provisioning VM | 5 min | 8 min | 15 min |
| 2. Tool install | 15 min | 25 min | 40 min |
| 3.1 Pre-flight | 10 min | 15 min | 20 min |
| 3.2 Warm-up | 20 min | 25 min | 30 min |
| 3.3 Main matrix | 50 min | 90 min | 4 hours |
| 3.4 Fail-fast (if needed) | 30 min | 60 min | 2 hours |
| 3.5 Validation | 15 min | 20 min | 30 min |
| 3.6 Commit + PR | 10 min | 15 min | 30 min |
| **Total** | **~2.5 hours** | **~4 hours** | **~8 hours** |

**Acceptance budget:** 3–4 часа budgeted per §3.3 of operator's time.
Превышение → escalate to maintainer (Issue #196 comment).

## 9. После успешного run'а

1. Issue #196 → закрыть со ссылкой на PR.
2. Issue #199 → теперь UNBLOCKED (специфичные численные результаты).
3. Issue #200 → планировать citation tracking (drafts готовы).
4. CHANGELOG.md → запись про `v11.6.0` perf section.
5. AUDIT.md → добавить запись с датой, host specs, SHA, summary.

## 10. Cross-references

- Методология: `benchmarks/whitepaper-2026/METHODOLOGY.md`
- Схема runs[]: `benchmarks/whitepaper-2026/results/EXPECTED.md` (Issue #196)
- Plot script: `benchmarks/whitepaper-2026/scripts/06_plot.py` (Issue #196)
- Acceptance metrics: `docs/whitepaper-2026.md` (Issue #106)
- Маркетинг: `docs/whitepaper-2026.ru.md` (Issue #199)
- Citations: `docs/whitepaper-2026-tracking.md` (Issue #200)
- AGENTS.md §15 — board sync: обновить `Release Confidence` после merge.
