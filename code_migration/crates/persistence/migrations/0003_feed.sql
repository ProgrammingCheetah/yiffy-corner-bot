-- Feed model (2026-07-05): one curated feed, per-consumer cursors.
--
-- Posts gain curated tags (space-joined; e621 tags at submission or
-- submitter-provided) and a monotonic feed_position assigned when accepted
-- into the feed. Posters gain the consumer cursor.

ALTER TABLE posts ADD COLUMN tags TEXT NOT NULL DEFAULT '';
ALTER TABLE posts ADD COLUMN feed_position INTEGER;
CREATE UNIQUE INDEX posts_feed_position ON posts (feed_position)
    WHERE feed_position IS NOT NULL;

ALTER TABLE posters ADD COLUMN cursor INTEGER NOT NULL DEFAULT 0;

-- Backfill: posts accepted under the old pool model enter the feed in id
-- order so existing deployments keep publishing without re-curation.
UPDATE posts
SET feed_position = (
    SELECT COUNT(*) FROM posts AS earlier
    WHERE earlier.status = 'accepted' AND earlier.id <= posts.id
)
WHERE status = 'accepted' AND feed_position IS NULL;
