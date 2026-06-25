// SPDX-License-Identifier: GPL-3.0-or-later
//! Load-time reconciliation for /api/matches (mirrors JF get_matches prelude):
//! eager-materialize the full KO/repechage tree (TBD rows) + resolve WB byes.
//! Idempotent. Mirrors `_ensure_ko_tree_materialized`, `_ensure_repe_tree_
//! materialized`, `_resolve_pending_byes`. (LB/repe bye CASCADES — when a WB bye
//! leaves a dead loser slot — are deferred; full brackets need them rarely.)

use ccr_domain::ko::wb_num_rounds;
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

/// Eager-materialize every KO and repechage bracket's full tree. Returns count created.
pub async fn eager_materialize(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut created = 0u64;
    for bid in bracket_ids_of_type(pool, "ko").await? {
        let nr = wb_num_rounds(fights::wb_round0_count(pool, bid).await?);
        if nr == 0 {
            continue;
        }
        let table_id = wb_r0_table_id(pool, bid).await?;
        for node in ko_nodes(nr) {
            if create_if_missing(pool, bid, node, table_id).await? {
                created += 1;
            }
        }
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
