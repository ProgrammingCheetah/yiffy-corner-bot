-- Moderation audit: who acted on a post and when (approve/reject/save/takedown).
ALTER TABLE posts ADD COLUMN moderated_by INTEGER REFERENCES users (id);
ALTER TABLE posts ADD COLUMN moderated_at TEXT;
