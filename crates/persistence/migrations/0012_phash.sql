-- Perceptual hash (64-bit dHash) of the resolved media, for duplicate
-- resistance beyond exact source-URL matching. NULL = not computed yet or
-- media is not a still image. Stored as the i64 bit pattern.
ALTER TABLE posts ADD COLUMN phash INTEGER;
