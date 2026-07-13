-- "Fulfilling request": while a curator's toggle is ON, every post they
-- save from browse is stamped with the request text, and its publication
-- caption reads "Fulfilling request <text>". Per-curator, survives restarts.

ALTER TABLE posts ADD COLUMN fulfills TEXT;

CREATE TABLE fulfilling_requests (
    telegram_id INTEGER PRIMARY KEY,
    request     TEXT    NOT NULL,
    since       TEXT    NOT NULL
);
