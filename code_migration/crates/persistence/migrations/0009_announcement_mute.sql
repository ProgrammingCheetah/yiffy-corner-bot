-- Per-channel announcement mute: the chat still APPEARS in the directory,
-- it just doesn't receive the broadcast.
ALTER TABLE publisher_configs ADD COLUMN receive_announcements INTEGER NOT NULL DEFAULT 1;
