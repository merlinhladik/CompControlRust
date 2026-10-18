// SPDX-License-Identifier: GPL-3.0-or-later
//! Printable PDF output (pure Rust, printpdf) — hand-fillable forms.
//! Weigh-in cards are laid out as 6 bordered cards per A4 portrait page
//! (3 × 2), each with a blank Gewicht line (hand-filled at the scale, no
//! Unterschrift line). The results list stays a gridded, paginated table.

use printpdf::path::PaintMode;
use printpdf::{BuiltinFont, Line, Mm, PdfDocument, PdfLayerReference, Point, Rect};
use std::io::BufWriter;

// ── Gridded table (landscape A4) — used by the results list ─────────────────
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
        hline(&layer, MARGIN, MARGIN + table_w, table_top - ROW_H);
        for r in i..end {
            let row_top = table_top - ROW_H * ((r - i) as f32 + 1.0);
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

/// Weigh-in cards PDF: 6 cards per A4 portrait page (3 × 2 grid), each a
/// bordered box with name/club/meta and a blank Gewicht line. No Unterschrift.
pub fn wiegekarten_pdf(rows: &[WiegekarteRow]) -> Vec<u8> {
    const W: f32 = 210.0; // A4 portrait
    const H: f32 = 297.0;
    const M: f32 = 10.0;
    const GAP: f32 = 4.0;
    const COLS: usize = 3;
    const ROWS: usize = 2;
    const PER_PAGE: usize = COLS * ROWS;
    let card_w = (W - 2.0 * M - (COLS - 1) as f32 * GAP) / COLS as f32;
    let card_h = (H - 2.0 * M - (ROWS - 1) as f32 * GAP) / ROWS as f32;

    let (doc, page1, layer1) = PdfDocument::new("Wiegekarten", Mm(W), Mm(H), "L1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).unwrap();

    let mut layer = doc.get_page(page1).get_layer(layer1);
    let mut first = true;
    let mut i = 0usize;
    while i < rows.len() || first {
        if !first {
            let (p, l) = doc.add_page(Mm(W), Mm(H), "L");
            layer = doc.get_page(p).get_layer(l);
        }
        first = false;

        for slot in 0..PER_PAGE.min(rows.len() - i) {
            let r = &rows[i + slot];
            let (c, row) = (slot % COLS, slot / COLS);
            let x0 = M + c as f32 * (card_w + GAP);
            let y_top = H - M - row as f32 * (card_h + GAP);
            let y_bot = y_top - card_h;
            let px = x0 + 4.0; // inner padding

            // Card border (stroke only).
            layer.add_rect(
                Rect::new(Mm(x0), Mm(y_bot), Mm(x0 + card_w), Mm(y_top))
                    .with_mode(PaintMode::Stroke),
            );
            // id (top right)
            layer.use_text(&format!("#{}", r.id), 8.0, Mm(x0 + card_w - 16.0), Mm(y_top - 5.0), &font);
            // name (bold)
            layer.use_text(&format!("{}, {}", r.nachname, r.vorname), 15.0, Mm(px), Mm(y_top - 12.0), &bold);
            // club
            layer.use_text(&r.verein, 11.0, Mm(px), Mm(y_top - 22.0), &font);
            // meta: gender · year · age class, empty parts dropped
            let meta = [r.geschlecht.as_str(), r.jahrgang.as_str(), r.altersklasse.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" · ");
            if !meta.is_empty() {
                layer.use_text(&meta, 10.0, Mm(px), Mm(y_top - 31.0), &font);
            }
            // Gewicht fill line (blank, hand-filled at the scale).
            let line_y = y_bot + 10.0;
            layer.use_text("Gewicht", 11.0, Mm(px), Mm(line_y + 1.0), &font);
            layer.add_line(Line {
                points: vec![
                    (Point::new(Mm(px + 22.0), Mm(line_y)), false),
                    (Point::new(Mm(x0 + card_w - 12.0), Mm(line_y)), false),
                ],
                is_closed: false,
            });
            layer.use_text("kg", 10.0, Mm(x0 + card_w - 10.0), Mm(line_y + 1.0), &font);
        }
        i += PER_PAGE;
        if i >= rows.len() {
            break;
        }
    }

    let mut buf = Vec::new();
    doc.save(&mut BufWriter::new(&mut buf)).unwrap();
    buf
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

    fn sample(n: usize) -> Vec<WiegekarteRow> {
        (0..n)
            .map(|i| WiegekarteRow {
                id: i as i32, nachname: format!("Name{i}"), vorname: "V".into(), verein: "Club".into(),
                geschlecht: "m".into(), jahrgang: "2014".into(), altersklasse: "U13".into(),
            })
            .collect()
    }

    /// Count PDF pages: "/Type/Page" occurrences, minus the "/Type/Pages"
    /// node (which shares the prefix).
    fn page_count(pdf: &[u8]) -> usize {
        let s = String::from_utf8_lossy(pdf);
        s.matches("/Type/Page").count() - s.matches("/Type/Pages").count()
    }

    #[test]
    fn wiegekarten_pdf_is_valid_and_paginates_six_per_page() {
        let pdf = wiegekarten_pdf(&sample(12));
        assert!(pdf.starts_with(b"%PDF"), "valid PDF header");
        assert!(pdf.len() > 2000, "non-trivial: {} bytes", pdf.len());
        // 12 cards / 6 per page = 2 pages.
        assert_eq!(page_count(&pdf), 2, "two pages for 12 cards");
    }

    #[test]
    fn wiegekarten_pdf_partial_last_page() {
        let pdf = wiegekarten_pdf(&sample(7));
        assert!(pdf.starts_with(b"%PDF"));
        assert_eq!(page_count(&pdf), 2, "7 cards → 2 pages (6 + 1)");
    }

    #[test]
    fn wiegekarten_pdf_single_card() {
        let pdf = wiegekarten_pdf(&sample(1));
        assert!(pdf.starts_with(b"%PDF"));
        assert_eq!(page_count(&pdf), 1);
    }
}
