-- Shadowbans: silently dropped reports/wishes/submissions. Keyed by raw
-- Telegram id — reporters don't need to be registered Users.

CREATE TABLE shadow_bans (
    telegram_id INTEGER PRIMARY KEY,
    banned_by   INTEGER NOT NULL,
    banned_at   TEXT    NOT NULL
);
