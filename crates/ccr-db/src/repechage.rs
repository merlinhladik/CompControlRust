// SPDX-License-Identifier: GPL-3.0-or-later
//! Repechage live engine (bracket_type='repechage').
//! Mirrors JF main.py `_apply_repe_graph_result`, `_trace_pool_victims`,
//! `_fill_repechage_plost_slots`, `_finalize_repechage_bracket`. Topology from
//! `ccr-domain::repechage`. 32/64 certified; 8/16 extrapolated.
//!
//! NOT ported (deferred, graceful with full pools): `_resolve_repe_byes`
//! (bye cascade when a pool winner had an R0 bye) and eager tree materialization.

use ccr_domain::ko::Slot;
use ccr_domain::ko32::Kind;
use ccr_domain::repechage as topo;
use sqlx::PgPool;

use crate::brackets::Placements;
use crate::doppel_ko::{find_or_create_node, loser_id, set_slot};
use crate::fights;
use crate::models::Fight;

fn phase_lit(phase: &str) -> &'static str {
    match phase {
        "wb" => "wb",
        _ => "rep",
    }
}

/// Push winner (W edges) and HF loser (the crossed bronze L edges) along the
/// repechage graph. plost entries are filled separately. JF `_apply_repe_graph_result`.
pub async fn apply_graph_result(
    pool: &PgPool,
    num_rounds: u32,
    fight: &Fight,
) -> Result<Vec<Fight>, sqlx::Error> {
    if !matches!(fight.bracket_phase.as_str(), "wb" | "rep") {
        return Ok(Vec::new());
    }
    let (Some(round), Some(pos)) = (fight.round, fight.pos_in_round) else {
        return Ok(Vec::new());
    };
    let winner = fight.winner_id;
    let loser = loser_id(fight);
    let mut touched = Vec::new();
    for (dst, slot, kind) in topo::consumers(num_rounds, (phase_lit(&fight.bracket_phase), round, pos)) {
        let val = match kind {
            Kind::Winner => winner,
            Kind::Loser => loser,
        };
        let Some(v) = val else { continue };
        let (phase, r, p) = dst;
        let nxt = find_or_create_node(pool, fight.bracket_id, phase, r, p, fight.table_id).await?;
        set_slot(pool, nxt.id, if slot == 1 { Slot::P1 } else { Slot::P2 }, v).await?;
        if let Some(f) = fights::find(pool, nxt.id).await? {
            touched.push(f);
        }
    }
    Ok(touched)
}

/// Fighters the pool winner beat, EARLIEST loss first (index i = level i+1).
/// Walks the winner's wb path from the pool final back to R0. JF `_trace_pool_victims`.
async fn trace_pool_victims(
    pool: &PgPool,
    pool_final: &Fight,
) -> Result<Vec<Option<i32>>, sqlx::Error> {
    let w = pool_final.winner_id;
    let mut victims = Vec::new();
    let mut cur = pool_final.clone();
    loop {
        victims.push(loser_id(&cur));
        let Some(round) = cur.round else { break };
        if round == 0 {
            break;
        }
        let p = cur.pos_in_round.unwrap_or(0);
        // child = wb fight one round down at pos 2p or 2p+1 that the winner won.
        let child = sqlx::query_as::<_, Fight>(
            "SELECT id, bracket_id, participant1_id, participant2_id, fight_number, \
                    score1, score2, duration, status, bracket_phase, round, \
                    pos_in_round, pool_index, table_id, winner_id \
             FROM fights WHERE bracket_id=$1 AND bracket_phase='wb' AND round=$2 \
             AND pos_in_round IN ($3,$4) AND winner_id=$5",
        )
        .bind(cur.bracket_id)
        .bind(round - 1)
        .bind(2 * p)
        .bind(2 * p + 1)
        .bind(w)
        .fetch_optional(pool)
        .await?;
        match child {
            Some(c) => cur = c,
            None => break,
        }
    }
    victims.reverse(); // earliest loss (R0) first → level 1
    Ok(victims)
}

/// On a pool-final close, seat the pool winner's victims into their plost slots.
/// JF `_fill_repechage_plost_slots`. Returns touched fights.
pub async fn fill_plost_slots(
    pool: &PgPool,
    num_rounds: u32,
    fight: &Fight,
) -> Result<Vec<Fight>, sqlx::Error> {
    if fight.bracket_phase != "wb" || fight.winner_id.is_none() {
        return Ok(Vec::new());
    }
    let Some(round) = fight.round else { return Ok(Vec::new()) };
    if round != topo::poolfinal_round(num_rounds) {
        return Ok(Vec::new());
    }
    let Some(pos) = fight.pos_in_round else { return Ok(Vec::new()) };
    if !(0..4).contains(&pos) {
        return Ok(Vec::new());
    }
    let pool_idx = pos as u8;
    let victims = trace_pool_victims(pool, fight).await?;
    let targets: std::collections::HashMap<_, _> = topo::plost_targets(num_rounds).into_iter().collect();
    let mut touched = Vec::new();
    for (i, victim) in victims.iter().enumerate() {
        let Some(v) = victim else { continue };
        let level = i as i32 + 1;
        let Some(&((phase, r, p), slot)) = targets.get(&(pool_idx, level)) else { continue };
        let f = find_or_create_node(pool, fight.bracket_id, phase, r, p, fight.table_id).await?;
        set_slot(pool, f.id, if slot == 1 { Slot::P1 } else { Slot::P2 }, *v).await?;
        if let Some(rf) = fights::find(pool, f.id).await? {
            touched.push(rf);
        }
    }
    Ok(touched)
}

/// Repechage finalize: 1./2. from the final, 3./3. from the bronze WINNERS
/// (unlike the 32er Doppel-KO where bronze = medal-round loser). No 5th place.
/// JF `_finalize_repechage_bracket`.
pub async fn finalize(
    pool: &PgPool,
    bracket_id: i32,
    num_rounds: u32,
) -> Result<Option<Placements>, sqlx::Error> {
    let Some(bracket) = crate::brackets::find(pool, bracket_id).await? else {
        return Ok(None);
    };
    if bracket.bracket_type.as_deref() != Some("repechage")
        || bracket.status.as_deref() == Some("completed")
    {
        return Ok(None);
    }
    let (fp, fr, fpos) = topo::final_node(num_rounds);
    let Some(final_fight) = fights::find_at(pool, bracket_id, fp, fr, fpos).await? else {
        return Ok(None);
    };
    let Some(winner) = final_fight.winner_id else {
        return Ok(None);
    };
    let (b0, b1) = topo::bronze_nodes(num_rounds);
    let mut bronze = Vec::new();
    for (p, r, pos) in [b0, b1] {
        let Some(f) = fights::find_at(pool, bracket_id, p, r, pos).await? else {
            return Ok(None);
        };
        if f.winner_id.is_none() && f.status.as_deref() != Some("bye") {
            return Ok(None);
        }
        bronze.push(f);
    }
    let second = if Some(winner) == final_fight.participant2_id {
        final_fight.participant1_id
    } else {
        final_fight.participant2_id
    };
    let placements = Placements {
        first: Some(winner),
        second,
        third_1: bronze[0].winner_id, // bronze = WINNER (repechage rule)
        third_2: bronze[1].winner_id,
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
