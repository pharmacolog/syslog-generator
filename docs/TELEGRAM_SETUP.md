# Telegram notifications setup (syslog-generator CI)

Опциональная интеграция для получения уведомлений в Telegram когда CI упал.

## 1. Создать Telegram бота

1. Открыть [@BotFather](https://t.me/BotFather) в Telegram.
2. Отправить `/newbot`.
3. Следовать инструкциям (имя, username).
4. Скопировать **bot token** (формат: `123456789:ABCdefGHI...`).

## 2. Получить chat_id

1. Добавить бота в чат/канал (для канала — сделать админом).
2. Открыть [@userinfobot](https://t.me/userinfobot) в чате ИЛИ переслать
   любое сообщение из чата боту [@RawDataBot](https://t.me/RawDataBot).
3. Скопировать **chat id**:
   - Для личного чата / группы: отрицательное число (например, `-1001234567890`).
   - Для канала: начинается с `-100` (например, `-1001234567890`).
4. Для топика в супергруппе — дополнительно нужен **message_thread_id** (число).

## 3. Добавить secrets в GitHub repo

Settings → Secrets and variables → Actions → New repository secret:

| Name | Value |
|------|-------|
| `TELEGRAM_BOT_TOKEN` | bot token из шага 1 |
| `TELEGRAM_CHAT_ID` | chat id из шага 2 |
| `TELEGRAM_THREAD_ID` | (опционально) thread id для топика |

## 4. Тестирование

После push в `main` / `dev` / `release/*`, CI упадёт (можно временно сломать
что-то), и сообщение должно прийти в Telegram в течение ~1 минуты.

Если не приходит — проверьте:
- Бот добавлен в чат (для канала — админ).
- `chat_id` правильный (отрицательный для групп/каналов).
- Secrets сохранены в GitHub repo.

## Безопасность

- **НЕ коммитьте токен в код.** Только через GitHub Secrets.
- Токен даёт полный доступ к боту — revoke через @BotFather если утечёт.
- Рекомендуется использовать **приватный канал** (не публичный чат).