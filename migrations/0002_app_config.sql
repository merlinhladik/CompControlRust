-- SPDX-License-Identifier: GPL-3.0-or-later
-- CCR-owned, UI-editable tournament configuration (single JSONB row): generation
-- thresholds, youth pool size, age classes + birth-year eligibility (incl.
-- doublestart overlaps), weight classes, event year. Seeded once from
-- bracket_config.xlsx + sensible defaults, then edited via /api/config.
CREATE TABLE IF NOT EXISTS app_config (
    id   integer PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    data jsonb NOT NULL
);
