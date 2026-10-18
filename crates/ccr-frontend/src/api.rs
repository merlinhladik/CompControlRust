// SPDX-License-Identifier: GPL-3.0-or-later
//! API data models + fetch helpers for the admin views.

use serde::Deserialize;

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
    // Club of each placed fighter — feeds the "Auswertungen" view.
    #[serde(rename = "firstClub", default)]
    pub first_club: String,
    #[serde(rename = "secondClub", default)]
    pub second_club: String,
    #[serde(rename = "third1Club", default)]
    pub third1_club: String,
    #[serde(rename = "third2Club", default)]
    pub third2_club: String,
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
