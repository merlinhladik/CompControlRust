// SPDX-License-Identifier: GPL-3.0-or-later
//! Load-time reconciliation for /api/matches (mirrors JF get_matches prelude):
//! eager-materialize the full KO/repechage tree (TBD rows) + resolve WB byes.
//! Idempotent. Mirrors `_ensure_ko_tree_materialized`, `_ensure_repe_tree_
//! materialized`, `_resolve_pending_byes`. (LB/repe bye CASCADES — when a WB bye
//! leaves a dead loser slot — are deferred; full brackets need them rarely.)

use std::collections::HashMap;

use ccr_domain::ko::wb_num_rounds;
use ccr_domain::ko32::Kind;
use ccr_domain::ko32::Node;
use ccr_domain::{doppel_ko as dk_topo, ko_big, repechage as rep_topo};
use sqlx::PgPool;

use crate::fights;

async fn bracket_ids_of_type(pool: &PgPool, ty: &str) -> Result<Vec<i32>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM brackets WHERE bracket_type=$1")
        .bind(ty)
        .fetch_all(pool)
        .await
}

async fn wb_r0_table_id(pool: &PgPool, bracket_id: i32) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT table_id FROM fights WHERE bracket_id=$1 AND bracket_phase='wb' AND round=0 LIMIT 1",
    )
    .bind(bracket_id)
    .fetch_optional(pool)
    .await
    .map(|o| o.flatten())
}

/// Create a TBD fight at a node if absent. Idempotent via the position unique
/// constraint. Returns true if a row was inserted.
async fn create_if_missing(
    pool: &PgPool,
    bracket_id: i32,
    node: Node,
    table_id: Option<i32>,
) -> Result<bool, sqlx::Error> {
    let (phase, round, pos) = node;
    let res = sqlx::query(
        "INSERT INTO fights (bracket_id, bracket_phase, round, pos_in_round, status, \
             fight_number, table_id) \
         VALUES ($1,$2,$3,$4,'pending', \
             (SELECT COALESCE(MAX(fight_number),0)+1 FROM fights WHERE bracket_id=$1), $5) \
         ON CONFLICT (bracket_id, bracket_phase, round, pos_in_round) DO NOTHING",
    )
    .bind(bracket_id)
    .bind(phase)
    .bind(round)
    .bind(pos)
    .bind(table_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Nodes (excluding wb round 0, which edv seeds) for a KO bracket of num_rounds.
fn ko_nodes(num_rounds: u32) -> Vec<Node> {
    if ko_big::is_supported(num_rounds) {
        // 32/64: explicit graph node set.
        return ko_big::all_nodes(num_rounds).into_iter().filter(|n| !(n.0 == "wb" && n.1 == 0)).collect();
    }
    // 8/16: WB binary rounds 1..R-1 + LB rounds per _LB_ROUND_SIZES.
    let nr = num_rounds as i32;
    let mut nodes = Vec::new();
    for r in 1..nr {
        for p in 0..(1 << (nr - 1 - r)) {
            nodes.push(("wb", r, p));
        }
    }
    for (lb_round, &size) in dk_topo::lb_round_sizes(num_rounds).iter().enumerate() {
        for p in 0..size {
            nodes.push(("lb", lb_round as i32, p));
        }
    }
    nodes
}

/// Official DJB sheet round order (Kampfnummern-Reihenfolge) per draw size.
/// 8er/16er decoded from ko_8/16.xls (edv/tests/test_ko_form_order.py),
/// 32er from ko_32.xls; 64er extrapolates the 32er pattern (ohne Gewähr, like
/// its topology). Returns (phase, round) in numbering order.
fn ko_round_order(num_rounds: u32) -> Vec<(&'static str, i32)> {
    match num_rounds {
        3 => vec![("wb", 0), ("lb", 0), ("wb", 1), ("lb", 1), ("wb", 2)],
        4 => vec![
            ("wb", 0), ("wb", 1), ("lb", 0), ("lb", 1),
            ("wb", 2), ("lb", 2), ("lb", 3), ("wb", 3),
        ],
        _ => {
            // 32/64: wb0, wb1, lb0, then alternate wb r / lb r-1 while wb
            // rounds remain, then the lb tail (incl. medal round + final).
            let nodes = ko_big::all_nodes(num_rounds);
            let wb_max = nodes.iter().filter(|n| n.0 == "wb").map(|n| n.1).max().unwrap_or(0);
            let lb_max = nodes.iter().filter(|n| n.0 == "lb").map(|n| n.1).max().unwrap_or(0);
            let mut seq = vec![("wb", 0), ("wb", 1), ("lb", 0)];
            for r in 2..=wb_max {
                seq.push(("wb", r));
                seq.push(("lb", r - 1));
            }
            for r in (wb_max)..=lb_max {
                seq.push(("lb", r));
            }
            seq
        }
    }
}

/// Renumber a KO bracket's fights to the official sheet order (1..N). Runs on
/// fresh materialization only, so a later manual REORDER is never clobbered.
async fn renumber_ko(pool: &PgPool, bracket_id: i32, num_rounds: u32) -> Result<(), sqlx::Error> {
    let mut nr = 0i32;
    for (phase, round) in ko_round_order(num_rounds) {
        let positions: Vec<i32> = sqlx::query_scalar(
            "SELECT pos_in_round FROM fights WHERE bracket_id=$1 AND bracket_phase=$2              AND round=$3 ORDER BY pos_in_round",
        )
        .bind(bracket_id)
        .bind(phase)
        .bind(round)
        .fetch_all(pool)
        .await?;
        for p in positions {
            nr += 1;
            sqlx::query(
                "UPDATE fights SET fight_number=$1 WHERE bracket_id=$2 AND bracket_phase=$3                  AND round=$4 AND pos_in_round=$5",
            )
            .bind(nr)
            .bind(bracket_id)
            .bind(phase)
            .bind(round)
            .bind(p)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// Eager-materialize every KO and repechage bracket's full tree. Returns count created.
pub async fn eager_materialize(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut created = 0u64;
    for bid in bracket_ids_of_type(pool, "ko").await? {
        let nr = wb_num_rounds(fights::wb_round0_count(pool, bid).await?);
        if nr == 0 {
            continue;
        }
        let table_id = wb_r0_table_id(pool, bid).await?;
        let mut created_here = 0u64;
        for node in ko_nodes(nr) {
            if create_if_missing(pool, bid, node, table_id).await? {
                created_here += 1;
            }
        }
        if created_here > 0 {
            // fresh tree: stamp the official sheet numbering (Kampfnummern)
            renumber_ko(pool, bid, nr).await?;
        }
        created += created_here;
    }
    for bid in bracket_ids_of_type(pool, "repechage").await? {
        let nr = wb_num_rounds(fights::wb_round0_count(pool, bid).await?);
        if !rep_topo::is_supported(nr) {
            continue;
        }
        let table_id = wb_r0_table_id(pool, bid).await?;
        for node in rep_topo::all_nodes(nr) {
            if node.0 == "wb" && node.1 == 0 {
                continue; // edv seeds the main-draw R0
            }
            if create_if_missing(pool, bid, node, table_id).await? {
                created += 1;
            }
        }
    }
    Ok(created)
}


/// Feeder edges per target slot for a KO bracket: (target node, slot 1|2) ->
/// (source node, kind). Winner edges advance winners, Loser edges drop losers.
fn ko_feeders(num_rounds: u32) -> HashMap<(Node, i32), (Node, Kind)> {
    let mut m = HashMap::new();
    if ko_big::is_supported(num_rounds) {
        for n in ko_big::all_nodes(num_rounds) {
            for (t, slot, k) in ko_big::consumers(num_rounds, n) {
                m.insert((t, slot), (n, k));
            }
        }
        return m;
    }
    // 8/16: wb binary tree + frozen _LB_STRUCTURE
    let nr = num_rounds as i32;
    for r in 0..(nr - 1) {
        for p in 0..(1i32 << (nr - 1 - r)) {
            let slot = if p % 2 == 0 { 1 } else { 2 };
            m.insert((("wb", r + 1, p / 2), slot), (("wb", r, p), Kind::Winner));
        }
    }
    for r in 0..nr {
        for p in 0..(1i32 << (nr - 1 - r).max(0)) {
            if let Some((r2, p2, sl)) = dk_topo::wb_drop(num_rounds, r, p) {
                let slot = if sl == ccr_domain::ko::Slot::P1 { 1 } else { 2 };
                m.insert((("lb", r2, p2), slot), (("wb", r, p), Kind::Loser));
            }
        }
    }
    for (lb_round, &size) in dk_topo::lb_round_sizes(num_rounds).iter().enumerate() {
        for p in 0..size {
            if let Some((r2, p2, sl)) = dk_topo::lb_advance(num_rounds, lb_round as i32, p) {
                let slot = if sl == ccr_domain::ko::Slot::P1 { 1 } else { 2 };
                m.insert((("lb", r2, p2), slot), (("lb", lb_round as i32, p), Kind::Winner));
            }
        }
    }
    m
}

/// A feeder that can never deliver a fighter into its slot: a missing row, a
/// bye for a Loser edge (a walkover has no loser), or a fully-dead fight for a
/// Winner edge.
fn feeder_dead(src: Option<&crate::models::Fight>, kind: Kind) -> bool {
    match src {
        None => true,
        Some(f) => match kind {
            Kind::Loser => f.status.as_deref() == Some("bye"),
            Kind::Winner => f.status.as_deref() == Some("bye") && f.winner_id.is_none(),
        },
    }
}

/// LB/graph bye cascade (JF `_resolve_lb_byes`): WB byes leave dead slots in
/// the Trostrunde. Fixpoint per KO bracket: one live + one dead slot => the
/// live fighter advances by walkover (p1==p2, status='bye', winner set); two
/// dead slots => the fight itself is dead and cascades. Idempotent.
pub async fn resolve_lb_byes(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut changed_total = 0u64;
    for bid in bracket_ids_of_type(pool, "ko").await? {
        let nr = wb_num_rounds(fights::wb_round0_count(pool, bid).await?);
        if nr == 0 {
            continue;
        }
        let feeders = ko_feeders(nr);
        loop {
            let rows = sqlx::query_as::<_, crate::models::Fight>(&format!(
                "SELECT {} FROM fights WHERE bracket_id=$1",
                crate::fights::FIGHT_COLS
            ))
            .bind(bid)
            .fetch_all(pool)
            .await?;
            let by_node: HashMap<Node, &crate::models::Fight> = rows
                .iter()
                .map(|f| {
                    let phase: &'static str = match f.bracket_phase.as_str() {
                        "wb" => "wb",
                        "lb" => "lb",
                        _ => "rep",
                    };
                    ((phase, f.round.unwrap_or(0), f.pos_in_round.unwrap_or(0)), f)
                })
                .collect();
            let mut changed = false;
            for f in &rows {
                if f.status.as_deref() != Some("pending") {
                    continue;
                }
                let phase: &'static str = match f.bracket_phase.as_str() {
                    "wb" => "wb",
                    "lb" => "lb",
                    _ => "rep",
                };
                let node: Node = (phase, f.round.unwrap_or(0), f.pos_in_round.unwrap_or(0));
                let slot_state = |slot: i32, filled: Option<i32>| -> (Option<i32>, bool) {
                    let dead = filled.is_none()
                        && feeders
                            .get(&(node, slot))
                            .map(|(srcn, kind)| feeder_dead(by_node.get(srcn).copied(), *kind))
                            .unwrap_or(false);
                    (filled, dead)
                };
                let (p1, d1) = slot_state(1, f.participant1_id);
                let (p2, d2) = slot_state(2, f.participant2_id);
                let walkover_gp = match (p1, d1, p2, d2) {
                    (Some(gp), _, None, true) => Some(Some(gp)),
                    (None, true, Some(gp), _) => Some(Some(gp)),
                    (None, true, None, true) => Some(None), // fully dead
                    _ => None,
                };
                let Some(gp) = walkover_gp else { continue };
                sqlx::query(
                    "UPDATE fights SET participant1_id=$2, participant2_id=$2,                          winner_id=$2, status='bye' WHERE id=$1",
                )
                .bind(f.id)
                .bind(gp)
                .execute(pool)
                .await?;
                changed = true;
                changed_total += 1;
                // advance a walkover winner into its Winner-edge target slot
                if let Some(gp) = gp {
                    for ((tnode, slot), (srcn, kind)) in &feeders {
                        if *srcn == node && *kind == Kind::Winner {
                            let col = if *slot == 1 { "participant1_id" } else { "participant2_id" };
                            sqlx::query(&format!(
                                "UPDATE fights SET {col}=$1 WHERE bracket_id=$2 AND                                  bracket_phase=$3 AND round=$4 AND pos_in_round=$5 AND {col} IS NULL"
                            ))
                            .bind(gp)
                            .bind(bid)
                            .bind(tnode.0)
                            .bind(tnode.1)
                            .bind(tnode.2)
                            .execute(pool)
                            .await?;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        // a cascade can decide the medal round/bronze - attempt the finalize
        if ko_big::is_supported(nr) {
            let _ = crate::doppel_ko::finalize_graph(pool, bid, nr).await?;
        } else {
            let _ = crate::doppel_ko::finalize(pool, bid, nr).await?;
        }
    }
    Ok(changed_total)
}

/// Propagate already-decided WB byes (status='bye', winner set, p1==p2) into
/// their follow-up fight. edv seeds these for non-power-of-2 fields; they never
/// pass through the live finish handler. Idempotent. JF `_resolve_pending_byes`.
pub async fn resolve_pending_byes(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut resolved = 0u64;
    for bid in bracket_ids_of_type(pool, "ko").await? {
        let byes = sqlx::query_as::<_, crate::models::Fight>(&format!(
            "SELECT {} FROM fights WHERE bracket_id=$1 AND bracket_phase='wb' \
             AND status='bye' AND winner_id IS NOT NULL",
            crate::fights::FIGHT_COLS
        ))
        .bind(bid)
        .fetch_all(pool)
        .await?;
        for bye in byes {
            // Winner-only advance (a bye has no loser → no LB drop). Idempotent.
            if fights::propagate_wb_winner(pool, &bye).await?.is_some() {
                resolved += 1;
            }
        }
    }
    Ok(resolved)
}
