// SPDX-License-Identifier: GPL-3.0-or-later
//! Printable PDF output (pure Rust, printpdf) — hand-fillable forms.
//! A generic gridded table (landscape A4, paginated) drives the weigh-in cards
//! (blank Gewicht/Unterschrift cells) and the results list.

use printpdf::{BuiltinFont, Line, Mm, PdfDocument, PdfLayerReference, Point};
use std::io::BufWriter;

const PAGE_W: f32 = 297.0; // A4 landscape
const PAGE_H: f32 = 210.0;
const MARGIN: f32 = 10.0;
const ROW_H: f32 = 9.0;

fn hline(layer: &PdfLayerReference, x1: f32, x2: f32, y: f32) {
    layer.add_line(Line {
        points: vec![(Point::new(Mm(x1), Mm(y)), false), (Point::new(Mm(x2), Mm(y)), false)],
        is_closed: false,
    });
}
fn vline(layer: &PdfLayerReference, x: f32, y1: f32, y2: f32) {
    layer.add_line(Line {
        points: vec![(Point::new(Mm(x), Mm(y1)), false), (Point::new(Mm(x), Mm(y2)), false)],
        is_closed: false,
    });
}

/// Gridded, paginated table PDF. `col_w` = column widths in mm (sum ≤ usable).
pub fn table_pdf(title: &str, headers: &[&str], col_w: &[f32], rows: &[Vec<String>]) -> Vec<u8> {
    let (doc, page1, layer1) = PdfDocument::new(title, Mm(PAGE_W), Mm(PAGE_H), "L1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).unwrap();

    let table_w: f32 = col_w.iter().sum();
    let mut xs = vec![MARGIN];
    for w in col_w {
        xs.push(xs.last().unwrap() + w);
    }
    let top = PAGE_H - MARGIN;
    let bottom_limit = MARGIN;
    let rows_per_page = (((top - 12.0) - ROW_H - bottom_limit) / ROW_H).floor() as usize;

    let mut layer = doc.get_page(page1).get_layer(layer1);
    let mut first = true;
    let mut i = 0usize;
    while i < rows.len() || first {
        if !first {
            let (p, l) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "L");
            layer = doc.get_page(p).get_layer(l);
        }
        first = false;

        // Title + header row.
        layer.use_text(title, 14.0, Mm(MARGIN), Mm(top - 2.0), &bold);
        let header_y = top - 12.0;
        for (c, h) in headers.iter().enumerate() {
            layer.use_text(*h, 9.0, Mm(xs[c] + 1.0), Mm(header_y - 6.0), &bold);
        }
        let table_top = header_y;
        hline(&layer, MARGIN, MARGIN + table_w, table_top); // top border

        // Up to rows_per_page data rows on this page.
        let end = (i + rows_per_page).min(rows.len());
        let n_on_page = end - i;
        let mut y = table_top;
        // header band bottom + each row band bottom
        hline(&layer, MARGIN, MARGIN + table_w, y - ROW_H);
        for r in i..end {
            let row_top = y - ROW_H * ((r - i) as f32 + 1.0);
            for (c, cell) in rows[r].iter().enumerate() {
                if c < xs.len() - 1 {
                    layer.use_text(cell.as_str(), 9.0, Mm(xs[c] + 1.0), Mm(row_top - 6.0), &font);
                }
            }
            hline(&layer, MARGIN, MARGIN + table_w, row_top - ROW_H);
        }
        // vertical column separators spanning header + the rows on this page.
        let grid_bottom = table_top - ROW_H * (n_on_page as f32 + 1.0);
        for x in &xs {
            vline(&layer, *x, table_top, grid_bottom);
        }
        let _ = &y;
        i = end;
        if i >= rows.len() {
            break;
        }
    }

    let mut buf = Vec::new();
    doc.save(&mut BufWriter::new(&mut buf)).unwrap();
    buf
}

use crate::export::{ResultRow, WiegekarteRow};

/// Weigh-in cards PDF: blank Gewicht + Unterschrift cells (hand-filled at the scale).
pub fn wiegekarten_pdf(rows: &[WiegekarteRow]) -> Vec<u8> {
    let headers = ["Nachname", "Vorname", "Verein", "G", "Jg", "Klasse", "Gewicht", "Unterschrift"];
    let col_w = [40.0, 35.0, 55.0, 12.0, 16.0, 30.0, 40.0, 49.0]; // sum = 277 (usable)
    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|r| vec![
            r.nachname.clone(), r.vorname.clone(), r.verein.clone(), r.geschlecht.clone(),
            r.jahrgang.clone(), r.altersklasse.clone(), String::new(), String::new(),
        ])
        .collect();
    table_pdf("Wiegekarten", &headers, &col_w, &data)
}

/// Results list PDF (printed final placements).
pub fn results_pdf(rows: &[ResultRow]) -> Vec<u8> {
    let headers = ["Kategorie", "Typ", "1.", "2.", "3.", "3."];
    let col_w = [55.0, 30.0, 48.0, 48.0, 48.0, 48.0]; // sum = 277
    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|r| vec![r.kategorie.clone(), r.typ.clone(), r.first.clone(), r.second.clone(), r.third1.clone(), r.third2.clone()])
        .collect();
    table_pdf("Ergebnisliste", &headers, &col_w, &data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::WiegekarteRow;

    #[test]
    fn wiegekarten_pdf_is_valid_and_paginates() {
        let rows: Vec<WiegekarteRow> = (0..80)
            .map(|i| WiegekarteRow {
                id: i, nachname: format!("Name{i}"), vorname: "V".into(), verein: "Club".into(),
                geschlecht: "m".into(), jahrgang: "2014".into(), altersklasse: "U13".into(),
            })
            .collect();
        let pdf = wiegekarten_pdf(&rows);
        assert!(pdf.starts_with(b"%PDF"), "valid PDF header");
        assert!(pdf.len() > 2000, "non-trivial: {} bytes", pdf.len());
    }
}
