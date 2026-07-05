-- Report loop: viewer reports + the publication audit trail behind takedowns.

CREATE TABLE reports (
    post_id              INTEGER NOT NULL REFERENCES posts (id),
    -- Raw Telegram id: reporters don't need to be registered Users.
    reporter_telegram_id INTEGER NOT NULL,
    reported_at          TEXT    NOT NULL,
    -- Abuse prevention (MVP): one report per (post, reporter).
    PRIMARY KEY (post_id, reporter_telegram_id)
);

CREATE TABLE publications (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    post_id      INTEGER NOT NULL REFERENCES posts (id),
    chat_id      INTEGER NOT NULL,
    message_id   INTEGER NOT NULL,
    published_at TEXT    NOT NULL
);

CREATE INDEX publications_post ON publications (post_id);
