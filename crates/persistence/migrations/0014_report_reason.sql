-- Reports carry the reporter's reason so moderators know why a post was
-- flagged. Nullable: legacy report buttons can't always collect one, and
-- pre-existing rows never had one.

ALTER TABLE reports ADD COLUMN reason TEXT;
