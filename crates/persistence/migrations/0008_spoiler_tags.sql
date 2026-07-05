-- Content-warning tags: posts owning one publish with the media spoilered.
CREATE TABLE spoiler_tags (
    tag TEXT PRIMARY KEY
);
-- Seed with the initially requested hard-kink warnings.
INSERT INTO spoiler_tags (tag) VALUES ('watersports'), ('questionable_consent');
