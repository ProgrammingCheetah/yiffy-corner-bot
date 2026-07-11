-- Browse skiplist: sources a moderator waved off. Dedupe can't catch a
-- video re-upload (no pHash for videos), so the verdict is remembered and
-- browse never shows the source again.

CREATE TABLE browse_skips (
    source     TEXT    PRIMARY KEY,
    skipped_by INTEGER NOT NULL,
    skipped_at TEXT    NOT NULL
);
