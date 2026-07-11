-- Forbidden tags can say WHY they're banned; the reason is surfaced
-- wherever the ban bites (auto-ban replies, save refusals, the admin UI).

ALTER TABLE forbidden_tags ADD COLUMN reason TEXT;
