// SPDX-License-Identifier: GPL-3.0-or-later
//! Doppelpool (bracket_type='double') live engine.
//! Mirrors JF main.py `_initialize_double_pool_ko_stage` +
//! `_finalize_double_pool_bracket`.
//!
//! After all pool fights are decided, 3 wb fights are created eagerly:
//!   HF1 = A1 vs B2 (wb r1 pos0), HF2 = A2 vs B1 (wb r1 pos1), Finale (wb r2 pos0).
//! HF→Finale uses the ordinary binary-tree propagation (HF1→p1, HF2→p2).
//! The finale win finalizes: 1./2. from the finale, the two HF losers are the
//! two bronze (NO bronze match).

use ccr_domain::standings::{pool_standings, PoolFight};
use sqlx::PgPool;

use crate::brackets::{self, Placements};
use crate::doppel_ko::loser_id;
use crate::fights;

fn decided(s: &Option<String>) -> bool {
    matches!(s.as_deref(), Some("finished") | Some("bye"))
}

fn to_poolfights(fs: &[crate::models::Fight]) -> Vec<PoolFight> {
    fs.iter()
        .map(|f| PoolFight {
            p1: f.participant1_id,
            p2: f.participant2_id,
            winner: f.winner_id,
            decided: decided(&f.status),
        })
        .collect()
}

/// If all pool fights of a 'double' bracket are decided and the KO stage does
/// not yet exist, create HF1/HF2/Finale from the per-pool standings (crossover
/// A1×B2, A2×B1). Returns the new fight ids, or None. Idempotent.
pub async fn init_ko_stage_if_pools_done(
    pool: &PgPool,
    bracket_id: i32,
) -> Result<Option<Vec<i32>>, sqlx::Error> {
    let Some(bracket) = brackets::find(pool, bracket_id).await? else {
        return Ok(None);
    };
    if bracket.bracket_type.as_deref() != Some("double")
        || bracket.status.as_deref() == Some("completed")
    {
        return Ok(None);
    }

    let all_pool = fights::pool_fights(pool, bracket_id, None).await?;
    if all_pool.iter().any(|f| !decided(&f.status)) {
        return Ok(None); // doppelpool always plays every pool fight
    }
    // Idempotent: KO stage already created?
    if !fights::wb_fights(pool, bracket_id).await?.is_empty() {
        return Ok(None);
    }

    let a = pool_standings(&to_poolfights(&fights::pool_fights(pool, bracket_id, Some(0)).await?));
    let b = pool_standings(&to_poolfights(&fights::pool_fights(pool, bracket_id, Some(1)).await?));
    if a.len() < 2 || b.len() < 2 {
        tracing::warn!(
            "Doppelpool bracket={bracket_id}: pool A={a:?} B={b:?} — too few for KO stage"
        );
        return Ok(None);
    }
    let (a1, a2, b1, b2) = (a[0], a[1], b[0], b[1]);

    // Inherit table_id from a pool fight so the frontend's table filter keeps the
    // new KO fights visible. JF main.py:1166-1171.
    let table_id = all_pool.iter().find_map(|f| f.table_id);

    // HF1 = A1 vs B2 (r1 p0), HF2 = A2 vs B1 (r1 p1), Finale (r2 p0).
    let mut ids = Vec::with_capacity(3);
    for (p1, p2, round, pos) in [
        (Some(a1), Some(b2), 1, 0),
        (Some(a2), Some(b1), 1, 1),
        (None, None, 2, 0),
    ] {
        let number = fights::next_fight_number(pool, bracket_id).await?;
        let id: i32 = sqlx::query_scalar(
            "INSERT INTO fights (bracket_id, bracket_phase, round, pos_in_round, status, \
                 fight_number, table_id, participant1_id, participant2_id) \
             VALUES ($1,'wb',$2,$3,'pending',$4,$5,$6,$7) RETURNING id",
        )
        .bind(bracket_id)
        .bind(round)
        .bind(pos)
        .bind(number)
        .bind(table_id)
        .bind(p1)
        .bind(p2)
        .fetch_one(pool)
        .await?;
        ids.push(id);
    }
    Ok(Some(ids))
}

/// Finalize a doppelpool once the finale (wb r2 p0) is decided: 1./2. from the
/// finale, the two HF losers (wb r1) become the two bronze. JF main.py:1196.
pub async fn finalize(pool: &PgPool, bracket_id: i32) -> Result<Option<Placements>, sqlx::Error> {
    let Some(bracket) = brackets::find(pool, bracket_id).await? else {
        return Ok(None);
    };
    if bracket.bracket_type.as_deref() != Some("double")
        || bracket.status.as_deref() == Some("completed")
    {
        return Ok(None);
    }

    let Some(finale) = fights::find_at(pool, bracket_id, "wb", 2, 0).await? else {
        return Ok(None);
    };
    let Some(winner) = finale.winner_id else {
        return Ok(None);
    };
    let second = if Some(winner) == finale.participant2_id {
        finale.participant1_id
    } else {
        finale.participant2_id
    };

    let semis = fights::wb_round_fights(pool, bracket_id, 1).await?;
    if semis.len() < 2 {
        return Ok(None);
    }
    let placements = Placements {
        first: Some(winner),
        second,
        third_1: loser_id(&semis[0]),
        third_2: loser_id(&semis[1]),
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
