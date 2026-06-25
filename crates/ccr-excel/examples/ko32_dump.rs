// SPDX-License-Identifier: GPL-3.0-or-later
//! Why does calamine see almost no values in ko_32.xls? Dump everything.
//! Run: cargo run -p ccr-excel --example ko32_dump

use calamine::{open_workbook_auto, Data, Reader};

fn main() {
    let path = "../edv/backend/services/templates/ko_32.xls";
    let mut wb = open_workbook_auto(path).expect("open");
    println!("sheets: {:?}", wb.sheet_names());

    for idx in 0..wb.sheet_names().len() {
        let range = match wb.worksheet_range_at(idx) {
            Some(Ok(r)) => r,
            _ => { println!("sheet {idx}: unreadable"); continue; }
        };
        let (h, w) = range.get_size();
        let start = range.start().unwrap_or((0, 0));
        println!("\n=== sheet {idx} size {h}x{w} start={:?} ===", start);

        let mut kinds = std::collections::HashMap::<&str, usize>::new();
        let mut samples: Vec<String> = Vec::new();
        for (r, c, d) in range.cells() {
            let k = match d {
                Data::Int(_) => "Int",
                Data::Float(_) => "Float",
                Data::String(_) => "String",
                Data::Bool(_) => "Bool",
                Data::Empty => "Empty",
                _ => "other",
            };
            if !matches!(d, Data::Empty) {
                *kinds.entry(k).or_default() += 1;
                if samples.len() < 40 {
                    samples.push(format!("({r},{c})={:?}", d));
                }
            }
        }
        println!("non-empty cell kinds: {:?}", kinds);
        println!("first non-empty cells:");
        for s in &samples { println!("  {s}"); }
    }
}
