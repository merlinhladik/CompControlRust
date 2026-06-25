// SPDX-License-Identifier: GPL-3.0-or-later
//! Row structs mirroring the edv-owned Postgres schema 1:1.
//!
//! Source of truth: edv/backend/data/models.py (+ edv/alembic). CCR is a SECOND
//! reader/writer during the strangler migration — it does NOT own this schema
//! until Phase 5. Keep column names, nullability and types in lockstep with edv.
//!
//! Type mapping (Postgres -> Rust):
//!   INTEGER / SERIAL  -> i32
//!   VARCHAR / CHAR    -> String
//!   NUMERIC(5,2)      -> rust_decimal::Decimal
//!   DATE              -> chrono::NaiveDate
//!   TIMESTAMP         -> chrono::NaiveDateTime
//!   BOOLEAN           -> bool
//! Nullable columns are Option<T>.

use chrono::{NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// `participants` — the athlete identity (edv is schema owner; weighed-in data).
#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct Participant {
    pub id: i32,
    pub first_name: String,
    pub last_name: String,
    pub gender: Option<String>,       // 'm' | 'w'
    pub birth_date: Option<NaiveDate>,
    pub weight: Option<Decimal>,      // NUMERIC(5,2)
    pub club: Option<String>,
    pub association: Option<String>,
    pub valid: Option<bool>,
    pub paid: Option<bool>,
    pub doublestart: Option<String>,  // 'nein' | 'ja' | 'höher' (String(10))
}

/// `groups` — a (gender, age, weight) bracket bucket, or 'QUARANTINE'.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct Group {
    pub id: i32,
    pub name: String,                 // 'm | U15 | -66kg' | 'QUARANTINE'
    pub gender: Option<String>,
    pub age_group: Option<String>,
    pub weight_class: Option<String>,
}

/// `age_class_locks` — edv-only gate (live path is lock-independent; see CLAUDE.md).
#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct AgeClassLock {
    pub id: i32,
    pub scope_key: String,
    pub age_group: String,
    pub gender: Option<String>,
    pub locked_at: NaiveDateTime,
    pub reason: Option<String>,
}

/// `group_participants` — membership of a participant in a group/bracket.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct GroupParticipant {
    pub id: i32,
    pub group_id: i32,
    pub participant_id: i32,
}

/// `mats` — physical mat numbers.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct Mat {
    pub id: i32,
    pub mat_number: i32,
}

/// `brackets` — one bracket per group; placements set when status='completed'.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct Bracket {
    pub id: i32,
    pub group_id: i32,
    pub mat_id: Option<i32>,
    pub bracket_type: Option<String>, // pools | double | ko | repechage | special
    pub status: Option<String>,       // pending | in_progress | completed
    pub first_place: Option<i32>,
    pub second_place: Option<i32>,
    pub third_place_1: Option<i32>,
    pub third_place_2: Option<i32>,
}

/// `fights` — the unit JF scores live. Column-type invariants per CLAUDE.md:
/// score1/score2/duration/table_id are INTEGER NULL; status is VARCHAR(20).
#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct Fight {
    pub id: i32,
    pub bracket_id: i32,
    pub participant1_id: Option<i32>,
    pub participant2_id: Option<i32>,
    pub fight_number: Option<i32>,
    pub score1: Option<i32>,
    pub score2: Option<i32>,
    pub duration: Option<i32>,
    pub status: Option<String>,       // pending | finished | bye
    pub bracket_phase: String,        // pool | wb | lb  (NOT NULL, default 'wb')
    pub round: Option<i32>,
    pub pos_in_round: Option<i32>,
    pub pool_index: Option<i32>,
    pub table_id: Option<i32>,
    pub winner_id: Option<i32>,
    // Per-fighter sub-scores (Ippon/Waza-ari/Yuko/Shido). `#[sqlx(default)]` so the
    // few queries that don't list them still map; FIGHT_COLS selects them everywhere.
    #[sqlx(default)]
    pub ippon1: i32,
    #[sqlx(default)]
    pub wazari1: i32,
    #[sqlx(default)]
    pub yuko1: i32,
    #[sqlx(default)]
    pub shido1: i32,
    #[sqlx(default)]
    pub ippon2: i32,
    #[sqlx(default)]
    pub wazari2: i32,
    #[sqlx(default)]
    pub yuko2: i32,
    #[sqlx(default)]
    pub shido2: i32,
}

impl Fight {
    /// fighter 1's sub-scores as the domain type.
    pub fn sub1(&self) -> ccr_domain::jvp::SubScores {
        ccr_domain::jvp::SubScores::new(self.ippon1, self.wazari1, self.yuko1, self.shido1)
    }
    /// fighter 2's sub-scores as the domain type.
    pub fn sub2(&self) -> ccr_domain::jvp::SubScores {
        ccr_domain::jvp::SubScores::new(self.ippon2, self.wazari2, self.yuko2, self.shido2)
    }
}
