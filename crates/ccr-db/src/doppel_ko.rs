// SPDX-License-Identifier: GPL-3.0-or-later
//! Modified Doppel-KO live engine (loser bracket) for 8er/16er.
//! Mirrors JF main.py `_drop_loser_to_lb`, `_advance_lb_winner`,
//! `_finalize_doppel_ko_bracket`. Topology comes from `ccr-domain::doppel_ko`
//! (frozen `_LB_STRUCTURE`). 32er stays unwired (graceful None).

use ccr_domain::doppel_ko as topo;
use ccr_domain::ko::Slot;
use ccr_domain::ko_big;
use sqlx::PgPool;

use crate::brackets::Placements;
use crate::fights;
use crate::models::Fight;

fn slot_col(slot: Slot) -> &'static str {
    match slot {
        Slot::P1 => "participant1_id",
        Slot::P2 => "participant2_id",
    }
}

/// Loser gp_id; None on bye/phantom (p1==p2 or a missing participant).
/// JF main.py:_loser_id.
pub fn loser_id(f: &Fight) -> Option<i32> {
    let (w, p1, p2) = (f.winner_id?, f.participant1_id?, f.participant2_id?);
    if p1 == p2 {
        return None;
    }
    Some(if w == p2 { p1 } else { p2 })
}

/// Find or create a fight at any tree node (phase-generic), returning it.
pub(crate) async fn find_or_create_node(
    pool: &PgPool,
    bracket_id: i32,
    phase: &str,
    round: i32,
    pos: i32,
    table_id: Option<i32>,
) -> Result<Fight, sqlx::Error> {
    if let Some(f) = fights::find_at(pool, bracket_id, phase, round, pos).await? {
        return Ok(f);
    }
    let number = fights::next_fight_number(pool, bracket_id).await?;
    let id: i32 = sqlx::query_scalar(
        "INSERT INTO fights (bracket_id, bracket_phase, round, pos_in_round, status, \
             fight_number, table_id) VALUES ($1,$2,$3,$4,'pending',$5,$6) RETURNING id",
    )
    .bind(bracket_id)
    .bind(phase)
    .bind(round)
    .bind(pos)
    .bind(number)
    .bind(table_id)
    .fetch_one(pool)
    .await?;
    Ok(fights::find(pool, id).await?.expect("just inserted"))
}

async fn find_or_create_lb(
    pool: &PgPool,
    bracket_id: i32,
    round: i32,
    pos: i32,
    table_id: Option<i32>,
) -> Result<Fight, sqlx::Error> {
    find_or_create_node(pool, bracket_id, "lb", round, pos, table_id).await
}

pub(crate) async fn set_slot(pool: &PgPool, fight_id: i32, slot: Slot, gp: i32) -> Result<(), sqlx::Error> {
    sqlx::query(&format!("UPDATE fights SET {} = $1 WHERE id = $2", slot_col(slot)))
        .bind(gp)
        .bind(fight_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Drop the WB loser into its LB slot (find-or-create). Returns the touched lb
/// fight, or None (bye / WB final / unwired size). JF `_drop_loser_to_lb`.
pub async fn drop_loser_to_lb(
    pool: &PgPool,
    num_rounds: u32,
    wb_fight: &Fight,
) -> Result<Option<Fight>, sqlx::Error> {
    if wb_fight.bracket_phase != "wb" {
        return Ok(None);
    }
    let (Some(round), Some(pos)) = (wb_fight.round, wb_fight.pos_in_round) else {
        return Ok(None);
    };
    let Some(loser) = loser_id(wb_fight) else {
        return Ok(None);
    };
    let Some((lb_round, lb_pos, slot)) = topo::wb_drop(num_rounds, round, pos) else {
        return Ok(None);
    };
    let lb = find_or_create_lb(pool, wb_fight.bracket_id, lb_round, lb_pos, wb_fight.table_id).await?;
    set_slot(pool, lb.id, slot, loser).await?;
    fights::find(pool, lb.id).await
}

/// Advance the LB winner within the loser bracket (pos-preserving / merge per
/// topology). Returns the follow-up fight, or None if this was the bronze match
/// (caller finalizes). JF `_advance_lb_winner`.
pub async fn advance_lb_winner(
    pool: &PgPool,
    num_rounds: u32,
    lb_fight: &Fight,
) -> Result<Option<Fight>, sqlx::Error> {
    if lb_fight.bracket_phase != "lb" || lb_fight.winner_id.is_none() {
        return Ok(None);
    }
    let (Some(round), Some(pos)) = (lb_fight.round, lb_fight.pos_in_round) else {
        return Ok(None);
    };
    match topo::bronze_round(num_rounds) {
        Some(b) if round >= b => return Ok(None), // already bronze
        None => return Ok(None),                  // unwired
        _ => {}
    }
    let Some((nr, np, slot)) = topo::lb_advance(num_rounds, round, pos) else {
        return Ok(None);
    };
    let nxt = find_or_create_lb(pool, lb_fight.bracket_id, nr, np, lb_fight.table_id).await?;
    set_slot(pool, nxt.id, slot, lb_fight.winner_id.unwrap()).await?;
    fights::find(pool, nxt.id).await
}

// ── Graph-driven Doppel-KO (32er certified; 64er extrapolated) ───────────────

/// Push BOTH winner and loser of a finished fight along the frozen/generated
/// feeder graph (`ko_big::consumers`, dispatched by num_rounds) into their
/// follow-up slots (find-or-create). Unlike 8er/16er, one fight often propagates
/// two results (e.g. SF winner → medal round, SF loser → LB semi). Returns the
/// touched fights. JF `_apply_ko_graph_result` (32er); 64er is extrapolated.
pub async fn apply_graph_result(
    pool: &PgPool,
    num_rounds: u32,
    fight: &Fight,
) -> Result<Vec<Fight>, sqlx::Error> {
    if !matches!(fight.bracket_phase.as_str(), "wb" | "lb") {
        return Ok(Vec::new());
    }
    let (Some(round), Some(pos)) = (fight.round, fight.pos_in_round) else {
        return Ok(Vec::new());
    };
    let winner = fight.winner_id;
    let loser = loser_id(fight);
    let mut touched = Vec::new();
    for (dst, slot, kind) in ko_big::consumers(num_rounds, (phase_lit(&fight.bracket_phase), round, pos)) {
        let val = match kind {
            ccr_domain::ko32::Kind::Winner => winner,
            ccr_domain::ko32::Kind::Loser => loser,
        };
        let Some(v) = val else { continue }; // bye/phantom has no loser
        let (phase, r, p) = dst;
        let nxt = find_or_create_node(pool, fight.bracket_id, phase, r, p, fight.table_id).await?;
        set_slot(pool, nxt.id, if slot == 1 { Slot::P1 } else { Slot::P2 }, v).await?;
        if let Some(f) = fights::find(pool, nxt.id).await? {
            touched.push(f);
        }
    }
    Ok(touched)
}

/// The graph node phase is always one of two static literals — map the owned
/// String to one so it matches `ko_big`'s `&'static str` node keys.
fn phase_lit(phase: &str) -> &'static str {
    match phase {
        "wb" => "wb",
        _ => "lb",
    }
}

/// Graph-driven finalize: 1./2. from the final, 3./3. from the medal-round
/// LOSERS. A fully-dead medal fight (bye, winner NULL) does not block. The two
/// 5th places (LB-semi losers) are NOT persisted. JF `_finalize_32er_bracket`
/// (32er certified; 64er extrapolated, same shape).
pub async fn finalize_graph(
    pool: &PgPool,
    bracket_id: i32,
    num_rounds: u32,
) -> Result<Option<Placements>, sqlx::Error> {
    let Some(bracket) = crate::brackets::find(pool, bracket_id).await? else {
        return Ok(None);
    };
    if bracket.bracket_type.as_deref() != Some("ko") || bracket.status.as_deref() == Some("completed") {
        return Ok(None);
    }
    let (fp, fr, fpos) = ko_big::final_node(num_rounds);
    let Some(final_fight) = fights::find_at(pool, bracket_id, fp, fr, fpos).await? else {
        return Ok(None);
    };
    let Some(final_winner) = final_fight.winner_id else {
        return Ok(None);
    };
    let (m0, m1) = ko_big::medal_nodes(num_rounds);
    let medals = [m0, m1];
    let mut medal_fights = Vec::new();
    for (p, r, pos) in medals {
        let Some(f) = fights::find_at(pool, bracket_id, p, r, pos).await? else {
            return Ok(None);
        };
        // Undecided real medal fight blocks; a fully-dead bye does not.
        if f.winner_id.is_none() && f.status.as_deref() != Some("bye") {
            return Ok(None);
        }
        medal_fights.push(f);
    }

    // walkover final (p1==p2): there IS no second finalist
    let second = if final_fight.participant1_id == final_fight.participant2_id {
        None
    } else if Some(final_winner) == final_fight.participant2_id {
        final_fight.participant1_id
    } else {
        final_fight.participant2_id
    };
    let placements = Placements {
        first: Some(final_winner),
        second,
        third_1: loser_id(&medal_fights[0]),
        third_2: loser_id(&medal_fights[1]),
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

/// Finalize an 8er/16er Doppel-KO: 1./2. from WB final, two bronze winners →
/// third_1/third_2. Only when WB final + both bronze are decided (a fully-dead
/// bronze bye does not block). JF `_finalize_doppel_ko_bracket`.
pub async fn finalize(
    pool: &PgPool,
    bracket_id: i32,
    num_rounds: u32,
) -> Result<Option<Placements>, sqlx::Error> {
    let Some(bracket) = crate::brackets::find(pool, bracket_id).await? else {
        return Ok(None);
    };
    if bracket.bracket_type.as_deref() != Some("ko") || bracket.status.as_deref() == Some("completed") {
        return Ok(None);
    }
    let Some(bronze_round) = topo::bronze_round(num_rounds) else {
        return Ok(None); // 32er / unwired
    };
    if num_rounds < 2 {
        return Ok(None);
    }
    let final_round = num_rounds as i32 - 1;

    let Some(wb_final) = fights::find_at(pool, bracket_id, "wb", final_round, 0).await? else {
        return Ok(None);
    };
    let Some(final_winner) = wb_final.winner_id else {
        return Ok(None);
    };

    let bronzes = fights::lb_round_fights(pool, bracket_id, bronze_round).await?;
    // A fully-dead bronze (status='bye', winner NULL) does NOT block; an
    // undecided real bronze does. JF main.py:1937-1939.
    let unresolved = bronzes
        .iter()
        .any(|b| b.winner_id.is_none() && b.status.as_deref() != Some("bye"));
    if bronzes.len() < 2 || unresolved {
        return Ok(None);
    }

    // walkover final (p1==p2): there IS no second finalist
    let second = if wb_final.participant1_id == wb_final.participant2_id {
        None
    } else if Some(final_winner) == wb_final.participant2_id {
        wb_final.participant1_id
    } else {
        wb_final.participant2_id
    };
    let placements = Placements {
        first: Some(final_winner),
        second,
        third_1: bronzes[0].winner_id,
        third_2: bronzes[1].winner_id,
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
