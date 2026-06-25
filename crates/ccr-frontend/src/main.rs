// SPDX-License-Identifier: GPL-3.0-or-later
//! CompControlRust live frontend (Leptos CSR / WASM).
//!
//! Phase 3 slices:
//!   1 live mat list · 2 scoring (WS send) · 3 queue-able filter
//!   4 mat grouping (sections per mat) · 5 bracket-tree view (phase/round columns)

use futures::{SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use leptos::prelude::*;
use serde::Deserialize;
use serde_json::json;

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Score {
    points: i32,
    #[serde(default)]
    ippon: i32,
    #[serde(default)]
    wazari: i32,
    #[serde(default)]
    yuko: i32,
    #[serde(default)]
    shido: i32,
}
impl Score {
    /// Compact breakdown of the actual sub-scores (shown on fight click).
    fn breakdown(&self) -> String {
        format!("I{} W{} Y{} S{}", self.ippon, self.wazari, self.yuko, self.shido)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Fighter {
    #[serde(rename = "firstName", default)]
    first_name: String,
    #[serde(rename = "lastName", default)]
    last_name: String,
    #[serde(default)]
    score: Score,
}
impl Fighter {
    fn name(&self) -> String {
        let n = format!("{} {}", self.first_name, self.last_name);
        let n = n.trim();
        if n.is_empty() { "—".into() } else { n.to_string() }
    }
    fn present(&self) -> bool {
        !self.last_name.is_empty() && self.last_name != "TBD"
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Match {
    #[serde(rename = "matchId")]
    match_id: i64,
    #[serde(rename = "fightNr", default)]
    fight_nr: i64,
    #[serde(rename = "tableId", default)]
    table_id: Option<i64>,
    #[serde(rename = "bracketId", default)]
    bracket_id: i64,
    #[serde(rename = "bracketType", default)]
    bracket_type: String,
    #[serde(default)]
    gender: String,
    #[serde(rename = "ageGroup", default)]
    age_group: String,
    #[serde(rename = "weightClass", default)]
    weight_class: String,
    #[serde(default)]
    phase: String,
    #[serde(default)]
    round: i64,
    #[serde(rename = "posInRound", default)]
    pos_in_round: i64,
    #[serde(default)]
    p1: Fighter,
    #[serde(default)]
    p2: Fighter,
    #[serde(default)]
    status: String,
    #[serde(rename = "winnerName", default)]
    winner_name: String,
}
impl Match {
    fn scoreable(&self) -> bool {
        self.p1.present() && self.p2.present() && self.status != "finished" && self.status != "bye"
    }
    /// U9/U11 use the JVP additive system (Ippon10/Waza5/Yuko3/Shido+2, ≥20 wins).
    fn is_youth(&self) -> bool {
        self.age_group == "U9" || self.age_group == "U11"
    }
    fn listable(&self) -> bool {
        (self.p1.present() && self.p2.present()) || self.status == "finished" || self.status == "bye"
    }
    fn category(&self) -> String {
        format!("{} {} {}", self.gender, self.weight_class, self.bracket_type)
            .split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

#[derive(Deserialize)]
struct MatchesResp {
    #[serde(default)]
    matches: Vec<Match>,
}

// ── Admin (Phase 5) read models ──────────────────────────────────────────────
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct AdminParticipant {
    id: i64,
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
    #[serde(default)]
    gender: String,
    #[serde(default)]
    club: Option<String>,
    #[serde(default)]
    association: Option<String>,
    #[serde(default)]
    birth_date: Option<String>, // "YYYY-MM-DD"
    #[serde(default)]
    weight: Option<String>, // Decimal serialized as string
    #[serde(default)]
    valid: Option<bool>,
    #[serde(default)]
    paid: Option<bool>,
    #[serde(default)]
    doublestart: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct BracketResult {
    id: i64,
    #[serde(default)]
    category: String,
    #[serde(rename = "bracketType", default)]
    bracket_type: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    first: String,
    #[serde(default)]
    second: String,
    #[serde(default)]
    third1: String,
    #[serde(default)]
    third2: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct LockInfo {
    scope_key: String,
    #[serde(default)]
    age_group: String,
    #[serde(default)]
    gender: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}
#[derive(Deserialize)]
struct LocksResp {
    #[serde(default)]
    locks: Vec<LockInfo>,
}
async fn fetch_locks() -> Vec<LockInfo> {
    match gloo_net::http::Request::get("/api/locks").send().await {
        Ok(r) => r.json::<LocksResp>().await.map(|x| x.locks).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

#[derive(Deserialize)]
struct ParticipantsResp {
    #[serde(default)]
    participants: Vec<AdminParticipant>,
}
#[derive(Deserialize)]
struct BracketsResp {
    #[serde(default)]
    brackets: Vec<BracketResult>,
}

async fn fetch_participants() -> Vec<AdminParticipant> {
    match gloo_net::http::Request::get("/api/participants").send().await {
        Ok(r) => r.json::<ParticipantsResp>().await.map(|x| x.participants).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}
async fn fetch_results() -> Vec<BracketResult> {
    match gloo_net::http::Request::get("/api/brackets").send().await {
        Ok(r) => r.json::<BracketsResp>().await.map(|x| x.brackets).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

async fn fetch_matches() -> Vec<Match> {
    match gloo_net::http::Request::get("/api/matches").send().await {
        Ok(resp) => resp.json::<MatchesResp>().await.map(|r| r.matches).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn ws_url() -> String {
    let loc = web_sys::window().unwrap().location();
    let proto = if loc.protocol().unwrap_or_default() == "https:" { "wss" } else { "ws" };
    let host = loc.host().unwrap_or_else(|_| "localhost:5001".into());
    format!("{proto}://{host}/ws")
}

/// Group items into (key, items) preserving first-seen key order.
fn group_by<T: Clone, K: PartialEq + Clone>(items: &[T], key: impl Fn(&T) -> K) -> Vec<(K, Vec<T>)> {
    let mut out: Vec<(K, Vec<T>)> = Vec::new();
    for it in items {
        let k = key(it);
        match out.iter_mut().find(|(gk, _)| *gk == k) {
            Some((_, v)) => v.push(it.clone()),
            None => out.push((k, vec![it.clone()])),
        }
    }
    out
}

fn main() {
    leptos::mount::mount_to_body(App);
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    List,
    Tree,
    Admin,
}

/// In-flight edit of a fighter. `id = None` → a new fighter (POST), else PUT.
#[derive(Clone, PartialEq, Default)]
struct EditState {
    id: Option<i64>,
    first_name: String,
    last_name: String,
    gender: String,
    birthyear: String,
    club: String,
    association: String,
    weight: String,
    valid: bool,
    paid: bool,
    ds: String,
}

#[component]
fn App() -> impl IntoView {
    let (matches, set_matches) = signal(Vec::<Match>::new());
    let (connected, set_connected) = signal(false);
    let (mode, set_mode) = signal(Mode::List);
    let (sel_bracket, set_sel_bracket) = signal(None::<i64>);
    let (parts, set_parts) = signal(Vec::<AdminParticipant>::new());
    let (results, set_results) = signal(Vec::<BracketResult>::new());
    let search = RwSignal::new(String::new());
    let edit = RwSignal::new(None::<EditState>);

    // Save the open fighter edit (PUT existing / POST new), then reload + close.
    let save_edit = move || {
        let Some(e) = edit.get() else { return };
        leptos::task::spawn_local(async move {
            let body = json!({
                "first_name": e.first_name.trim(), "last_name": e.last_name.trim(),
                "gender": e.gender, "birthyear": e.birthyear.trim(),
                "club": e.club.trim(), "association": e.association.trim(),
                "weight": e.weight.trim().replace(',', "."),
                "valid": e.valid, "paid": e.paid, "doublestart": e.ds
            });
            let req = match e.id {
                Some(id) => gloo_net::http::Request::put(&format!("/api/participants/{id}")),
                None => gloo_net::http::Request::post("/api/participants"),
            };
            let _ = req.header("content-type", "application/json")
                .body(body.to_string()).unwrap().send().await;
            set_parts.set(fetch_participants().await);
        });
        edit.set(None);
    };

    // Cmd+S (macOS) / Ctrl+S (others) = "Speichern": save the open edit panel
    // and suppress the browser's save-page dialog.
    let key_handle = leptos::prelude::window_event_listener(leptos::ev::keydown, move |ev| {
        if (ev.meta_key() || ev.ctrl_key()) && ev.key().eq_ignore_ascii_case("s") {
            ev.prevent_default();
            if edit.get().is_some() {
                save_edit();
            }
        }
    });
    std::mem::forget(key_handle); // keep the listener for the app's lifetime

    let (tx, mut rx) = futures::channel::mpsc::unbounded::<String>();
    let tx = StoredValue::new(tx);

    leptos::task::spawn_local(async move { set_matches.set(fetch_matches().await); });

    leptos::task::spawn_local(async move {
        let Ok(ws) = WebSocket::open(&ws_url()) else { return };
        let (mut write, mut read) = ws.split();
        set_connected.set(true);
        leptos::task::spawn_local(async move {
            while let Some(msg) = rx.next().await {
                if write.send(Message::Text(msg)).await.is_err() { break; }
            }
        });
        while let Some(msg) = read.next().await {
            let Ok(Message::Text(txt)) = msg else { continue };
            let Ok(val) = serde_json::from_str::<serde_json::Value>(&txt) else { continue };
            match val.get("type").and_then(|t| t.as_str()) {
                Some("SCORE_SYNC") => {
                    if let Some(u) = val.get("match").cloned().and_then(|m| serde_json::from_value::<Match>(m).ok()) {
                        set_matches.update(|list| match list.iter_mut().find(|x| x.match_id == u.match_id) {
                            Some(slot) => *slot = u,
                            None => list.push(u),
                        });
                    }
                }
                Some(_) => set_matches.set(fetch_matches().await),
                None => {}
            }
        }
        set_connected.set(false);
    });

    let send = move |msg: serde_json::Value| {
        tx.with_value(|t| { let _ = t.unbounded_send(msg.to_string()); });
    };

    // Distinct brackets for the tree-view selector: (bracket_id, label).
    let brackets = move || {
        let mut seen: Vec<(i64, String)> = Vec::new();
        for m in matches.get() {
            if !seen.iter().any(|(b, _)| *b == m.bracket_id) {
                seen.push((m.bracket_id, format!("#{} {}", m.bracket_id, m.category())));
            }
        }
        seen.sort_by_key(|(b, _)| *b);
        seen
    };

    view! {
        <main style="font-family: system-ui, sans-serif; max-width: 1100px; margin: 1rem auto; padding: 0 1rem;">
            <h1 style="font-size: 1.4rem;">"TOP Team Combat Control — Live"</h1>
            <p style="color:#666;">
                {move || if connected.get() { "● live".to_string() } else { "○ verbinde…".to_string() }}
                " · "
                <button on:click=move |_| set_mode.set(Mode::List)>"Mattenliste"</button>
                " "
                <button on:click=move |_| set_mode.set(Mode::Tree)>"Bracket-Baum"</button>
                " "
                <button on:click=move |_| {
                    set_mode.set(Mode::Admin);
                    leptos::task::spawn_local(async move {
                        set_parts.set(fetch_participants().await);
                        set_results.set(fetch_results().await);
                    });
                }>"Admin"</button>
            </p>
            {move || match mode.get() {
                Mode::List => list_view(matches, send).into_any(),
                Mode::Tree => tree_view(matches, sel_bracket, set_sel_bracket, brackets, send).into_any(),
                Mode::Admin => admin_view(parts, set_parts, results, set_results, search, edit, save_edit).into_any(),
            }}
        </main>
    }
}

/// Slice 1+2+3+4: queue-able matches grouped into sections per mat.
fn list_view(
    matches: ReadSignal<Vec<Match>>,
    send: impl Fn(serde_json::Value) + Copy + Send + 'static,
) -> impl IntoView {
    let by_mat = move || {
        let mut v: Vec<Match> = matches.get().into_iter().filter(Match::listable).collect();
        v.sort_by_key(|m| (m.table_id.unwrap_or(i64::MAX), m.fight_nr));
        group_by(&v, |m| m.table_id)
    };
    view! {
        <p style="color:#666;">
            {move || format!("{} startbare Kämpfe", matches.get().iter().filter(|m| m.listable()).count())}
        </p>
        {move || by_mat().into_iter().map(|(mat, fights)| {
            let title = mat.map(|t| format!("Matte {t}")).unwrap_or_else(|| "ohne Matte".into());
            view! {
                <section style="margin-bottom:1.2rem;">
                    <h2 style="font-size:1.05rem; border-bottom:2px solid #333;">{title}</h2>
                    {fights.into_iter().map(|m| fight_row(m, send)).collect_view()}
                </section>
            }
        }).collect_view()}
    }
}

/// One match as a row with scoring controls.
fn fight_row(m: Match, send: impl Fn(serde_json::Value) + Copy + Send + 'static) -> impl IntoView {
    let id = m.match_id;
    let (s1, s2) = (m.p1.score.points, m.p2.score.points);
    let scoreable = m.scoreable();
    let youth = m.is_youth();
    // Labels + breakdown strings (cloned so the row and the breakdown line can
    // each own a copy — Leptos view closures capture by move).
    let label = format!("{} ({}) — {} ({})", m.p1.name(), s1, m.p2.name(), s2);
    let winner_suffix = (!m.winner_name.is_empty()).then(|| format!("  → {}", m.winner_name));
    let status = m.status.clone();
    let breakdown = format!("{}: {}   |   {}: {}",
        m.p1.name(), m.p1.score.breakdown(), m.p2.name(), m.p2.score.breakdown());
    // Higher classes: click the label to reveal the actual sub-scores. Youth show
    // them always (the scoring IS the breakdown).
    let expanded = RwSignal::new(false);

    let sc = move |player: i32, value: i32| json!({
        "type": "SCORE_UPDATE", "matchId": id, "playerNum": player, "value": value.max(0)
    });
    let sub = move |player: i32, kind: &'static str| json!({
        "type": "SUBSCORE_UPDATE", "matchId": id, "playerNum": player, "kind": kind, "delta": 1
    });
    let end = move || send(json!({"type":"STATUS_UPDATE","matchId":id,"status":"finished"}));

    let buttons = scoreable.then(move || {
        if youth {
            // JVP additive entry: +Ippon/+Waza/+Yuko/+Shido per fighter; backend
            // keeps the total + auto-finishes at ≥20. "Ende" = time-up outcome.
            view! {
                <span style="white-space:nowrap;">
                    <b>"P1:"</b>
                    <button on:click=move |_| send(sub(1,"ippon"))>"Ippon"</button>
                    <button on:click=move |_| send(sub(1,"wazari"))>"Waza"</button>
                    <button on:click=move |_| send(sub(1,"yuko"))>"Yuko"</button>
                    <button on:click=move |_| send(sub(1,"shido"))>"Shido"</button>
                    <b style="margin-left:6px;">"P2:"</b>
                    <button on:click=move |_| send(sub(2,"ippon"))>"Ippon"</button>
                    <button on:click=move |_| send(sub(2,"wazari"))>"Waza"</button>
                    <button on:click=move |_| send(sub(2,"yuko"))>"Yuko"</button>
                    <button on:click=move |_| send(sub(2,"shido"))>"Shido"</button>
                    <button style="margin-left:6px; font-weight:bold;" on:click=move |_| end()>"Ende"</button>
                </span>
            }.into_any()
        } else {
            view! {
                <span style="white-space:nowrap;">
                    <button on:click=move |_| send(sc(1, s1 + 1))>"P1+"</button>
                    <button on:click=move |_| send(sc(1, s1 - 1))>"P1-"</button>
                    <button on:click=move |_| send(sc(2, s2 + 1))>"P2+"</button>
                    <button on:click=move |_| send(sc(2, s2 - 1))>"P2-"</button>
                    <button style="margin-left:6px; font-weight:bold;" on:click=move |_| end()>"Ende"</button>
                </span>
            }.into_any()
        }
    });

    view! {
        <div style="border-bottom:1px solid #eee; padding:3px 0;">
            <div style="display:flex; gap:0.6rem; align-items:center;">
                <span style="width:2.5rem; color:#999;">{format!("#{}", m.fight_nr)}</span>
                <span style="flex:1; cursor:pointer;" title="Klick: Wertungen ein-/ausblenden"
                    on:click=move |_| expanded.update(|e| *e = !*e)>
                    {label}{winner_suffix}{youth.then_some(" · JVP")}
                </span>
                <span style="width:5rem; color:#777;">{status}</span>
                {buttons}
            </div>
            {move || (youth || expanded.get()).then(|| view! {
                <div style="font-size:0.8rem; color:#777; padding-left:2.5rem;">{breakdown.clone()}</div>
            })}
        </div>
    }
}

/// Slice 5: pick a bracket, lay out ALL its fights (incl. TBD) by phase + round.
fn tree_view(
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
        <p>
            "Kategorie: "
            <select on:change=move |ev| {
                let v = event_target_value(&ev);
                set_sel.set(v.parse::<i64>().ok());
            }>
                {move || brackets().into_iter().map(|(id, label)| {
                    view! { <option value=id.to_string()>{label}</option> }
                }).collect_view()}
            </select>
        </p>
        <div style="display:flex; gap:1rem; overflow-x:auto; align-items:flex-start;">
            {move || columns().into_iter().map(|((phase, round), fights)| {
                view! {
                    <div style="min-width:14rem;">
                        <h3 style="font-size:0.9rem; color:#444;">
                            {format!("{} · R{}", phase_label(&phase), round + 1)}
                        </h3>
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

/// Age-class locks panel (own reactive scope): list + add + remove.
#[component]
fn LocksPanel() -> impl IntoView {
    let (locks, set_locks) = signal(Vec::<LockInfo>::new());
    let age = RwSignal::new(String::new());
    let gender = RwSignal::new("m".to_string());
    leptos::task::spawn_local(async move { set_locks.set(fetch_locks().await); });

    let add = move || {
        let (a, g) = (age.get(), gender.get());
        if a.trim().is_empty() { return; }
        leptos::task::spawn_local(async move {
            let body = json!({ "ageGroup": a, "gender": g, "reason": "manual" });
            let _ = gloo_net::http::Request::post("/api/locks")
                .header("content-type", "application/json").body(body.to_string()).unwrap().send().await;
            set_locks.set(fetch_locks().await);
        });
    };
    let remove = move |key: String| {
        leptos::task::spawn_local(async move {
            let enc = key.replace('|', "%7C");
            let _ = gloo_net::http::Request::delete(&format!("/api/locks/{enc}")).send().await;
            set_locks.set(fetch_locks().await);
        });
    };

    view! {
        <section style="margin:0.6rem 0; padding:0.6rem; background:#fff0f0; border-radius:6px;">
            <strong>"Alters-Sperren"</strong>
            <ul>
                {move || locks.get().into_iter().map(|l| {
                    let key = l.scope_key.clone();
                    view! { <li>
                        {format!("{} {} — {}", l.gender.clone().unwrap_or_else(|| "alle".into()), l.age_group, l.reason.clone().unwrap_or_default())}
                        " " <button on:click=move |_| remove(key.clone())>"entsperren"</button>
                    </li> }
                }).collect_view()}
            </ul>
            "Klasse sperren: Alter "
            <input style="width:4rem;" placeholder="U15" prop:value=move || age.get()
                on:input=move |ev| age.set(event_target_value(&ev)) />
            " Geschlecht "
            <select prop:value=move || gender.get() on:change=move |ev| gender.set(event_target_value(&ev))>
                <option value="m">"m"</option>
                <option value="w">"w"</option>
                <option value="">"alle"</option>
            </select>
            " " <button on:click=move |_| add()>"sperren"</button>
        </section>
    }
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct MethodRow {
    min_fighters: i32,
    method: String,
}
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct BirthRow {
    year: i64,
    classes: Vec<String>,
}
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct WcRow {
    gender: String,
    age_group: String,
    max_weight: f64,
    label: String,
}
#[derive(Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
struct ConfigData {
    event_year: i64,
    #[serde(default)]
    age_classes: Vec<String>,
    #[serde(default)]
    youth_classes: Vec<String>,
    youth_pool_size: i32,
    #[serde(default)]
    youth_max_weight_spread: f64,
    #[serde(default)]
    adult_methods: Vec<MethodRow>,
    #[serde(default)]
    birth_years: Vec<BirthRow>,
    #[serde(default)]
    weight_classes: Vec<WcRow>,
}

/// Editor for the classification config: generation thresholds, youth pool size,
/// and the per-birthyear age classes (multiple classes = doublestart overlap).
#[component]
fn ConfigPanel() -> impl IntoView {
    let cfg = RwSignal::new(None::<ConfigData>);
    leptos::task::spawn_local(async move {
        if let Ok(r) = gloo_net::http::Request::get("/api/config").send().await {
            if let Ok(c) = r.json::<ConfigData>().await {
                cfg.set(Some(c));
            }
        }
    });
    let save = move || {
        let Some(c) = cfg.get() else { return };
        leptos::task::spawn_local(async move {
            let _ = gloo_net::http::Request::put("/api/config")
                .header("content-type", "application/json")
                .body(serde_json::to_string(&c).unwrap())
                .unwrap()
                .send()
                .await;
        });
    };
    let methods = ["special", "pools", "double", "ko", "repechage"];

    view! {
        <section style="margin:0.6rem 0; padding:0.6rem; background:#eef3ff; border-radius:6px;">
            <strong>"Klassen-Konfiguration"</strong>
            {move || cfg.get().map(|c| view! {
                <div style="margin:.4rem 0;">
                    "Eventjahr "
                    <input type="number" style="width:6rem;" prop:value=c.event_year.to_string()
                        on:change=move |ev| { let v = event_target_value(&ev).parse().unwrap_or(0); cfg.update(|o| if let Some(o)=o { o.event_year=v; }); } />
                    " · Jugend-Pool-Größe "
                    <input type="number" style="width:4rem;" prop:value=c.youth_pool_size.to_string()
                        on:change=move |ev| { let v = event_target_value(&ev).parse().unwrap_or(4); cfg.update(|o| if let Some(o)=o { o.youth_pool_size=v; }); } />
                    " · Jugend max. Gewichts-Spread (kg, 0=aus) "
                    <input type="number" step="0.5" style="width:5rem;" prop:value=c.youth_max_weight_spread.to_string()
                        on:change=move |ev| { let v = event_target_value(&ev).replace(',', ".").parse().unwrap_or(0.0); cfg.update(|o| if let Some(o)=o { o.youth_max_weight_spread=v; }); } />
                    " · Jugend-Klassen (nur Pool) "
                    <input style="width:8rem;" prop:value=c.youth_classes.join(",")
                        on:change=move |ev| { let v = csv_to_vec(&event_target_value(&ev)); cfg.update(|o| if let Some(o)=o { o.youth_classes=v; }); } />
                </div>
                <p style="margin:.3rem 0 .1rem;"><strong>"Generierung U13+: ab N Kämpfern → Methode"</strong></p>
                <table style="border-collapse:collapse;">
                    {c.adult_methods.iter().enumerate().map(|(i, m)| view! {
                        <tr>
                            <td>"ab "
                                <input type="number" style="width:4rem;" prop:value=m.min_fighters.to_string()
                                    on:change=move |ev| { let v = event_target_value(&ev).parse().unwrap_or(0); cfg.update(|o| if let Some(o)=o { o.adult_methods[i].min_fighters=v; }); } />
                            </td>
                            <td>" → "
                                <select prop:value=m.method.clone()
                                    on:change=move |ev| { let v = event_target_value(&ev); cfg.update(|o| if let Some(o)=o { o.adult_methods[i].method=v; }); }>
                                    {methods.iter().map(|me| view! { <option value=*me>{*me}</option> }).collect_view()}
                                </select>
                            </td>
                        </tr>
                    }).collect_view()}
                </table>
                <p style="margin:.4rem 0 .1rem;"><strong>"Altersklassen je Jahrgang"</strong>" (mehrere = Doppelstart-Überlappung)"</p>
                <table style="border-collapse:collapse;">
                    <thead><tr style="text-align:left;"><th>"Jahrgang"</th><th>"Klassen (Komma)"</th><th></th></tr></thead>
                    <tbody>
                        {c.birth_years.iter().enumerate().map(|(i, b)| view! {
                            <tr>
                                <td><input type="number" style="width:5rem;" prop:value=b.year.to_string()
                                    on:change=move |ev| { let v = event_target_value(&ev).parse().unwrap_or(0); cfg.update(|o| if let Some(o)=o { o.birth_years[i].year=v; }); } /></td>
                                <td><input style="width:12rem;" prop:value=b.classes.join(",")
                                    on:change=move |ev| { let v = csv_to_vec(&event_target_value(&ev)); cfg.update(|o| if let Some(o)=o { o.birth_years[i].classes=v; }); } /></td>
                                <td><button on:click=move |_| cfg.update(|o| if let Some(o)=o { o.birth_years.remove(i); })>"🗑"</button></td>
                            </tr>
                        }).collect_view()}
                    </tbody>
                </table>
                <button on:click=move |_| cfg.update(|o| if let Some(o)=o { o.birth_years.push(BirthRow { year: 2020, classes: vec![] }); })>"+ Jahrgang"</button>
                <p style="margin:.4rem 0 .1rem;"><strong>"Gewichtsklassen"</strong>" (Geschlecht · Altersklasse · MaxGewicht · Label)"</p>
                <table style="border-collapse:collapse;">
                    <thead><tr style="text-align:left;"><th>"G"</th><th>"Klasse"</th><th>"≤ kg"</th><th>"Label"</th><th></th></tr></thead>
                    <tbody>
                        {c.weight_classes.iter().enumerate().map(|(i, w)| view! {
                            <tr>
                                <td><select prop:value=w.gender.clone()
                                    on:change=move |ev| { let v = event_target_value(&ev); cfg.update(|o| if let Some(o)=o { o.weight_classes[i].gender=v; }); }>
                                    <option value="m">"m"</option><option value="w">"w"</option>
                                </select></td>
                                <td><input style="width:4rem;" prop:value=w.age_group.clone()
                                    on:change=move |ev| { let v = event_target_value(&ev); cfg.update(|o| if let Some(o)=o { o.weight_classes[i].age_group=v; }); } /></td>
                                <td><input type="number" step="0.5" style="width:5rem;" prop:value=w.max_weight.to_string()
                                    on:change=move |ev| { let v = event_target_value(&ev).replace(',', ".").parse().unwrap_or(0.0); cfg.update(|o| if let Some(o)=o { o.weight_classes[i].max_weight=v; }); } /></td>
                                <td><input style="width:6rem;" prop:value=w.label.clone()
                                    on:change=move |ev| { let v = event_target_value(&ev); cfg.update(|o| if let Some(o)=o { o.weight_classes[i].label=v; }); } /></td>
                                <td><button on:click=move |_| cfg.update(|o| if let Some(o)=o { o.weight_classes.remove(i); })>"🗑"</button></td>
                            </tr>
                        }).collect_view()}
                    </tbody>
                </table>
                <button on:click=move |_| cfg.update(|o| if let Some(o)=o { o.weight_classes.push(WcRow { gender: "m".into(), age_group: "U15".into(), max_weight: 0.0, label: String::new() }); })>"+ Gewichtsklasse"</button>
                <p/>
                <button style="font-weight:bold;" on:click=move |_| save()>"Konfiguration speichern"</button>
            })}
        </section>
    }
}

/// Split a comma-separated input into trimmed, non-empty parts.
fn csv_to_vec(s: &str) -> Vec<String> {
    s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

/// Phase 5: admin — import, results overview, and an editable participant list.
fn admin_view(
    parts: ReadSignal<Vec<AdminParticipant>>,
    set_parts: WriteSignal<Vec<AdminParticipant>>,
    results: ReadSignal<Vec<BracketResult>>,
    set_results: WriteSignal<Vec<BracketResult>>,
    search: RwSignal<String>,
    edit: RwSignal<Option<EditState>>,
    save_edit: impl Fn() + Copy + Send + 'static,
) -> impl IntoView {
    let medal = |s: &str| if s.is_empty() { "—".to_string() } else { s.to_string() };

    // Generate fights for a bracket (pools), then reload the results.
    let generate = move |bracket_id: i64| {
        leptos::task::spawn_local(async move {
            let _ = gloo_net::http::Request::post(&format!("/api/brackets/{bracket_id}/generate"))
                .send()
                .await;
            set_results.set(fetch_results().await);
        });
    };

    // Create a bracket + fights for every group that has participants (one click).
    let create_all = move || {
        leptos::task::spawn_local(async move {
            let _ = gloo_net::http::Request::post("/api/brackets/create-all").send().await;
            set_results.set(fetch_results().await);
        });
    };

    // Delete a fighter (with a confirm), then reload the list.
    let delete = move |pid: i64| {
        let ok = web_sys::window().and_then(|w| w.confirm_with_message("Kämpfer löschen?").ok()).unwrap_or(false);
        if !ok {
            return;
        }
        leptos::task::spawn_local(async move {
            let _ = gloo_net::http::Request::delete(&format!("/api/participants/{pid}")).send().await;
            set_parts.set(fetch_participants().await);
        });
    };

    // Import a picked contestants file (JSON or CSV) → POST → reload.
    let on_file = move |ev: leptos::ev::Event| {
        let input: web_sys::HtmlInputElement = event_target(&ev);
        let Some(file) = input.files().and_then(|fl| fl.get(0)) else { return };
        leptos::task::spawn_local(async move {
            let gf = gloo_file::File::from(file);
            if let Ok(text) = gloo_file::futures::read_as_text(&gf).await {
                let _ = gloo_net::http::Request::post("/api/import-contestants")
                    .body(text)
                    .unwrap()
                    .send()
                    .await;
                set_parts.set(fetch_participants().await);
            }
        });
    };

    // Filtered participants (name or club contains the search text).
    let filtered = move || {
        let q = search.get().to_lowercase();
        parts.get().into_iter().filter(|p| {
            q.is_empty()
                || format!("{} {} {}", p.first_name, p.last_name, p.club.clone().unwrap_or_default())
                    .to_lowercase()
                    .contains(&q)
        }).collect::<Vec<_>>()
    };


    view! {
        <section style="margin:0.6rem 0; padding:0.6rem; background:#f4f4f8; border-radius:6px;">
            <strong>"Contestants importieren"</strong>" (.json / .csv) "
            <input type="file" accept=".json,.csv" on:change=on_file />
            " · "
            <button on:click=move |_| {
                leptos::task::spawn_local(async move {
                    let _ = gloo_net::http::Request::post("/api/assign-groups").send().await;
                    set_results.set(fetch_results().await);
                });
            }>"Gruppen zuordnen (Alters-/Gewichtsklasse)"</button>
            " "
            <button on:click=move |_| create_all()>"Listen erstellen (Brackets generieren)"</button>
        </section>
        <ConfigPanel/>
        <LocksPanel/>

        <section style="margin:0.6rem 0; padding:0.6rem; background:#eef7ee; border-radius:6px;">
            <strong>"Excel-Export"</strong>" "
            <a href="/api/export/results.xlsx" download>"Ergebnisliste (.xlsx)"</a>
            " · "
            <a href="/api/export/results.pdf" download>"Ergebnisliste (.pdf)"</a>
            " · "
            <a href="/api/export/urkunden.xlsx" download>"Urkunden (.xlsx)"</a>
            " · "
            <a href="/api/export/urkunden.csv" download>"Urkunden (.csv → Affinity)"</a>
            " · "
            <a href="/api/export/wiegekarten.xlsx" download>"Wiegekarten-Liste (.xlsx)"</a>
            " · "
            <a href="/api/export/wiegekarten.pdf" download>"Wiegekarten-Liste (.pdf)"</a>
            " · "
            <a href="/api/export/wiegekarten.csv" download>"Wiegekarten (.csv → Affinity)"</a>
            " · "
            <a href="/print/wiegekarten" target="_blank">"Wiegekarten drucken (HTML)"</a>
        </section>

        <h2 style="font-size:1.1rem; margin-top:1rem;">"Ergebnisse"</h2>
        <table style="width:100%; border-collapse:collapse; margin-bottom:1.5rem;">
            <thead><tr style="text-align:left; border-bottom:2px solid #333;">
                <th>"Kategorie"</th><th>"Typ"</th><th>"Status"</th>
                <th>"1."</th><th>"2."</th><th>"3."</th><th>"3."</th><th></th>
            </tr></thead>
            <tbody>
                {move || results.get().into_iter().map(|b| {
                    let id = b.id;
                    view! {
                        <tr style="border-bottom:1px solid #eee;">
                            <td>{b.category}</td>
                            <td style="color:#666;">{b.bracket_type.unwrap_or_default()}</td>
                            <td>{b.status.unwrap_or_default()}</td>
                            <td>{medal(&b.first)}</td><td>{medal(&b.second)}</td>
                            <td>{medal(&b.third1)}</td><td>{medal(&b.third2)}</td>
                            <td><button on:click=move |_| generate(id)>"⚙ Generieren / Neu"</button></td>
                        </tr>
                    }
                }).collect_view()}
            </tbody>
        </table>

        <h2 style="font-size:1.1rem;">
            "Teilnehmer " {move || format!("({})", parts.get().len())} " "
            <button on:click=move |_| edit.set(Some(EditState {
                id: None, gender: "m".into(), ds: "nein".into(), ..Default::default()
            }))>"+ Neuer Kämpfer"</button>
        </h2>
        <p>
            "Suche: "
            <input prop:value=move || search.get()
                on:input=move |ev| search.set(event_target_value(&ev)) />
        </p>

        // Edit panel (sticky): full fighter record (new or existing).
        {move || edit.get().map(|st| {
            let title = if st.id.is_none() { "Neuer Kämpfer" } else { "Kämpfer bearbeiten" };
            // text input bound to one EditState field via getter/setter closures
            let txt = move |get: fn(&EditState) -> String, set: fn(&mut EditState, String), w: &'static str| view! {
                <input style=format!("width:{w};")
                    prop:value=move || edit.get().as_ref().map(get).unwrap_or_default()
                    on:input=move |ev| { let v = event_target_value(&ev); edit.update(|e| if let Some(e)=e { set(e, v); }); } />
            };
            view! {
                <div style="position:sticky; top:0; background:#fffbe6; border:1px solid #e0c000; padding:8px; margin-bottom:6px;">
                    <strong>{title}</strong>
                    <div style="display:flex; flex-wrap:wrap; gap:6px; align-items:center; margin:4px 0;">
                        "Vorname " {txt(|e| e.first_name.clone(), |e, v| e.first_name = v, "8rem")}
                        "Nachname " {txt(|e| e.last_name.clone(), |e, v| e.last_name = v, "8rem")}
                        "Geschl. "
                        <select prop:value=move || edit.get().map(|e| e.gender).unwrap_or_default()
                            on:change=move |ev| { let v = event_target_value(&ev); edit.update(|e| if let Some(e)=e { e.gender=v; }); }>
                            <option value="m">"m"</option><option value="w">"w"</option>
                        </select>
                        "Jahrgang " {txt(|e| e.birthyear.clone(), |e, v| e.birthyear = v, "4rem")}
                        "Verein " {txt(|e| e.club.clone(), |e, v| e.club = v, "9rem")}
                        "Verband " {txt(|e| e.association.clone(), |e, v| e.association = v, "8rem")}
                        "Gewicht " {txt(|e| e.weight.clone(), |e, v| e.weight = v, "4rem")}
                        "Gültig "
                        <input type="checkbox" prop:checked=move || edit.get().map(|e| e.valid).unwrap_or(false)
                            on:change=move |ev| { let v = event_target_checked(&ev); edit.update(|e| if let Some(e)=e { e.valid=v; }); } />
                        "Bezahlt "
                        <input type="checkbox" prop:checked=move || edit.get().map(|e| e.paid).unwrap_or(false)
                            on:change=move |ev| { let v = event_target_checked(&ev); edit.update(|e| if let Some(e)=e { e.paid=v; }); } />
                        "Doppelstart "
                        <select prop:value=move || edit.get().map(|e| e.ds).unwrap_or_default()
                            on:change=move |ev| { let v = event_target_value(&ev); edit.update(|e| if let Some(e)=e { e.ds=v; }); }>
                            <option value="nein">"nein"</option>
                            <option value="ja">"ja (doppel)"</option>
                            <option value="höher">"höher"</option>
                        </select>
                    </div>
                    <button style="font-weight:bold;" on:click=move |_| save_edit()>"Speichern (Cmd/Strg+S)"</button>
                    <button on:click=move |_| edit.set(None)>"Abbrechen"</button>
                </div>
            }
        })}

        <table style="width:100%; border-collapse:collapse;">
            <thead><tr style="text-align:left; border-bottom:2px solid #333;">
                <th>"Name"</th><th>"Verein"</th><th>"G"</th><th>"Gewicht"</th>
                <th>"Gültig"</th><th>"Bezahlt"</th><th>"Doppelstart"</th><th></th>
            </tr></thead>
            <tbody>
                {move || filtered().into_iter().map(|p| {
                    let pid = p.id;
                    let e = EditState {
                        id: Some(p.id),
                        first_name: p.first_name.clone(),
                        last_name: p.last_name.clone(),
                        gender: p.gender.clone(),
                        birthyear: p.birth_date.clone().unwrap_or_default().chars().take(4).collect(),
                        club: p.club.clone().unwrap_or_default(),
                        association: p.association.clone().unwrap_or_default(),
                        weight: p.weight.clone().unwrap_or_default(),
                        valid: p.valid.unwrap_or(false),
                        paid: p.paid.unwrap_or(false),
                        ds: p.doublestart.clone().unwrap_or_else(|| "nein".into()),
                    };
                    view! {
                        <tr style="border-bottom:1px solid #f0f0f0;">
                            <td>{format!("{} {}", p.first_name, p.last_name)}</td>
                            <td style="color:#666;">{p.club.unwrap_or_default()}</td>
                            <td>{p.gender}</td>
                            <td>{p.weight.unwrap_or_default()}</td>
                            <td>{if p.valid.unwrap_or(false) { "✓" } else { "" }}</td>
                            <td>{if p.paid.unwrap_or(false) { "✓" } else { "" }}</td>
                            <td>{p.doublestart.unwrap_or_default()}</td>
                            <td style="white-space:nowrap;">
                                <button on:click=move |_| edit.set(Some(e.clone()))>"✎"</button>
                                <button on:click=move |_| delete(pid)>"🗑"</button>
                            </td>
                        </tr>
                    }
                }).collect_view()}
            </tbody>
        </table>
    }
}

/// One fight box in the tree (TBD-aware; scoring inline when scoreable).
fn tree_node(m: Match, send: impl Fn(serde_json::Value) + Copy + Send + 'static) -> impl IntoView {
    let id = m.match_id;
    let (s1, s2) = (m.p1.score.points, m.p2.score.points);
    let scoreable = m.scoreable();
    let bg = if m.status == "finished" { "#eef7ee" } else if scoreable { "#fff" } else { "#f6f6f6" };
    view! {
        <div style=format!("border:1px solid #ccc; border-radius:4px; padding:4px 6px; margin-bottom:8px; background:{bg};")>
            <div style="font-size:0.75rem; color:#999;">{format!("#{}", m.fight_nr)}</div>
            <div>{format!("{} ({})", m.p1.name(), s1)}</div>
            <div>{format!("{} ({})", m.p2.name(), s2)}</div>
            {scoreable.then(|| view! {
                <div style="margin-top:3px;">
                    <button on:click=move |_| send(json!({"type":"SCORE_UPDATE","matchId":id,"playerNum":1,"value":(s1+1)}))>"P1+"</button>
                    <button on:click=move |_| send(json!({"type":"SCORE_UPDATE","matchId":id,"playerNum":2,"value":(s2+1)}))>"P2+"</button>
                    <button on:click=move |_| send(json!({"type":"STATUS_UPDATE","matchId":id,"status":"finished"}))>"Ende"</button>
                </div>
            })}
        </div>
    }
}
