-- Credited artists per post (e621 artist bucket minus non-artist markers),
-- space-joined like tags. Empty for sources without artist metadata.
ALTER TABLE posts ADD COLUMN artists TEXT NOT NULL DEFAULT '';
