# Druckfunktion mit Affinity Publisher (Data Merge)

CCR liefert für die Affinity-Druckstücke **Merge-fertige CSVs** (UTF-8,
Kommata, Kopfzeile = Feldname = Merge-Placeholder). Die Vorlage wird einmal in
Affinity Publisher entworfen; je Turnier wird nur die CSV-Datei getauscht.

## Druckstücke und Datenquellen

| Druckstück | Endpunkt | Merge-Felder | Zeitpunkt |
|---|---|---|---|
| Wiegekarten | `/api/export/wiegekarten.csv` | `id, nachname, vorname, verein, geschlecht, jahrgang, altersklasse` | **vor** dem Wiegen (gewichts-unabhängig) |
| Urkunden | `/api/export/urkunden.csv[?bracket=N]` | `vorname, nachname, platz, klasse, altersklasse, gewichtsklasse, verein` | am Ende, pro Klasse (Podium) |
| Ergebnisliste | `/api/export/ergebnisliste.csv` | `kategorie, typ, gold, goldVerein, silber, silberVerein, bronze1, bronze1Verein, bronze2, bronze2Verein` | am Ende, 1 Zeile/Liste |

Wettkampflisten (Pool-Grid, KO/Repechage-Trees) laufen **nicht** über Affinity:
die Struktur variiert je Klasse (Spalten/Baumtiefe) — dafür bleibt der
CCR-HTML-Druck (`/print/listen`).

## Vorlagenspezifikationen

### Wiegekarten (A4-Porträt, 3 × 2 Raster)

- Karte ≈ 130 × 75 mm, Rand 10 mm, 6 pro Seite.
- Felder: `nachname` (groß, fett), `vorname`, `verein`,
  `geschlecht · jahrgang · altersklasse` (klein, Meta-Zeile).
- Leere Zeile zum Handausfüllen: **„Gewicht: ______ kg“** (offiziell füllt an
  der Waage). Kein QR, keine Unterschrifts-Zeile.
- `id` kann klein in eine Ecke (z. B. Ecke unten rechts) für den
  Abtipp-Bezug am Haupt-PC.

### Urkunden (1 Vorlage, 1 Urkunde/Seite)

- Felder: `vorname nachname` (groß), `platz` (z. B. „1.“), `klasse`
  (z. B. „w | U15 | -52kg“), `verein`.
- `altersklasse` / `gewichtsklasse` stehen nur zur Verfügung, falls die
  Vorlage sie einzeln braucht (stehen sonst schon in `klasse`).

### Ergebnisliste (Tabelle, 1 Zeile pro Liste)

- Kopfzeile aus den Merge-Feldern, darunter eine Merge-Wiederholung (Repeat
  Group) mit den 10 Feldern pro Zeile.
- Pro Turnier eine Seite pro Spaltengruppe (z. B. „Klasse | Gold | Silber |
  Bronze“), leere Zellen bleiben leer (unfertige Listen haben keine Namen).

## Data Merge einrichten (einmalig)

1. Affinity Publisher: Vorlage anlegen, Textfelder mit den obigen
   Feldnamen beschriften (oder Merge-Platzhalter aus dem CSV importieren).
2. **Veröffentlichen → Datenzusammenführung → Neues Datenfeld** → CSV wählen.
3. Felder zuordnen (die stabilen Header garantieren 1:1-Zuordnung, bleibt
   bei allen Turnieren gleich).
4. Merge-Seiten erzeugen lassen. Die Zuordnung bleibt in der Datei gespeichert.

**Je Turnier:** nur die neue CSV aus CCR exportieren (Admin → Export, bzw.
pro Klasse über die `?bracket=N`-Links), in der bestehenden Datenzusammen-
führung die Datei austauschen, Merge aktualisieren, drucken.

## Export-Zeitpunkte im Turnierverlauf

| Zeitpunkt | Was | Wie |
|---|---|---|
| Vor dem Wiegen | Alle Wiegekarten | `wiegekarten.csv` → Affinity → drucken |
| Während | — (Wettkampflisten = CCR-HTML-Druck) | `/print/listen` im Browser |
| Ende, pro Klasse | Urkunden | `urkunden.csv?bracket=N` (Link in der Klassen-Unterseite) |
| Ende | Ergebnisliste | `ergebnisliste.csv` (Admin → Export) |

## Hinweise

- CSVs sind merge-tauglich getestet: Kommas in Namen/vereinen werden
  korrekt quoted (z. B. „Adler, jr“).
- Leerwerte bleiben leere Zellen (keine Platzhalter wie „-“).
- Urkunden-Dateiname je Klasse (z. B. `urkunden-U11-Pool-2.csv`) — als
  Ordnungs-Hilfe beim Mehr-Klassen-Export.
