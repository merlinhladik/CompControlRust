// SPDX-License-Identifier: GPL-3.0-or-later
//! Resolve bracket_id → group attributes (Phase 1) + pool finalize (Phase 4).
//! Mirrors JF main.py `_resolve_groups`, `_finalize_pool_bracket_if_complete`,
//! `_best_of_three_decided`.

use std::collections::{BTreeSet, HashMap};

use serde::Serialize;
use sqlx::PgPool;

use crate::fights;
use crate::models::Bracket;

/// Group attributes carried on a bracket, as JF's match dict needs them.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ResolvedGroup {
    pub bracket_id: i32,
    pub gender: Option<String>,
    pub age_group: Option<String>,
    pub weight_class: Option<String>,
    pub bracket_type: Option<String>,
}

/// Batch-resolve a set of bracket ids to their group attributes.
pub async fn resolve(
    pool: &PgPool,
    bracket_ids: &[i32],
) -> Result<Vec<ResolvedGroup>, sqlx::Error> {
    if bracket_ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as::<_, ResolvedGroup>(
        "SELECT b.id AS bracket_id, g.gender, g.age_group, g.weight_class, \
                b.bracket_type \
         FROM brackets b \
         JOIN groups g ON g.id = b.group_id \
         WHERE b.id = ANY($1)",
    )
    .bind(bracket_ids)
    .fetch_all(pool)
    .await
}

const BRACKET_COLS: &str = "id, group_id, mat_id, bracket_type, status, \
     first_place, second_place, third_place_1, third_place_2";

/// group_participants ids belonging to a bracket's group (generation input).
pub async fn group_participant_ids(pool: &PgPool, bracket_id: i32) -> Result<Vec<i32>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT gp.id FROM group_participants gp \
         JOIN brackets b ON b.group_id = gp.group_id \
         WHERE b.id = $1 ORDER BY gp.id",
    )
    .bind(bracket_id)
    .fetch_all(pool)
    .await
}

/// (group_participant id, club) for a bracket's group — generation seeding input.
pub async fn group_participants_with_club(
    pool: &PgPool,
    bracket_id: i32,
) -> Result<Vec<(i32, String)>, sqlx::Error> {
    let rows: Vec<(i32, Option<String>)> = sqlx::query_as(
        "SELECT gp.id, p.club FROM group_participants gp \
         JOIN brackets b ON b.group_id = gp.group_id \
         JOIN participants p ON p.id = gp.participant_id \
         WHERE b.id = $1 ORDER BY gp.id",
    )
    .bind(bracket_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id, club)| (id, club.unwrap_or_default())).collect())
}

/// (gender, age_group) of a bracket's group (lock check at generation).
pub async fn group_class(
    pool: &PgPool,
    bracket_id: i32,
) -> Result<Option<(Option<String>, Option<String>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT g.gender, g.age_group FROM brackets b \
         JOIN groups g ON g.id = b.group_id WHERE b.id = $1",
    )
    .bind(bracket_id)
    .fetch_optional(pool)
    .await
}

/// Solo bracket (1 participant): set 1st place + completed, no fights.
pub async fn complete_solo(pool: &PgPool, id: i32, gp: i32) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE brackets SET bracket_type='special', first_place=$2, status='completed' WHERE id=$1")
        .bind(id)
        .bind(gp)
        .execute(pool)
        .await?;
    Ok(())
}

/// Create a pending bracket for a group; returns the new id.
pub async fn create_for_group(pool: &PgPool, group_id: i32) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("INSERT INTO brackets (group_id, status) VALUES ($1,'pending') RETURNING id")
        .bind(group_id)
        .fetch_one(pool)
        .await
}

/// Groups that have ≥1 participant but no bracket yet (excluding QUARANTINE) —
/// the lists to create. Returns (group_id, name, gender, age_group).
pub async fn needing_brackets(
    pool: &PgPool,
) -> Result<Vec<(i32, String, Option<String>, Option<String>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT g.id, g.name, g.gender, g.age_group FROM groups g \
         WHERE g.name <> 'QUARANTINE' \
           AND EXISTS(SELECT 1 FROM group_participants gp WHERE gp.group_id=g.id) \
           AND NOT EXISTS(SELECT 1 FROM brackets b WHERE b.group_id=g.id) \
         ORDER BY g.id",
    )
    .fetch_all(pool)
    .await
}

/// Reset a bracket for regeneration: clear type/placements, status back to
/// 'pending'. (Caller deletes the fights and re-runs generation.)

/// Manual placement write (paper-result entry): set all four places + status.
pub async fn set_places(
    pool: &PgPool,
    id: i32,
    first: Option<i32>,
    second: Option<i32>,
    third_1: Option<i32>,
    third_2: Option<i32>,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE brackets SET first_place=$1, second_place=$2, third_place_1=$3, \
             third_place_2=$4, status=$5 WHERE id=$6",
    )
    .bind(first)
    .bind(second)
    .bind(third_1)
    .bind(third_2)
    .bind(status)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn reset(pool: &PgPool, id: i32) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE brackets SET bracket_type=NULL, status='pending', \
            first_place=NULL, second_place=NULL, third_place_1=NULL, third_place_2=NULL \
         WHERE id=$1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Set a bracket's type (at generation time).
pub async fn set_type(pool: &PgPool, id: i32, bracket_type: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE brackets SET bracket_type=$2 WHERE id=$1")
        .bind(id)
        .bind(bracket_type)
        .execute(pool)
        .await?;
    Ok(())
}

/// Fetch one bracket row.
pub async fn find(pool: &PgPool, id: i32) -> Result<Option<Bracket>, sqlx::Error> {
    sqlx::query_as::<_, Bracket>(&format!("SELECT {BRACKET_COLS} FROM brackets WHERE id=$1"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// A bracket + its group attributes + placements (admin results overview).
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct BracketSummary {
    pub id: i32,
    pub group_name: String,
    pub gender: Option<String>,
    pub age_group: Option<String>,
    pub weight_class: Option<String>,
    pub bracket_type: Option<String>,
    pub status: Option<String>,
    pub first_place: Option<i32>,
    pub second_place: Option<i32>,
    pub third_place_1: Option<i32>,
    pub third_place_2: Option<i32>,
}

/// All brackets with group label + placements, for the admin results view.
pub async fn all_summaries(pool: &PgPool) -> Result<Vec<BracketSummary>, sqlx::Error> {
    sqlx::query_as::<_, BracketSummary>(
        "SELECT b.id, g.name AS group_name, g.gender, g.age_group, g.weight_class, \
                b.bracket_type, b.status, b.first_place, b.second_place, \
                b.third_place_1, b.third_place_2 \
         FROM brackets b JOIN groups g ON g.id = b.group_id ORDER BY b.id",
    )
    .fetch_all(pool)
    .await
}

/// Persisted bracket placements (pool + doppel-KO share this shape).
#[derive(Debug, Clone, Serialize)]
pub struct Placements {
    pub first: Option<i32>,
    pub second: Option<i32>,
    pub third_1: Option<i32>,
    pub third_2: Option<i32>,
}

/// Welle 2B.1: if all pool fights of a single 'pools' bracket are decided,
/// persist DJB standings + status='completed'. Best-of-three (2er pool) closes
/// the moot leftover as 'bye'. Returns the placements when it just completed.
///
/// Doppelpool ('double') is NOT handled here — its KO-stage creation is Phase 4
/// (returns Ok(None), logged by the caller). Mirrors JF main.py:1007-1083.
pub async fn finalize_pool_if_complete(
    pool: &PgPool,
    bracket_id: i32,
) -> Result<Option<Placements>, sqlx::Error> {
    let Some(bracket) = find(pool, bracket_id).await? else {
        return Ok(None);
    };
    if bracket.status.as_deref() == Some("completed") {
        return Ok(None);
    }
    if bracket.bracket_type.as_deref() == Some("double") {
        return Ok(None); // Phase 4: double-pool KO stage
    }

    let mut fs = fights::pool_fights(pool, bracket_id, None).await?;
    let decided = |s: &Option<String>| matches!(s.as_deref(), Some("finished") | Some("bye"));
    let open: Vec<i32> = fs.iter().filter(|f| !decided(&f.status)).map(|f| f.id).collect();

    if !open.is_empty() {
        if !best_of_three_decided(&fs) {
            return Ok(None);
        }
        for id in &open {
            fights::close_as_bye(pool, *id).await?;
        }
        // reflect the closes in memory so standings sees them decided
        for f in fs.iter_mut() {
            if open.contains(&f.id) {
                f.status = Some("bye".into());
            }
        }
    }

    let pfs: Vec<ccr_domain::standings::PoolFight> = fs
        .iter()
        .map(|f| ccr_domain::standings::PoolFight {
            p1: f.participant1_id,
            p2: f.participant2_id,
            winner: f.winner_id,
            decided: decided(&f.status),
        })
        .collect();
    let order = ccr_domain::standings::pool_standings(&pfs);

    let placements = Placements {
        first: order.first().copied(),
        second: order.get(1).copied(),
        third_1: order.get(2).copied(),
        // >3 participants → two bronze (rank 3 AND 4); exactly 3 → one. JF:1070.
        third_2: if order.len() > 3 { order.get(3).copied() } else { None },
    };
    sqlx::query(
        "UPDATE brackets SET first_place=$1, second_place=$2, third_place_1=$3, \
             third_place_2=$4, status='completed' WHERE id=$5",
    )
    .bind(placements.first)
    .bind(placements.second)
    .bind(placements.third_1)
    .bind(placements.third_2)
    .bind(bracket_id)
    .execute(pool)
    .await?;

    Ok(Some(placements))
}

/// 2er best-of-three is decided once one fighter has ≥2 wins. JF main.py:1003.
fn best_of_three_decided(fs: &[crate::models::Fight]) -> bool {
    let mut gp_set: BTreeSet<i32> = BTreeSet::new();
    let mut wins: HashMap<i32, i32> = HashMap::new();
    for f in fs {
        for id in [f.participant1_id, f.participant2_id].into_iter().flatten() {
            gp_set.insert(id);
        }
        if matches!(f.status.as_deref(), Some("finished") | Some("bye")) {
            if let Some(w) = f.winner_id {
                *wins.entry(w).or_insert(0) += 1;
            }
        }
    }
    gp_set.len() == 2 && wins.values().copied().max().unwrap_or(0) >= 2
}
