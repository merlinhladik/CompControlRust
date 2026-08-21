// SPDX-License-Identifier: GPL-3.0-or-later
//! /api/matches read path + the canonical match dict (mirrors JF main.py).

use std::collections::HashMap;

use axum::{extract::State, Json};
use serde_json::{json, Value};

use ccr_db::{brackets, fights, models::Fight, participants, reconcile, PgPoolHandle as PgPool};

use crate::{AppError, AppState};

type People = HashMap<i32, participants::ResolvedParticipant>;
type Groups = HashMap<i32, brackets::ResolvedGroup>;

/// Build one match dict. Field names + fallbacks mirror JF main.py:_build_match_dict
/// for the Phase-1 subset. Topology/label fields are null and owned by Phase 4.
/// `running_fight_nr` overrides fightNr (get_matches path); None keeps the raw
/// per-bracket fight_number (single-fight SCORE_SYNC path, JF main.py:510).
pub fn build_match(f: &Fight, people: &People, groups: &Groups, running_fight_nr: Option<i32>) -> Value {
    let g = groups.get(&f.bracket_id);
    let bracket_type = g.and_then(|g| g.bracket_type.clone()).unwrap_or_default();
    let gender = g.and_then(|g| g.gender.clone()).unwrap_or_default();
    let age_group = g.and_then(|g| g.age_group.clone()).unwrap_or_default();
    let weight_class = g.and_then(|g| g.weight_class.clone()).unwrap_or_default();

    // sub = (ippon, wazari, yuko, shido) — shown on the fight (display for higher
    // classes, the JVP-additive components for U9/U11). `points` is the headline
    // score (JVP total for youth, generic for the rest).
    let fighter = |gp_id: Option<i32>, score: Option<i32>, sub: (i32, i32, i32, i32)| -> Value {
        let info = gp_id.and_then(|id| people.get(&id));
        json!({
            "id": gp_id.map(|id| id.to_string()).unwrap_or_else(|| "WAIT".into()),
            "gpId": gp_id,
            "participantId": info.map(|i| i.participant_id),
            "firstName": info.map(|i| i.first_name.clone()).unwrap_or_default(),
            "lastName": info.map(|i| i.last_name.clone()).unwrap_or_else(|| "TBD".into()),
            "club": info.and_then(|i| i.club.clone()).unwrap_or_default(),
            "score": {
                "points": score.unwrap_or(0),
                "ippon": sub.0, "wazari": sub.1, "yuko": sub.2, "shido": sub.3,
            },
        })
    };

    let winner_name = f
        .winner_id
        .and_then(|id| people.get(&id))
        .map(|i| format!("{} {}", i.first_name, i.last_name).trim().to_string())
        .unwrap_or_default();

    // JF: "finished" if status=="completed" else (status or "upcoming").
    let status = match f.status.as_deref() {
        Some("completed") => "finished".to_string(),
        Some(s) if !s.is_empty() => s.to_string(),
        _ => "upcoming".to_string(),
    };
    let raw_nr = f.fight_number.unwrap_or(f.id);

    json!({
        "matchId": f.id,
        "tableId": f.table_id,
        "fightNr": running_fight_nr.unwrap_or(raw_nr),
        "bracketId": f.bracket_id,
        "bracketType": bracket_type,
        "gender": gender,
        "ageGroup": age_group,
        "weightClass": weight_class,
        "poolIndex": f.pool_index,
        "round": f.round.unwrap_or(0) + 1,
        "posInRound": f.pos_in_round.unwrap_or(0),
        "p1": fighter(f.participant1_id, f.score1, (f.ippon1, f.wazari1, f.yuko1, f.shido1)),
        "p2": fighter(f.participant2_id, f.score2, (f.ippon2, f.wazari2, f.yuko2, f.shido2)),
        "status": status,
        "order": raw_nr,
        "phase": f.bracket_phase,
        "winnerId": f.winner_id,
        "winnerName": winner_name,
        // PHASE 4 — topology + label helpers not yet ported:
        "nextMatchId": Value::Null,
        "nextMatchPos": Value::Null,
        "stageLabel": Value::Null,
        "categoryLabel": Value::Null,
        "groupLabel": Value::Null,
        "category": Value::Null,
        "bracketTypeLabel": Value::Null,
        "p1From": Value::Null,
        "p2From": Value::Null,
        "restTimeMin": 0,
    })
}

/// Resolve one fight's people + group and build its match dict (SCORE_SYNC path).
pub async fn build_one(pool: &PgPool, f: &Fight) -> Result<Value, ccr_db::DbError> {
    let gp_ids: Vec<i32> = [f.participant1_id, f.participant2_id, f.winner_id]
        .into_iter()
        .flatten()
        .collect();
    let people: People = participants::resolve(pool, &gp_ids)
        .await?
        .into_iter()
        .map(|p| (p.gp_id, p))
        .collect();
    let groups: Groups = brackets::resolve(pool, &[f.bracket_id])
        .await?
        .into_iter()
        .map(|g| (g.bracket_id, g))
        .collect();
    Ok(build_match(f, &people, &groups, None))
}

/// GET /api/matches — read-only mirror of JF's get_matches (Phase-1 subset).
pub async fn get_matches(State(st): State<AppState>) -> Result<Json<Value>, AppError> {
    // Load-time reconciliation (idempotent), mirroring JF get_matches: build the
    // full KO/repechage tree as TBD rows, then propagate already-decided WB byes.
    reconcile::eager_materialize(&st.pool).await?;
    reconcile::resolve_pending_byes(&st.pool).await?;
    reconcile::resolve_lb_byes(&st.pool).await?;

    let all = fights::all_fights(&st.pool).await?;

    let mut gp_ids: Vec<i32> = Vec::new();
    let mut bracket_ids: Vec<i32> = Vec::new();
    for f in &all {
        for id in [f.participant1_id, f.participant2_id, f.winner_id].into_iter().flatten() {
            gp_ids.push(id);
        }
        bracket_ids.push(f.bracket_id);
    }
    gp_ids.sort_unstable();
    gp_ids.dedup();
    bracket_ids.sort_unstable();
    bracket_ids.dedup();

    let people: People = participants::resolve(&st.pool, &gp_ids)
        .await?
        .into_iter()
        .map(|p| (p.gp_id, p))
        .collect();
    let groups: Groups = brackets::resolve(&st.pool, &bracket_ids)
        .await?
        .into_iter()
        .map(|g| (g.bracket_id, g))
        .collect();

    // Tournament-wide running fight number (1..N) by id order — JF main.py:465.
    let mut ids: Vec<i32> = all.iter().map(|f| f.id).collect();
    ids.sort_unstable();
    let running: HashMap<i32, i32> =
        ids.iter().enumerate().map(|(i, &id)| (id, i as i32 + 1)).collect();

    let matches: Vec<Value> = all
        .iter()
        .map(|f| build_match(f, &people, &groups, running.get(&f.id).copied()))
        .collect();

    Ok(Json(json!({
        "tournamentName": "Automated Tournament",
        "matches": matches,
        "currentMatchId": Value::Null,
    })))
}
