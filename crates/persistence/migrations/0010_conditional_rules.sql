-- Per-poster conditional tag rules, stored in their textual syntax:
-- "[solo]->[-male] [duo]->[female]".
ALTER TABLE posters ADD COLUMN conditional_rules TEXT NOT NULL DEFAULT '';
