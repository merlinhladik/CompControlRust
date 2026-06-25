<!-- SPDX-License-Identifier: CC0-1.0 -->
# Phase 0.5 — Excel-Spike: Ergebnis

**Verdikt: GO** (2026-06-18). Die 100-%-Rust-Excel-Entscheidung trägt auf der
Lese-Seite. `calamine` liest alle Legacy-`.xls`-(BIFF)-DJB-Templates.

Reproduzieren:
```sh
cargo run -p ccr-excel --example xls_probe     # Werte-Probe (Frage A + B)
cargo run -p ccr-excel --example ko32_dump     # Roh-Dump eines Blatts
```

## Frage A — VALUES (Laufzeit-Bedarf: Seeding-Reimport, Ergebnis-Backfill)

| Template | calamine liest | Beleg |
|---|---|---|
| ko_8.xls  | Los `[1,5,3,7,2,6,4,8]`, max Kf **11** | exakt balancierte KO-Seedung + 11 Kämpfe |
| ko_16.xls | Los `[1,9,5,13,…]`, max Kf **27** | exakt + 27 Kämpfe |
| ko_32.xls | 129 Int-Zellen, max Kf **61** | Final Kf61 vorhanden ⇒ vollständig |
| repechage_32/64, pool_*, alle Blätter | öffnen, Dim + Werte ok | — |

**Alle Werte vorhanden, alle Blätter lesbar.** Kein Datenverlust.

## Caveat 1 — per-Blatt Spalten-Offset
ko_8/16: calamine-Spaltenindizes == xlrd (Decoder-Spalten matchen direkt).
ko_32: genutzter Bereich beginnt nicht bei A1 (`start=(0,1)`); Kampffolge-Floats
liegen bei calamine 1 Spalte links der xlrd-Indizes (27→26, 31→30, …). **Daten
identisch, nur absolute Spaltennummer verschoben.** ⇒ Spalten pro Blatt empirisch
festlegen (Phase 4), nicht den xlrd-`_FORM`-Index blind übernehmen. Außerdem:
`calamine::Range::get((r,c))` ist START-RELATIV — immer über `cells()` mit
absoluten Koordinaten lesen (das war der erste Fehlalarm „ko_32 leer").

## Frage B — FILL-Formatierung (`fill_pattern`, nur Orakel)
calamines öffentliche Reader-API liefert für BIFF KEIN per-Zelle `fill_pattern`.
Der Python-Decoder (`edv/tests/ko_form_decoder.py`) braucht das, um grau-schattierte
Verlierer-Referenzen von echten Knoten zu trennen und so die Topologie
*herzuleiten*. **CCR braucht das zur Laufzeit nicht:** wie JF kopiert CCR die
eingefrorenen `_LB_STRUCTURE`/`_REPECHAGE_STRUCTURE`-Konstanten. Die Orakel-Tests
bleiben in der edv-Python-Suite (xlrd 1.2) als Quelle der Wahrheit; CCR-`ccr-domain`
spiegelt deren eingefrorenes Ergebnis und testet dagegen.

## Noch offen (Phase 4, nicht Go/No-Go)
- **Schreib-Round-Trip:** `rust_xlsxwriter` gegen ein `excel_form_filler`-Referenz-
  `.xlsx` prüfen (DJB-Formular bitnah genug für den Druck?). Niedrigeres Risiko.
- Urkunden-Export-Format (PDF? `.xlsx`?) — in Phase 4 prüfen.
