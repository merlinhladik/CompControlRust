// SPDX-License-Identifier: GPL-3.0-or-later
//! Phase 0.5 Go/No-Go spike: can `calamine` read the legacy DJB `.xls` (BIFF)
//! oracle sheets well enough for CCR's needs?
//!
//! Two distinct questions, reported separately:
//!   (A) VALUES — needed at RUNTIME (seeding reimport, results backfill).
//!   (B) FILL formatting (`fill_pattern`) — needed ONLY to re-derive the frozen
//!       topology, which CCR copies from constants like JF does. Nice-to-have.
//!
//! Mirrors what edv/tests/ko_form_decoder.py reads:
//!   ko_8  : sheet 1, los_col 0, los_rows 11..19, cols {2,6,10}
//!   ko_16 : sheet 1, los_col 0, los_rows 11..27, cols {2,6,10,14,24,28,32,36,40}
//!   ko_32 : sheet 0, los_col 2, los_rows 9..41,  cols {6,10,14,18,21,27,31,35,39,43,47,51}
//!
//! Run: cargo run -p ccr-excel --example xls_probe

use calamine::{open_workbook_auto, Data, Reader};

const TEMPLATE_DIR: &str = "../edv/backend/services/templates";

struct FormSpec {
    file: &'static str,
    sheet: usize,
    los_col: usize,
    los_rows: std::ops::Range<usize>,
    diagram_cols: &'static [usize],
}

fn forms() -> Vec<FormSpec> {
    vec![
        FormSpec { file: "ko_8.xls",  sheet: 1, los_col: 0, los_rows: 11..19,
                   diagram_cols: &[2, 6, 10] },
        FormSpec { file: "ko_16.xls", sheet: 1, los_col: 0, los_rows: 11..27,
                   diagram_cols: &[2, 6, 10, 14, 24, 28, 32, 36, 40] },
        FormSpec { file: "ko_32.xls", sheet: 0, los_col: 2, los_rows: 9..41,
                   diagram_cols: &[6, 10, 14, 18, 21, 27, 31, 35, 39, 43, 47, 51] },
    ]
}

fn as_int(d: &Data) -> Option<i64> {
    match d {
        Data::Int(n) => Some(*n),
        Data::Float(f) => Some(*f as i64),
        Data::String(s) => s.trim().parse::<f64>().ok().map(|f| f as i64),
        _ => None,
    }
}

fn main() {
    println!("=== Phase 0.5 calamine .xls probe ===\n");

    // First: can we even open every template and enumerate sheets? (Question A, baseline)
    for f in ["ko_8.xls", "ko_16.xls", "ko_32.xls",
              "repechage_32.xls", "repechage_64.xls",
              "pool_listen.xls", "pool_2er.xls"] {
        let path = format!("{TEMPLATE_DIR}/{f}");
        match open_workbook_auto(&path) {
            Ok(mut wb) => {
                let names = wb.sheet_names();
                let dims: Vec<String> = (0..names.len())
                    .map(|i| match wb.worksheet_range_at(i) {
                        Some(Ok(r)) => { let (h, w) = r.get_size(); format!("{h}x{w}") }
                        _ => "ERR".into(),
                    })
                    .collect();
                println!("OPEN ok  {f:22} sheets={:?} dims={:?}", names, dims);
            }
            Err(e) => println!("OPEN FAIL {f:22} {e}"),
        }
    }

    // Question A: do the integer VALUES in the decoder's columns come through?
    println!("\n--- VALUES in decoder columns (Question A: runtime need) ---");
    for spec in forms() {
        let path = format!("{TEMPLATE_DIR}/{}", spec.file);
        let mut wb = match open_workbook_auto(&path) {
            Ok(wb) => wb,
            Err(e) => { println!("{}: open fail: {e}", spec.file); continue; }
        };
        let range = match wb.worksheet_range_at(spec.sheet) {
            Some(Ok(r)) => r,
            _ => { println!("{}: sheet {} unreadable", spec.file, spec.sheet); continue; }
        };

        // Use ABSOLUTE coordinates via cells() — Range::get() is start-relative,
        // which silently mis-reads any sheet whose used range doesn't start at A1.
        let mut los: Vec<(usize, i64)> = Vec::new();
        let mut diagram: Vec<i64> = Vec::new();
        for (r, c, d) in range.cells() {
            if c == spec.los_col && spec.los_rows.contains(&r) {
                if let Some(n) = as_int(d) { los.push((r, n)); }
            }
            if spec.diagram_cols.contains(&c) {
                if let Some(n) = as_int(d) { if n > 0 { diagram.push(n); } }
            }
        }
        los.sort_by_key(|&(r, _)| r);
        let los: Vec<i64> = los.into_iter().map(|(_, n)| n).collect();
        diagram.sort_unstable();
        let max = diagram.last().copied().unwrap_or(0);
        // Robustness check, offset-independent: ALL integer cells on the sheet.
        let all_ints: Vec<i64> =
            range.cells().filter_map(|(_, _, d)| as_int(d)).filter(|&n| n > 0).collect();
        let all_max = all_ints.iter().max().copied().unwrap_or(0);
        println!(
            "{:9} los={:>2} vals {:?}  | decoder-cols: {} ints max={}  | ALL ints: {} cells max={}",
            spec.file, los.len(), los, diagram.len(), max, all_ints.len(), all_max
        );
    }

    // Question B: is cell FILL formatting reachable at all via calamine?
    println!("\n--- FILL formatting (Question B: oracle-only) ---");
    println!("calamine's public Reader API exposes cell VALUES, not per-cell");
    println!("fill_pattern for BIFF .xls. If the lines above show correct values,");
    println!("the RUNTIME need (A) is met; the topology is copied from frozen");
    println!("constants regardless, so (B) is not a runtime blocker.");
}
