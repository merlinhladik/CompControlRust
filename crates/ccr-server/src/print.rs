// SPDX-License-Identifier: GPL-3.0-or-later
//! Printable Wettkampflisten (HTML, browser print → paper/PDF).
//!
//! `/print/liste/:bracket_id` = one bracket, `/print/listen` = all brackets
//! (page break per bracket). Layout follows the DJB paper forms in spirit:
//! pools as fighter-rows × bout-columns grid in canonical run order
//! (`pool_fight_schedule`), KO/Doppel-KO/Repechage as round columns. Cells stay
//! blank while a fight is open — the sheets are hand-fillable.

use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
};

use ccr_db::{brackets, brackets::BracketSummary, fights, models::Fight, participants};
use ccr_domain::{doppel_ko, ko, ko32, ko_big, pools, repechage};

use crate::{AppError, AppState};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

struct Person {
    name: String,
    club: String,
}
type People = HashMap<i32, Person>;

async fn people_for(st: &AppState, fights: &[Fight], s: &BracketSummary) -> Result<People, AppError> {
    let mut gp_ids: Vec<i32> = fights
        .iter()
        .flat_map(|f| [f.participant1_id, f.participant2_id, f.winner_id])
        .flatten()
        .collect();
    gp_ids.extend([s.first_place, s.second_place, s.third_place_1, s.third_place_2].into_iter().flatten());
    gp_ids.sort_unstable();
    gp_ids.dedup();
    Ok(participants::resolve(&st.pool, &gp_ids)
        .await?
        .into_iter()
        .map(|p| {
            (p.gp_id, Person {
                name: format!("{} {}", p.first_name, p.last_name).trim().to_string(),
                club: p.club.unwrap_or_default(),
            })
        })
        .collect())
}

fn type_label(t: &str) -> &'static str {
    match t {
        "pools" => "Pool (Jeder gegen Jeden)",
        "double" => "Doppelpool",
        "ko" => "Doppel-KO",
        "repechage" => "KO mit doppelter Trostrunde",
        "special" => "Sonderwertung",
        _ => "",
    }
}

/// Placement label for a gp id ("1." … "3.") from the bracket record.
fn platz_of(s: &BracketSummary, gp: i32) -> &'static str {
    if s.first_place == Some(gp) { "1." }
    else if s.second_place == Some(gp) { "2." }
    else if s.third_place_1 == Some(gp) || s.third_place_2 == Some(gp) { "3." }
    else { "" }
}

/// One pool as the DJB-style grid: rows = fighters in slot order, one column
/// per bout in run order (header shows fight number + the pairing as row
/// numbers), then Siege + Platz.
fn pool_grid(title: &str, ctx: Option<&str>, pool_fights: &[Fight], people: &People, s: &BracketSummary) -> String {
    let mut fs: Vec<&Fight> = pool_fights.iter().collect();
    fs.sort_by_key(|f| f.fight_number.unwrap_or(f.id));

    // Reconstruct the slot order from the canonical schedule; fall back to
    // first-seen order if the fight list doesn't match (edited brackets).
    let mut seen: Vec<i32> = Vec::new();
    for f in &fs {
        for gp in [f.participant1_id, f.participant2_id].into_iter().flatten() {
            if !seen.contains(&gp) {
                seen.push(gp);
            }
        }
    }
    let n = seen.len();
    let sched = pools::pool_fight_schedule(n);
    let mut slots: Vec<Option<i32>> = vec![None; n];
    let mut pairs: Vec<(usize, usize)> = Vec::new(); // per fight: (row a, row b)
    if sched.len() == fs.len() {
        for (i, f) in fs.iter().enumerate() {
            let (a, b) = sched[i];
            slots[a] = f.participant1_id;
            slots[b] = f.participant2_id;
            pairs.push((a, b));
        }
    } else {
        slots = seen.iter().copied().map(Some).collect();
        for f in &fs {
            let a = seen.iter().position(|&g| Some(g) == f.participant1_id).unwrap_or(0);
            let b = seen.iter().position(|&g| Some(g) == f.participant2_id).unwrap_or(0);
            pairs.push((a, b));
        }
    }

    // wins per gp (finished fights only; draws count for nobody — DJB)
    let mut wins: HashMap<i32, u32> = HashMap::new();
    for f in &fs {
        if let Some(w) = f.winner_id {
            *wins.entry(w).or_default() += 1;
        }
    }

    let mut h = String::new();
    let ctx_html = ctx.map(|c| format!(" <span class=\"ctx\">— {}</span>", esc(c))).unwrap_or_default();
    h.push_str(&format!("<div class=\"part\"><h2>{}{}</h2><table class=\"pool\"><tr><th>Nr</th><th class=\"nm\">Name</th><th class=\"nm\">Verein</th>", esc(title), ctx_html));
    for (i, f) in fs.iter().enumerate() {
        let (a, b) = pairs[i];
        h.push_str(&format!(
            "<th>K{}<div class=\"pair\">{}–{}</div></th>",
            f.fight_number.unwrap_or(f.id), a + 1, b + 1
        ));
    }
    h.push_str("<th>Siege</th><th>Platz</th></tr>");

    for (row, slot) in slots.iter().enumerate() {
        let (name, club) = slot
            .and_then(|gp| people.get(&gp))
            .map(|p| (p.name.clone(), p.club.clone()))
            .unwrap_or_default();
        h.push_str(&format!("<tr><td>{}</td><td class=\"nm\">{}</td><td class=\"nm\">{}</td>", row + 1, esc(&name), esc(&club)));
        for (i, f) in fs.iter().enumerate() {
            let (a, b) = pairs[i];
            if row != a && row != b {
                h.push_str("<td class=\"x\"></td>");
                continue;
            }
            let me = if row == a { f.participant1_id } else { f.participant2_id };
            let score = if row == a { f.score1 } else { f.score2 };
            let finished = matches!(f.status.as_deref(), Some("finished") | Some("completed"));
            let won = finished && f.winner_id.is_some() && f.winner_id == me;
            h.push_str(&format!(
                "<td class=\"b{}\">{}</td>",
                if won { " w" } else { "" },
                if finished { score.unwrap_or(0).to_string() } else { String::new() }
            ));
        }
        let (w, platz) = slot
            .map(|gp| (wins.get(&gp).map(|w| w.to_string()).unwrap_or_default(), platz_of(s, gp)))
            .unwrap_or_default();
        h.push_str(&format!("<td class=\"b\">{w}</td><td class=\"b\">{platz}</td></tr>"));
    }
    h.push_str("</table></div>");
    h
}

/// Fighter name (+ small club) as SVG text content for a slot line.
fn svg_name(gp: Option<i32>, people: &People) -> String {
    match gp.and_then(|g| people.get(&g)) {
        None => String::new(),
        Some(p) => {
            if p.club.is_empty() {
                esc(&p.name)
            } else {
                format!("{} <tspan font-size=\"9\" fill=\"#444\">({})</tspan>", esc(&p.name), esc(&p.club))
            }
        }
    }
}

/// Hauptrunden-Turnierbaum as the classic KO-Spielplan: one horizontal line per
/// slot, pairs joined by a vertical connector that feeds the next round's line
/// (midpoint recursion). One SVG, auto-scaled to the page — prints crisp.
fn wb_tree_svg(wb: &[Fight], people: &People) -> String {
    if wb.is_empty() {
        return String::new();
    }
    let rmax = wb.iter().map(|f| f.round.unwrap_or(0)).max().unwrap_or(0) as usize;
    let nrounds = rmax + 1; // wb COLUMNS - a 32er/64er has no wb final (it lives in lb)
    let mut at: HashMap<(usize, i32), &Fight> = HashMap::new();
    for f in wb {
        at.insert((f.round.unwrap_or(0) as usize, f.pos_in_round.unwrap_or(0)), f);
    }
    // fighter slots incl. byes, from the round-0 WIDTH (not the round count -
    // rmax+1 undercounts the 32er/64er, whose final is a lb-phase fight)
    let n0 = wb
        .iter()
        .filter(|f| f.round.unwrap_or(0) == 0)
        .map(|f| f.pos_in_round.unwrap_or(0))
        .max()
        .unwrap_or(0) as usize
        + 1;
    let slots0 = (2 * n0).next_power_of_two();
    let (mut u, mut colw, top, pad) = (48.0f64, 210.0f64, 26.0f64, 12.0f64);
    // Fill the printable page (A4 landscape ~277x175mm): tall trees get wider
    // columns, flat trees a larger line pitch - capped so small brackets don't
    // stretch absurdly.
    const ASPECT: f64 = 277.0 / 160.0;
    {
        let w = pad * 2.0 + (nrounds as f64 + 1.0) * colw;
        let h = top + pad * 2.0 + u * (slots0 - 1).max(1) as f64 + 14.0;
        if w / h > ASPECT {
            u = ((w / ASPECT - top - pad * 2.0 - 14.0) / (slots0 - 1).max(1) as f64).min(110.0);
        } else {
            colw = (((h * ASPECT) - pad * 2.0) / (nrounds as f64 + 1.0)).min(380.0);
        }
    }
    let y = |r: usize, i: usize| {
        let p = (1u64 << r) as f64;
        top + pad + u * (p * i as f64 + (p - 1.0) / 2.0)
    };
    let x0 = |r: usize| pad + r as f64 * colw + 8.0;
    let x1 = |r: usize| pad + (r + 1) as f64 * colw - 8.0;
    let width = pad * 2.0 + (nrounds as f64 + 1.0) * colw;
    let height = top + pad * 2.0 + u * (slots0 - 1) as f64 + 14.0;

    // slot content: r==0 from the round-0 fight's p1/p2; r>0 the feeder's winner
    let slot_gp = |r: usize, i: usize| -> (Option<i32>, bool) {
        if r == 0 {
            match at.get(&(0, (i / 2) as i32)) {
                Some(f) if f.status.as_deref() == Some("bye") && f.participant1_id == f.participant2_id => {
                    if i % 2 == 0 { (f.participant1_id, false) } else { (None, true) }
                }
                Some(f) => (if i % 2 == 0 { f.participant1_id } else { f.participant2_id }, false),
                None => (None, false),
            }
        } else {
            (at.get(&(r - 1, i as i32)).and_then(|f| f.winner_id), false)
        }
    };

    let mut s = format!(
        "<svg viewBox=\"0 0 {width:.0} {height:.0}\" style=\"width:100%;height:auto;max-height:160mm;\" preserveAspectRatio=\"xMinYMin meet\" xmlns=\"http://www.w3.org/2000/svg\" font-family=\"system-ui, sans-serif\">"
    );
    for r in 0..nrounds {
        let label: String = match slots0 >> (r + 1) {
            1 => "Finale".into(),
            2 => "Halbfinale".into(),
            4 => "Viertelfinale".into(),
            _ => format!("Runde {}", r + 1),
        };
        s += &format!("<text x=\"{:.0}\" y=\"16\" font-size=\"12\" font-weight=\"bold\">{label}</text>", x0(r));
    }
    let last_label = if slots0 >> nrounds == 1 { "Sieger" } else { "Medaillenrunde" };
    s += &format!("<text x=\"{:.0}\" y=\"16\" font-size=\"12\" font-weight=\"bold\">{last_label}</text>", x0(nrounds));
    // slot lines + names (last column = winner line)
    for r in 0..=nrounds {
        let n = slots0 >> r;
        for i in 0..n {
            let yy = y(r, i);
            s += &format!(
                "<line x1=\"{:.0}\" y1=\"{yy:.1}\" x2=\"{:.0}\" y2=\"{yy:.1}\" stroke=\"#000\" stroke-width=\"1.6\"/>",
                x0(r), x1(r)
            );
            let (gp, bye) = slot_gp(r, i);
            if bye {
                s += &format!(
                    "<text x=\"{:.0}\" y=\"{:.1}\" font-size=\"11\" font-style=\"italic\" fill=\"#555\">Freilos</text>",
                    x0(r) + 2.0, yy - 5.0
                );
            } else {
                let nm = svg_name(gp, people);
                if !nm.is_empty() {
                    s += &format!("<text x=\"{:.0}\" y=\"{:.1}\" font-size=\"13\">{nm}</text>", x0(r) + 2.0, yy - 5.0);
                }
            }
        }
    }
    // pair connectors + bridge to the next slot line + Kampfnummer
    for r in 0..nrounds {
        let nf = slots0 >> (r + 1);
        for k in 0..nf {
            let (ya, yb) = (y(r, 2 * k), y(r, 2 * k + 1));
            let ym = y(r + 1, k);
            let xc = x1(r);
            s += &format!("<line x1=\"{xc:.0}\" y1=\"{ya:.1}\" x2=\"{xc:.0}\" y2=\"{yb:.1}\" stroke=\"#000\" stroke-width=\"1.6\"/>");
            s += &format!(
                "<line x1=\"{xc:.0}\" y1=\"{ym:.1}\" x2=\"{:.0}\" y2=\"{ym:.1}\" stroke=\"#000\" stroke-width=\"1.6\"/>",
                x0(r + 1)
            );
            if let Some(f) = at.get(&(r, k as i32)) {
                if let Some(nr_) = f.fight_number {
                    s += &format!(
                        "<text x=\"{:.0}\" y=\"{:.1}\" font-size=\"10\" fill=\"#333\" text-anchor=\"end\">K{nr_}</text>",
                        xc - 4.0, (ya + yb) / 2.0 + 4.0
                    );
                }
            }
        }
    }
    s += "</svg>";
    s
}

/// Collected topology edges for the consolation diagram of one bracket.
struct KoEdges {
    /// intra-phase winner edges (src r,p) -> (dst r,p, slot 1|2)
    intra: Vec<((i32, i32), (i32, i32), i32)>,
    /// labels for cross-phase drop-ins: (dst r, dst p, slot) -> text
    labels: HashMap<(i32, i32, i32), String>,
}

/// Build the real feeder edges for the lb/rep phase from ccr-domain topology.
/// `nrounds` = log2(draw size); `wbnr` maps wb (round,pos) -> Kampfnummer.
fn ko_edges(ty: &str, phase: &str, nrounds: u32, wbnr: &HashMap<(i32, i32), (i32, bool)>) -> KoEdges {
    let mut winner: Vec<(ko32::Node, ko32::Node, i32)> = Vec::new();
    let mut loser: Vec<(ko32::Node, ko32::Node, i32)> = Vec::new();
    let mut plost: Vec<((u8, i32), (ko32::Node, i32))> = Vec::new();
    let slot_nr = |sl: ko::Slot| if sl == ko::Slot::P1 { 1 } else { 2 };

    if ty == "repechage" {
        if repechage::is_supported(nrounds) {
            for n in repechage::all_nodes(nrounds) {
                for (t, slot, k) in repechage::consumers(nrounds, n) {
                    match k {
                        ko32::Kind::Winner => winner.push((n, t, slot)),
                        ko32::Kind::Loser => loser.push((n, t, slot)),
                    }
                }
            }
            plost = repechage::plost_targets(nrounds);
        }
    } else {
        match nrounds {
            3 | 4 => {
                for (r, &sz) in doppel_ko::lb_round_sizes(nrounds).iter().enumerate() {
                    for p in 0..sz {
                        if let Some((r2, p2, sl)) = doppel_ko::lb_advance(nrounds, r as i32, p) {
                            winner.push((("lb", r as i32, p), ("lb", r2, p2), slot_nr(sl)));
                        }
                    }
                }
                for r in 0..nrounds as i32 {
                    let nf = 1i32 << (nrounds as i32 - 1 - r);
                    for p in 0..nf {
                        if let Some((r2, p2, sl)) = doppel_ko::wb_drop(nrounds, r, p) {
                            loser.push((("wb", r, p), ("lb", r2, p2), slot_nr(sl)));
                        }
                    }
                }
            }
            _ if ko_big::is_supported(nrounds) => {
                for n in ko_big::all_nodes(nrounds) {
                    for (t, slot, k) in ko_big::consumers(nrounds, n) {
                        match k {
                            ko32::Kind::Winner => winner.push((n, t, slot)),
                            ko32::Kind::Loser => loser.push((n, t, slot)),
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let mut e = KoEdges { intra: Vec::new(), labels: HashMap::new() };
    for (sn, dn, slot) in &winner {
        if sn.0 == phase && dn.0 == phase {
            e.intra.push(((sn.1, sn.2), (dn.1, dn.2), *slot));
        } else if dn.0 == phase && sn.0 == "wb" {
            let (nr_, _) = wbnr.get(&(sn.1, sn.2)).copied().unwrap_or((0, false));
            e.labels.insert((dn.1, dn.2, *slot), format!("Sieger K{nr_}"));
        }
    }
    for (sn, dn, slot) in &loser {
        if dn.0 == phase && sn.0 == "wb" {
            // a bye produces no loser - mark the dead slot instead of pointing
            // at a Freilos fight
            let (nr_, bye) = wbnr.get(&(sn.1, sn.2)).copied().unwrap_or((0, false));
            let lbl = if bye { "Freilos".to_string() } else { format!("Verlierer K{nr_}") };
            e.labels.insert((dn.1, dn.2, *slot), lbl);
        }
    }
    for ((pool, level), (dn, slot)) in &plost {
        if dn.0 == phase {
            let letter = (b'A' + *pool) as char;
            e.labels.insert((dn.1, dn.2, *slot), format!("Pool {} {}", letter, "*".repeat(*level as usize)));
        }
    }
    e
}

/// Trostrunden-/Repechage-Baum in the Hauptrunden line style: slot lines, pair
/// connectors, real winner edges (incl. Judo-cross) as elbow polylines. A fight
/// sits at the vertical MIDPOINT of its two feeder fights (Hauptrunden rule);
/// a single-feeder fight aligns that slot line with its feeder so the
/// connector runs straight. WB drop-in slots carry their sheet label
/// ("Verlierer K13", "Pool A *") until a name is known.
fn lb_tree_svg(fights: &[Fight], people: &People, edges: &KoEdges) -> String {
    if fights.is_empty() {
        return String::new();
    }
    let mut fs: Vec<&Fight> = fights.iter().collect();
    fs.sort_by_key(|f| (f.round.unwrap_or(0), f.pos_in_round.unwrap_or(0)));
    let mut rounds: Vec<(i32, Vec<&Fight>)> = Vec::new();
    for f in fs {
        let r = f.round.unwrap_or(0);
        match rounds.last_mut() {
            Some((k, v)) if *k == r => v.push(f),
            _ => rounds.push((r, vec![f])),
        }
    }
    let (u, mut colw, top, pad) = (44.0f64, 240.0f64, 26.0f64, 12.0f64);

    // pass 1 — positions (midpoint of feeders / straight-line alignment)
    let mut geo: HashMap<(i32, i32), (f64, f64, f64, usize)> = HashMap::new(); // ya, yb, yc, col
    for (c, (r, v)) in rounds.iter().enumerate() {
        let mut cursor = top + pad;
        for f in v {
            let p = f.pos_in_round.unwrap_or(0);
            let feed = |slot: i32| {
                edges.intra.iter()
                    .find(|(_, d, sl)| *d == (*r, p) && *sl == slot)
                    .and_then(|(src, _, _)| geo.get(src))
                    .map(|g| g.2)
            };
            let (f1, f2) = (feed(1), feed(2));
            let (mut ya, mut yb) = match (f1, f2) {
                (Some(a), Some(b)) if b > a + 8.0 => (a, b),
                (Some(a), Some(b)) => { let m = (a + b) / 2.0; (m - u / 2.0, m + u / 2.0) }
                (Some(a), None) => (a, a + u),
                (None, Some(b)) => (b - u, b),
                (None, None) => (cursor, cursor + u),
            };
            if ya < cursor {
                let d = cursor - ya;
                ya += d;
                yb += d;
            }
            geo.insert((*r, p), (ya, yb, (ya + yb) / 2.0, c));
            cursor = yb + 30.0;
        }
    }
    // Fill the printable page: stretch columns to the A4-landscape aspect and
    // the fight positions vertically (uniform, connectors follow), with caps.
    const ASPECT: f64 = 277.0 / 168.0;
    let content_h = geo.values().fold(0.0f64, |m, g| m.max(g.1)) - (top + pad);
    let nat_h = top + pad * 2.0 + content_h + 14.0;
    let nat_w = pad * 2.0 + rounds.len() as f64 * colw;
    let mut fy = 1.0f64;
    if nat_w / nat_h > ASPECT {
        fy = ((nat_w / ASPECT - top - pad * 2.0 - 14.0) / content_h.max(1.0)).min(2.2);
    } else {
        colw = (((nat_h * ASPECT) - pad * 2.0) / rounds.len() as f64).min(420.0);
    }
    let ty = move |y: f64| top + pad + (y - top - pad) * fy;
    let span = (colw - 100.0).max(120.0);
    let width = pad * 2.0 + rounds.len() as f64 * colw;
    let xa = |c: usize| pad + c as f64 * colw + 8.0;
    let height = top + pad * 2.0 + content_h * fy + 14.0;
    let has_out = |r: i32, p: i32| edges.intra.iter().any(|(s, _, _)| *s == (r, p));

    // pass 2 — draw
    let mut s = format!(
        "<svg viewBox=\"0 0 {width:.0} {height:.0}\" style=\"width:100%;height:auto;max-height:168mm;\" preserveAspectRatio=\"xMinYMin meet\" xmlns=\"http://www.w3.org/2000/svg\" font-family=\"system-ui, sans-serif\">"
    );
    for (c, (r, v)) in rounds.iter().enumerate() {
        let x0 = xa(c);
        let x1 = x0 + span;
        s += &format!(
            "<text x=\"{x0:.0}\" y=\"16\" font-size=\"12\" font-weight=\"bold\">Runde {}</text>",
            r + 1
        );
        for f in v {
            let p = f.pos_in_round.unwrap_or(0);
            let (ya, yb, yc, _) = geo[&(*r, p)];
            let (ya, yb, yc) = (ty(ya), ty(yb), ty(yc));
            let bye = f.status.as_deref() == Some("bye") && f.participant1_id == f.participant2_id;
            for (li, (yy, gp)) in [(ya, f.participant1_id), (yb, f.participant2_id)].into_iter().enumerate() {
                s += &format!("<line x1=\"{x0:.0}\" y1=\"{yy:.1}\" x2=\"{x1:.0}\" y2=\"{yy:.1}\" stroke=\"#000\" stroke-width=\"1.6\"/>");
                let nm = if bye && li == 1 { String::new() } else { svg_name(gp, people) };
                if !nm.is_empty() {
                    s += &format!("<text x=\"{:.0}\" y=\"{:.1}\" font-size=\"13\">{nm}</text>", x0 + 2.0, yy - 5.0);
                } else if bye && li == 1 {
                    s += &format!("<text x=\"{:.0}\" y=\"{:.1}\" font-size=\"11\" font-style=\"italic\" fill=\"#555\">Freilos</text>", x0 + 2.0, yy - 5.0);
                } else if let Some(lbl) = edges.labels.get(&(*r, p, li as i32 + 1)) {
                    s += &format!("<text x=\"{:.0}\" y=\"{:.1}\" font-size=\"10\" font-style=\"italic\" fill=\"#666\">{}</text>", x0 + 2.0, yy - 5.0, esc(lbl));
                }
            }
            s += &format!("<line x1=\"{x1:.0}\" y1=\"{ya:.1}\" x2=\"{x1:.0}\" y2=\"{yb:.1}\" stroke=\"#000\" stroke-width=\"1.6\"/>");
            if let Some(nr_) = f.fight_number {
                s += &format!("<text x=\"{:.0}\" y=\"{:.1}\" font-size=\"10\" fill=\"#333\" text-anchor=\"end\">K{nr_}</text>", x1 - 4.0, yc + 4.0);
            }
            if !has_out(*r, p) {
                s += &format!("<line x1=\"{x1:.0}\" y1=\"{yc:.1}\" x2=\"{:.0}\" y2=\"{yc:.1}\" stroke=\"#000\" stroke-width=\"1.6\"/>", x1 + 60.0);
                let wn = svg_name(f.winner_id, people);
                if !wn.is_empty() {
                    s += &format!("<text x=\"{:.0}\" y=\"{:.1}\" font-size=\"12\">{wn}</text>", x1 + 4.0, yc - 5.0);
                }
            }
        }
    }
    // real winner edges as elbow polylines (straight when heights align)
    for ((sr, sp), (dr, dp), slot) in &edges.intra {
        let (Some(&(_, _, syc, sc)), Some(&(dya, dyb, _, dc))) = (geo.get(&(*sr, *sp)), geo.get(&(*dr, *dp))) else { continue };
        let syc = ty(syc);
        let sx = xa(sc) + span;
        let dy = ty(if *slot == 1 { dya } else { dyb });
        let dx = xa(dc);
        let xv = dx - 14.0 - (sp.rem_euclid(4) as f64) * 8.0;
        s += &format!(
            "<polyline points=\"{sx:.0},{syc:.1} {xv:.0},{syc:.1} {xv:.0},{dy:.1} {dx:.0},{dy:.1}\" fill=\"none\" stroke=\"#000\" stroke-width=\"1.6\"/>"
        );
    }
    s += "</svg>";
    s
}


/// Empty Endrunde tree for a Doppelpool whose KO stage is not yet materialized:
/// HF (1.A-2.B / 2.A-1.B) -> Finale -> Sieger, drawn like the KO Spielplan.
fn double_endrunde_placeholder_svg() -> String {
    let (u, colw, top, pad) = (80.0f64, 260.0f64, 26.0f64, 12.0f64);
    let labels = ["1. Pool A", "2. Pool B", "2. Pool A", "1. Pool B"];
    let y0 = |i: usize| top + pad + u * i as f64;
    let y1 = |k: usize| (y0(2 * k) + y0(2 * k + 1)) / 2.0;
    let y2 = (y1(0) + y1(1)) / 2.0;
    let x0 = |c: usize| pad + c as f64 * colw + 8.0;
    let x1 = |c: usize| pad + (c + 1) as f64 * colw - 8.0;
    let width = pad * 2.0 + 3.0 * colw;
    let height = top + pad * 2.0 + u * 3.0 + 14.0;
    let mut s = format!(
        "<svg viewBox=\"0 0 {width:.0} {height:.0}\" style=\"width:100%;height:auto;max-height:160mm;\" preserveAspectRatio=\"xMinYMin meet\" xmlns=\"http://www.w3.org/2000/svg\" font-family=\"system-ui, sans-serif\">"
    );
    for (c, h) in ["Halbfinale", "Finale", "Sieger"].iter().enumerate() {
        s += &format!("<text x=\"{:.0}\" y=\"16\" font-size=\"12\" font-weight=\"bold\">{h}</text>", x0(c));
    }
    let line = |s: &mut String, x_a: f64, x_b: f64, y: f64| {
        *s += &format!("<line x1=\"{x_a:.0}\" y1=\"{y:.1}\" x2=\"{x_b:.0}\" y2=\"{y:.1}\" stroke=\"#000\" stroke-width=\"1.6\"/>");
    };
    for (i, lbl) in labels.iter().enumerate() {
        line(&mut s, x0(0), x1(0), y0(i));
        s += &format!(
            "<text x=\"{:.0}\" y=\"{:.1}\" font-size=\"11\" font-style=\"italic\" fill=\"#666\">{lbl}</text>",
            x0(0) + 2.0, y0(i) - 5.0
        );
    }
    for k in 0..2 {
        let xc = x1(0);
        s += &format!("<line x1=\"{xc:.0}\" y1=\"{:.1}\" x2=\"{xc:.0}\" y2=\"{:.1}\" stroke=\"#000\" stroke-width=\"1.6\"/>", y0(2 * k), y0(2 * k + 1));
        line(&mut s, x0(1), x1(1), y1(k));
        line(&mut s, xc, x0(1), y1(k));
    }
    let xc = x1(1);
    s += &format!("<line x1=\"{xc:.0}\" y1=\"{:.1}\" x2=\"{xc:.0}\" y2=\"{:.1}\" stroke=\"#000\" stroke-width=\"1.6\"/>", y1(0), y1(1));
    line(&mut s, x0(2), x1(2), y2);
    line(&mut s, xc, x0(2), y2);
    s += "</svg>";
    s
}

/// Medal footer once the bracket is completed.
fn places_html(s: &BracketSummary, people: &People) -> String {
    let nm = |gp: Option<i32>| gp.and_then(|g| people.get(&g)).map(|p| esc(&p.name)).unwrap_or_default();
    if s.first_place.is_none() && s.second_place.is_none() {
        return String::new();
    }
    let mut h = format!("<p class=\"places\"><b>1.</b> {} &nbsp; <b>2.</b> {}", nm(s.first_place), nm(s.second_place));
    if s.third_place_1.is_some() {
        h.push_str(&format!(" &nbsp; <b>3.</b> {}", nm(s.third_place_1)));
    }
    if s.third_place_2.is_some() {
        h.push_str(&format!(" &nbsp; <b>3.</b> {}", nm(s.third_place_2)));
    }
    h.push_str("</p>");
    h
}

/// One bracket → its printable section, or None (no fights and no result yet).
async fn render_bracket(st: &AppState, s: &BracketSummary, fights: &[Fight]) -> Result<Option<String>, AppError> {
    if fights.is_empty() && s.first_place.is_none() {
        return Ok(None);
    }
    let people = people_for(st, fights, s).await?;
    let ty = s.bracket_type.as_deref().unwrap_or("");

    let mut body = String::new();
    match ty {
        "pools" => {
            let pf: Vec<Fight> = fights.iter().filter(|f| f.bracket_phase == "pool").cloned().collect();
            body.push_str(&pool_grid("Pool", None, &pf, &people, s));
        }
        "double" => {
            for (pi, label) in [(0, "Pool A"), (1, "Pool B")] {
                let pf: Vec<Fight> = fights
                    .iter()
                    .filter(|f| f.bracket_phase == "pool" && f.pool_index == Some(pi))
                    .cloned()
                    .collect();
                if !pf.is_empty() {
                    body.push_str(&pool_grid(label, (pi == 1).then_some(s.group_name.as_str()), &pf, &people, s));
                }
            }
            let ko: Vec<Fight> = fights.iter().filter(|f| f.bracket_phase == "wb").cloned().collect();
            body.push_str(&format!(
                "<div class=\"part\"><h2>Endrunde (A1–B2 / A2–B1) <span class=\"ctx\">— {}</span></h2>",
                esc(&s.group_name)
            ));
            if ko.is_empty() {
                // KO stage is created live at pool close - print the empty
                // Spielplan tree (same line style as the KO brackets) so the
                // paper sheet can be hand-filled anyway.
                body.push_str(&double_endrunde_placeholder_svg());
            } else {
                body.push_str(&wb_tree_svg(&ko, &people));
            }
            body.push_str("</div>");
        }
        "ko" | "repechage" => {
            let wb: Vec<Fight> = fights.iter().filter(|f| f.bracket_phase == "wb").cloned().collect();
            body.push_str("<div class=\"part\"><h2>Hauptrunde</h2>");
            body.push_str(&wb_tree_svg(&wb, &people));
            body.push_str("</div>");
            // draw size from the wb round-0 width (robust against missing rows)
            let n0 = wb.iter().filter(|f| f.round.unwrap_or(0) == 0)
                .map(|f| f.pos_in_round.unwrap_or(0)).max().unwrap_or(0) + 1;
            let nrounds = (2 * n0.max(1) as u32).ilog2();
            let wbnr: HashMap<(i32, i32), (i32, bool)> = wb.iter()
                .map(|f| {
                    let bye = f.status.as_deref() == Some("bye");
                    ((f.round.unwrap_or(0), f.pos_in_round.unwrap_or(0)), (f.fight_number.unwrap_or(0), bye))
                })
                .collect();
            for (phase, title) in [("lb", "Trostrunde"), ("rep", "Repechage")] {
                let pf: Vec<Fight> = fights.iter().filter(|f| f.bracket_phase == phase).cloned().collect();
                if !pf.is_empty() {
                    let edges = ko_edges(ty, phase, nrounds, &wbnr);
                    body.push_str(&format!(
                        "<div class=\"part\"><h2>{title} <span class=\"ctx\">— {}</span></h2>",
                        esc(&s.group_name)
                    ));
                    body.push_str(&lb_tree_svg(&pf, &people, &edges));
                    body.push_str("</div>");
                }
            }
        }
        _ => {} // special/solo: places footer only
    }
    body.push_str(&places_html(s, &people));

    Ok(Some(format!(
        "<section class=\"bracket\"><header><h1>{}</h1><span class=\"ty\">{}</span></header>{}</section>",
        esc(&s.group_name),
        type_label(ty),
        body
    )))
}

const STYLE: &str = r#"
@page { size: A4 landscape; margin: 10mm; }
* { box-sizing: border-box; -webkit-print-color-adjust: exact; print-color-adjust: exact; }
body { font-family: system-ui, sans-serif; font-size: 13pt; color: #000; margin: 12px; }
.bracket { page-break-after: always; }
.bracket:last-child { page-break-after: auto; }
header { display: flex; align-items: baseline; gap: 1rem; border-bottom: 2px solid #000; margin-bottom: 8px; }
h1 { font-size: 18pt; margin: 0 0 4px; }
h2 { font-size: 14pt; margin: 10px 0 4px; break-after: avoid-page; page-break-after: avoid; }
.part { break-inside: avoid; page-break-inside: avoid; }
.ctx { color: #555; font-weight: normal; font-size: 11pt; }
.ty { color: #444; }
table.pool { border-collapse: collapse; margin-bottom: 8px; width: 100%; }
table.pool th, table.pool td { border: 1.5px solid #000; padding: 4px 6px; text-align: center; min-width: 44px; height: 50px; }
table.pool th { height: auto; }
table.pool .nm { text-align: left; min-width: 190px; width: 210px; }
table.pool .pair { font-weight: normal; font-size: 10pt; color: #333; }
td.x { background: #bbb; }
td.b { background: #fff; }
td.w, .ln.w { font-weight: 700; text-decoration: underline; }
.cols { display: flex; gap: 12px; align-items: flex-start; }
.col { min-width: 170px; flex: 1; }
.col h3 { font-size: 12pt; margin: 0 0 6px; border-bottom: 1.5px solid #000; }
.box { border: 1.5px solid #000; border-radius: 3px; padding: 4px 8px; margin-bottom: 12px; }
.box .nr { font-size: 9pt; color: #333; }
.ln { border-bottom: 1px solid #000; padding: 5px 0; min-height: 34px; font-size: 12pt; }
.ln:last-child { border-bottom: none; }
.ln small { color: #444; }
.sc { float: right; font-weight: 700; }
.places { margin-top: 10px; font-size: 14pt; }
@media screen { body { max-width: 1400px; margin: 12px auto; } .bracket { margin-bottom: 40px; } }
"#;

fn page(title: &str, body: String) -> Response {
    let html = format!(
        "<!DOCTYPE html><html lang=\"de\"><head><meta charset=\"utf-8\"><title>{}</title><style>{}</style></head><body>{}</body></html>",
        esc(title), STYLE, body
    );
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(html))
        .unwrap()
}

/// GET /print/liste/:bracket_id — one printable Wettkampfliste.
pub async fn print_bracket(State(st): State<AppState>, Path(id): Path<i32>) -> Result<Response, AppError> {
    let s = brackets::all_summaries(&st.pool)
        .await?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| AppError::status(StatusCode::NOT_FOUND, format!("bracket {id} not found")))?;
    let fs: Vec<Fight> = fights::all_fights(&st.pool).await?.into_iter().filter(|f| f.bracket_id == id).collect();
    let section = render_bracket(&st, &s, &fs)
        .await?
        .unwrap_or_else(|| format!("<section class=\"bracket\"><h1>{}</h1><p>Noch keine Kämpfe generiert.</p></section>", esc(&s.group_name)));
    Ok(page(&format!("Liste — {}", s.group_name), section))
}

#[derive(serde::Deserialize)]
pub struct PrintQuery {
    /// Optional comma-separated bracket ids ("3,50,51") — absent = all.
    ids: Option<String>,
}

/// GET /print/listen[?ids=1,2,3] — brackets with fights (or a result), one page
/// each; `ids` limits to a selection (checkbox multi-print).
pub async fn print_all(
    State(st): State<AppState>,
    Query(q): Query<PrintQuery>,
) -> Result<Response, AppError> {
    let filter: Option<Vec<i32>> =
        q.ids.map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect());
    let mut summaries = brackets::all_summaries(&st.pool).await?;
    if let Some(ids) = &filter {
        summaries.retain(|s| ids.contains(&s.id));
    }
    let all = fights::all_fights(&st.pool).await?;
    let mut by_bracket: HashMap<i32, Vec<Fight>> = HashMap::new();
    for f in all {
        by_bracket.entry(f.bracket_id).or_default().push(f);
    }
    let mut body = String::new();
    for s in &summaries {
        let fs = by_bracket.remove(&s.id).unwrap_or_default();
        if let Some(section) = render_bracket(&st, s, &fs).await? {
            body.push_str(&section);
        }
    }
    if body.is_empty() {
        body = "<p>Keine Listen vorhanden.</p>".into();
    }
    Ok(page("Wettkampflisten", body))
}
