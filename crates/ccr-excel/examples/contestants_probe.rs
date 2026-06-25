// SPDX-License-Identifier: GPL-3.0-or-later
//! Round-trip CCR's contestants I/O against the REAL edv export files.
//! Run: cargo run -p ccr-excel --example contestants_probe

use ccr_excel::contestants;

fn main() {
    for name in ["contestants_male.json", "contestants_female.json"] {
        let path = format!("../edv/ImportTest/{name}");
        let people = match contestants::read_json(&path) {
            Ok(p) => p,
            Err(e) => {
                println!("{name}: read FAIL: {e}");
                continue;
            }
        };
        // JSON -> CSV -> JSON round trip.
        let csv = contestants::render_csv(&people).unwrap();
        let back = contestants::read_csv_str(&csv).unwrap();
        let lossless = people.len() == back.len()
            && people.iter().zip(&back).all(|(a, b)| {
                a.id == b.id && a.firstname == b.firstname && a.lastname == b.lastname
                    && a.birthyear == b.birthyear && a.gender == b.gender
                    && a.weight == b.weight && a.valid == b.valid && a.paid == b.paid
            });
        let g0 = people.first().map(|c| c.gender.clone()).unwrap_or_default();
        println!(
            "{name}: read {} contestants | sample gender={:?} weight={:?} | JSON↔CSV lossless={}",
            people.len(), g0, people.first().and_then(|c| c.weight), lossless
        );
    }
}
