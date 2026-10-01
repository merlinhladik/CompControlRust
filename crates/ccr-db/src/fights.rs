// SPDX-License-Identifier: GPL-3.0-or-later
//! Fight queries + live-path mutations.
//! Read path (Phase 1): all_fights. Live path (Phase 2): score/status/winner
//! update, WB binary-tree propagation, REORDER. Mirrors JF main.py.

use crate::models::Fight;
use ccr_domain::ko::{self, Slot};
use sqlx::PgPool;

pub(crate) const FIGHT_COLS: &str = "id, bracket_id, participant1_id, participant2_id, fight_number, \
     score1, score2, duration, status, bracket_phase, round, \
     pos_in_round, pool_index, table_id, winner_id, \
     ippon1, wazari1, yuko1, shido1, ippon2, wazari2, yuko2, shido2";

/// Map a sub-score kind to its column suffix, rejecting anything else (the value
/// is interpolated into SQL, so it must be a fixed whitelist).
fn subscore_col(kind: &str, player_num: i32) -> Option<String> {
    let base = match kind {
        "ippon" | "wazari" | "yuko" | "shido" => kind,
        _ => return None,
    };
    let n = if player_num == 2 { 2 } else { 1 };
    Some(format!("{base}{n}"))
}

/// All fights, ordered the way JF reads them: by bracket, then canonical
/// `fight_number` (main.py sorts on fight_number; NULLs fall back to id).
pub async fn all_fights(pool: &PgPool) -> Result<Vec<Fight>, sqlx::Error> {
    sqlx::query_as::<_, Fight>(&format!(
        "SELECT {FIGHT_COLS} FROM fights ORDER BY bracket_id, COALESCE(fight_number, id)"
    ))
    .fetch_all(pool)
    .await
}

/// Next per-bracket fight_number (max+1). JF main.py:_next_fight_number.
pub async fn next_fight_number(pool: &PgPool, bracket_id: i32) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(MAX(fight_number),0)+1 FROM fights WHERE bracket_id=$1")
        .bind(bracket_id)
        .fetch_one(pool)
        .await
}

/// Fetch one fight at a bracket tree node (phase, round, pos).
pub async fn find_at(
    pool: &PgPool,
    bracket_id: i32,
    phase: &str,
    round: i32,
    pos: i32,
) -> Result<Option<Fight>, sqlx::Error> {
    sqlx::query_as::<_, Fight>(&format!(
        "SELECT {FIGHT_COLS} FROM fights WHERE bracket_id=$1 AND bracket_phase=$2 \
         AND round=$3 AND pos_in_round=$4"
    ))
    .bind(bracket_id)
    .bind(phase)
    .bind(round)
    .bind(pos)
    .fetch_optional(pool)
    .await
}

/// Fetch one fight by id.
pub async fn find(pool: &PgPool, id: i32) -> Result<Option<Fight>, sqlx::Error> {
    sqlx::query_as::<_, Fight>(&format!("SELECT {FIGHT_COLS} FROM fights WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// SCORE_UPDATE: set score for player 1 or 2. JF main.py:504-507.
pub async fn update_score(
    pool: &PgPool,
    id: i32,
    player_num: i32,
    value: Option<i32>,
) -> Result<Option<Fight>, sqlx::Error> {
    let col = if player_num == 1 { "score1" } else { "score2" };
    sqlx::query(&format!("UPDATE fights SET {col} = $1 WHERE id = $2"))
        .bind(value)
        .bind(id)
        .execute(pool)
        .await?;
    find(pool, id).await
}

/// Add `delta` to one fighter's sub-score (Ippon/Waza-ari/Yuko/Shido), clamped
/// at 0. Native CCR scoring. `kind` is whitelisted; bad kind ⇒ Ok(None-ish no-op
/// returns the unchanged fight). JF has no equivalent — sub-scores are new in CCR.
pub async fn add_subscore(
    pool: &PgPool,
    id: i32,
    player_num: i32,
    kind: &str,
    delta: i32,
) -> Result<Option<Fight>, sqlx::Error> {
    if let Some(col) = subscore_col(kind, player_num) {
        sqlx::query(&format!("UPDATE fights SET {col} = GREATEST({col} + $1, 0) WHERE id = $2"))
            .bind(delta)
            .bind(id)
            .execute(pool)
            .await?;
    }
    find(pool, id).await
}

/// Update the displayed point totals without finishing (live JVP additive total).
pub async fn set_scores(
    pool: &PgPool,
    id: i32,
    score1: i32,
    score2: i32,
) -> Result<Option<Fight>, sqlx::Error> {
    sqlx::query("UPDATE fights SET score1=$1, score2=$2 WHERE id=$3")
        .bind(score1)
        .bind(score2)
        .bind(id)
        .execute(pool)
        .await?;
    find(pool, id).await
}

/// Finalize a fight with an explicit winner (or `None` = Hiki-wake/draw) and the
/// given displayed scores. Used by the JVP path, where the winner can be a draw.
pub async fn set_result(
    pool: &PgPool,
    id: i32,
    winner_id: Option<i32>,
    score1: i32,
    score2: i32,
) -> Result<Option<Fight>, sqlx::Error> {
    sqlx::query(
        "UPDATE fights SET score1=$1, score2=$2, winner_id=$3, status='finished' WHERE id=$4",
    )
    .bind(score1)
    .bind(score2)
    .bind(winner_id)
    .bind(id)
    .execute(pool)
    .await?;
    find(pool, id).await
}

/// STATUS_UPDATE: set status; on 'finished' derive winner from scores
/// (s1>s2 → p1, s2>s1 → p2, tie → NULL). JF main.py:518-527.
pub async fn set_status(
    pool: &PgPool,
    id: i32,
    status: &str,
) -> Result<Option<Fight>, sqlx::Error> {
    let Some(f) = find(pool, id).await? else {
        return Ok(None);
    };
    let winner_id = if status == "finished" {
        let s1 = f.score1.unwrap_or(0);
        let s2 = f.score2.unwrap_or(0);
        if s1 > s2 {
            f.participant1_id
        } else if s2 > s1 {
            f.participant2_id
        } else {
            None
        }
    } else {
        f.winner_id
    };
    sqlx::query("UPDATE fights SET status = $1, winner_id = $2 WHERE id = $3")
        .bind(status)
        .bind(winner_id)
        .bind(id)
        .execute(pool)
        .await?;
    find(pool, id).await
}

/// Count round-0 WB fights of a bracket (drives wb_num_rounds). JF main.py:1566.
pub async fn wb_round0_count(pool: &PgPool, bracket_id: i32) -> Result<u32, sqlx::Error> {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM fights WHERE bracket_id=$1 AND bracket_phase='wb' AND round=0",
    )
    .bind(bracket_id)
    .fetch_one(pool)
    .await?;
    Ok(n as u32)
}

/// WB winners-bracket propagation. Sets the winner into the follow-up fight
/// (lazy-creating rounds 1+ when within the tree). Returns the updated/created
/// follow-up fight, or None when there is none (→ finalize). JF _propagate_winner.
///
/// LB/repechage/double topology is NOT handled here — Phase 4.
pub async fn propagate_wb_winner(
    pool: &PgPool,
    fight: &Fight,
) -> Result<Option<Fight>, sqlx::Error> {
    if fight.bracket_phase != "wb" && fight.bracket_phase != "lb" {
        return Ok(None);
    }
    let (Some(round), Some(pos)) = (fight.round, fight.pos_in_round) else {
        return Ok(None);
    };
    let (next_round, next_pos, slot) = ko::wb_next(round, pos);
    let slot_col = match slot {
        Slot::P1 => "participant1_id",
        Slot::P2 => "participant2_id",
    };

    let existing = sqlx::query_as::<_, Fight>(&format!(
        "SELECT {FIGHT_COLS} FROM fights WHERE bracket_id=$1 AND bracket_phase=$2 \
         AND round=$3 AND pos_in_round=$4"
    ))
    .bind(fight.bracket_id)
    .bind(&fight.bracket_phase)
    .bind(next_round)
    .bind(next_pos)
    .fetch_optional(pool)
    .await?;

    if let Some(next) = existing {
        sqlx::query(&format!("UPDATE fights SET {slot_col} = $1 WHERE id = $2"))
            .bind(fight.winner_id)
            .bind(next.id)
            .execute(pool)
            .await?;
        return find(pool, next.id).await;
    }

    // Lazy-create rounds 1+ (only within the WB tree, never behind the final).
    let num_rounds = if fight.bracket_phase == "wb" {
        ko::wb_num_rounds(wb_round0_count(pool, fight.bracket_id).await?)
    } else {
        0
    };
    if num_rounds > 0 && next_round <= num_rounds as i32 - 1 {
        let next_number: Option<i32> = sqlx::query_scalar(
            "SELECT COALESCE(MAX(fight_number),0)+1 FROM fights WHERE bracket_id=$1",
        )
        .bind(fight.bracket_id)
        .fetch_one(pool)
        .await?;
        let new_id: i32 = sqlx::query_scalar(&format!(
            "INSERT INTO fights (bracket_id, bracket_phase, round, pos_in_round, status, \
                 fight_number, table_id, {slot_col}) \
             VALUES ($1,$2,$3,$4,'pending',$5,$6,$7) RETURNING id"
        ))
        .bind(fight.bracket_id)
        .bind(&fight.bracket_phase)
        .bind(next_round)
        .bind(next_pos)
        .bind(next_number)
        .bind(fight.table_id)
        .bind(fight.winner_id)
        .fetch_one(pool)
        .await?;
        return find(pool, new_id).await;
    }
    Ok(None)
}

/// All pool-phase fights of a bracket (optionally one pool_index).
pub async fn pool_fights(
    pool: &PgPool,
    bracket_id: i32,
    pool_index: Option<i32>,
) -> Result<Vec<Fight>, sqlx::Error> {
    let sql = format!(
        "SELECT {FIGHT_COLS} FROM fights WHERE bracket_id=$1 AND bracket_phase='pool' \
         AND ($2::int IS NULL OR pool_index = $2)"
    );
    sqlx::query_as::<_, Fight>(&sql)
        .bind(bracket_id)
        .bind(pool_index)
        .fetch_all(pool)
        .await
}

/// All wb fights of a bracket (any round). Used for double-pool idempotency.
pub async fn wb_fights(pool: &PgPool, bracket_id: i32) -> Result<Vec<Fight>, sqlx::Error> {
    sqlx::query_as::<_, Fight>(&format!(
        "SELECT {FIGHT_COLS} FROM fights WHERE bracket_id=$1 AND bracket_phase='wb'"
    ))
    .bind(bracket_id)
    .fetch_all(pool)
    .await
}

/// All wb fights of a bracket at one round, ordered by pos.
pub async fn wb_round_fights(
    pool: &PgPool,
    bracket_id: i32,
    round: i32,
) -> Result<Vec<Fight>, sqlx::Error> {
    sqlx::query_as::<_, Fight>(&format!(
        "SELECT {FIGHT_COLS} FROM fights WHERE bracket_id=$1 AND bracket_phase='wb' \
         AND round=$2 ORDER BY pos_in_round"
    ))
    .bind(bracket_id)
    .bind(round)
    .fetch_all(pool)
    .await
}

/// All lb fights of a bracket at one round, ordered by pos (e.g. the 2 bronze).
pub async fn lb_round_fights(
    pool: &PgPool,
    bracket_id: i32,
    round: i32,
) -> Result<Vec<Fight>, sqlx::Error> {
    sqlx::query_as::<_, Fight>(&format!(
        "SELECT {FIGHT_COLS} FROM fights WHERE bracket_id=$1 AND bracket_phase='lb' \
         AND round=$2 ORDER BY pos_in_round"
    ))
    .bind(bracket_id)
    .bind(round)
    .fetch_all(pool)
    .await
}

/// Count all fights of a bracket (generation idempotency guard).
pub async fn count_for_bracket(pool: &PgPool, bracket_id: i32) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM fights WHERE bracket_id=$1")
        .bind(bracket_id)
        .fetch_one(pool)
        .await
}

/// True if any fight in the bracket has a real played result (status='finished').
/// Byes/walkovers ('bye') and pending fights don't count — regeneration is safe.
pub async fn any_finished_for_bracket(pool: &PgPool, bracket_id: i32) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM fights WHERE bracket_id=$1 AND status='finished')")
        .bind(bracket_id)
        .fetch_one(pool)
        .await
}

/// Delete all fights of a bracket (regeneration). Returns rows removed.
pub async fn delete_for_bracket(pool: &PgPool, bracket_id: i32) -> Result<u64, sqlx::Error> {
    let r = sqlx::query("DELETE FROM fights WHERE bracket_id=$1")
        .bind(bracket_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

/// Create one pool-phase fight (generation). round/pos NULL; pool_index set.
pub async fn create_pool_fight(
    pool: &PgPool,
    bracket_id: i32,
    pool_index: i32,
    fight_number: i32,
    p1: i32,
    p2: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO fights (bracket_id, bracket_phase, pool_index, fight_number, \
             participant1_id, participant2_id, status) \
         VALUES ($1,'pool',$2,$3,$4,$5,'pending')",
    )
    .bind(bracket_id)
    .bind(pool_index)
    .bind(fight_number)
    .bind(p1)
    .bind(p2)
    .execute(pool)
    .await?;
    Ok(())
}

/// Create one WB round-0 fight (generation). A bye is p1==p2 + status='bye' +
/// winner set (edv convention; CCR's `_resolve_pending_byes` then advances it).
pub async fn create_wb_round0(
    pool: &PgPool,
    bracket_id: i32,
    pos: i32,
    p1: i32,
    p2: i32,
    status: &str,
    winner: Option<i32>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO fights (bracket_id, bracket_phase, round, pos_in_round, fight_number, \
             participant1_id, participant2_id, status, winner_id) \
         VALUES ($1,'wb',0,$2,$3,$4,$5,$6,$7)",
    )
    .bind(bracket_id)
    .bind(pos)
    .bind(pos + 1)
    .bind(p1)
    .bind(p2)
    .bind(status)
    .bind(winner)
    .execute(pool)
    .await?;
    Ok(())
}

/// Close a fight as a dead 'bye' (winner stays NULL → no win). JF main.py:1043.
pub async fn close_as_bye(pool: &PgPool, id: i32) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE fights SET status='bye' WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// REORDER: write new fight_number per match id. JF main.py:620-621.

/// Re-seed: apply a gp permutation to every participant/winner reference of a
/// bracket in one pass per column (slot structure, schedule and fight numbers
/// stay untouched). `olds[i]` is replaced by `news[i]`.
pub async fn remap_participants(
    pool: &PgPool,
    bracket_id: i32,
    olds: &[i32],
    news: &[i32],
) -> Result<(), sqlx::Error> {
    for col in ["participant1_id", "participant2_id", "winner_id"] {
        sqlx::query(&format!(
            "UPDATE fights SET {col} = m.new FROM \
             (SELECT unnest($2::int4[]) AS old, unnest($3::int4[]) AS new) m \
             WHERE bracket_id=$1 AND {col} = m.old"
        ))
        .bind(bracket_id)
        .bind(olds)
        .bind(news)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn reorder(pool: &PgPool, orders: &[(i32, i32)]) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    for &(match_id, order) in orders {
        sqlx::query("UPDATE fights SET fight_number = $1 WHERE id = $2")
            .bind(order)
            .bind(match_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}
