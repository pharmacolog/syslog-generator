# Roadmap: syslog-generator

> **Версия документа**: 2026-07-23 (синхронизировано с GitHub Project V2 #1)
> **Покрывает**: релизы v11.0 → v13.0 (Вехи G и H + GTM)

## Где актуальный roadmap

- **GitHub Project V2**: https://github.com/users/pharmacolog/projects/1
- **GitHub Milestones**: https://github.com/pharmacolog/syslog-generator/milestones
- **GitHub Issues**: https://github.com/pharmacolog/syslog-generator/issues

Project V2 содержит все 27 issues с полями:
- `Status` (Backlog / Ready / In Progress / In Review / Blocked / Done)
- `Priority` (P0 / P1 / P2 / P3)
- `Track` (track-a / track-b / track-c / track-h / track-gtm)
- `Effort` (XS / S / M / L / XL)
- `Risk` (Low / Medium / High)
- `Target Date`

## Структура roadmap

### Веха G — «High-throughput UX-rich CLI» (v11.0 → v11.7, 6-9 мес)

| Релиз | Scope | Issues | Трудозатраты |
|---|---|---|---|
| **v11.0** | A0 baseline + A1 quick wins | #85, #87 | 4-6 нед |
| **v11.1** | A2 CompiledPlan (⚠️ breaking) | #88 | 8-12 нед |
| **v11.2** | A4 concurrent + A5 transport | #86, #82 | 6-10 нед |
| **v11.3** | A3 delivery policies | #89 | 3-4 нед |
| **v11.4** | B1 CLI overrides + B3 presets + GTM-2 .deb/.rpm | #93, #92, #107 | 3-4 нед |
| **v11.5** | B2 --set paths + env bindings | #83 | 2-3 нед |
| **v11.6** | A6 perf gates + C1 CI + C3 tests + GTM-1 whitepaper | #84, #90, #81, #106 | 4-6 нед |
| **v11.7** | C2 docs refresh | #91 | 2-3 нед |

### Веха H — «Enterprise Observability Platform» (v12.0 → v12.3, 9-18 мес)

| Релиз | Scope | Issues | Трудозатраты |
|---|---|---|---|
| **v12.0** | H1 Distributed mode (F18) + gRPC transport (F19) | #95, #96 | 12-16 нед |
| **v12.1** | H2 Hot-reload (F20) + OTel (F21) + Self-monitoring (F9-bis) | #97, #98, #99 | 6-8 нед |
| **v12.2** | H3 SLA assertions (F22) + Anomaly library (F23) | #100, #101 | 4-6 нед |
| **v12.3** | H4 Replay (F24) + SIEM examples (F25) + GTM-3 partnerships | #102, #103, #108 | 6-8 нед |

### Веха I — «Enterprise SaaS-ready» (v13.0, 18-30 мес)

| Релиз | Scope | Issues | Трудозатраты |
|---|---|---|---|
| **v13.0** | H5 Web UI (F26) + WASM plugins (F27) | #104, #105 | 8-12 нед |

## Critical path

```
v11.0 (#85, #87 baseline + quick wins)
   ↓
v11.1 (#88 CompiledPlan foundation)
   ↓
v11.2 (#86 concurrent pipeline + #82 transport opt) [parallel после #88]
   ↓
v11.6 (#84 perf governance gate — закрепляет результаты)
```

## Параллельные branches

| Branch | Issues | Фокус |
|---|---|---|
| **Branch-α** (perf) | #85, #87, #88, #86, #82, #84, #89 | Critical path + extensions |
| **Branch-β** (UX) | #93, #92, #83 | CLI overrides, presets, --set paths |
| **Branch-γ** (Hardening) | #90, #81, #91 | CI, tests, docs |
| **Branch-δ** (Enterprise H1-H2) | #95, #96, #97, #98, #99 | Distributed, gRPC, OTel, self-mon, hot-reload |
| **Branch-ε** (Enterprise H3-H5) | #100, #101, #102, #103, #104, #105 | SLA, anomalies, replay, SIEM, UI, plugins |
| **Branch-ζ** (GTM) | #106, #107, #108 | Whitepaper, .deb/.rpm, SIEM partnerships |

## Конфликты merge

| Файл | Конкуренты | Sequencing |
|---|---|---|
| `src/cli.rs` | #93, #92, #83, #89 | #93 → #92 → #83 → #89 |
| `src/generator/core.rs` | #88, #86, #89, #85 | #85 → #88 → #86/#89 |
| `src/transport/` | #82, #89 | Параллельно OK (разные файлы) |
| `Cargo.toml` | #88, #86, #90 | Строго по одному merge |

## Breaking changes

| Релиз | Issue | Breaking |
|---|---|---|
| **v11.1** | #88 CompiledPlan | Deprecated старые API (полное удаление в v12.0) |
| **v12.0** | (cleanup) | Удаление всех deprecated из v11.1 |

## Приоритеты

### P0 (критические)
- #85, #87 (v11.0)
- #88 (v11.1, ⚠️ breaking)
- #86 (v11.2)

### P1 (высокие)
- #82, #89 (v11.2-v11.3)
- #93, #92, #107 (v11.4)
- #84 (v11.6)
- #95 (v12.0)
- #99 (v12.1)
- #103 (v12.2)
- #104 (v13.0)

### P2 (средние)
- #83 (v11.5)
- #90, #81, #106 (v11.6)
- #96, #97 (v12.1)
- #100, #101, #102, #105 (v12.2-v12.3)

### P3 (низкие)
- #91 (v11.7)
- #98 (v12.1)
- #107 (v11.4 GTM-2)
- #108 (v12.3 GTM-3)

## Maintainer strategy

- **CODEOWNERS** для критичных модулей (`src/plan/`, `src/format/`, `src/transport/`)
- **"good first issue"** labeling для новых контрибьюторов
- **Contributor ladder**: contributor → maintainer → co-maintainer → release captain
- **Quarterly roadmap sync** через GitHub Discussions
- **GTM**: Linux-пакеты (.deb/.rpm) + SIEM vendor partnerships + Whitepaper (RU+EN)

## Метрики успеха

| KPI | Baseline | Цель Q+12 | Цель Q+18 |
|---|---|---|---|
| Throughput на 8 cores | 100k msg/s | ≥400k | ≥1M |
| Coverage | ~94% | ≥97% | ≥98% |
| Docker pulls/month | Текущее | ×3 | ×10 |
| Stars | Текущее | +500 | +1500 |
| .deb/.rpm downloads | 0 | 1000 | 5000 |
| Active contributors | 1 | 3-5 | 8-12 |

## Связанные документы

- `PLAN-v10.0.0.md` — оригинальный план (вехи A-F closed)
- `AUDIT.md` — детальный аудит с базиса v7.4.0
- `CLAUDE_HANDOFF.md` — перенос контекста для AI-агентов
- `CHANGELOG.md` — история всех релизов
- `docs/USER_GUIDE.md` — руководство пользователя
- `docs/DEVELOPER_GUIDE.md` — руководство разработчика
- `docs/PERFORMANCE.md` — оптимизации производительности

---

**Последнее обновление**: 2026-07-23 (Project V2 #1 создан)
