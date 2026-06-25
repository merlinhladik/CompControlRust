// SPDX-License-Identifier: GPL-3.0-or-later
//! Generated .xlsx output (results list + Urkunden) via rust_xlsxwriter.
//!
//! NOTE: this produces CCR's OWN clean .xlsx layout — it does NOT fill the pixel-
//! exact DJB `.xls` (BIFF) templates in place (no pure-Rust BIFF writer exists;
//! calamine is read-only, rust_xlsxwriter writes fresh .xlsx). Faithful template
//! filling would need the rejected Python sidecar. Content mirrors edv
//! `urkunden_export_service` (one row per awarded placement: vorname, nachname,
//! platz, klasse, verein).

use rust_xlsxwriter::{Format, Workbook, XlsxError};

/// One results row: a completed category and its medallists' names.
pub struct ResultRow {
    pub kategorie: String,
    pub typ: String,
    pub first: String,
    pub second: String,
    pub third1: String,
    pub third2: String,
}

/// One certificate row (mirrors edv COLUMNS = vorname/nachname/platz/klasse/verein).
pub struct UrkundeRow {
    pub vorname: String,
    pub nachname: String,
    pub platz: String,
    pub klasse: String,
    pub verein: String,
}

/// One weigh-in card row (the Gewicht/Unterschrift columns stay blank to fill).
/// `id` is the participant id — carried so a card can print a QR/barcode the
/// WeighIn scanner reads (and as a stable merge key for Affinity / LibreOffice).
pub struct WiegekarteRow {
    pub id: i32,
    pub nachname: String,
    pub vorname: String,
    pub verein: String,
    pub geschlecht: String,
    pub jahrgang: String,
    pub altersklasse: String,
}

/// Weigh-in cards / list: one row per fighter, blank Gewicht + Unterschrift.
pub fn wiegekarten_xlsx(rows: &[WiegekarteRow]) -> Result<Vec<u8>, XlsxError> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("Wiegekarten")?;
    let bold = Format::new().set_bold();
    for (c, h) in ["Nachname", "Vorname", "Verein", "Geschlecht", "Jahrgang", "Altersklasse", "Gewicht", "Unterschrift"]
        .iter()
        .enumerate()
    {
        ws.write_with_format(0, c as u16, *h, &bold)?;
    }
    for (i, row) in rows.iter().enumerate() {
        let r = i as u32 + 1;
        ws.write(r, 0, row.nachname.as_str())?;
        ws.write(r, 1, row.vorname.as_str())?;
        ws.write(r, 2, row.verein.as_str())?;
        ws.write(r, 3, row.geschlecht.as_str())?;
        ws.write(r, 4, row.jahrgang.as_str())?;
        ws.write(r, 5, row.altersklasse.as_str())?;
        // columns 6 (Gewicht) + 7 (Unterschrift) intentionally blank.
    }
    wb.save_to_buffer()
}

/// Weigh-in cards as a merge-ready CSV (Affinity Publisher / LibreOffice data
/// merge): comma-delimited, UTF-8, header = the merge field names. One row per
/// fighter, NO weight — the card template carries a blank line the official
/// fills at the scale. Mirrors `urkunden_csv`; `id` is the merge key / QR value.
pub fn wiegekarten_csv(rows: &[WiegekarteRow]) -> Result<String, csv::Error> {
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record(["id", "nachname", "vorname", "verein", "geschlecht", "jahrgang", "altersklasse"])?;
    for r in rows {
        let id = r.id.to_string();
        w.write_record([
            id.as_str(), r.nachname.as_str(), r.vorname.as_str(), r.verein.as_str(),
            r.geschlecht.as_str(), r.jahrgang.as_str(), r.altersklasse.as_str(),
        ])?;
    }
    Ok(String::from_utf8(w.into_inner().expect("csv writer")).expect("utf8"))
}

/// Results overview workbook (one sheet, all completed categories + placements).
pub fn results_xlsx(rows: &[ResultRow]) -> Result<Vec<u8>, XlsxError> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("Ergebnisse")?;
    let bold = Format::new().set_bold();
    for (c, h) in ["Kategorie", "Typ", "1.", "2.", "3.", "3."].iter().enumerate() {
        ws.write_with_format(0, c as u16, *h, &bold)?;
    }
    for (i, row) in rows.iter().enumerate() {
        let r = i as u32 + 1;
        ws.write(r, 0, row.kategorie.as_str())?;
        ws.write(r, 1, row.typ.as_str())?;
        ws.write(r, 2, row.first.as_str())?;
        ws.write(r, 3, row.second.as_str())?;
        ws.write(r, 4, row.third1.as_str())?;
        ws.write(r, 5, row.third2.as_str())?;
    }
    wb.save_to_buffer()
}

/// Urkunden as a merge-ready CSV (Affinity Publisher / LibreOffice data merge):
/// comma-delimited, UTF-8, header = the merge field names. One row per placement.
pub fn urkunden_csv(rows: &[UrkundeRow]) -> Result<String, csv::Error> {
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record(["vorname", "nachname", "platz", "klasse", "verein"])?;
    for r in rows {
        w.write_record([&r.vorname, &r.nachname, &r.platz, &r.klasse, &r.verein])?;
    }
    Ok(String::from_utf8(w.into_inner().expect("csv writer")).expect("utf8"))
}

/// Urkunden workbook (one row per awarded placement = mail-merge data source).
pub fn urkunden_xlsx(rows: &[UrkundeRow]) -> Result<Vec<u8>, XlsxError> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("Urkunden")?;
    let bold = Format::new().set_bold();
    for (c, h) in ["vorname", "nachname", "platz", "klasse", "verein"].iter().enumerate() {
        ws.write_with_format(0, c as u16, *h, &bold)?;
    }
    for (i, row) in rows.iter().enumerate() {
        let r = i as u32 + 1;
        ws.write(r, 0, row.vorname.as_str())?;
        ws.write(r, 1, row.nachname.as_str())?;
        ws.write(r, 2, row.platz.as_str())?;
        ws.write(r, 3, row.klasse.as_str())?;
        ws.write(r, 4, row.verein.as_str())?;
    }
    wb.save_to_buffer()
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{Reader, Xlsx};
    use std::io::Cursor;

    #[test]
    fn urkunden_xlsx_roundtrips_via_calamine() {
        let rows = vec![
            UrkundeRow { vorname: "Anna".into(), nachname: "Adler".into(), platz: "1.".into(),
                         klasse: "w | U15 | -52kg".into(), verein: "JC Berlin".into() },
            UrkundeRow { vorname: "Bea".into(), nachname: "Berger".into(), platz: "2.".into(),
                         klasse: "w | U15 | -52kg".into(), verein: "JC Köln".into() },
        ];
        let bytes = urkunden_xlsx(&rows).unwrap();
        assert!(bytes.len() > 100);
        // Read the generated workbook back and check the header + a data cell.
        let mut wb: Xlsx<_> = calamine::open_workbook_from_rs(Cursor::new(bytes)).unwrap();
        let range = wb.worksheet_range("Urkunden").unwrap();
        assert_eq!(range.get((0, 0)).unwrap().to_string(), "vorname");
        assert_eq!(range.get((1, 0)).unwrap().to_string(), "Anna");
        assert_eq!(range.get((1, 2)).unwrap().to_string(), "1.");
        assert_eq!(range.get((2, 4)).unwrap().to_string(), "JC Köln");
    }

    #[test]
    fn urkunden_csv_is_merge_ready() {
        let rows = vec![UrkundeRow {
            vorname: "Anna".into(), nachname: "Adler, jr".into(), platz: "1.".into(),
            klasse: "w | U15 | -52kg".into(), verein: "JC Berlin".into(),
        }];
        let csv = urkunden_csv(&rows).unwrap();
        let mut lines = csv.lines();
        assert_eq!(lines.next().unwrap(), "vorname,nachname,platz,klasse,verein");
        // comma in a field is quoted (valid CSV for data merge).
        assert!(lines.next().unwrap().contains("\"Adler, jr\""));
    }

    #[test]
    fn wiegekarten_csv_is_merge_ready() {
        let rows = vec![
            WiegekarteRow {
                id: 42, nachname: "Adler".into(), vorname: "Anna".into(), verein: "JC Berlin".into(),
                geschlecht: "w".into(), jahrgang: "2014".into(), altersklasse: "U13".into(),
            },
            WiegekarteRow {
                id: 7, nachname: "Berger, jr".into(), vorname: "Bea".into(), verein: "".into(),
                geschlecht: "".into(), jahrgang: "".into(), altersklasse: "".into(),
            },
        ];
        let csv = wiegekarten_csv(&rows).unwrap();
        let mut lines = csv.lines();
        assert_eq!(lines.next().unwrap(), "id,nachname,vorname,verein,geschlecht,jahrgang,altersklasse");
        assert_eq!(lines.next().unwrap(), "42,Adler,Anna,JC Berlin,w,2014,U13");
        // comma in a field is quoted (valid CSV for data merge); empty fields stay empty.
        assert_eq!(lines.next().unwrap(), "7,\"Berger, jr\",Bea,,,,");
    }

    #[test]
    fn results_xlsx_has_header_and_rows() {
        let rows = vec![ResultRow {
            kategorie: "m U15 -34kg".into(), typ: "ko".into(),
            first: "X Y".into(), second: "A B".into(), third1: "C D".into(), third2: "E F".into(),
        }];
        let bytes = results_xlsx(&rows).unwrap();
        let mut wb: Xlsx<_> = calamine::open_workbook_from_rs(Cursor::new(bytes)).unwrap();
        let range = wb.worksheet_range("Ergebnisse").unwrap();
        assert_eq!(range.get((0, 0)).unwrap().to_string(), "Kategorie");
        assert_eq!(range.get((1, 1)).unwrap().to_string(), "ko");
        assert_eq!(range.get((1, 2)).unwrap().to_string(), "X Y");
    }
}
