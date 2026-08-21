// SPDX-License-Identifier: GPL-3.0-or-later
//! Admin tabs: Kämpfer (participants + import + edit overlay), Listen &
//! Ergebnisse (group assignment, bracket generation, results, exports) and
//! Einstellungen (config, age-class locks, clubs).

use leptos::prelude::*;
use serde_json::json;

use crate::api::{
    fetch_clubs, fetch_locks, fetch_participants, fetch_results, AdminParticipant, ClubInfo,
    EditState, LockInfo,
};
use crate::api::BracketResult;

// ── Tab: Kämpfer ─────────────────────────────────────────────────────────────

pub fn participants_view(
    parts: ReadSignal<Vec<AdminParticipant>>,
    set_parts: WriteSignal<Vec<AdminParticipant>>,
    search: RwSignal<String>,
    edit: RwSignal<Option<EditState>>,
    set_clubs: WriteSignal<Vec<ClubInfo>>,
) -> impl IntoView {
    // refresh on tab entry
    leptos::task::spawn_local(async move {
        set_parts.set(fetch_participants().await);
        set_clubs.set(fetch_clubs().await);
    });

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

    // Import a picked contestants file (JSON or CSV) → POST → reload; the
    // outcome (counts or the server's error text) is shown next to the input.
    let import_msg = RwSignal::new(String::new());
    let on_file = move |ev: leptos::ev::Event| {
        let input: web_sys::HtmlInputElement = event_target(&ev);
        let Some(file) = input.files().and_then(|fl| fl.get(0)) else { return };
        leptos::task::spawn_local(async move {
            let gf = gloo_file::File::from(file);
            if let Ok(text) = gloo_file::futures::read_as_text(&gf).await {
                match gloo_net::http::Request::post("/api/import-contestants")
                    .body(text).unwrap().send().await
                {
                    Ok(r) => {
                        let v = r.json::<serde_json::Value>().await.unwrap_or_default();
                        if r.ok() {
                            let g = |k| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                            let mut m = format!("{} importiert ({} neu, {} aktualisiert)",
                                g("imported"), g("created"), g("updated"));
                            if g("skipped") > 0 {
                                m += &format!(", {} übersprungen (leere Namen)", g("skipped"));
                            }
                            import_msg.set(m);
                        } else {
                            import_msg.set(format!("Import fehlgeschlagen: {}",
                                v.get("error").and_then(|e| e.as_str()).unwrap_or("unbekannt")));
                        }
                    }
                    Err(e) => import_msg.set(format!("Import fehlgeschlagen: {e}")),
                }
                set_parts.set(fetch_participants().await);
                set_clubs.set(fetch_clubs().await);
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
        <div class="card toolbar">
            <input placeholder="Suche Name/Verein…" prop:value=move || search.get()
                on:input=move |ev| search.set(event_target_value(&ev)) />
            <button class="primary" on:click=move |_| edit.set(Some(EditState {
                id: None, gender: "m".into(), ds: "nein".into(),
                valid: true, paid: true, ..Default::default()
            }))>"+ Neuer Kämpfer"</button>
            <span class="muted">{move || format!("{} Teilnehmer", parts.get().len())}</span>
        </div>

        <div class="card">
            <strong>"Import & Wiegekarten"</strong>
            <div class="toolbar">
                <span>"Contestants (.json / .csv):"</span>
                <input type="file" accept=".json,.csv" on:change=on_file />
                <span class="hint">{move || import_msg.get()}</span>
            </div>
            <div class="links" style="margin-top:0.4rem;">
                <a href="/api/export/wiegekarten.xlsx" download>"Wiegekarten (.xlsx)"</a>
                <a href="/api/export/wiegekarten.pdf" download>"Wiegekarten (.pdf)"</a>
                <a href="/api/export/wiegekarten.csv" download>"Wiegekarten (.csv → Affinity)"</a>
                <a href="/print/wiegekarten" target="_blank">"Wiegekarten drucken (HTML)"</a>
            </div>
        </div>

        <div class="card scroll-x">
            <table>
                <thead><tr>
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
                            <tr>
                                <td>{format!("{} {}", p.first_name, p.last_name)}</td>
                                <td class="muted">{p.club.unwrap_or_default()}</td>
                                <td>{p.gender}</td>
                                <td>{p.weight.unwrap_or_default()}</td>
                                <td>{if p.valid.unwrap_or(false) { "✓" } else { "" }}</td>
                                <td>{if p.paid.unwrap_or(false) { "✓" } else { "" }}</td>
                                <td>{p.doublestart.unwrap_or_default()}</td>
                                <td style="white-space:nowrap;">
                                    <button class="icon" title="Bearbeiten"
                                        on:click=move |_| edit.set(Some(e.clone()))>"✎"</button>
                                    <button class="icon danger" title="Löschen"
                                        on:click=move |_| delete(pid)>"🗑"</button>
                                </td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
}

/// Fighter edit overlay (new or existing). Mounted once per open (the caller
/// gates on Some/None via a memo), so typing never re-creates the inputs.
pub fn edit_modal(
    edit: RwSignal<Option<EditState>>,
    clubs_list: ReadSignal<Vec<ClubInfo>>,
    save_edit: impl Fn() + Copy + Send + 'static,
) -> impl IntoView {
    let is_new = edit.with_untracked(|e| e.as_ref().map(|e| e.id.is_none()).unwrap_or(true));
    let title = if is_new { "Neuer Kämpfer" } else { "Kämpfer bearbeiten" };
    // text input bound to one EditState field via getter/setter closures
    let txt = move |label: &'static str, get: fn(&EditState) -> String, set: fn(&mut EditState, String)| view! {
        <label class="field">
            <span>{label}</span>
            <input prop:value=move || edit.with(|e| e.as_ref().map(get).unwrap_or_default())
                on:input=move |ev| { let v = event_target_value(&ev); edit.update(|e| if let Some(e)=e { set(e, v); }); } />
        </label>
    };

    view! {
        <div class="overlay">
            <div class="modal">
                <h3>{title}</h3>
                <div class="form-grid">
                    {txt("Vorname", |e| e.first_name.clone(), |e, v| e.first_name = v)}
                    {txt("Nachname", |e| e.last_name.clone(), |e, v| e.last_name = v)}
                    <label class="field">
                        <span>"Geschlecht"</span>
                        <select prop:value=move || edit.with(|e| e.as_ref().map(|e| e.gender.clone()).unwrap_or_default())
                            on:change=move |ev| { let v = event_target_value(&ev); edit.update(|e| if let Some(e)=e { e.gender=v; }); }>
                            <option value="m">"m"</option><option value="w">"w"</option>
                        </select>
                    </label>
                    {txt("Jahrgang", |e| e.birthyear.clone(), |e, v| e.birthyear = v)}
                    <label class="field">
                        <span>"Verein"</span>
                        <select prop:value=move || edit.with(|e| e.as_ref().map(|e| e.club.clone()).unwrap_or_default())
                            on:change=move |ev| {
                                let v = event_target_value(&ev);
                                // picking a club also fills its association
                                let assoc = clubs_list.get_untracked().iter()
                                    .find(|c| c.name == v)
                                    .and_then(|c| c.association.clone());
                                edit.update(|e| if let Some(e) = e {
                                    e.club = v;
                                    if let Some(a) = assoc { e.association = a; }
                                });
                            }>
                            <option value="">"— Verein wählen —"</option>
                            {move || clubs_list.get().into_iter().map(|c| {
                                let n = c.name;
                                view! { <option value=n.clone()>{n.clone()}</option> }
                            }).collect_view()}
                        </select>
                    </label>
                    {txt("Verband", |e| e.association.clone(), |e, v| e.association = v)}
                    {txt("Gewicht (kg)", |e| e.weight.clone(), |e, v| e.weight = v)}
                    <label class="field">
                        <span>"Doppelstart"</span>
                        <select prop:value=move || edit.with(|e| e.as_ref().map(|e| e.ds.clone()).unwrap_or_default())
                            on:change=move |ev| { let v = event_target_value(&ev); edit.update(|e| if let Some(e)=e { e.ds=v; }); }>
                            <option value="nein">"nein"</option>
                            <option value="ja">"ja (doppel)"</option>
                            <option value="höher">"höher"</option>
                        </select>
                    </label>
                    <label class="field check">
                        <input type="checkbox" prop:checked=move || edit.with(|e| e.as_ref().map(|e| e.valid).unwrap_or(false))
                            on:change=move |ev| { let v = event_target_checked(&ev); edit.update(|e| if let Some(e)=e { e.valid=v; }); } />
                        <span>"Gültig"</span>
                    </label>
                    <label class="field check">
                        <input type="checkbox" prop:checked=move || edit.with(|e| e.as_ref().map(|e| e.paid).unwrap_or(false))
                            on:change=move |ev| { let v = event_target_checked(&ev); edit.update(|e| if let Some(e)=e { e.paid=v; }); } />
                        <span>"Bezahlt"</span>
                    </label>
                </div>
                <div class="modal-actions">
                    <button on:click=move |_| edit.set(None)>"Abbrechen (Esc)"</button>
                    <button class="primary" on:click=move |_| save_edit()>"Speichern (Cmd/Strg+S)"</button>
                </div>
            </div>
        </div>
    }
}


// ── Unterseite: eine Klasse / ein Pool mit editierbaren Nummern ──────────────

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
struct SeedRow {
    pos: i64,
    #[serde(rename = "gpId")]
    gp_id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    club: String,
    #[serde(default)]
    pool: Option<String>,
}

/// Seeding editor + manual placement entry for one bracket. Fighters are
/// reordered via drag & drop (fixed numbers); places 1/2/3/3 are entered via
/// dropdowns restricted to the bracket's own fighters.
pub fn klasse_view(bracket_id: i64) -> impl IntoView {
    let title = RwSignal::new(String::new());
    let has_results = RwSignal::new(false);
    let rows = RwSignal::new(Vec::<SeedRow>::new());
    let msg = RwSignal::new(String::new());
    let drag_from = RwSignal::new(None::<usize>);
    // places as gp ids ("" = leer)
    let p_first = RwSignal::new(String::new());
    let p_second = RwSignal::new(String::new());
    let p_third1 = RwSignal::new(String::new());
    let p_third2 = RwSignal::new(String::new());
    let places_msg = RwSignal::new(String::new());

    let load = move || {
        leptos::task::spawn_local(async move {
            if let Ok(r) = gloo_net::http::Request::get(&format!("/api/brackets/{bracket_id}/seeding")).send().await {
                if let Ok(v) = r.json::<serde_json::Value>().await {
                    title.set(format!(
                        "{} — {}",
                        v["category"].as_str().unwrap_or(""),
                        v["bracketType"].as_str().unwrap_or("?")
                    ));
                    has_results.set(v["hasResults"].as_bool().unwrap_or(false));
                    rows.set(serde_json::from_value(v["rows"].clone()).unwrap_or_default());
                    let g = |k: &str| v["places"][k].as_i64().map(|x| x.to_string()).unwrap_or_default();
                    p_first.set(g("first"));
                    p_second.set(g("second"));
                    p_third1.set(g("third1"));
                    p_third2.set(g("third2"));
                }
            }
        });
    };
    load();

    let drop_at = move |to: usize| {
        let Some(from) = drag_from.get_untracked() else { return };
        drag_from.set(None);
        if from == to {
            return;
        }
        rows.update(|v| {
            let item = v.remove(from);
            let to = to.min(v.len());
            v.insert(to, item);
        });
    };

    let save = move |_| {
        let order: Vec<i64> = rows.get().iter().map(|r| r.gp_id).collect();
        leptos::task::spawn_local(async move {
            let body = json!({ "order": order });
            match gloo_net::http::Request::put(&format!("/api/brackets/{bracket_id}/seeding"))
                .header("content-type", "application/json")
                .body(body.to_string()).unwrap().send().await
            {
                Ok(r) if r.ok() => {
                    msg.set("Umlosung gespeichert".into());
                    load();
                }
                Ok(r) => {
                    let v = r.json::<serde_json::Value>().await.unwrap_or_default();
                    msg.set(v["error"].as_str().unwrap_or("Fehler").to_string());
                }
                Err(e) => msg.set(e.to_string()),
            }
        });
    };

    let save_places = move |_| {
        let to_id = |s: String| s.parse::<i64>().ok();
        let body = json!({
            "first": to_id(p_first.get()), "second": to_id(p_second.get()),
            "third1": to_id(p_third1.get()), "third2": to_id(p_third2.get()),
        });
        leptos::task::spawn_local(async move {
            match gloo_net::http::Request::put(&format!("/api/brackets/{bracket_id}/places"))
                .header("content-type", "application/json")
                .body(body.to_string()).unwrap().send().await
            {
                Ok(r) if r.ok() => {
                    places_msg.set("Platzierungen gespeichert".into());
                    load();
                }
                Ok(r) => {
                    let v = r.json::<serde_json::Value>().await.unwrap_or_default();
                    places_msg.set(v["error"].as_str().unwrap_or("Fehler").to_string());
                }
                Err(e) => places_msg.set(e.to_string()),
            }
        });
    };

    // one placement dropdown: fighters of THIS bracket only
    let place_select = move |label: &'static str, sig: RwSignal<String>| view! {
        <label class="field">
            <span>{label}</span>
            <select prop:value=move || sig.get()
                on:change=move |ev| sig.set(event_target_value(&ev))>
                <option value="">"—"</option>
                {move || rows.get().into_iter().map(|r| {
                    view! {
                        <option value=r.gp_id.to_string() selected=move || sig.get() == r.gp_id.to_string()>
                            {r.name.clone()}
                        </option>
                    }
                }).collect_view()}
            </select>
        </label>
    };

    view! {
        <div class="card toolbar">
            <a href="#/listen">"← Listen & Ergebnisse"</a>
            <h2 style="margin:0;">{move || title.get()}</h2>
            <a href=format!("/print/liste/{bracket_id}") target="_blank">"🖨 Drucken"</a>
            <a href=format!("/api/export/urkunden.xlsx?bracket={bracket_id}") download>"Urkunden (.xlsx)"</a>
            <a href=format!("/api/export/urkunden.csv?bracket={bracket_id}") download>"Urkunden (.csv → Affinity)"</a>
        </div>

        <div class="card">
            <strong>"Sieger eintragen (Platz 1–3)"</strong>
            <p class="muted">"Nur Kämpfer dieser Liste wählbar; Speichern setzt die Liste auf abgeschlossen. Alle leer = zurück auf offen."</p>
            <div class="form-grid" style="max-width:900px;">
                {place_select("1. Platz", p_first)}
                {place_select("2. Platz", p_second)}
                {place_select("3. Platz", p_third1)}
                {place_select("3. Platz", p_third2)}
            </div>
            <div class="toolbar" style="margin-top:0.6rem;">
                <button class="primary" on:click=save_places>"Platzierungen speichern"</button>
                <span class="hint">{move || places_msg.get()}</span>
            </div>
        </div>

        <div class="card">
            {move || if has_results.get() {
                view! { <p class="hint">"Ergebnisse vorhanden — Umlosung gesperrt."</p> }.into_any()
            } else {
                view! { <p class="muted">"Kämpfer per Drag & Drop auf die gewünschte Position ziehen, dann speichern. Die Nummern bleiben fest. Nur möglich, solange kein Ergebnis eingetragen ist."</p> }.into_any()
            }}
            <div class="scroll-x">
                <table>
                    <thead><tr><th></th><th>"Nr"</th><th>"Name"</th><th>"Verein"</th><th>"Pool"</th></tr></thead>
                    <tbody>
                        {move || rows.get().into_iter().enumerate().map(|(i, r)| {
                            view! {
                                <tr class="drag-row"
                                    class:dragging=move || drag_from.get() == Some(i)
                                    draggable="true"
                                    on:dragstart=move |ev: leptos::ev::DragEvent| {
                                        drag_from.set(Some(i));
                                        if let Some(dt) = ev.data_transfer() {
                                            let _ = dt.set_data("text/plain", &i.to_string());
                                        }
                                    }
                                    on:dragover=move |ev: leptos::ev::DragEvent| ev.prevent_default()
                                    on:drop=move |ev: leptos::ev::DragEvent| {
                                        ev.prevent_default();
                                        drop_at(i);
                                    }
                                    on:dragend=move |_| drag_from.set(None)>
                                    <td class="grip">"⠿"</td>
                                    <td>{i + 1}</td>
                                    <td>{r.name.clone()}</td>
                                    <td class="muted">{r.club.clone()}</td>
                                    <td>{r.pool.clone().unwrap_or_default()}</td>
                                </tr>
                            }
                        }).collect_view()}
                    </tbody>
                </table>
            </div>
            <div class="toolbar" style="margin-top:0.5rem;">
                <button class="primary" prop:disabled=move || has_results.get() on:click=save>
                    "Umlosung speichern"
                </button>
                <span class="hint">{move || msg.get()}</span>
            </div>
        </div>
    }
}

// ── Tab: Wettkampflisten (Ansicht + Einzeldruck) ─────────────────────────────

/// Sort keys so filter chips appear in natural order (U9 < U11 < … < 18+,
/// -24kg < … < +66kg). Mixed/None fields always pass a non-empty filter.
fn age_key(a: &str) -> i32 {
    let n: i32 = a.trim_start_matches('U').trim_end_matches('+').parse().unwrap_or(9999);
    if a.starts_with('U') { n } else { 1000 + n } // "18+" etc. after all U-classes
}
fn weight_key(w: &str) -> i32 {
    let n: i32 = w.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0);
    if w.starts_with('+') { 1000 + n } else { n }
}
fn toggle_in(sig: RwSignal<Vec<String>>, v: &str) {
    sig.update(|s| match s.iter().position(|x| x == v) {
        Some(p) => { s.remove(p); }
        None => s.push(v.to_string()),
    });
}

/// One filter line: label + toggle chips (multi-select).
fn chip_row(
    label: &'static str,
    values: impl Fn() -> Vec<String> + Copy + Send + 'static,
    sig: RwSignal<Vec<String>>,
) -> impl IntoView {
    view! {
        <div class="toolbar" style="margin-bottom:0.3rem;">
            <span class="muted" style="width:8rem;">{label}</span>
            {move || values().into_iter().map(|v| {
                let shown = v.clone();
                let active = v.clone();
                view! {
                    <button class="chip" class:active=move || sig.with(|s| s.contains(&active))
                        on:click=move |_| toggle_in(sig, &v)>{shown}</button>
                }
            }).collect_view()}
        </div>
    }
}

/// Filter the generated lists by Altersklasse/Geschlecht/Gewichtsklasse (each
/// multi-select), preview all matching lists in the print layout and print them
/// (one page per list). Mixed youth pools (no gender/weight) match any filter.
pub fn listen_view(
    results: ReadSignal<Vec<BracketResult>>,
    set_results: WriteSignal<Vec<BracketResult>>,
) -> impl IntoView {
    leptos::task::spawn_local(async move { set_results.set(fetch_results().await); });
    let sel_age = RwSignal::new(Vec::<String>::new());
    let sel_gender = RwSignal::new(Vec::<String>::new());
    let sel_weight = RwSignal::new(Vec::<String>::new());
    let frame = NodeRef::<leptos::html::Iframe>::new();

    let listed = move || {
        results.get().into_iter().filter(|b| b.bracket_type.is_some()).collect::<Vec<_>>()
    };
    let ages = move || {
        let mut v: Vec<String> = listed().iter().filter_map(|b| b.age_group.clone()).collect();
        v.sort_by_key(|a| age_key(a));
        v.dedup();
        v
    };
    let genders = move || {
        let mut v: Vec<String> = listed().iter().filter_map(|b| b.gender.clone()).collect();
        v.sort();
        v.dedup();
        v
    };
    let weights = move || {
        let mut v: Vec<String> = listed().iter().filter_map(|b| b.weight_class.clone()).collect();
        v.sort_by_key(|w| weight_key(w));
        v.dedup();
        v
    };

    let matching = move || {
        let (a, g, w) = (sel_age.get(), sel_gender.get(), sel_weight.get());
        let ok = |sel: &Vec<String>, val: &Option<String>| {
            sel.is_empty() || val.as_ref().map(|v| sel.contains(v)).unwrap_or(true)
        };
        listed().into_iter()
            .filter(|b| ok(&a, &b.age_group) && ok(&g, &b.gender) && ok(&w, &b.weight_class))
            .collect::<Vec<_>>()
    };
    let src = move || {
        let ids: Vec<String> = matching().iter().map(|b| b.id.to_string()).collect();
        format!("/print/listen?ids={}", ids.join(","))
    };

    let print = move |_| {
        if let Some(w) = frame.get().and_then(|f| f.content_window()) {
            let _ = w.print();
        }
    };

    view! {
        <div class="card">
            {chip_row("Altersklasse", ages, sel_age)}
            {chip_row("Geschlecht", genders, sel_gender)}
            {chip_row("Gewichtsklasse", weights, sel_weight)}
            <div class="toolbar" style="margin-top:0.5rem;">
                <button class="primary" on:click=print>
                    {move || format!("🖨 Drucken ({} Listen, je 1 Seite)", matching().len())}
                </button>
                <button on:click=move |_| { sel_age.set(vec![]); sel_gender.set(vec![]); sel_weight.set(vec![]); }>
                    "Filter zurücksetzen"
                </button>
            </div>
        </div>
        <iframe class="preview-frame" node_ref=frame src=src />
    }
}

// ── Tab: Listen & Ergebnisse ─────────────────────────────────────────────────

pub fn brackets_view(
    results: ReadSignal<Vec<BracketResult>>,
    set_results: WriteSignal<Vec<BracketResult>>,
) -> impl IntoView {
    // refresh on tab entry
    leptos::task::spawn_local(async move { set_results.set(fetch_results().await); });

    let medal = |s: &str| if s.is_empty() { "—".to_string() } else { s.to_string() };

    // checkbox selection → /print/listen?ids=…, one page per list
    let selected = RwSignal::new(Vec::<i64>::new());
    let toggle = move |id: i64| {
        selected.update(|v| match v.iter().position(|&x| x == id) {
            Some(p) => { v.remove(p); }
            None => v.push(id),
        });
    };
    let print_selected = move |_| {
        let ids = selected.get();
        if ids.is_empty() {
            return;
        }
        let url = format!(
            "/print/listen?ids={}",
            ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
        );
        if let Some(w) = web_sys::window() {
            let _ = w.open_with_url_and_target(&url, "_blank");
        }
    };

    // Generate fights for a bracket, then reload the results.
    let generate = move |bracket_id: i64| {
        leptos::task::spawn_local(async move {
            let _ = gloo_net::http::Request::post(&format!("/api/brackets/{bracket_id}/generate"))
                .send()
                .await;
            set_results.set(fetch_results().await);
        });
    };

    view! {
        <div class="card toolbar">
            <button on:click=move |_| {
                leptos::task::spawn_local(async move {
                    let _ = gloo_net::http::Request::post("/api/assign-groups").send().await;
                    set_results.set(fetch_results().await);
                });
            }>"Gruppen zuordnen (Alters-/Gewichtsklasse)"</button>
            <button class="primary" on:click=move |_| {
                leptos::task::spawn_local(async move {
                    let _ = gloo_net::http::Request::post("/api/brackets/create-all").send().await;
                    set_results.set(fetch_results().await);
                });
            }>"Listen erstellen (Brackets generieren)"</button>
            <button on:click=print_selected>
                {move || format!("🖨 Ausgewählte drucken ({})", selected.get().len())}
            </button>
        </div>

        <div class="card">
            <strong>"Export"</strong>
            <div class="links">
                <a href="/api/export/results.xlsx" download>"Ergebnisliste (.xlsx)"</a>
                <a href="/api/export/results.pdf" download>"Ergebnisliste (.pdf)"</a>
                <a href="/api/export/urkunden.xlsx" download>"Urkunden (.xlsx)"</a>
                <a href="/api/export/urkunden.csv" download>"Urkunden (.csv → Affinity)"</a>
                <a href="/print/listen" target="_blank">"🖨 Alle Wettkampflisten drucken"</a>
            </div>
        </div>

        <div class="card scroll-x">
            <table>
                <thead><tr>
                    <th>"🖨"</th>
                    <th>"Kategorie"</th><th>"Typ"</th><th>"Status"</th>
                    <th>"1."</th><th>"2."</th><th>"3."</th><th>"3."</th><th></th>
                </tr></thead>
                <tbody>
                    {move || results.get().into_iter().map(|b| {
                        let id = b.id;
                        let status = b.status.unwrap_or_default();
                        view! {
                            <tr>
                                <td><input type="checkbox"
                                    prop:checked=move || selected.get().contains(&id)
                                    on:change=move |_| toggle(id) /></td>
                                <td><a href=format!("#/klasse/{id}")>{b.category}</a></td>
                                <td class="muted">{b.bracket_type.unwrap_or_default()}</td>
                                <td><span class=format!("badge {status}")>{status.clone()}</span></td>
                                <td>{medal(&b.first)}</td><td>{medal(&b.second)}</td>
                                <td>{medal(&b.third1)}</td><td>{medal(&b.third2)}</td>
                                <td style="white-space:nowrap;">
                                    <button on:click=move |_| generate(id)>"⚙ Generieren / Neu"</button>
                                    " "
                                    <a href=format!("/print/liste/{id}") target="_blank" title="Liste drucken">"🖨"</a>
                                    " "
                                    <button class="icon" title="Sieger eintragen"
                                        on:click=move |_| { if let Some(w) = web_sys::window() { let _ = w.location().set_hash(&format!("/klasse/{id}")); } }>
                                        "🏆"
                                    </button>
                                </td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
}

// ── Tab: Einstellungen ───────────────────────────────────────────────────────

pub fn settings_view(
    clubs_list: ReadSignal<Vec<ClubInfo>>,
    set_clubs: WriteSignal<Vec<ClubInfo>>,
) -> impl IntoView {
    leptos::task::spawn_local(async move { set_clubs.set(fetch_clubs().await); });
    view! {
        <ConfigPanel/>
        <LocksPanel/>
        {clubs_panel(clubs_list, set_clubs)}
    }
}

/// Club master data: add, rename (cascades onto fighters), delete (blocked
/// while fighters reference the club). Feeds the editor dropdown.
fn clubs_panel(
    clubs_list: ReadSignal<Vec<ClubInfo>>,
    set_clubs: WriteSignal<Vec<ClubInfo>>,
) -> impl IntoView {
    let new_name = RwSignal::new(String::new());
    let new_assoc = RwSignal::new(String::new());
    let msg = RwSignal::new(String::new());

    let add = move || {
        let (n, a) = (new_name.get(), new_assoc.get());
        if n.trim().is_empty() { return; }
        leptos::task::spawn_local(async move {
            let body = json!({ "name": n.trim(), "association": a.trim() });
            let _ = gloo_net::http::Request::post("/api/clubs")
                .header("content-type", "application/json")
                .body(body.to_string()).unwrap().send().await;
            new_name.set(String::new());
            new_assoc.set(String::new());
            set_clubs.set(fetch_clubs().await);
        });
    };
    let save = move |id: i32, name: String, assoc: String| {
        leptos::task::spawn_local(async move {
            let body = json!({ "name": name.trim(), "association": assoc.trim() });
            let r = gloo_net::http::Request::put(&format!("/api/clubs/{id}"))
                .header("content-type", "application/json")
                .body(body.to_string()).unwrap().send().await;
            if let Ok(r) = r {
                if let Ok(v) = r.json::<serde_json::Value>().await {
                    let n = v.get("fightersRenamed").and_then(|x| x.as_u64()).unwrap_or(0);
                    if n > 0 { msg.set(format!("{n} Kämpfer umbenannt")); }
                }
            }
            set_clubs.set(fetch_clubs().await);
        });
    };
    let remove = move |id: i32| {
        leptos::task::spawn_local(async move {
            if let Ok(r) = gloo_net::http::Request::delete(&format!("/api/clubs/{id}")).send().await {
                if r.status() == 409 {
                    msg.set("Verein hat noch Kämpfer — nicht löschbar".into());
                }
            }
            set_clubs.set(fetch_clubs().await);
        });
    };

    view! {
        <section class="card">
            <strong>"Vereine"</strong>
            <span class="hint">{move || msg.get()}</span>
            <table class="compact">
                {move || clubs_list.get().into_iter().map(|c| {
                    // row-local edit buffers, saved via ✓
                    let name = RwSignal::new(c.name.clone());
                    let assoc = RwSignal::new(c.association.clone().unwrap_or_default());
                    let id = c.id;
                    view! {
                        <tr>
                            <td><input prop:value=move || name.get()
                                on:input=move |ev| name.set(event_target_value(&ev)) /></td>
                            <td><input style="width:7rem;" prop:value=move || assoc.get()
                                on:input=move |ev| assoc.set(event_target_value(&ev)) /></td>
                            <td>
                                <button class="icon" title="Speichern" on:click=move |_| save(id, name.get(), assoc.get())>"✓"</button>
                                <button class="icon danger" title="Löschen" on:click=move |_| remove(id)>"🗑"</button>
                            </td>
                        </tr>
                    }
                }).collect_view()}
                <tr>
                    <td><input placeholder="Neuer Verein" prop:value=move || new_name.get()
                        on:input=move |ev| new_name.set(event_target_value(&ev)) /></td>
                    <td><input style="width:7rem;" placeholder="Verband" prop:value=move || new_assoc.get()
                        on:input=move |ev| new_assoc.set(event_target_value(&ev)) /></td>
                    <td><button class="icon" on:click=move |_| add()>"+"</button></td>
                </tr>
            </table>
        </section>
    }
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
        <section class="card">
            <strong>"Alters-Sperren"</strong>
            <ul>
                {move || locks.get().into_iter().map(|l| {
                    let key = l.scope_key.clone();
                    view! { <li>
                        {format!("{} {} — {}", l.gender.clone().unwrap_or_else(|| "alle".into()), l.age_group, l.reason.clone().unwrap_or_default())}
                        " " <button class="icon" on:click=move |_| remove(key.clone())>"entsperren"</button>
                    </li> }
                }).collect_view()}
            </ul>
            <div class="toolbar">
                <span>"Klasse sperren:"</span>
                <input style="width:4.5rem;" placeholder="U15" prop:value=move || age.get()
                    on:input=move |ev| age.set(event_target_value(&ev)) />
                <select prop:value=move || gender.get() on:change=move |ev| gender.set(event_target_value(&ev))>
                    <option value="m">"m"</option>
                    <option value="w">"w"</option>
                    <option value="">"alle"</option>
                </select>
                <button on:click=move |_| add()>"sperren"</button>
            </div>
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
        <section class="card">
            <strong>"Klassen-Konfiguration"</strong>
            {move || cfg.get().map(|c| view! {
                <div class="toolbar" style="margin:.4rem 0;">
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
                <h3>"Generierung U13+: ab N Kämpfern → Methode"</h3>
                <table class="compact">
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
                <h3>"Altersklassen je Jahrgang"<span class="muted">" (mehrere = Doppelstart-Überlappung)"</span></h3>
                <table class="compact">
                    <thead><tr><th>"Jahrgang"</th><th>"Klassen (Komma)"</th><th></th></tr></thead>
                    <tbody>
                        {c.birth_years.iter().enumerate().map(|(i, b)| view! {
                            <tr>
                                <td><input type="number" style="width:5rem;" prop:value=b.year.to_string()
                                    on:change=move |ev| { let v = event_target_value(&ev).parse().unwrap_or(0); cfg.update(|o| if let Some(o)=o { o.birth_years[i].year=v; }); } /></td>
                                <td><input style="width:12rem;" prop:value=b.classes.join(",")
                                    on:change=move |ev| { let v = csv_to_vec(&event_target_value(&ev)); cfg.update(|o| if let Some(o)=o { o.birth_years[i].classes=v; }); } /></td>
                                <td><button class="icon danger" on:click=move |_| cfg.update(|o| if let Some(o)=o { o.birth_years.remove(i); })>"🗑"</button></td>
                            </tr>
                        }).collect_view()}
                    </tbody>
                </table>
                <button on:click=move |_| cfg.update(|o| if let Some(o)=o { o.birth_years.push(BirthRow { year: 2020, classes: vec![] }); })>"+ Jahrgang"</button>
                <h3>"Gewichtsklassen"<span class="muted">" (Geschlecht · Altersklasse · MaxGewicht · Label)"</span></h3>
                <table class="compact">
                    <thead><tr><th>"G"</th><th>"Klasse"</th><th>"≤ kg"</th><th>"Label"</th><th></th></tr></thead>
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
                                <td><button class="icon danger" on:click=move |_| cfg.update(|o| if let Some(o)=o { o.weight_classes.remove(i); })>"🗑"</button></td>
                            </tr>
                        }).collect_view()}
                    </tbody>
                </table>
                <button on:click=move |_| cfg.update(|o| if let Some(o)=o { o.weight_classes.push(WcRow { gender: "m".into(), age_group: "U15".into(), max_weight: 0.0, label: String::new() }); })>"+ Gewichtsklasse"</button>
                <p/>
                <button class="primary" on:click=move |_| save()>"Konfiguration speichern"</button>
            })}
        </section>
    }
}

/// Split a comma-separated input into trimmed, non-empty parts.
fn csv_to_vec(s: &str) -> Vec<String> {
    s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}
