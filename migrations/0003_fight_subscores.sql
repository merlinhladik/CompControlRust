-- Per-fighter sub-scores (Ippon / Waza-ari / Yuko / Shido). Storage for native
-- CCR scoring (entered over WS). U9/U11 derive the
-- JVP-Additiv-20 total from these (Ippon 10 / Waza-ari 5 / Yuko 3 / Shido +2 to
-- opponent); higher age classes only display them on fight click. Additive,
-- DEFAULT 0 ⇒ existing rows + edv reads are unaffected.
ALTER TABLE fights ADD COLUMN IF NOT EXISTS ippon1  INTEGER NOT NULL DEFAULT 0;
ALTER TABLE fights ADD COLUMN IF NOT EXISTS wazari1 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE fights ADD COLUMN IF NOT EXISTS yuko1   INTEGER NOT NULL DEFAULT 0;
ALTER TABLE fights ADD COLUMN IF NOT EXISTS shido1  INTEGER NOT NULL DEFAULT 0;
ALTER TABLE fights ADD COLUMN IF NOT EXISTS ippon2  INTEGER NOT NULL DEFAULT 0;
ALTER TABLE fights ADD COLUMN IF NOT EXISTS wazari2 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE fights ADD COLUMN IF NOT EXISTS yuko2   INTEGER NOT NULL DEFAULT 0;
ALTER TABLE fights ADD COLUMN IF NOT EXISTS shido2  INTEGER NOT NULL DEFAULT 0;
