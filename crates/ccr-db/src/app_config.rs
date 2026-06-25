// SPDX-License-Identifier: GPL-3.0-or-later
//! CCR-owned, UI-editable tournament config (single JSONB row in `app_config`).
//! Holds the classification rules that used to live in bracket_config.xlsx:
//! generation thresholds, youth pool size, age classes + birth-year eligibility
//! (incl. doublestart overlaps), weight classes, event year. All pure logic
//! (recommend / age / weight) is a method on `AppConfig`.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// Adult generation threshold: the bracket type used from `min_fighters` upward.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodRange {
    pub min_fighters: i32,
    pub method: String, // special | pools | double | ko | repechage
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightClassDef {
    pub gender: String,
    pub age_group: String,
    pub max_weight: f64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirthYearRow {
    pub year: i64,
    pub classes: Vec<String>, // eligible age groups; >1 = doublestart overlap year
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub event_year: i64,
    pub age_classes: Vec<String>,        // ordered, e.g. U9..18+
    pub youth_classes: Vec<String>,      // pool-only classes (U9, U11)
    pub youth_pool_size: i32,            // e.g. 4
    /// Max in-pool weight spread (kg) for youth pool cutting; ≤0 = no weight cut
    /// (pool-size split only). See ccr-domain `split_into_pools`.
    #[serde(default)]
    pub youth_max_weight_spread: f64,
    pub adult_methods: Vec<MethodRange>, // thresholds for U13+
    pub birth_years: Vec<BirthYearRow>,
    pub weight_classes: Vec<WeightClassDef>,
}

impl AppConfig {
    /// Bracket type for `n` fighters in `age_group`. Youth classes are always
    /// pools; otherwise the highest `min_fighters ≤ n` adult method applies.
    pub fn recommend(&self, n: usize, age_group: Option<&str>) -> String {
        if let Some(a) = age_group {
            if self.youth_classes.iter().any(|y| y == a) {
                return "pools".into();
            }
        }
        let n = n as i32;
        let mut chosen = "special";
        let mut best = i32::MIN;
        for m in &self.adult_methods {
            if m.min_fighters <= n && m.min_fighters > best {
                best = m.min_fighters;
                chosen = &m.method;
            }
        }
        chosen.to_string()
    }

    /// True if `age_group` is a youth class (JVP-Additiv-20 additive scoring: U9/U11).
    pub fn is_youth(&self, age_group: Option<&str>) -> bool {
        matches!(age_group, Some(a) if self.youth_classes.iter().any(|y| y == a))
    }

    /// Eligible age groups for a birth year (config; >1 = doublestart overlap).
    pub fn eligible_age_groups(&self, year: i64) -> Vec<String> {
        if let Some(r) = self.birth_years.iter().find(|r| r.year == year) {
            return r.classes.clone();
        }
        Vec::new()
    }
    pub fn age_group(&self, year: i64) -> Option<String> {
        self.eligible_age_groups(year).into_iter().next()
    }

    /// Lightest weight class whose MaxWeight ≥ weight (None for youth/no class).
    pub fn weight_class(&self, gender: &str, age_group: &str, weight: f64) -> Option<String> {
        let mut cands: Vec<&WeightClassDef> = self
            .weight_classes
            .iter()
            .filter(|w| w.gender == gender && w.age_group == age_group)
            .collect();
        cands.sort_by(|a, b| a.max_weight.partial_cmp(&b.max_weight).unwrap());
        cands.iter().find(|w| weight <= w.max_weight).or_else(|| cands.last()).map(|w| w.label.clone())
    }
}

/// Load the config row, or None if not yet seeded.
pub async fn load(pool: &PgPool) -> Result<Option<AppConfig>, sqlx::Error> {
    let row: Option<sqlx::types::Json<AppConfig>> =
        sqlx::query_scalar("SELECT data FROM app_config WHERE id = 1").fetch_optional(pool).await?;
    Ok(row.map(|j| j.0))
}

/// Upsert the single config row.
pub async fn save(pool: &PgPool, cfg: &AppConfig) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO app_config (id, data) VALUES (1, $1) \
         ON CONFLICT (id) DO UPDATE SET data = EXCLUDED.data",
    )
    .bind(sqlx::types::Json(cfg))
    .execute(pool)
    .await?;
    Ok(())
}
