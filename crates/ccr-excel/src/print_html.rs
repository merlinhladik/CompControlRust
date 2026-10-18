// SPDX-License-Identifier: GPL-3.0-or-later
//! Printable HTML (CSS paged-media) for hand-fillable weigh-in cards.
//!
//! Served straight from the LAN server; the operator prints with Cmd/Ctrl+P,
//! which also yields a PDF. Self-contained: inline CSS, no external assets, no
//! framework. One card per fighter, 6 cards per A4 portrait page (3 × 2 grid),
//! `break-inside: avoid` so a card is never split across a page. The Gewicht
//! line is blank — filled by hand at the scale. (No Unterschrift line.)

use crate::export::WiegekarteRow;

/// Minimal HTML-attribute/text escaping for athlete-supplied strings.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Render all weigh-in cards as one printable A4 page-set (6 per page).
pub fn wiegekarten_html(rows: &[WiegekarteRow]) -> String {
    let mut cards = String::new();
    for r in rows {
        // gender · year · age class, dropping any empty part.
        let meta = [r.geschlecht.as_str(), r.jahrgang.as_str(), r.altersklasse.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" · ");
        cards.push_str(&format!(
            "<div class=\"card\">\
               <div class=\"id\">#{id}</div>\
               <div class=\"name\">{nach}, {vor}</div>\
               <div class=\"club\">{club}</div>\
               <div class=\"meta\">{meta}</div>\
               <div class=\"fill\"><span class=\"lbl\">Gewicht</span><span class=\"line\"></span><span class=\"unit\">kg</span></div>\
             </div>",
            id = r.id,
            nach = esc(&r.nachname),
            vor = esc(&r.vorname),
            club = esc(&r.verein),
            meta = esc(&meta),
        ));
    }

    PAGE_TEMPLATE
        .replace("__COUNT__", &rows.len().to_string())
        .replace("__CARDS__", &cards)
}

/// The page shell. Real CSS braces (no `format!` escaping); data is spliced via
/// `__COUNT__` / `__CARDS__` placeholders so the stylesheet stays readable.
/// A4 portrait, 3 × 2 grid = 6 cards per page.
const PAGE_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="de"><head><meta charset="utf-8">
<title>Wiegekarten</title>
<style>
  * { box-sizing: border-box; }
  html, body { margin: 0; font-family: Helvetica, Arial, sans-serif; color: #000; }
  .toolbar { padding: 8px 12px; background: #eef7ee; border-bottom: 1px solid #cfe0cf;
              font-size: 14px; position: sticky; top: 0; }
  .toolbar button { font-size: 14px; padding: 4px 10px; cursor: pointer; }
  .cards { display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 4mm; padding: 10mm; }
  .card { border: 1px solid #000; border-radius: 2mm; padding: 4mm; height: 130mm;
          display: flex; flex-direction: column; position: relative; }
  .card .id   { position: absolute; top: 3mm; right: 4mm; font-size: 8pt; color: #777; }
  .card .name { font-size: 16pt; font-weight: bold; padding-right: 14mm; }
  .card .club { font-size: 12pt; margin-top: 2mm; }
  .card .meta { font-size: 11pt; color: #333; margin-top: 2mm; }
  .card .fill { font-size: 12pt; display: flex; align-items: flex-end; gap: 2mm; margin-top: auto; }
  .card .fill .lbl  { min-width: 22mm; }
  .card .fill .line { flex: 1; border-bottom: 1px solid #000; min-width: 20mm; height: 1.1em; }
  .card .fill .unit { color: #333; }
  @page { size: A4 portrait; margin: 0; }
  @media print {
    .toolbar { display: none; }
    .card { break-inside: avoid; }
  }
</style></head>
<body>
  <div class="toolbar">__COUNT__ Wiegekarten &mdash;
    <button onclick="window.print()">Drucken / Als PDF speichern</button>
    <span style="color:#555">(6 pro Seite, A4 hochkant)</span>
  </div>
  <div class="cards">__CARDS__</div>
</body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::WiegekarteRow;

    fn row(id: i32, last: &str, club: &str) -> WiegekarteRow {
        WiegekarteRow {
            id, nachname: last.into(), vorname: "V".into(), verein: club.into(),
            geschlecht: "m".into(), jahrgang: "2014".into(), altersklasse: "U13".into(),
        }
    }

    #[test]
    fn renders_one_card_per_fighter_and_escapes() {
        let rows = vec![row(1, "A & B", "JC <Berlin>"), row(2, "Mayer", "JC Köln")];
        let html = wiegekarten_html(&rows);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert_eq!(html.matches("class=\"card\"").count(), 2, "one card per fighter");
        assert!(html.contains("2 Wiegekarten"));
        assert!(html.contains("#1") && html.contains("#2"), "ids printed");
        // HTML-special chars in athlete data are escaped.
        assert!(html.contains("A &amp; B"));
        assert!(html.contains("JC &lt;Berlin&gt;"));
        assert!(!html.contains("JC <Berlin>"));
        // no leftover placeholders
        assert!(!html.contains("__CARDS__") && !html.contains("__COUNT__"));
    }

    #[test]
    fn no_signature_line_and_six_per_page_layout() {
        let rows: Vec<WiegekarteRow> = (1..=7).map(|i| row(i, "X", "Y")).collect();
        let html = wiegekarten_html(&rows);
        // Unterschrift is gone from the card and the toolbar hint.
        assert!(!html.contains("Unterschrift"));
        // 3 columns × 2 rows per A4 portrait page.
        assert!(html.contains("grid-template-columns: 1fr 1fr 1fr"));
        assert!(html.contains("A4 portrait"));
        assert!(html.contains("break-inside: avoid"));
        assert!(html.contains("6 pro Seite"));
        assert_eq!(html.matches("class=\"card\"").count(), 7, "all 7 cards rendered");
    }

    #[test]
    fn drops_empty_meta_parts() {
        let mut r = row(9, "Solo", "");
        r.geschlecht = "".into();
        r.altersklasse = "".into();
        let html = wiegekarten_html(&[r]);
        // only jahrgang remains in the meta line — no stray separators.
        assert!(!html.contains(" ·  · "));
    }
}
