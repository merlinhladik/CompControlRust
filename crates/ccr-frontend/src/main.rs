// SPDX-License-Identifier: GPL-3.0-or-later
//! CompControlRust frontend (Leptos CSR / WASM).
//!
//! App shell: hash-router (#/kaempfer, #/listen, #/wettkampflisten,
//! #/auswertungen, #/einstellungen — deep-linkable), topbar navigation and the
//! global fighter-edit overlay (Cmd/Strg+S saves, Esc closes). Views live in
//! `admin`. The live mat/tree views are gone — score entry happens on the mats.

mod admin;
mod api;

use leptos::prelude::*;
use serde_json::json;

use api::{fetch_participants, AdminParticipant, BracketResult, ClubInfo, EditState};

fn main() {
    leptos::mount::mount_to_body(App);
}

#[derive(Clone, Copy, PartialEq)]
enum Route {
    Kaempfer,
    Vereine,
    Listen,
    Wettkampflisten,
    Auswertungen,
    Einstellungen,
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
            "vereine" => Route::Vereine,
            "listen" => Route::Listen,
            "wettkampflisten" => Route::Wettkampflisten,
            "auswertungen" => Route::Auswertungen,
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
    let (route, set_route) = signal(Route::from_hash(&current_hash()));
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
                {tab(Route::Kaempfer, "#/kaempfer", "Kämpfer")}
                {tab(Route::Vereine, "#/vereine", "Vereine")}
                {tab(Route::Listen, "#/listen", "Listen & Ergebnisse")}
                {tab(Route::Wettkampflisten, "#/wettkampflisten", "Wettkampflisten")}
                {tab(Route::Auswertungen, "#/auswertungen", "Auswertungen")}
                {tab(Route::Einstellungen, "#/einstellungen", "Einstellungen")}
            </nav>
        </header>
        <main>
            {move || match route.get() {
                Route::Kaempfer => admin::participants_view(parts, set_parts, search, edit, set_clubs).into_any(),
                Route::Vereine => admin::clubs_view(clubs_list, set_clubs).into_any(),
                Route::Listen => admin::brackets_view(results, set_results).into_any(),
                Route::Wettkampflisten => admin::listen_view(results, set_results).into_any(),
                Route::Auswertungen => admin::auswertungen_view(results, set_results).into_any(),
                Route::Einstellungen => admin::settings_view().into_any(),
                Route::Klasse(id) => admin::klasse_view(id).into_any(),
            }}
        </main>
        {move || editing.get().then(|| admin::edit_modal(edit, clubs_list, save_edit).into_any())}
    }
}
