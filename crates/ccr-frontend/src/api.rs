// SPDX-License-Identifier: GPL-3.0-or-later
//! API data models + fetch helpers shared by the live and admin views.

use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Score {
    pub points: i32,
    #[serde(default)]
    pub ippon: i32,
    #[serde(default)]
    pub wazari: i32,
    #[serde(default)]
    pub yuko: i32,
    #[serde(default)]
    pub shido: i32,
}
impl Score {
    /// Compact breakdown of the actual sub-scores (shown on fight click).
    pub fn breakdown(&self) -> String {
        format!("I{} W{} Y{} S{}", self.ippon, self.wazari, self.yuko, self.shido)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Fighter {
    #[serde(rename = "firstName", default)]
    pub first_name: String,
    #[serde(rename = "lastName", default)]
    pub last_name: String,
    #[serde(default)]
    pub score: Score,
}
impl Fighter {
    pub fn name(&self) -> String {
        let n = format!("{} {}", self.first_name, self.last_name);
        let n = n.trim();
        if n.is_empty() { "—".into() } else { n.to_string() }
    }
    pub fn present(&self) -> bool {
        !self.last_name.is_empty() && self.last_name != "TBD"
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Match {
    #[serde(rename = "matchId")]
    pub match_id: i64,
    #[serde(rename = "fightNr", default)]
    pub fight_nr: i64,
    #[serde(rename = "tableId", default)]
    pub table_id: Option<i64>,
    #[serde(rename = "bracketId", default)]
    pub bracket_id: i64,
    #[serde(rename = "bracketType", default)]
    pub bracket_type: String,
    #[serde(default)]
    pub gender: String,
    #[serde(rename = "ageGroup", default)]
    pub age_group: String,
    #[serde(rename = "weightClass", default)]
    pub weight_class: String,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub round: i64,
    #[serde(rename = "posInRound", default)]
    pub pos_in_round: i64,
    #[serde(default)]
    pub p1: Fighter,
    #[serde(default)]
    pub p2: Fighter,
    #[serde(default)]
    pub status: String,
    #[serde(rename = "winnerName", default)]
    pub winner_name: String,
}
impl Match {
    pub fn scoreable(&self) -> bool {
        self.p1.present() && self.p2.present() && self.status != "finished" && self.status != "bye"
    }
    /// U9/U11 use the JVP additive system (Ippon10/Waza5/Yuko3/Shido+2, ≥20 wins).
    pub fn is_youth(&self) -> bool {
        self.age_group == "U9" || self.age_group == "U11"
    }
    pub fn listable(&self) -> bool {
        (self.p1.present() && self.p2.present()) || self.status == "finished" || self.status == "bye"
    }
    pub fn category(&self) -> String {
        format!("{} {} {}", self.gender, self.weight_class, self.bracket_type)
            .split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

#[derive(Deserialize)]
struct MatchesResp {
    #[serde(default)]
    matches: Vec<Match>,
}

// ── Admin read models ────────────────────────────────────────────────────────
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct AdminParticipant {
    pub id: i64,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub last_name: String,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub club: Option<String>,
    #[serde(default)]
    pub association: Option<String>,
    #[serde(default)]
    pub birth_date: Option<String>, // "YYYY-MM-DD"
    #[serde(default)]
    pub weight: Option<String>, // Decimal serialized as string
    #[serde(default)]
    pub valid: Option<bool>,
    #[serde(default)]
    pub paid: Option<bool>,
    #[serde(default)]
    pub doublestart: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct BracketResult {
    pub id: i64,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub gender: Option<String>,
    #[serde(rename = "ageGroup", default)]
    pub age_group: Option<String>,
    #[serde(rename = "weightClass", default)]
    pub weight_class: Option<String>,
    #[serde(rename = "bracketType", default)]
    pub bracket_type: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub first: String,
    #[serde(default)]
    pub second: String,
    #[serde(default)]
    pub third1: String,
    #[serde(default)]
    pub third2: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct LockInfo {
    pub scope_key: String,
    #[serde(default)]
    pub age_group: String,
    #[serde(default)]
    pub gender: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct ClubInfo {
    pub id: i32,
    pub name: String,
    #[serde(default)]
    pub association: Option<String>,
}

/// In-flight edit of a fighter. `id = None` → a new fighter (POST), else PUT.
#[derive(Clone, PartialEq, Default)]
pub struct EditState {
    pub id: Option<i64>,
    pub first_name: String,
    pub last_name: String,
    pub gender: String,
    pub birthyear: String,
    pub club: String,
    pub association: String,
    pub weight: String,
    pub valid: bool,
    pub paid: bool,
    pub ds: String,
}

#[derive(Deserialize)]
struct LocksResp {
    #[serde(default)]
    locks: Vec<LockInfo>,
}
#[derive(Deserialize)]
struct ClubsResp {
    #[serde(default)]
    clubs: Vec<ClubInfo>,
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

pub async fn fetch_locks() -> Vec<LockInfo> {
    match gloo_net::http::Request::get("/api/locks").send().await {
        Ok(r) => r.json::<LocksResp>().await.map(|x| x.locks).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}
pub async fn fetch_clubs() -> Vec<ClubInfo> {
    match gloo_net::http::Request::get("/api/clubs").send().await {
        Ok(r) => r.json::<ClubsResp>().await.map(|x| x.clubs).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}
pub async fn fetch_participants() -> Vec<AdminParticipant> {
    match gloo_net::http::Request::get("/api/participants").send().await {
        Ok(r) => r.json::<ParticipantsResp>().await.map(|x| x.participants).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}
pub async fn fetch_results() -> Vec<BracketResult> {
    match gloo_net::http::Request::get("/api/brackets").send().await {
        Ok(r) => r.json::<BracketsResp>().await.map(|x| x.brackets).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}
pub async fn fetch_matches() -> Vec<Match> {
    match gloo_net::http::Request::get("/api/matches").send().await {
        Ok(resp) => resp.json::<MatchesResp>().await.map(|r| r.matches).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn ws_url() -> String {
    let loc = web_sys::window().unwrap().location();
    let proto = if loc.protocol().unwrap_or_default() == "https:" { "wss" } else { "ws" };
    let host = loc.host().unwrap_or_else(|_| "localhost:5001".into());
    format!("{proto}://{host}/ws")
}

/// Group items into (key, items) preserving first-seen key order.
pub fn group_by<T: Clone, K: PartialEq + Clone>(
    items: &[T],
    key: impl Fn(&T) -> K,
) -> Vec<(K, Vec<T>)> {
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
