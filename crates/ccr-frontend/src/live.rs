// SPDX-License-Identifier: GPL-3.0-or-later
//! Live views: Mattenliste (queue-able fights per mat) + Bracket-Baum.

use leptos::prelude::*;
use serde_json::json;

use crate::api::{group_by, Match};

/// Queue-able matches grouped into sections per mat.
pub fn list_view(
    matches: ReadSignal<Vec<Match>>,
    send: impl Fn(serde_json::Value) + Copy + Send + 'static,
) -> impl IntoView {
    let by_mat = move || {
        let mut v: Vec<Match> = matches.get().into_iter().filter(Match::listable).collect();
        v.sort_by_key(|m| (m.table_id.unwrap_or(i64::MAX), m.fight_nr));
        group_by(&v, |m| m.table_id)
    };
    view! {
        <p class="muted">
            {move || format!("{} startbare Kämpfe", matches.get().iter().filter(|m| m.listable()).count())}
        </p>
        {move || by_mat().into_iter().map(|(mat, fights)| {
            let title = mat.map(|t| format!("Matte {t}")).unwrap_or_else(|| "ohne Matte".into());
            view! {
                <section class="mat-section card">
                    <h2>{title}</h2>
                    {fights.into_iter().map(|m| fight_row(m, send)).collect_view()}
                </section>
            }
        }).collect_view()}
    }
}

/// Scoring controls (JVP sub-scores for youth, ±points otherwise). Shared by the
/// Mattenliste row and the tree node.
fn score_buttons(m: &Match, send: impl Fn(serde_json::Value) + Copy + Send + 'static) -> Option<AnyView> {
    if !m.scoreable() {
        return None;
    }
    let id = m.match_id;
    let (s1, s2) = (m.p1.score.points, m.p2.score.points);
    let sc = move |player: i32, value: i32| json!({
        "type": "SCORE_UPDATE", "matchId": id, "playerNum": player, "value": value.max(0)
    });
    let sub = move |player: i32, kind: &'static str| json!({
        "type": "SUBSCORE_UPDATE", "matchId": id, "playerNum": player, "kind": kind, "delta": 1
    });
    let end = move || send(json!({"type":"STATUS_UPDATE","matchId":id,"status":"finished"}));

    Some(if m.is_youth() {
        // JVP additive entry: +Ippon/+Waza/+Yuko/+Shido per fighter; backend
        // keeps the total + auto-finishes at ≥20. "Ende" = time-up outcome.
        view! {
            <span class="score-btns">
                <b>"P1"</b>
                <button on:click=move |_| send(sub(1,"ippon"))>"Ippon"</button>
                <button on:click=move |_| send(sub(1,"wazari"))>"Waza"</button>
                <button on:click=move |_| send(sub(1,"yuko"))>"Yuko"</button>
                <button on:click=move |_| send(sub(1,"shido"))>"Shido"</button>
                <b>"P2"</b>
                <button on:click=move |_| send(sub(2,"ippon"))>"Ippon"</button>
                <button on:click=move |_| send(sub(2,"wazari"))>"Waza"</button>
                <button on:click=move |_| send(sub(2,"yuko"))>"Yuko"</button>
                <button on:click=move |_| send(sub(2,"shido"))>"Shido"</button>
                <button class="primary" on:click=move |_| end()>"Ende"</button>
            </span>
        }.into_any()
    } else {
        view! {
            <span class="score-btns">
                <button on:click=move |_| send(sc(1, s1 + 1))>"P1+"</button>
                <button on:click=move |_| send(sc(1, s1 - 1))>"P1-"</button>
                <button on:click=move |_| send(sc(2, s2 + 1))>"P2+"</button>
                <button on:click=move |_| send(sc(2, s2 - 1))>"P2-"</button>
                <button class="primary" on:click=move |_| end()>"Ende"</button>
            </span>
        }.into_any()
    })
}

/// One match as a row with scoring controls.
fn fight_row(m: Match, send: impl Fn(serde_json::Value) + Copy + Send + 'static) -> impl IntoView {
    let (s1, s2) = (m.p1.score.points, m.p2.score.points);
    let youth = m.is_youth();
    // Labels + breakdown strings (cloned so the row and the breakdown line can
    // each own a copy — Leptos view closures capture by move).
    let label = format!("{} ({}) — {} ({})", m.p1.name(), s1, m.p2.name(), s2);
    let winner_suffix = (!m.winner_name.is_empty()).then(|| format!(" → {}", m.winner_name));
    let status = m.status.clone();
    let breakdown = format!("{}: {}   |   {}: {}",
        m.p1.name(), m.p1.score.breakdown(), m.p2.name(), m.p2.score.breakdown());
    // Higher classes: click the label to reveal the actual sub-scores. Youth show
    // them always (the scoring IS the breakdown).
    let expanded = RwSignal::new(false);

    let buttons = score_buttons(&m, send);

    view! {
        <div>
            <div class="fight-row">
                <span class="fight-nr">{format!("#{}", m.fight_nr)}</span>
                <span class="fight-label" title="Klick: Wertungen ein-/ausblenden"
                    on:click=move |_| expanded.update(|e| *e = !*e)>
                    {label}
                    <span class="fight-winner">{winner_suffix}</span>
                    {youth.then_some(" · JVP")}
                </span>
                <span class=format!("badge {status}")>{status.clone()}</span>
                {buttons}
            </div>
            {move || (youth || expanded.get()).then(|| view! {
                <div class="breakdown">{breakdown.clone()}</div>
            })}
        </div>
    }
}

/// Pick a bracket, lay out ALL its fights (incl. TBD) by phase + round.
pub fn tree_view(
    matches: ReadSignal<Vec<Match>>,
    sel: ReadSignal<Option<i64>>,
    set_sel: WriteSignal<Option<i64>>,
    brackets: impl Fn() -> Vec<(i64, String)> + Copy + Send + 'static,
    send: impl Fn(serde_json::Value) + Copy + Send + 'static,
) -> impl IntoView {
    let phase_label = |p: &str| match p {
        "wb" => "Hauptrunde (WB)",
        "lb" => "Trostrunde (LB)",
        "rep" => "Trostrunde (Repechage)",
        "pool" => "Pool",
        other => other,
    }.to_string();

    let columns = move || {
        let Some(b) = sel.get().or_else(|| brackets().first().map(|(id, _)| *id)) else {
            return Vec::new();
        };
        let mut fs: Vec<Match> = matches.get().into_iter().filter(|m| m.bracket_id == b).collect();
        fs.sort_by_key(|m| (phase_rank(&m.phase), m.round, m.pos_in_round));
        // group by (phase, round) → a column each
        group_by(&fs, |m| (m.phase.clone(), m.round))
    };

    view! {
        <div class="card toolbar">
            <span>"Kategorie:"</span>
            <select on:change=move |ev| {
                let v = event_target_value(&ev);
                set_sel.set(v.parse::<i64>().ok());
            }>
                {move || brackets().into_iter().map(|(id, label)| {
                    view! { <option value=id.to_string()>{label}</option> }
                }).collect_view()}
            </select>
        </div>
        <div class="tree">
            {move || columns().into_iter().map(|((phase, round), fights)| {
                view! {
                    <div class="tree-col">
                        <h3>{format!("{} · R{}", phase_label(&phase), round + 1)}</h3>
                        {fights.into_iter().map(|m| tree_node(m, send)).collect_view()}
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

fn phase_rank(p: &str) -> i64 {
    match p { "pool" => 0, "wb" => 1, "lb" => 2, "rep" => 2, _ => 3 }
}

/// One fight box in the tree (TBD-aware; scoring inline when scoreable).
fn tree_node(m: Match, send: impl Fn(serde_json::Value) + Copy + Send + 'static) -> impl IntoView {
    let (s1, s2) = (m.p1.score.points, m.p2.score.points);
    let state = if m.status == "finished" { "finished" } else if m.scoreable() { "" } else { "idle" };
    let buttons = score_buttons(&m, send);
    view! {
        <div class=format!("tree-node {state}")>
            <div class="nr">{format!("#{}", m.fight_nr)}</div>
            <div>{format!("{} ({})", m.p1.name(), s1)}</div>
            <div>{format!("{} ({})", m.p2.name(), s2)}</div>
            {buttons.map(|b| view! { <div style="margin-top:4px;">{b}</div> })}
        </div>
    }
}
