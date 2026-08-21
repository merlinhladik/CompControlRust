-- SPDX-License-Identifier: GPL-3.0-or-later
-- Club master data: feeds the fighter-editor dropdown, editable on its own.
-- ponytail: participants.club stays denormalized text (contestants JSON/CSV wire
-- format + all exports use the name); a club rename cascades via UPDATE. Switch
-- to a club_id FK only if name-cascading ever proves too fragile.
CREATE TABLE IF NOT EXISTS clubs (
    id          SERIAL PRIMARY KEY,
    name        varchar(100) NOT NULL UNIQUE,
    association varchar(100)
);

-- Seed from whatever fighters already exist.
INSERT INTO clubs (name, association)
SELECT DISTINCT ON (club) club, NULLIF(association, '')
FROM participants
WHERE club IS NOT NULL AND club <> ''
ORDER BY club, association
ON CONFLICT (name) DO NOTHING;
