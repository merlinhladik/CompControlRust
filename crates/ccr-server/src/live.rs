// SPDX-License-Identifier: GPL-3.0-or-later
//! Live path (Phase 2): WS `/ws`, Ipponboard webhook, push pointer.
//! Mirrors JF main.py WS loop + /api/ippon-score.
//!
//! Implemented: SCORE_UPDATE, STATUS_UPDATE (winner + WB binary-tree
//! propagation), REORDER, SIGNAL; SCORE_SYNC / REFRESH_LIST broadcasts.
//! DEFERRED to Phase 4 (logged, graceful): pool standings finalize,
//! double-pool, doppel-KO LB drop/advance, repechage — all need ccr-domain
//! topology. STATUS_UPDATE still broadcasts the scored fight for those phases.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use ccr_db::{brackets, doppel_ko, doppel_pool, fights, repechage};
use ccr_domain::{jvp, ko};

use crate::{admin, matches, AppError, AppState};

/// Push a JSON message to every connected WS client.
fn broadcast(st: &AppState, msg: Value) {
    if let Ok(text) = serde_json::to_string(&msg) {
        let _ = st.tx.send(text); // Err only when no receivers — fine.
    }
}

pub async fn ws_handler(ws: WebSocketUpgrade, State(st): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, st))
}

async fn handle_socket(mut socket: WebSocket, st: AppState) {
    let mut rx = st.tx.subscribe();
    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(t))) => {
                    if let Err(e) = handle_text(&st, &t, &mut socket).await {
                        tracing::error!("ws message error: {:#}", e.0);
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(_)) => break,
                _ => {}
            },
            outgoing = rx.recv() => match outgoing {
                Ok(text) => {
                    if socket.send(Message::Text(text)).await.is_err() { break; }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {} // skip missed, keep going
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
}

fn parse_value(v: &Value) -> Option<i32> {
    match v {
        Value::Number(n) => n.as_i64().map(|x| x as i32),
        Value::String(s) if !s.trim().is_empty() => s.trim().parse().ok(),
        _ => None,
    }
}

async fn handle_text(st: &AppState, text: &str, socket: &mut WebSocket) -> Result<(), AppError> {
    let data: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let msg_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let match_id = data.get("matchId").and_then(|v| v.as_i64()).map(|x| x as i32);

    match msg_type {
        "SCORE_UPDATE" => {
            let Some(id) = match_id else { return Ok(()) };
            let player = data.get("playerNum").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
            let value = data.get("value").and_then(parse_value);
            match fights::update_score(&st.pool, id, player, value).await? {
                None => send_unknown(socket, id, "SCORE_UPDATE").await?,
                Some(f) => {
                    let m = matches::build_one(&st.pool, &f).await?;
                    broadcast(st, json!({ "type": "SCORE_SYNC", "matchId": id, "match": m }));
                }
            }
        }
        "SUBSCORE_UPDATE" => {
            // Native per-fighter sub-score (Ippon/Waza-ari/Yuko/Shido). For U9/U11
            // the JVP additive total drives the displayed score + the winner
            // (≥20 = Sore Made auto-finish); higher classes only track them for
            // the on-click breakdown.
            let Some(id) = match_id else { return Ok(()) };
            let player = data.get("playerNum").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
            let kind = data.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let delta = data.get("delta").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
            let Some(fight) = fights::add_subscore(&st.pool, id, player, kind, delta).await? else {
                return send_unknown(socket, id, "SUBSCORE_UPDATE").await.map(|_| ());
            };
            let age = age_group_of(st, fight.bracket_id).await?;
            let cfg = admin::get_config(st).await?;
            if cfg.is_youth(age.as_deref()) && fight.status.as_deref() != Some("finished") {
                let t1 = jvp::total(&fight.sub1(), &fight.sub2());
                let t2 = jvp::total(&fight.sub2(), &fight.sub1());
                let winner = match jvp::outcome(&fight.sub1(), &fight.sub2(), false) {
                    jvp::Outcome::Fighter1 => Some(fight.participant1_id),
                    jvp::Outcome::Fighter2 => Some(fight.participant2_id),
                    _ => None, // Ongoing — not decided until time
                };
                if let Some(wid) = winner {
                    let f = fights::set_result(&st.pool, id, wid, t1, t2).await?.unwrap_or(fight);
                    jvp_finish_broadcast(st, &f).await?;
                } else {
                    let f = fights::set_scores(&st.pool, id, t1, t2).await?.unwrap_or(fight);
                    let m = matches::build_one(&st.pool, &f).await?;
                    broadcast(st, json!({ "type": "SCORE_SYNC", "matchId": id, "match": m }));
                }
            } else {
                // Higher classes / already finished: sub-score is display-only.
                let m = matches::build_one(&st.pool, &fight).await?;
                broadcast(st, json!({ "type": "SCORE_SYNC", "matchId": id, "match": m }));
            }
        }
        "STATUS_UPDATE" => {
            let Some(id) = match_id else { return Ok(()) };
            let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
            let Some(fight) = fights::set_status(&st.pool, id, status).await? else {
                return send_unknown(socket, id, "STATUS_UPDATE").await.map(|_| ());
            };

            let mut propagated = None;
            let mut bracket_done = None; // pool / doppel-KO / doppelpool finalize (same shape)
            let mut lb_touched = false;
            let mut new_ko_stage: Option<Vec<i32>> = None;
            // Pool fights finalize on status, so a Hiki-wake (winner_id=None, e.g.
            // a JVP draw entered as a finished pool fight) still closes the pool.
            // KO/LB propagation below still needs a winner.
            if status == "finished" && (fight.winner_id.is_some() || fight.bracket_phase == "pool") {
                let bracket = brackets::find(&st.pool, fight.bracket_id).await?;
                let btype = bracket.and_then(|b| b.bracket_type).unwrap_or_default();
                let num_rounds = if matches!(fight.bracket_phase.as_str(), "wb" | "lb" | "rep")
                    && (btype == "ko" || btype == "repechage")
                {
                    ko::wb_num_rounds(fights::wb_round0_count(&st.pool, fight.bracket_id).await?)
                } else {
                    0
                };
                // Repechage (KO mit doppelter Trostrunde): graph engine + dynamic
                // plost fill at pool-final close. 32/64 certified, 8/16 extrapolated.
                if btype == "repechage" && ccr_domain::repechage::is_supported(num_rounds) {
                    let mut touched =
                        repechage::apply_graph_result(&st.pool, num_rounds, &fight).await?;
                    touched.extend(repechage::fill_plost_slots(&st.pool, num_rounds, &fight).await?);
                    lb_touched = !touched.is_empty();
                    bracket_done =
                        repechage::finalize(&st.pool, fight.bracket_id, num_rounds).await?;
                } else if btype == "ko" && ccr_domain::ko_big::is_supported(num_rounds) {
                    let touched =
                        doppel_ko::apply_graph_result(&st.pool, num_rounds, &fight).await?;
                    lb_touched = !touched.is_empty();
                    bracket_done =
                        doppel_ko::finalize_graph(&st.pool, fight.bracket_id, num_rounds).await?;
                } else {
                match fight.bracket_phase.as_str() {
                    "wb" => {
                        // HF→Finale (doppelpool) and WB tree advance share the binary tree.
                        propagated = fights::propagate_wb_winner(&st.pool, &fight).await?;
                        if btype == "ko" {
                            // Doppel-KO: drop the WB loser into the loser bracket.
                            lb_touched = doppel_ko::drop_loser_to_lb(&st.pool, num_rounds, &fight)
                                .await?
                                .is_some();
                            if propagated.is_none() {
                                // WB final done → try to finalize (needs both bronze too).
                                bracket_done =
                                    doppel_ko::finalize(&st.pool, fight.bracket_id, num_rounds).await?;
                            }
                        } else if btype == "double" && propagated.is_none() {
                            // Finale done → 1./2. + two HF losers as bronze.
                            bracket_done = doppel_pool::finalize(&st.pool, fight.bracket_id).await?;
                        }
                    }
                    "lb" => {
                        propagated = doppel_ko::advance_lb_winner(&st.pool, num_rounds, &fight).await?;
                        lb_touched = true;
                        if propagated.is_none() {
                            bracket_done =
                                doppel_ko::finalize(&st.pool, fight.bracket_id, num_rounds).await?;
                        }
                    }
                    "pool" => {
                        if btype == "double" {
                            // All pool fights done → create HF1/HF2/Finale eagerly.
                            new_ko_stage =
                                doppel_pool::init_ko_stage_if_pools_done(&st.pool, fight.bracket_id)
                                    .await?;
                        } else {
                            // Single-pool standings finalize (DJB).
                            bracket_done =
                                brackets::finalize_pool_if_complete(&st.pool, fight.bracket_id).await?;
                        }
                    }
                    "rep" => {
                        tracing::info!(
                            "STATUS_UPDATE finished phase=rep bracket={} — repechage deferred to Phase 4",
                            fight.bracket_id
                        );
                    }
                    _ => {}
                }
                } // end else (non-32er path)
            }

            let m = matches::build_one(&st.pool, &fight).await?;
            broadcast(st, json!({ "type": "SCORE_SYNC", "matchId": id, "match": m }));
            if let Some(next) = propagated {
                let nm = matches::build_one(&st.pool, &next).await?;
                broadcast(st, json!({ "type": "SCORE_SYNC", "matchId": next.id, "match": nm }));
            }
            if let Some(p) = bracket_done {
                broadcast(st, json!({
                    "type": "BRACKET_COMPLETED",
                    "bracketId": fight.bracket_id,
                    "placements": {
                        "first": p.first, "second": p.second,
                        "third_1": p.third_1, "third_2": p.third_2,
                    },
                }));
            }
            if let Some(ids) = new_ko_stage {
                // JF main.py:594-602 doppelpool event shape; clients reload the list.
                broadcast(st, json!({
                    "type": "DOUBLE_POOL_KO_STAGE_CREATED",
                    "bracketId": fight.bracket_id,
                    "newFightIds": ids,
                }));
                broadcast(st, json!({ "type": "REFRESH_LIST" }));
            }
            if lb_touched {
                broadcast(st, json!({ "type": "REFRESH_LIST" }));
            }
        }
        "REORDER" => {
            if let Some(orders) = data.get("orders").and_then(|v| v.as_object()) {
                let parsed: Vec<(i32, i32)> = orders
                    .iter()
                    .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_i64()? as i32)))
                    .collect();
                fights::reorder(&st.pool, &parsed).await?;
                broadcast(st, json!({ "type": "REFRESH_LIST" }));
            }
        }
        "SIGNAL" => broadcast(st, data),
        _ => {}
    }
    Ok(())
}

async fn send_unknown(socket: &mut WebSocket, id: i32, event: &str) -> Result<(), AppError> {
    let msg = json!({ "type": "ERROR", "event": event, "matchId": id, "error": "unknown matchId" });
    let _ = socket.send(Message::Text(serde_json::to_string(&msg).unwrap())).await;
    Ok(())
}

/// Age group of a bracket's group (None if unset). Drives the JVP youth path.
async fn age_group_of(st: &AppState, bracket_id: i32) -> Result<Option<String>, AppError> {
    Ok(brackets::group_class(&st.pool, bracket_id)
        .await?
        .and_then(|(_, age)| age))
}

/// Broadcast a JVP/webhook-finished fight and, if it was a pool fight, finalize
/// the pool when it was the last one. Works for a win OR a Hiki-wake (finalize
/// keys on status, not winner). KO propagation via the webhook stays deferred.
async fn jvp_finish_broadcast(
    st: &AppState,
    fight: &ccr_db::models::Fight,
) -> Result<(), AppError> {
    let m = matches::build_one(&st.pool, fight).await?;
    broadcast(st, json!({ "type": "SCORE_SYNC", "matchId": fight.id, "match": m }));
    if fight.bracket_phase == "pool" {
        if let Some(p) = brackets::finalize_pool_if_complete(&st.pool, fight.bracket_id).await? {
            broadcast(st, json!({
                "type": "BRACKET_COMPLETED",
                "bracketId": fight.bracket_id,
                "placements": {
                    "first": p.first, "second": p.second,
                    "third_1": p.third_1, "third_2": p.third_2,
                },
            }));
        }
    }
    Ok(())
}

/// Parse a `fighterN: {ippon,wazari,yuko,shido}` object from the webhook payload.
/// Missing object/keys ⇒ 0 (tolerant; old Ipponboard sends no sub-scores).
fn parse_subscores(v: Option<&Value>) -> jvp::SubScores {
    let obj = v.and_then(|x| x.as_object());
    let g = |k: &str| -> i32 {
        obj.and_then(|m| m.get(k)).and_then(|x| x.as_i64()).unwrap_or(0) as i32
    };
    jvp::SubScores::new(g("ippon"), g("wazari"), g("yuko"), g("shido"))
}

/// POST /api/push-to-ipponboard/{match_id} — marks the match as pushed so the
/// Ipponboard webhook can apply its result. JF also POSTs the fighters to
/// Ipponboard's :8080/fighters; that outbound call is deferred (Ipponboard
/// integration, not the live core).
pub async fn push_to_ipponboard(
    State(st): State<AppState>,
    Path(match_id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    if fights::find(&st.pool, match_id).await?.is_none() {
        return Err(AppError::status(
            axum::http::StatusCode::NOT_FOUND,
            format!("Match {match_id} not found"),
        ));
    }
    *st.last_pushed.lock().await = Some(match_id);
    Ok(Json(json!({ "status": "ok", "pushedMatchId": match_id })))
}

/// POST /api/ippon-score — Ipponboard 'Senden' webhook. JF main.py:2494.
pub async fn ippon_score(
    State(st): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    use axum::http::StatusCode;

    let pushed = *st.last_pushed.lock().await;
    let Some(id) = pushed else {
        return Err(AppError::status(StatusCode::BAD_REQUEST, "No match pushed yet".into()));
    };
    let winner = payload.get("winner").and_then(|v| v.as_str()).unwrap_or("");
    let is_winner = winner == "fighter1" || winner == "fighter2";
    // Optional Hiki-wake flag (U9/U11): a real draw, distinct from "Senden early".
    let record_draw = payload.get("draw").and_then(|v| v.as_bool()) == Some(true) && !is_winner;
    if !is_winner && !record_draw {
        return Err(AppError::status(
            StatusCode::BAD_REQUEST,
            "Ipponboard hat keinen Sieger gemeldet.".into(),
        ));
    }
    let Some(fight) = fights::find(&st.pool, id).await? else {
        return Err(AppError::status(StatusCode::NOT_FOUND, format!("Match {id} not found")));
    };
    // Per-fighter sub-scores (optional, additive) — store for display + JVP totals.
    let s1 = parse_subscores(payload.get("fighter1"));
    let s2 = parse_subscores(payload.get("fighter2"));
    fights::set_subscores(&st.pool, id, s1, s2).await?;
    let age = age_group_of(&st, fight.bracket_id).await?;
    let youth = admin::get_config(&st).await?.is_youth(age.as_deref());
    // U9/U11 display the JVP additive total; higher classes keep the 1/0 win flag.
    let (t1, t2) = (jvp::total(&s1, &s2), jvp::total(&s2, &s1));

    let updated = if record_draw {
        if fight.bracket_phase != "pool" {
            return Err(AppError::status(
                StatusCode::BAD_REQUEST,
                "Remis nur in Pool-Kämpfen möglich (im KO nicht propagierbar).".into(),
            ));
        }
        fights::set_result(&st.pool, id, None, t1, t2).await?
    } else {
        let (wid, sc1, sc2) = if winner == "fighter1" {
            (fight.participant1_id, if youth { t1 } else { 1 }, if youth { t2 } else { 0 })
        } else {
            (fight.participant2_id, if youth { t1 } else { 0 }, if youth { t2 } else { 1 })
        };
        fights::set_result(&st.pool, id, wid, sc1, sc2).await?
    };
    let Some(fight) = updated else {
        return Err(AppError::status(StatusCode::NOT_FOUND, format!("Match {id} not found")));
    };
    // Broadcast + finalize the pool if this closed it (win or draw).
    jvp_finish_broadcast(&st, &fight).await?;
    // Clear the pointer so a late/duplicate callback can't re-apply. JF main.py:2538.
    *st.last_pushed.lock().await = None;
    Ok(Json(json!({
        "status": "ok",
        "appliedMatchId": fight.id,
        "winner": if record_draw { "draw" } else { winner },
    })))
}
