// SPDX-License-Identifier: GPL-3.0-or-later
//! CompControlRust frontend (Leptos CSR / WASM).
//!
//! App shell: hash-router (#/kaempfer, #/listen, #/einstellungen, #/matte,
//! #/baum — deep-linkable, so e.g. the Mattenliste can run in its own browser
//! window), topbar navigation, the live WebSocket and the global fighter-edit
//! overlay (Cmd/Strg+S saves, Esc closes). Views live in `live` + `admin`.

mod admin;
mod api;
mod live;

use futures::{SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use leptos::prelude::*;
use serde_json::json;

use api::{
    fetch_matches, fetch_participants, ws_url, AdminParticipant, BracketResult, ClubInfo,
    EditState, Match,
};

fn main() {
    leptos::mount::mount_to_body(App);
}

#[derive(Clone, Copy, PartialEq)]
enum Route {
    Kaempfer,
    Listen,
    Wettkampflisten,
    Einstellungen,
    Matte,
    Baum,
    Klasse(i64),
}

impl Route {
    fn from_hash(h: &str) -> Route {
        match h.trim_start_matches('#').trim_start_matches('/') {
            h if h.starts_with("klasse/") => h
                .trim_start_matches("klasse/")
                .parse::<i64>()
                .map(Route::Klasse)
                .unwrap_or(Route::Listen),
            "matte" => Route::Matte,
            "baum" => Route::Baum,
            "listen" => Route::Listen,
            "wettkampflisten" => Route::Wettkampflisten,
            "einstellungen" => Route::Einstellungen,
            _ => Route::Kaempfer,
        }
    }
}

fn current_hash() -> String {
    web_sys::window()
        .and_then(|w| w.location().hash().ok())
        .unwrap_or_default()
}

#[component]
fn App() -> impl IntoView {
    let (matches, set_matches) = signal(Vec::<Match>::new());
    let (connected, set_connected) = signal(false);
    let (route, set_route) = signal(Route::from_hash(&current_hash()));
    let (sel_bracket, set_sel_bracket) = signal(None::<i64>);
    let (parts, set_parts) = signal(Vec::<AdminParticipant>::new());
    let (results, set_results) = signal(Vec::<BracketResult>::new());
    let search = RwSignal::new(String::new());
    let edit = RwSignal::new(None::<EditState>);
    let (clubs_list, set_clubs) = signal(Vec::<ClubInfo>::new());

    // Hash-router: the nav renders plain <a href="#/…">, the browser handles
    // history, this listener keeps the route signal in sync.
    let hash_handle = window_event_listener(leptos::ev::hashchange, move |_| {
        set_route.set(Route::from_hash(&current_hash()));
    });
    std::mem::forget(hash_handle);

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

    // Cmd+S (macOS) / Ctrl+S (others) = "Speichern": save the open edit overlay
    // and suppress the browser's save-page dialog. Esc closes it unsaved.
    let key_handle = window_event_listener(leptos::ev::keydown, move |ev| {
        if (ev.meta_key() || ev.ctrl_key()) && ev.key().eq_ignore_ascii_case("s") {
            ev.prevent_default();
            if edit.get().is_some() {
                save_edit();
            }
        }
        if ev.key() == "Escape" && edit.get_untracked().is_some() {
            edit.set(None);
        }
    });
    std::mem::forget(key_handle);

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

    // Gate the modal on Some/None only (memo), so keystrokes inside the form
    // update EditState without re-mounting the inputs (focus survives typing).
    let editing = Memo::new(move |_| edit.with(|e| e.is_some()));

    let tab = move |r: Route, href: &'static str, label: &'static str| view! {
        <a class="tab" class:active=move || route.get() == r href=href>{label}</a>
    };

    view! {
        <header class="topbar">
            <span class="brand">"Competition Control"</span>
            <nav class="tabs">
                {tab(Route::Matte, "#/matte", "Mattenliste")}
                {tab(Route::Baum, "#/baum", "Baum")}
                {tab(Route::Kaempfer, "#/kaempfer", "Kämpfer")}
                {tab(Route::Listen, "#/listen", "Listen & Ergebnisse")}
                {tab(Route::Wettkampflisten, "#/wettkampflisten", "Wettkampflisten")}
                {tab(Route::Einstellungen, "#/einstellungen", "Einstellungen")}
            </nav>
            <span class="conn">
                <span class="dot" class:live=move || connected.get()></span>
                {move || if connected.get() { "live" } else { "verbinde…" }}
            </span>
        </header>
        <main>
            {move || match route.get() {
                Route::Matte => live::list_view(matches, send).into_any(),
                Route::Baum => live::tree_view(matches, sel_bracket, set_sel_bracket, brackets, send).into_any(),
                Route::Kaempfer => admin::participants_view(parts, set_parts, search, edit, set_clubs).into_any(),
                Route::Listen => admin::brackets_view(results, set_results).into_any(),
                Route::Wettkampflisten => admin::listen_view(results, set_results).into_any(),
                Route::Einstellungen => admin::settings_view(clubs_list, set_clubs).into_any(),
                Route::Klasse(id) => admin::klasse_view(id).into_any(),
            }}
        </main>
        {move || editing.get().then(|| admin::edit_modal(edit, clubs_list, save_edit).into_any())}
    }
}
