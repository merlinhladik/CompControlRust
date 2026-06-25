// SPDX-License-Identifier: GPL-3.0-or-later
//! Admin read views (Phase 5, slice 1): participant list + results overview.
//! Read-only — no writes to the edv-owned schema yet (import + bracket
//! generation are later slices).

use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use serde_json::{json, Value};

use ccr_db::{app_config, brackets, fights, groups, locks, participants};
use ccr_db::app_config::{AppConfig, BirthYearRow, MethodRange, WeightClassDef};
use ccr_db::participants::{ParticipantEdit, ParticipantUpsert};
use ccr_domain::{ko, pools};
use ccr_excel::config::BracketConfig;
use ccr_excel::contestants;
use ccr_excel::export::{self, ResultRow, UrkundeRow, WiegekarteRow};

use crate::{AppError, AppState};

/// edv tournament_service `_normalize_gender`: male/männlich→m, female/…→w.
fn norm_gender(g: &str) -> Option<String> {
    match g.trim().to_lowercase().as_str() {
        "" => None,
        "m" | "male" | "maennlich" | "männlich" | "mann" => Some("m".into()),
        "w" | "f" | "female" | "weiblich" | "frau" => Some("w".into()),
        other => Some(other.to_string()),
    }
}

/// edv tournament_service doublestart normalize: höher / ja(doppel,…) / else nein.
fn norm_doublestart(d: &str) -> String {
    match d.trim().to_lowercase().as_str() {
        "höher" | "hoeher" | "higher" => "höher",
        "ja" | "yes" | "true" | "1" | "y" | "doppel" | "double" | "duplex" => "ja",
        _ => "nein",
    }
    .to_string()
}

/// Build a full participant record from a JSON body (the in-app fighter editor).
fn parse_edit(b: &Value) -> ParticipantEdit {
    let s = |k: &str| b.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let num = |k: &str| {
        b.get(k).and_then(|v| match v {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().replace(',', ".").parse().ok(),
            _ => None,
        })
    };
    let birthyear = b.get("birthyear").and_then(|v| match v {
        Value::Number(n) => n.as_i64().map(|x| x as i32),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    });
    let ds = s("doublestart");
    ParticipantEdit {
        first_name: s("first_name"),
        last_name: s("last_name"),
        gender: norm_gender(&s("gender")),
        birthyear,
        weight_kg: num("weight"),
        club: s("club"),
        association: s("association"),
        valid: b.get("valid").and_then(|v| v.as_bool()).unwrap_or(false),
        paid: b.get("paid").and_then(|v| v.as_bool()).unwrap_or(false),
        doublestart: norm_doublestart(if ds.is_empty() { "nein" } else { &ds }),
    }
}

/// GET /api/participants — the full athlete list.
pub async fn list_participants(State(st): State<AppState>) -> Result<Json<Value>, AppError> {
    let ps = participants::all(&st.pool).await?;
    Ok(Json(json!({ "count": ps.len(), "participants": ps })))
}

/// GET /api/brackets — every bracket with category label, status and resolved
/// placement NAMES (the admin results overview).
pub async fn list_brackets(State(st): State<AppState>) -> Result<Json<Value>, AppError> {
    let summaries = brackets::all_summaries(&st.pool).await?;

    let mut gp_ids: Vec<i32> = Vec::new();
    for s in &summaries {
        for id in [s.first_place, s.second_place, s.third_place_1, s.third_place_2]
            .into_iter()
            .flatten()
        {
            gp_ids.push(id);
        }
    }
    gp_ids.sort_unstable();
    gp_ids.dedup();
    let people: HashMap<i32, _> = participants::resolve(&st.pool, &gp_ids)
        .await?
        .into_iter()
        .map(|p| (p.gp_id, p))
        .collect();
    let name = |id: Option<i32>| {
        id.and_then(|i| people.get(&i))
            .map(|p| format!("{} {}", p.first_name, p.last_name).trim().to_string())
            .unwrap_or_default()
    };

    let out: Vec<Value> = summaries
        .iter()
        .map(|s| {
            let cat = [s.gender.as_deref(), s.age_group.as_deref(), s.weight_class.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            json!({
                "id": s.id,
                "groupName": s.group_name,
                "category": cat,
                "bracketType": s.bracket_type,
                "status": s.status,
                "first": name(s.first_place),
                "second": name(s.second_place),
                "third1": name(s.third_place_1),
                "third2": name(s.third_place_2),
            })
        })
        .collect();
    Ok(Json(json!({ "count": out.len(), "brackets": out })))
}

/// POST /api/import-contestants — body is a contestants JSON array or CSV text.
/// Upserts into `participants` with the edv mapping (Birthyear→birth_date,
/// gender/doublestart normalize). Returns created/updated counts.
pub async fn import_contestants(
    State(st): State<AppState>,
    body: String,
) -> Result<Json<Value>, AppError> {
    let parsed = if body.trim_start().starts_with('[') {
        contestants::read_json_str(&body)
    } else {
        contestants::read_csv_str(&body)
    }
    .map_err(|e| AppError::status(StatusCode::BAD_REQUEST, format!("parse error: {e}")))?;

    let (mut created, mut updated) = (0u32, 0u32);
    for c in &parsed {
        let up = ParticipantUpsert {
            first_name: c.firstname.clone(),
            last_name: c.lastname.clone(),
            gender: norm_gender(&c.gender),
            birthyear: c.birthyear.map(|y| y as i32),
            weight_kg: c.weight,
            club: c.club.clone(),
            association: c.association.clone(),
            valid: c.valid.unwrap_or(false),
            paid: c.paid.unwrap_or(false),
            doublestart: norm_doublestart(c.doublestart.as_deref().unwrap_or("standard")),
        };
        if participants::upsert(&st.pool, up).await? {
            created += 1;
        } else {
            updated += 1;
        }
    }
    Ok(Json(json!({ "imported": parsed.len(), "created": created, "updated": updated })))
}

// ── Age-class locks (gate admin edit/assign/generate; never the live path) ───

/// GET /api/locks — list active age-class locks.
pub async fn list_locks(State(st): State<AppState>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "locks": locks::all(&st.pool).await? })))
}

/// POST /api/locks {ageGroup, gender?, reason?} — lock a (gender,age) class.
pub async fn add_lock(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let Some(age) = body.get("ageGroup").and_then(|v| v.as_str()) else {
        return Err(AppError::status(StatusCode::BAD_REQUEST, "ageGroup required".into()));
    };
    let gender = body.get("gender").and_then(|v| v.as_str()).and_then(norm_gender);
    let reason = body.get("reason").and_then(|v| v.as_str()).unwrap_or("manual");
    let key = locks::set(&st.pool, age, gender.as_deref(), reason).await?;
    Ok(Json(json!({ "locked": key })))
}

/// DELETE /api/locks/{scope_key} — unlock.
pub async fn remove_lock(
    State(st): State<AppState>,
    Path(scope_key): Path<String>,
) -> Result<Json<Value>, AppError> {
    let n = locks::remove(&st.pool, &scope_key).await?;
    Ok(Json(json!({ "removed": n })))
}

/// PUT /api/participants/{id} — full update of a fighter (the in-app editor:
/// name, gender, birth year, club, weight, valid, paid, doublestart).
/// Blocked (423) if the participant sits ONLY in locked age classes.
pub async fn update_participant(
    State(st): State<AppState>,
    Path(id): Path<i32>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let locked = locks::locked_keys(&st.pool).await?;
    if !locked.is_empty() {
        let memberships = participants::groups_of(&st.pool, id).await?;
        // Granularity: skip only if ALL of the participant's classes are locked
        // (a doublestarter straddling a locked + unlocked class stays editable).
        if !memberships.is_empty()
            && memberships.iter().all(|(g, a)| {
                a.as_deref().map_or(false, |age| locks::is_class_locked(&locked, g.as_deref(), age))
            })
        {
            return Err(AppError::status(
                StatusCode::LOCKED,
                format!("participant {id} is in a locked age class"),
            ));
        }
    }
    let edit = parse_edit(&body);
    let n = participants::update_full(&st.pool, id, &edit).await?;
    if n == 0 {
        return Err(AppError::status(StatusCode::NOT_FOUND, format!("participant {id} not found")));
    }
    Ok(Json(json!({ "status": "ok", "id": id })))
}

/// POST /api/participants — create a new fighter from the in-app editor.
pub async fn create_participant(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let edit = parse_edit(&body);
    if edit.first_name.is_empty() || edit.last_name.is_empty() {
        return Err(AppError::status(StatusCode::BAD_REQUEST, "first_name + last_name required".into()));
    }
    let id = participants::create(&st.pool, &edit).await?;
    Ok(Json(json!({ "status": "ok", "id": id })))
}

/// Weigh-in rows (one per fighter; age class via config when available).
async fn wiegekarte_rows(st: &AppState) -> Result<Vec<WiegekarteRow>, AppError> {
    let cfg = get_config(st).await?;
    Ok(participants::for_wiegekarten(&st.pool)
        .await?
        .into_iter()
        .map(|(id, nachname, vorname, verein, geschlecht, by)| WiegekarteRow {
            id,
            nachname,
            vorname,
            verein,
            geschlecht,
            jahrgang: by.map(|y| y.to_string()).unwrap_or_default(),
            altersklasse: by.and_then(|y| cfg.age_group(y as i64)).unwrap_or_default(),
        })
        .collect())
}

/// GET /api/export/wiegekarten.xlsx — weigh-in list (one row/fighter, blank Gewicht).
pub async fn export_wiegekarten(State(st): State<AppState>) -> Result<Response, AppError> {
    let bytes = export::wiegekarten_xlsx(&wiegekarte_rows(&st).await?)
        .map_err(|e| AppError::status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(xlsx_response("wiegekarten.xlsx", bytes))
}

/// GET /api/export/wiegekarten.pdf — hand-fillable weigh-in cards (blank Gewicht/Unterschrift).
pub async fn export_wiegekarten_pdf(State(st): State<AppState>) -> Result<Response, AppError> {
    Ok(pdf_response("wiegekarten.pdf", ccr_excel::pdf::wiegekarten_pdf(&wiegekarte_rows(&st).await?)))
}

/// GET /api/export/wiegekarten.csv — merge-ready CSV for Affinity Publisher /
/// LibreOffice data merge (one row per fighter; columns = merge field names;
/// no weight). Design the card once, merge → print. Mirrors urkunden.csv.
pub async fn export_wiegekarten_csv(State(st): State<AppState>) -> Result<Response, AppError> {
    let csv = export::wiegekarten_csv(&wiegekarte_rows(&st).await?)
        .map_err(|e| AppError::status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(header::CONTENT_DISPOSITION, "attachment; filename=\"wiegekarten.csv\"")
        .body(Body::from(csv))
        .unwrap())
}

/// GET /print/wiegekarten — printable HTML page of per-athlete weigh-in cards
/// (CSS paged-media 2-up grid). Opened in the browser; the operator prints with
/// Cmd/Ctrl+P (which also saves a PDF). Self-contained, no external assets.
pub async fn print_wiegekarten(State(st): State<AppState>) -> Result<Response, AppError> {
    let html = ccr_excel::print_html::wiegekarten_html(&wiegekarte_rows(&st).await?);
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(html))
        .unwrap())
}

/// POST /api/brackets/{id}/generate — generate fights for a bracket from its
/// group's participants. Slice 1: POOLS (3-5 fighters) only — picks the type by
/// count, and for a pool creates the canonical round-robin schedule. KO / double
/// / repechage generation (club-balanced seeding, byes, pool split) is a later
/// sub-slice and returns a 200 with a "not yet" note without writing.
pub async fn generate_bracket(
    State(st): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    if brackets::find(&st.pool, id).await?.is_none() {
        return Err(AppError::status(StatusCode::NOT_FOUND, format!("bracket {id} not found")));
    }
    // (Re)generation of a locked age class is gated.
    let locked = locks::locked_keys(&st.pool).await?;
    if !locked.is_empty() {
        if let Some((g, Some(age))) = brackets::group_class(&st.pool, id).await? {
            if locks::is_class_locked(&locked, g.as_deref(), &age) {
                return Err(AppError::status(
                    StatusCode::LOCKED,
                    format!("bracket {id} age class '{age}' is locked"),
                ));
            }
        }
    }
    // Regenerate-safe: if the bracket already has fights, wipe + rebuild — but
    // refuse once a real result exists (a finished fight), to protect live data.
    let regenerated = if fights::count_for_bracket(&st.pool, id).await? > 0 {
        if fights::any_finished_for_bracket(&st.pool, id).await? {
            return Err(AppError::status(
                StatusCode::CONFLICT,
                format!("bracket {id} hat bereits Ergebnisse — nicht neu generierbar"),
            ));
        }
        fights::delete_for_bracket(&st.pool, id).await?;
        brackets::reset(&st.pool, id).await?;
        true
    } else {
        false
    };
    let cfg = get_config(&st).await?;
    let mut out = generate_fights_for(&st.pool, id, &cfg).await?;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("regenerated".into(), json!(regenerated));
    }
    Ok(Json(out))
}

/// Core generation: read the bracket's participants, pick the type by the
/// editable config (youth → pools, else the threshold table), create the fights.
/// Shared by the single-bracket endpoint and create-all. (Guards — exists /
/// has-fights / lock — are the caller's responsibility.)
async fn generate_fights_for(
    pool: &ccr_db::PgPoolHandle,
    id: i32,
    cfg: &AppConfig,
) -> Result<Value, AppError> {
    let gps = brackets::group_participant_ids(pool, id).await?;
    let n = gps.len();
    if n == 0 {
        return Ok(json!({ "generated": false, "fighters": 0, "note": "keine Teilnehmer" }));
    }
    let age_group = brackets::group_class(pool, id).await?.and_then(|(_, a)| a);
    let ty = cfg.recommend(n, age_group.as_deref());
    match ty.as_str() {
        // A 1-fighter pool (e.g. a youth weight outlier) → solo, auto 1st place.
        "pools" if n == 1 => {
            brackets::complete_solo(pool, id, gps[0]).await?;
            Ok(json!({ "generated": true, "type": "special", "fighters": 1, "note": "Solo → 1. Platz" }))
        }
        "pools" => {
            let schedule = pools::pool_fight_schedule(n);
            for (i, (a, b)) in schedule.iter().enumerate() {
                fights::create_pool_fight(pool, id, 0, i as i32 + 1, gps[*a], gps[*b]).await?;
            }
            brackets::set_type(pool, id, "pools").await?;
            Ok(json!({ "generated": true, "type": "pools", "fighters": n, "fights": schedule.len() }))
        }
        // KO + repechage: edv seeds only WB round 0 (balanced snake seed + byes);
        // CCR materializes the rest live. Byes = p1==p2 status='bye' winner set.
        "ko" | "repechage" => {
            let parts = brackets::group_participants_with_club(pool, id).await?;
            let pairs = ko::generate_round0(&parts);
            let mut created = 0;
            for (pos, pair) in pairs.iter().enumerate() {
                let (p1, p2, status, winner) = match *pair {
                    (Some(a), Some(b)) => (a, b, "pending", None),
                    (Some(a), None) | (None, Some(a)) => (a, a, "bye", Some(a)),
                    (None, None) => continue,
                };
                fights::create_wb_round0(pool, id, pos as i32, p1, p2, status, winner).await?;
                created += 1;
            }
            brackets::set_type(pool, id, &ty).await?;
            Ok(json!({ "generated": true, "type": ty, "fighters": n, "wbRound0Fights": created }))
        }
        // Doppelpool: two balanced pools (round-robin by club, even split),
        // fight numbers 2-by-2 interleaved; KO stage created live at pool close.
        "double" => {
            let parts = brackets::group_participants_with_club(pool, id).await?;
            let ordered = ko::round_robin_by_club(&parts);
            let split = ordered.len() / 2;
            let pools_split = [&ordered[..split], &ordered[split..]];
            let scheds: Vec<_> = pools_split.iter().map(|p| pools::pool_fight_schedule(p.len())).collect();
            let (nums_a, nums_b) = pools::double_pool_fight_numbers(scheds[0].len(), scheds[1].len());
            let nums = [nums_a, nums_b];
            let mut created = 0;
            for (pi, members) in pools_split.iter().enumerate() {
                for (i, (a, b)) in scheds[pi].iter().enumerate() {
                    fights::create_pool_fight(pool, id, pi as i32, nums[pi][i], members[*a], members[*b]).await?;
                    created += 1;
                }
            }
            brackets::set_type(pool, id, "double").await?;
            Ok(json!({ "generated": true, "type": "double", "fighters": n,
                "poolSizes": [pools_split[0].len(), pools_split[1].len()], "fights": created }))
        }
        // 'special' (<3): solo → 1st place; 2 → best-of-three pool (typed 'pools').
        "special" => match n {
            1 => {
                brackets::complete_solo(pool, id, gps[0]).await?;
                Ok(json!({ "generated": true, "type": "special", "fighters": 1, "note": "Solo → 1. Platz" }))
            }
            2 => {
                let schedule = pools::pool_fight_schedule(2);
                for (i, (a, b)) in schedule.iter().enumerate() {
                    fights::create_pool_fight(pool, id, 0, i as i32 + 1, gps[*a], gps[*b]).await?;
                }
                brackets::set_type(pool, id, "pools").await?;
                Ok(json!({ "generated": true, "type": "pools", "fighters": 2, "fights": schedule.len(), "note": "Best-of-three" }))
            }
            _ => Ok(json!({ "generated": false, "fighters": n })),
        },
        other => Ok(json!({ "generated": false, "recommendedType": other, "fighters": n,
            "note": format!("Generierung für '{other}' noch nicht implementiert") })),
    }
}

/// POST /api/brackets/create-all — for every group with participants and no
/// bracket yet, create one and generate its fights. Locked classes are skipped.
/// This is "Listen erstellen": one click → all categories become scoreable.
pub async fn create_all_brackets(State(st): State<AppState>) -> Result<Json<Value>, AppError> {
    let locked = locks::locked_keys(&st.pool).await?;
    let cfg = get_config(&st).await?;
    let groups = brackets::needing_brackets(&st.pool).await?;
    let (mut created, mut locked_skipped, mut youth_pools) = (0u32, 0u32, 0u32);
    let mut outcomes = Vec::new();
    for (gid, name, gender, age) in groups {
        if let Some(a) = age.as_deref() {
            if locks::is_class_locked(&locked, gender.as_deref(), a) {
                locked_skipped += 1;
                continue;
            }
        }
        // Youth source group (e.g. "U11", mixed gender) → split by weight into
        // pool sub-groups + a bracket each (edv split_u9_u11_into_pools).
        let is_youth_source = age.as_deref().map_or(false, |a| cfg.youth_classes.iter().any(|y| y == a))
            && !name.contains("Pool");
        if is_youth_source {
            let age = age.unwrap();
            // Idempotent: don't re-split if pool groups already exist.
            if groups::exists_named(&st.pool, &format!("{age} | Pool 1")).await? {
                continue;
            }
            let members = groups::members_with_weight(&st.pool, gid).await?;
            let weights: Vec<f64> = members.iter().map(|(_, w)| *w).collect();
            let pools_ix = pools::split_into_pools(
                &weights,
                cfg.youth_pool_size.max(1) as usize,
                cfg.youth_max_weight_spread,
            );
            for (k, pool) in pools_ix.iter().enumerate() {
                let pname = format!("{age} | Pool {}", k + 1);
                let pgid = groups::find_or_create(&st.pool, &pname, None, Some(&age), None).await?;
                for &idx in pool {
                    groups::add_member(&st.pool, pgid, members[idx].0).await?;
                }
                let bid = brackets::create_for_group(&st.pool, pgid).await?;
                let outcome = generate_fights_for(&st.pool, bid, &cfg).await?;
                youth_pools += 1;
                outcomes.push(json!({ "bracketId": bid, "group": pname, "result": outcome }));
            }
            continue;
        }
        let bid = brackets::create_for_group(&st.pool, gid).await?;
        let outcome = generate_fights_for(&st.pool, bid, &cfg).await?;
        created += 1;
        outcomes.push(json!({ "bracketId": bid, "groupId": gid, "result": outcome }));
    }
    Ok(Json(json!({
        "createdBrackets": created, "youthPoolBrackets": youth_pools,
        "lockedSkipped": locked_skipped, "details": outcomes
    })))
}

/// DELETE /api/participants/{id} — remove a fighter (and group memberships).
/// Blocked if the fighter is in a fight/placement (409) or in a locked class (423).
pub async fn delete_participant(
    State(st): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let locked = locks::locked_keys(&st.pool).await?;
    if !locked.is_empty() {
        let memberships = participants::groups_of(&st.pool, id).await?;
        if !memberships.is_empty()
            && memberships.iter().all(|(g, a)| {
                a.as_deref().map_or(false, |age| locks::is_class_locked(&locked, g.as_deref(), age))
            })
        {
            return Err(AppError::status(StatusCode::LOCKED, format!("participant {id} is in a locked class")));
        }
    }
    if participants::is_referenced(&st.pool, id).await? {
        return Err(AppError::status(
            StatusCode::CONFLICT,
            format!("participant {id} competes/placed in a bracket — cannot delete"),
        ));
    }
    let n = participants::delete(&st.pool, id).await?;
    if n == 0 {
        return Err(AppError::status(StatusCode::NOT_FOUND, format!("participant {id} not found")));
    }
    Ok(Json(json!({ "deleted": id })))
}

/// POST /api/assign-groups — classify every participant by age (birth year) +
/// weight via the editable config and add them to the matching (gender|age|class)
/// group (find-or-create, idempotent). Mirrors edv's config-driven assignment,
/// expanding doublestarters into their eligible classes.
pub async fn assign_groups(State(st): State<AppState>) -> Result<Json<Value>, AppError> {
    let cfg = get_config(&st).await?;
    let locked = locks::locked_keys(&st.pool).await?;
    let rows = participants::for_assignment(&st.pool).await?;
    let (mut assigned, mut skipped, mut locked_skipped, mut doublestarts) = (0u32, 0u32, 0u32, 0u32);
    let mut groups_seen = std::collections::HashSet::new();
    for (pid, gender, birth_year, weight, doublestart) in rows {
        let (Some(g), Some(by)) = (gender.as_deref(), birth_year) else {
            skipped += 1;
            continue;
        };
        let eligible = cfg.eligible_age_groups(by as i64);
        if eligible.is_empty() {
            skipped += 1;
            continue;
        }
        // Doublestart flag picks how many eligible classes the athlete enters:
        // 'nein' → primary only; 'höher' → primary + next higher; 'ja' → all.
        let ds = doublestart.as_deref().unwrap_or("nein");
        let targets: &[String] = match ds {
            "ja" => &eligible,
            "höher" | "hoeher" => &eligible[..eligible.len().min(2)],
            _ => &eligible[..1],
        };
        if targets.len() > 1 {
            doublestarts += 1;
        }
        for age in targets {
            // Never (re)assign into a locked age class (per-target granularity).
            if locks::is_class_locked(&locked, Some(g), age) {
                locked_skipped += 1;
                continue;
            }
            let is_youth = cfg.youth_classes.iter().any(|y| y == age);
            // Youth (U9/U11): one mixed-gender, weight-class-free group per age
            // (split into weight pools at list creation). Others: gender|age|class.
            let (name, ggender, gwc) = if is_youth {
                (age.clone(), None, None)
            } else {
                let wc = weight.and_then(|w| cfg.weight_class(g, age, w));
                let label = wc.clone().unwrap_or_else(|| "no-class".into());
                (format!("{g} | {age} | {label}"), Some(g), wc)
            };
            let gid = groups::find_or_create(&st.pool, &name, ggender, Some(age), gwc.as_deref()).await?;
            groups_seen.insert(gid);
            if groups::add_member(&st.pool, gid, pid).await? {
                assigned += 1;
            }
        }
    }
    Ok(Json(json!({
        "assigned": assigned, "skipped": skipped, "lockedSkipped": locked_skipped,
        "doublestarts": doublestarts, "groupsTouched": groups_seen.len()
    })))
}

// ── Excel output (generated .xlsx; not pixel-exact DJB templates) ────────────

fn file_response(content_type: &str, filename: &str, bytes: Vec<u8>) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\""))
        .body(Body::from(bytes))
        .unwrap()
}
fn xlsx_response(filename: &str, bytes: Vec<u8>) -> Response {
    file_response("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", filename, bytes)
}
fn pdf_response(filename: &str, bytes: Vec<u8>) -> Response {
    file_response("application/pdf", filename, bytes)
}

fn place_label(i: usize) -> String {
    // placements vec is [1st, 2nd, 3rd, 3rd]
    match i {
        0 => "1.",
        1 => "2.",
        _ => "3.",
    }
    .to_string()
}

/// All categories + resolved placement names (results data source).
async fn result_rows(st: &AppState) -> Result<Vec<ResultRow>, AppError> {
    let summaries = brackets::all_summaries(&st.pool).await?;
    let people = resolve_placement_people(st, &summaries).await?;
    let name = |id: Option<i32>| {
        id.and_then(|i| people.get(&i))
            .map(|p: &ccr_db::participants::ResolvedParticipant| format!("{} {}", p.first_name, p.last_name).trim().to_string())
            .unwrap_or_default()
    };
    Ok(summaries
        .iter()
        .map(|s| {
            let cat = [s.gender.as_deref(), s.age_group.as_deref(), s.weight_class.as_deref()]
                .into_iter().flatten().collect::<Vec<_>>().join(" ");
            ResultRow {
                kategorie: if cat.trim().is_empty() { s.group_name.clone() } else { cat },
                typ: s.bracket_type.clone().unwrap_or_default(),
                first: name(s.first_place), second: name(s.second_place),
                third1: name(s.third_place_1), third2: name(s.third_place_2),
            }
        })
        .collect())
}

/// GET /api/export/results.xlsx
pub async fn export_results(State(st): State<AppState>) -> Result<Response, AppError> {
    let bytes = export::results_xlsx(&result_rows(&st).await?)
        .map_err(|e| AppError::status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(xlsx_response("ergebnisse.xlsx", bytes))
}

/// GET /api/export/results.pdf — printed results list.
pub async fn export_results_pdf(State(st): State<AppState>) -> Result<Response, AppError> {
    Ok(pdf_response("ergebnisse.pdf", ccr_excel::pdf::results_pdf(&result_rows(&st).await?)))
}

/// One row per awarded placement of every completed bracket (Urkunden source).
async fn urkunden_rows(st: &AppState) -> Result<Vec<UrkundeRow>, AppError> {
    let summaries = brackets::all_summaries(&st.pool).await?;
    let people = resolve_placement_people(st, &summaries).await?;
    let mut rows = Vec::new();
    for s in &summaries {
        if s.status.as_deref() != Some("completed") {
            continue;
        }
        let places = [s.first_place, s.second_place, s.third_place_1, s.third_place_2];
        for (i, gp) in places.iter().enumerate() {
            let Some(p) = gp.and_then(|id| people.get(&id)) else { continue };
            rows.push(UrkundeRow {
                vorname: p.first_name.clone(),
                nachname: p.last_name.clone(),
                platz: place_label(i),
                klasse: s.group_name.clone(),
                verein: p.club.clone().unwrap_or_default(),
            });
        }
    }
    Ok(rows)
}

/// GET /api/export/urkunden.xlsx — one row per awarded placement (completed only).
pub async fn export_urkunden(State(st): State<AppState>) -> Result<Response, AppError> {
    let rows = urkunden_rows(&st).await?;
    let bytes = export::urkunden_xlsx(&rows)
        .map_err(|e| AppError::status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(xlsx_response("urkunden.xlsx", bytes))
}

/// GET /api/export/urkunden.csv — merge-ready CSV for Affinity / LibreOffice
/// data merge (one row per placement; columns = merge field names).
pub async fn export_urkunden_csv(State(st): State<AppState>) -> Result<Response, AppError> {
    let rows = urkunden_rows(&st).await?;
    let csv = export::urkunden_csv(&rows)
        .map_err(|e| AppError::status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(header::CONTENT_DISPOSITION, "attachment; filename=\"urkunden.csv\"")
        .body(Body::from(csv))
        .unwrap())
}

/// Resolve all placement gp ids across the given brackets to participant data.
async fn resolve_placement_people(
    st: &AppState,
    summaries: &[brackets::BracketSummary],
) -> Result<HashMap<i32, ccr_db::participants::ResolvedParticipant>, AppError> {
    let mut gp_ids: Vec<i32> = Vec::new();
    for s in summaries {
        for id in [s.first_place, s.second_place, s.third_place_1, s.third_place_2].into_iter().flatten() {
            gp_ids.push(id);
        }
    }
    gp_ids.sort_unstable();
    gp_ids.dedup();
    Ok(participants::resolve(&st.pool, &gp_ids).await?.into_iter().map(|p| (p.gp_id, p)).collect())
}

// ── Editable tournament config (DB-backed; seeded from bracket_config.xlsx) ───

/// Build the initial config from the xlsx (age/weight/birthyears) + defaults
/// (thresholds, youth pools). edv defaults per Merlin: U15+ ≥5 pool / ≥8 double /
/// larger KO; U9/U11 only 4er pools.
fn seed_config(bc: Option<&BracketConfig>) -> AppConfig {
    let (event_year, age_classes, birth_years, weight_classes) = match bc {
        Some(bc) => (
            if bc.event_year > 0 { bc.event_year } else { 2026 },
            bc.age_classes().to_vec(),
            bc.birth_year_rows().into_iter().map(|(year, classes)| BirthYearRow { year, classes }).collect(),
            bc.weight_class_defs().into_iter()
                .map(|(gender, age_group, max_weight, label)| WeightClassDef { gender, age_group, max_weight, label })
                .collect(),
        ),
        None => (2026, vec!["U9", "U11", "U13", "U15", "U18", "18+"].into_iter().map(String::from).collect(), vec![], vec![]),
    };
    AppConfig {
        event_year,
        age_classes,
        youth_classes: vec!["U9".into(), "U11".into()],
        youth_pool_size: 4,
        youth_max_weight_spread: 0.0, // off by default; set in the UI to weight-cut

        adult_methods: vec![
            MethodRange { min_fighters: 0, method: "special".into() },
            MethodRange { min_fighters: 5, method: "pools".into() },
            MethodRange { min_fighters: 8, method: "double".into() },
            MethodRange { min_fighters: 16, method: "ko".into() },
            MethodRange { min_fighters: 33, method: "repechage".into() },
        ],
        birth_years,
        weight_classes,
    }
}

/// Load the editable config, seeding it from the xlsx + defaults on first use.
pub async fn get_config(st: &AppState) -> Result<AppConfig, AppError> {
    if let Some(c) = app_config::load(&st.pool).await? {
        return Ok(c);
    }
    let path = std::env::var("CCR_BRACKET_CONFIG")
        .unwrap_or_else(|_| "../edv/config/bracket_config.xlsx".into());
    let bc = BracketConfig::load(&path).ok();
    let cfg = seed_config(bc.as_ref());
    app_config::save(&st.pool, &cfg).await?;
    Ok(cfg)
}

/// GET /api/config — the editable tournament config.
pub async fn get_config_handler(State(st): State<AppState>) -> Result<Json<Value>, AppError> {
    Ok(Json(serde_json::to_value(get_config(&st).await?).unwrap()))
}

/// PUT /api/config — replace the config (full document).
pub async fn put_config(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let cfg: AppConfig = serde_json::from_value(body)
        .map_err(|e| AppError::status(StatusCode::BAD_REQUEST, format!("invalid config: {e}")))?;
    app_config::save(&st.pool, &cfg).await?;
    Ok(Json(json!({ "status": "ok" })))
}
