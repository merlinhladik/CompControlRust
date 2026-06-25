-- SPDX-License-Identifier: GPL-3.0-or-later
-- CCR schema baseline. Idempotent (CREATE TABLE IF NOT EXISTS): a no-op on the
-- existing edv-owned database, the full schema on a fresh one. This is the
-- schema-ownership cutover — from here CCR's migrations/ is canonical and edv's
-- Alembic freezes. Mirrors edv/backend/data/models.py + the live DDL exactly;
-- the two named constraints (uix_participant_identity, uix_age_class_lock_scope)
-- match the ON CONFLICT targets in ccr-db.

CREATE TABLE IF NOT EXISTS participants (
    id           SERIAL PRIMARY KEY,
    first_name   varchar(100) NOT NULL,
    last_name    varchar(100) NOT NULL,
    gender       varchar(1),
    birth_date   date,
    weight       numeric(5,2),
    club         varchar(200),
    association  varchar(200),
    valid        boolean,
    paid         boolean,
    doublestart  varchar(10),
    CONSTRAINT uix_participant_identity UNIQUE (first_name, last_name, gender, birth_date, club)
);

CREATE TABLE IF NOT EXISTS groups (
    id           SERIAL PRIMARY KEY,
    name         varchar(100) NOT NULL UNIQUE,
    gender       varchar(10),
    age_group    varchar(20),
    weight_class varchar(20)
);

CREATE TABLE IF NOT EXISTS mats (
    id         SERIAL PRIMARY KEY,
    mat_number integer NOT NULL
);

CREATE TABLE IF NOT EXISTS members (
    id         SERIAL PRIMARY KEY,
    last_name  varchar(50) NOT NULL,
    first_name varchar(50) NOT NULL,
    birth_date date NOT NULL,
    club       varchar(100) NOT NULL,
    gender     char(1),
    weight     numeric(5,2),
    valid      boolean DEFAULT false,
    paid       boolean DEFAULT false,
    CONSTRAINT members_gender_check CHECK (gender = ANY (ARRAY['m'::bpchar, 'w'::bpchar]))
);

CREATE TABLE IF NOT EXISTS age_class_locks (
    id         SERIAL PRIMARY KEY,
    scope_key  varchar(30) NOT NULL,
    age_group  varchar(20) NOT NULL,
    gender     varchar(10),
    locked_at  timestamp NOT NULL,
    reason     varchar(200),
    CONSTRAINT uix_age_class_lock_scope UNIQUE (scope_key)
);

-- Legacy table carried verbatim (not used by CCR; kept for schema fidelity).
CREATE TABLE IF NOT EXISTS matches (
    match_id      SERIAL PRIMARY KEY,
    table_id      varchar,
    fight_nr      integer,
    category      varchar,
    bracket_file  varchar,
    round         integer,
    pos_in_round  integer,
    p1            json,
    p2            json,
    status        varchar,
    "order"       integer,
    rest_time_min integer,
    next_match_id integer,
    next_match_pos varchar
);

CREATE TABLE IF NOT EXISTS group_participants (
    id             SERIAL PRIMARY KEY,
    group_id       integer NOT NULL REFERENCES groups(id),
    participant_id integer NOT NULL REFERENCES participants(id)
);

CREATE TABLE IF NOT EXISTS brackets (
    id            SERIAL PRIMARY KEY,
    group_id      integer NOT NULL REFERENCES groups(id),
    mat_id        integer REFERENCES mats(id),
    bracket_type  varchar(20),
    status        varchar(20),
    first_place   integer REFERENCES group_participants(id),
    second_place  integer REFERENCES group_participants(id),
    third_place_1 integer REFERENCES group_participants(id),
    third_place_2 integer REFERENCES group_participants(id)
);

CREATE TABLE IF NOT EXISTS fights (
    id              SERIAL PRIMARY KEY,
    bracket_id      integer NOT NULL REFERENCES brackets(id),
    participant1_id integer REFERENCES group_participants(id),
    participant2_id integer REFERENCES group_participants(id),
    fight_number    integer,
    score1          integer,
    score2          integer,
    duration        integer,
    status          varchar(20),
    bracket_phase   varchar(10) NOT NULL,
    round           integer,
    pos_in_round    integer,
    pool_index      integer,
    table_id        integer,
    winner_id       integer REFERENCES group_participants(id),
    CONSTRAINT uix_fight_position UNIQUE (bracket_id, bracket_phase, round, pos_in_round)
);
