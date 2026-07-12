# CLI quickstart (F11) и валидация (F13)

Примеры запуска генератора из командной строки — как из JSON-профиля, так и
целиком из флагов (быстрый режим). Флаги-оверрайды применяются к профилю
**перед валидацией и запуском**.

## Быстрый режим (без файла-профиля)

Одна фаза собирается из `--target` и `--message`:

```bash
# 100 сообщений на UDP-таргет, детерминированно по seed
syslog-generator -t 127.0.0.1:514:udp -m 'evt {{sequence}}' --total 100 --seed 42

# Запись в файл (транспорт file: адрес = путь к файлу)
syslog-generator -t /tmp/out.log:file -m 'line {{sequence}}' --total 10 --format raw

# Несколько шаблонов (равновероятный выбор) на два таргета (broadcast)
syslog-generator \
  -t 10.0.0.1:514:tcp -t 10.0.0.2:514:tcp \
  --distribution broadcast \
  -m 'login user={{faker.username}}' -m 'logout user={{faker.username}}' \
  --rate 200 --duration 30
```

## Профиль из файла + оверрайды

Оверрайды скаляров (`--rate/--duration/--total/--format/--seed`) применяются ко
**всем** фазам профиля; `--target`/`--distribution` заменяют соответствующие
поля профиля:

```bash
# Взять профиль, но прогнать быстрее и дольше
syslog-generator --profile examples/single_target.json --rate 500 --duration 60

# Перенаправить весь трафик профиля в файл (удобно для отладки пейлоада)
syslog-generator --profile examples/multi_target_weighted.json -t /tmp/debug.log:file
```

## Проверка без запуска

```bash
# Только валидация (dry-run): код возврата 0 = валиден, 1 = невалиден
syslog-generator --profile examples/rfc5424_tcp.json --validate

# Показать итоговый профиль (после оверрайдов) как JSON
syslog-generator --profile examples/single_target.json --rate 500 --print-config
```

## Форма спецификации `--target`

`ADDR` или `ADDR:TRANSPORT`, где TRANSPORT ∈ `tcp | udp | tls | file`
(по умолчанию `tcp`). Для `file` адрес — это путь к файлу.

| Ввод | address | transport |
|------|---------|-----------|
| `127.0.0.1:514` | `127.0.0.1:514` | `tcp` |
| `127.0.0.1:514:udp` | `127.0.0.1:514` | `udp` |
| `10.0.0.1:6514:tls` | `10.0.0.1:6514` | `tls` |
| `/var/log/out.log:file` | `/var/log/out.log` | `file` |

## Коды возврата

- `0` — успешный запуск или валидный профиль (`--validate`);
- `1` — ошибка чтения/парсинга профиля, ошибка `--target`, либо профиль
  невалиден (список проблем печатается в stderr).
