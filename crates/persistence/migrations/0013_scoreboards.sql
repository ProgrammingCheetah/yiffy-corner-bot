-- Scoreboard cycle: singleton settings row (id constrained to 1), mirroring
-- announcement_settings. interval_hours 0 = disabled.
CREATE TABLE scoreboard_settings (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    interval_hours INTEGER NOT NULL DEFAULT 0,
    last_posted_at TEXT
);
INSERT INTO scoreboard_settings (id) VALUES (1);
