-- Initial schema. Mirrors the domain entities 1:1; tags are never persisted
-- for posts (the bot is an indexer over e621 — tags are fetched fresh).

CREATE TABLE users (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    telegram_id  INTEGER NOT NULL UNIQUE,
    role         TEXT    NOT NULL,
    added_by     INTEGER REFERENCES users (id),
    display_name TEXT,
    is_banned    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE posts (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Canonical source URL; sources are value objects and unique per post.
    source_url   TEXT    NOT NULL UNIQUE,
    status       TEXT    NOT NULL,
    last_posted  TEXT,
    submitted_by INTEGER REFERENCES users (id),
    submitted_at TEXT    NOT NULL
);

CREATE INDEX posts_status_submitted_at ON posts (status, submitted_at);

CREATE TABLE posters (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Space-joined tag lists (e621 tags cannot contain spaces).
    subscribed_tags TEXT    NOT NULL,
    forbidden_tags  TEXT    NOT NULL,
    -- Minutes; a divisor of 60 (validated in the domain).
    time_interval   INTEGER NOT NULL
);

CREATE TABLE publisher_configs (
    -- 1:1 with posters: the poster id IS the key.
    poster_id  INTEGER PRIMARY KEY REFERENCES posters (id),
    chat_id    INTEGER NOT NULL,
    token_path TEXT    NOT NULL
);

CREATE TABLE forbidden_tags (
    tag TEXT PRIMARY KEY
);

CREATE TABLE required_tags (
    tag TEXT PRIMARY KEY
);
