# Affinity Publisher templates

Layout templates for the two CCR data-merge CSVs. `.afpub` is a proprietary
binary format, so the layouts ship as SVG (Affinity opens SVG with editable
text). Merge fields cannot be embedded in SVG — replace the «token»
placeholders once in Affinity, then save as `.aftemplate`.

| Template | Data source | CSV columns |
|---|---|---|
| `wiegekarte.svg` (105 × 59.4 mm card) | `GET /api/export/wiegekarten.csv` | id, nachname, vorname, verein, geschlecht, jahrgang, altersklasse |
| `urkunde.svg` (A4 portrait) | `GET /api/export/urkunden.csv` | vorname, nachname, platz, klasse, altersklasse, gewichtsklasse, verein |

## Urkunde (one record per page, pre-printed forms)

The certificate paper is pre-printed — the SVG is an overlay with merge
fields only, no decoration.

1. Open `urkunde.svg` in Affinity Publisher (File → Open).
2. Move the four text lines to match the printed form (coordinates are mm;
   print one test page on a blank form to calibrate).
3. Window → Data Merge Manager → add the downloaded `urkunden.csv`
   (comma-delimited, UTF-8); select each «token» with the Text tool and
   double-click the matching field. `«platz»` already contains the dot
   (`1.`). `klasse` is the combined label (`w | U15 | -52kg`) — the
   template uses `altersklasse` + `gewichtsklasse` instead; swap if you
   prefer the one-field version.
4. Generate → one page per placement; print onto the pre-printed forms.

## Wiegekarte (10 cards per A4 page)

1. New A4 portrait document, margins 0.
2. Draw a Data Merge Layout frame (Data Merge Layout Tool) covering the
   full page; set the grid to 2 columns × 5 rows (cell = 105 × 59.4 mm,
   no gaps — the card's dashed border is the cut line).
3. File → Place → `wiegekarte.svg` inside the first cell (or open the SVG
   separately and copy its objects in).
4. Replace the «token» placeholders with fields from `wiegekarten.csv`
   as above. `id` is the merge key / QR value — keep it on the card.
5. Generate → cards for all participants in record order. The CSV is
   weight-independent, so cards can be printed before weigh-in.

Save the finished documents as `.aftemplate` next to these SVGs.
