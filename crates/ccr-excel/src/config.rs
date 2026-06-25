// SPDX-License-Identifier: GPL-3.0-or-later
//! bracket_config.xlsx reader — age + weight classification for group assignment.
//! Mirrors edv `config_repository` + `bracket_utils.get_age_group/get_weight_class`.
//!
//! Sheets:
//!   AgeEligibility:  BirthYear | U9 | U11 | U13 | U15 | U18 | 18+   ('X' marks the class)
//!   Options:         OptionName | Value   (event_year, U9/U11 pool sizes, …)
//!   WeightClasses:   Gender | AgeGroup | MinWeight | MaxWeight | Label   ((min,max] ranges)
//!   GenerationMethods: MethodKey | … | MinFighters | MaxFighters | …

use std::collections::HashMap;

use calamine::{open_workbook_auto, Data, Reader};

#[derive(Debug, Clone)]
struct WeightClass {
    gender: String,
    age_group: String,
    max_weight: f64,
    label: String,
}

#[derive(Debug, Clone)]
pub struct BracketConfig {
    /// Birth year → eligible age groups (ascending; >1 = natural double-start year).
    age_by_year: HashMap<i64, Vec<String>>,
    /// Ordered age-class names from the AgeEligibility header (U9..18+).
    age_classes: Vec<String>,
    weight_classes: Vec<WeightClass>,
    pub event_year: i64,
}

fn cell_str(d: Option<&Data>) -> String {
    match d {
        Some(Data::String(s)) => s.trim().to_string(),
        Some(Data::Int(n)) => n.to_string(),
        Some(Data::Float(f)) => {
            if f.fract() == 0.0 { (*f as i64).to_string() } else { f.to_string() }
        }
        Some(Data::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}
fn cell_f64(d: Option<&Data>) -> Option<f64> {
    match d {
        Some(Data::Float(f)) => Some(*f),
        Some(Data::Int(n)) => Some(*n as f64),
        Some(Data::String(s)) => s.trim().replace(',', ".").parse().ok(),
        _ => None,
    }
}

impl BracketConfig {
    pub fn load(path: &str) -> Result<Self, calamine::Error> {
        let mut wb = open_workbook_auto(path)?;

        // AgeEligibility: header row has class names; each data row marks one 'X'.
        let mut age_by_year = HashMap::new();
        let age_sheet = wb.worksheet_range("AgeEligibility")?;
        let mut rows = age_sheet.rows();
        let header: Vec<String> = rows.next().map(|r| r.iter().map(|c| cell_str(Some(c))).collect()).unwrap_or_default();
        for row in rows {
            let Some(year) = cell_f64(row.first()).map(|f| f as i64) else { continue };
            // Collect ALL X-marked classes in column (ascending) order.
            let groups: Vec<String> = row
                .iter()
                .enumerate()
                .skip(1)
                .filter(|(_, c)| cell_str(Some(c)).eq_ignore_ascii_case("x"))
                .filter_map(|(col, _)| header.get(col).cloned())
                .collect();
            if !groups.is_empty() {
                age_by_year.insert(year, groups);
            }
        }

        // WeightClasses.
        let mut weight_classes = Vec::new();
        let wc_sheet = wb.worksheet_range("WeightClasses")?;
        for row in wc_sheet.rows().skip(1) {
            let gender = cell_str(row.first());
            let age_group = cell_str(row.get(1));
            let max_weight = cell_f64(row.get(3));
            let label = cell_str(row.get(4));
            if gender.is_empty() || label.is_empty() {
                continue;
            }
            if let Some(max_weight) = max_weight {
                weight_classes.push(WeightClass { gender, age_group, max_weight, label });
            }
        }

        // Options → event_year.
        let mut event_year = 0;
        if let Ok(opts) = wb.worksheet_range("Options") {
            for row in opts.rows().skip(1) {
                if cell_str(row.first()) == "event_year" {
                    event_year = cell_f64(row.get(1)).map(|f| f as i64).unwrap_or(0);
                }
            }
        }

        let age_classes: Vec<String> = header.into_iter().skip(1).collect();
        Ok(BracketConfig { age_by_year, age_classes, weight_classes, event_year })
    }

    /// Ordered age-class names (for seeding the editable config).
    pub fn age_classes(&self) -> &[String] {
        &self.age_classes
    }
    /// (birth_year, eligible classes) rows (for seeding).
    pub fn birth_year_rows(&self) -> Vec<(i64, Vec<String>)> {
        let mut v: Vec<(i64, Vec<String>)> =
            self.age_by_year.iter().map(|(y, g)| (*y, g.clone())).collect();
        v.sort_by_key(|(y, _)| std::cmp::Reverse(*y));
        v
    }
    /// (gender, age_group, max_weight, label) weight-class rows (for seeding).
    pub fn weight_class_defs(&self) -> Vec<(String, String, f64, String)> {
        self.weight_classes
            .iter()
            .map(|w| (w.gender.clone(), w.age_group.clone(), w.max_weight, w.label.clone()))
            .collect()
    }

    /// Primary age group (the lowest eligible) for a birth year.
    pub fn age_group(&self, birth_year: i64) -> Option<String> {
        self.eligible_age_groups(birth_year).into_iter().next()
    }

    /// All eligible age groups (ascending). >1 entry = a natural double-start
    /// year (config marks two classes); used to expand doublestarters.
    pub fn eligible_age_groups(&self, birth_year: i64) -> Vec<String> {
        if let Some(g) = self.age_by_year.get(&birth_year) {
            return g.clone();
        }
        if self.event_year == 0 {
            return Vec::new();
        }
        let age = self.event_year - birth_year;
        vec![match age {
            a if a >= 18 => "18+",
            a if a >= 15 => "U18",
            a if a >= 13 => "U15",
            a if a >= 11 => "U13",
            a if a >= 9 => "U11",
            _ => "U9",
        }
        .to_string()]
    }

    /// Weight class label for (gender, age_group, weight): the lightest class
    /// whose MaxWeight ≥ weight. U9/U11 have no weight classes → None (pooled).
    pub fn weight_class(&self, gender: &str, age_group: &str, weight: f64) -> Option<String> {
        let mut candidates: Vec<&WeightClass> = self
            .weight_classes
            .iter()
            .filter(|w| w.gender == gender && w.age_group == age_group)
            .collect();
        candidates.sort_by(|a, b| a.max_weight.partial_cmp(&b.max_weight).unwrap());
        candidates
            .iter()
            .find(|w| weight <= w.max_weight)
            .or_else(|| candidates.last())
            .map(|w| w.label.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: &str = "../../../edv/config/bracket_config.xlsx";

    #[test]
    fn classifies_against_real_config() {
        let cfg = BracketConfig::load(CFG).expect("load bracket_config.xlsx");
        assert_eq!(cfg.event_year, 2026);
        // AgeEligibility: 2015 → U13 (single); 2014 → U13+U15 (double-start year).
        assert_eq!(cfg.age_group(2015).as_deref(), Some("U13"));
        assert_eq!(cfg.eligible_age_groups(2015), vec!["U13"]);
        assert_eq!(cfg.eligible_age_groups(2014), vec!["U13", "U15"]);
        assert_eq!(cfg.age_group(2014).as_deref(), Some("U13")); // primary = lower
        // WeightClasses m/U13: 0-28→-28kg, 31-34→-34kg, >55→+55kg.
        assert_eq!(cfg.weight_class("m", "U13", 27.0).as_deref(), Some("-28kg"));
        assert_eq!(cfg.weight_class("m", "U13", 33.0).as_deref(), Some("-34kg"));
        assert_eq!(cfg.weight_class("m", "U13", 90.0).as_deref(), Some("+55kg"));
        // boundary: exactly 28 → -28kg (max inclusive).
        assert_eq!(cfg.weight_class("m", "U13", 28.0).as_deref(), Some("-28kg"));
        // U9/U11 have no weight classes.
        assert_eq!(cfg.weight_class("m", "U9", 25.0), None);
    }
}
