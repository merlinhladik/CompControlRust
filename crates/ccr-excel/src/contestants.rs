// SPDX-License-Identifier: GPL-3.0-or-later
//! contestants_*.{csv,json} exchange format (edv exporter ↔ WeighIn editor).
//!
//! Faithful port of edv `backend/services/contestants_csv.py` + the JSON schema.
//! Cross-repo invariant (WSP/CLAUDE.md, 2026-06-03):
//!   Columns: ID;Firstname;Lastname;Birthyear;Club;Association;Weight;Valid;
//!            Gender;Paid;Doublestart   (delimiter ';', UTF-8 WITH BOM, CRLF)
//!   Write canonically: int ID/Birthyear, '.'-decimal Weight, lowercase
//!     true/false for Valid/Paid, Gender verbatim, Doublestart verbatim.
//!   Read tolerantly: bool true/false/1/0/ja/nein/yes/no; Weight '.' or ',';
//!     Doublestart from 'Doublestart' OR legacy 'mode'; absent ⇒ "standard".

use serde::{Deserialize, Serialize};

pub const CSV_FIELDS: [&str; 11] = [
    "ID", "Firstname", "Lastname", "Birthyear", "Club", "Association", "Weight",
    "Valid", "Gender", "Paid", "Doublestart",
];
const BOM: &str = "\u{FEFF}";

/// One contestant. Same schema for CSV and JSON (JSON keys are the field names).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contestant {
    #[serde(rename = "ID", skip_serializing_if = "Option::is_none", default)]
    pub id: Option<i64>,
    #[serde(rename = "Firstname", default)]
    pub firstname: String,
    #[serde(rename = "Lastname", default)]
    pub lastname: String,
    #[serde(rename = "Birthyear", skip_serializing_if = "Option::is_none", default)]
    pub birthyear: Option<i64>,
    #[serde(rename = "Club", default)]
    pub club: String,
    #[serde(rename = "Association", default)]
    pub association: String,
    #[serde(rename = "Weight", skip_serializing_if = "Option::is_none", default)]
    pub weight: Option<f64>,
    #[serde(rename = "Valid", skip_serializing_if = "Option::is_none", default)]
    pub valid: Option<bool>,
    #[serde(rename = "Gender", default)]
    pub gender: String,
    #[serde(rename = "Paid", skip_serializing_if = "Option::is_none", default)]
    pub paid: Option<bool>,
    /// Wire values: standard | höher | doppel. Absent ⇒ "standard".
    #[serde(rename = "Doublestart", skip_serializing_if = "Option::is_none", default)]
    pub doublestart: Option<String>,
}

fn parse_bool(token: &str) -> bool {
    matches!(
        token.trim().to_lowercase().as_str(),
        "true" | "1" | "ja" | "yes" | "y" | "wahr" | "x"
    )
}

fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<i64>().ok().or_else(|| t.parse::<f64>().ok().map(|f| f as i64))
}

fn parse_weight(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.replace(',', ".").parse::<f64>().ok()
}

/// Python repr-faithful weight formatting: 26.3 → "26.3", 26.0 → "26.0".
fn format_weight(w: Option<f64>) -> String {
    match w {
        None => String::new(),
        Some(v) if v.fract() == 0.0 => format!("{v:.1}"),
        Some(v) => format!("{v}"),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ContestantsError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("csv: {0}")]
    Csv(#[from] csv::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Read a contestants CSV (BOM-tolerant, tolerant value parsing).
pub fn read_csv(path: &str) -> Result<Vec<Contestant>, ContestantsError> {
    let raw = std::fs::read_to_string(path)?;
    read_csv_str(raw.strip_prefix(BOM).unwrap_or(&raw))
}

/// Parse contestants from CSV text (no file).
pub fn read_csv_str(text: &str) -> Result<Vec<Contestant>, ContestantsError> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(b';')
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers: Vec<String> = rdr.headers()?.iter().map(|h| h.trim().to_string()).collect();
    let col = |name: &str| headers.iter().position(|h| h == name);
    let (i_id, i_fn, i_ln, i_by, i_club, i_assoc, i_w, i_valid, i_gender, i_paid, i_ds, i_mode) = (
        col("ID"), col("Firstname"), col("Lastname"), col("Birthyear"), col("Club"),
        col("Association"), col("Weight"), col("Valid"), col("Gender"), col("Paid"),
        col("Doublestart"), col("mode"),
    );

    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        let get = |idx: Option<usize>| -> &str { idx.and_then(|i| rec.get(i)).unwrap_or("").trim() };
        if rec.iter().all(|v| v.trim().is_empty()) {
            continue; // skip fully blank rows
        }
        let ds = {
            let d = get(i_ds);
            let m = get(i_mode);
            let v = if !d.is_empty() { d } else { m };
            if v.is_empty() { None } else { Some(v.to_string()) }
        };
        out.push(Contestant {
            id: parse_int(get(i_id)),
            firstname: get(i_fn).to_string(),
            lastname: get(i_ln).to_string(),
            birthyear: parse_int(get(i_by)),
            club: get(i_club).to_string(),
            association: get(i_assoc).to_string(),
            weight: parse_weight(get(i_w)),
            valid: { let v = get(i_valid); if v.is_empty() { None } else { Some(parse_bool(v)) } },
            gender: get(i_gender).to_string(),
            paid: { let v = get(i_paid); if v.is_empty() { None } else { Some(parse_bool(v)) } },
            doublestart: ds,
        });
    }
    Ok(out)
}

/// Write contestants CSV canonically (UTF-8 BOM, ';' delimiter, CRLF).
pub fn write_csv(path: &str, contestants: &[Contestant]) -> Result<(), ContestantsError> {
    std::fs::write(path, render_csv(contestants)?)?;
    Ok(())
}

/// Render the canonical CSV text (with BOM, CRLF) for the given contestants.
pub fn render_csv(contestants: &[Contestant]) -> Result<String, ContestantsError> {
    let mut wtr = csv::WriterBuilder::new()
        .delimiter(b';')
        .terminator(csv::Terminator::Any(b'\n')) // post-processed to CRLF below
        .from_writer(vec![]);
    wtr.write_record(CSV_FIELDS)?;
    for c in contestants {
        wtr.write_record(&[
            c.id.map(|v| v.to_string()).unwrap_or_default(),
            c.firstname.clone(),
            c.lastname.clone(),
            c.birthyear.map(|v| v.to_string()).unwrap_or_default(),
            c.club.clone(),
            c.association.clone(),
            format_weight(c.weight),
            match c.valid { Some(true) => "true", _ => "false" }.to_string(),
            c.gender.clone(),
            match c.paid { Some(true) => "true", _ => "false" }.to_string(),
            c.doublestart.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "standard".into()),
        ])?;
    }
    let body = String::from_utf8(wtr.into_inner().expect("csv writer")).expect("utf8");
    Ok(format!("{BOM}{}", body.replace('\n', "\r\n")))
}

/// Read contestants_*.json (same schema).
pub fn read_json(path: &str) -> Result<Vec<Contestant>, ContestantsError> {
    read_json_str(&std::fs::read_to_string(path)?)
}

/// Parse contestants from JSON text (BOM-tolerant).
pub fn read_json_str(text: &str) -> Result<Vec<Contestant>, ContestantsError> {
    Ok(serde_json::from_str(text.strip_prefix(BOM).unwrap_or(text))?)
}

/// Write contestants_*.json (pretty, canonical key order via struct).
pub fn write_json(path: &str, contestants: &[Contestant]) -> Result<(), ContestantsError> {
    std::fs::write(path, serde_json::to_string_pretty(contestants)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Contestant> {
        vec![
            Contestant {
                id: Some(1), firstname: "Anna".into(), lastname: "Adler".into(),
                birthyear: Some(2010), club: "Verein A".into(), association: "Pfalz".into(),
                weight: Some(26.3), valid: Some(true), gender: "w".into(), paid: Some(true),
                doublestart: Some("höher".into()),
            },
            Contestant {
                id: Some(2), firstname: "Bea".into(), lastname: "Berger".into(),
                birthyear: Some(2011), club: "Verein B".into(), association: "Pfalz".into(),
                weight: Some(30.0), valid: Some(false), gender: "w".into(), paid: Some(false),
                doublestart: None, // absent ⇒ "standard" on write
            },
        ]
    }

    #[test]
    fn csv_round_trip_is_stable_and_canonical() {
        let text = render_csv(&sample()).unwrap();
        assert!(text.starts_with('\u{FEFF}'), "must have BOM");
        assert!(text.contains(
            "ID;Firstname;Lastname;Birthyear;Club;Association;Weight;Valid;Gender;Paid;Doublestart"
        ));
        assert!(text.contains(";26.3;true;w;true;höher"));
        assert!(text.contains(";30.0;false;w;false;standard"), "absent ds → standard, 30.0 keeps decimal");
        assert!(text.contains("\r\n"), "CRLF line endings");
        let back = read_csv_str(&text).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].weight, Some(26.3));
        assert_eq!(back[0].doublestart.as_deref(), Some("höher"));
        assert_eq!(back[1].doublestart.as_deref(), Some("standard"));
    }

    #[test]
    fn read_is_tolerant() {
        let text = "ID;Firstname;Lastname;Birthyear;Club;Association;Weight;Valid;Gender;Paid;mode\n\
                    7;Cy;Cordes;2012;C;Pfalz;28,5;ja;m;nein;doppel\n";
        let c = read_csv_str(text).unwrap();
        assert_eq!(c[0].weight, Some(28.5), "comma decimal");
        assert_eq!(c[0].valid, Some(true), "'ja' → true");
        assert_eq!(c[0].paid, Some(false), "'nein' → false");
        assert_eq!(c[0].doublestart.as_deref(), Some("doppel"), "legacy 'mode' column");
    }

    #[test]
    fn csv_json_same_schema() {
        let json = serde_json::to_string(&sample()).unwrap();
        let from_json: Vec<Contestant> = serde_json::from_str(&json).unwrap();
        let from_csv = read_csv_str(&render_csv(&sample()).unwrap()).unwrap();
        assert_eq!(from_json[0], from_csv[0]);
        assert_eq!(from_json[0].gender, "w"); // Gender verbatim, not normalized
    }
}
