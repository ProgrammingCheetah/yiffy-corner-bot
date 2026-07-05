-- Announcement cycle: singleton settings row (id constrained to 1).
CREATE TABLE announcement_settings (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    interval_hours    INTEGER NOT NULL DEFAULT 0,
    spotlight_chat_id INTEGER,
    last_announced_at TEXT
);
INSERT INTO announcement_settings (id) VALUES (1);
