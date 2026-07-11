-- Attribution is a contact, not just a name: reports remember the
-- reporter's @username (when they have one) so moderators can reach out.

ALTER TABLE reports ADD COLUMN reporter_username TEXT;
