<!-- SPDX-License-Identifier: CC0-1.0 -->
# CompControlRust — Umsetzungsplan

Zusammenführung von **JudgeFrontend** (Live-Scoring-API + PWA) und **edv**
(Turnier-Admin, Tkinter) in **einen** Rust-Web-Server mit gemeinsamem
Web-Frontend.

## Getroffene Entscheidungen (2026-06-18)

1. **Scope: beide ablösen, voller Merge.** JF + edv → CCR. Endziel: CCR ist eine
   ganze Suite-Hälfte. Höchstes Risiko, höchster Aufwand — bewusst gewählt.
2. **Frontend: Leptos (Rust/WASM).** Full-Rust-Stack, geteilte Typen Frontend↔
   Backend (`ccr-domain`). Live- *und* Admin-UI in Leptos.
3. **Betrieb: LAN-Host + Browser.** Ein Hallen-Rechner hostet CCR, Bedienung per
   Browser im LAN. ⚠ Bricht edv's USB-Stick-pro-Person-Modell — siehe Risiko 4.
4. **Excel: 100 % Rust, kein Sidecar.** ⇒ Macht den `.xls`-Spike (Phase 0.5)
   zur **Go/No-Go-Vorbedingung**: wird die calamine-Parität nicht bewiesen, ist
   diese Entscheidung neu zu treffen, bevor Phase 4 beginnt.

## 0. Leitentscheidungen (vor Code lesen)

1. **Strangler statt Big-Bang.** CCR läuft neben der Python-Suite, teilt sich die
   bestehende Postgres-DB. Ein Bounded Context nach dem anderen wird übernommen;
   die DB ist die Brücke. Rückweg jederzeit möglich: Feature-Flag / Reverse-Proxy
   schaltet einen Pfad zurück auf JF/edv.
2. **Logik-Kern zuletzt.** Bracket-Topologien + Excel sind das Risiko. Erst
   Infrastruktur (Server, DB-Zugriff, einfache Endpunkte), dann Live-Pfad, dann
   der Algorithmus-Kern — und der nur mit den portierten Orakel-Tests als Netz.
3. **CCR-Schema ist kanonisch (Entscheidung 2026-10-01, todo B3).** `ccr-db` ist
   Schema-Owner der geteilten Postgres-`5432`; `migrations/` ist die kanonische
   DDL (Baseline idempotent: No-Op auf der edv-DB, volles Schema auf frischer DB).
   edv + JF sind Consumer (Modelle deklariert, eigene Migrationen: keine;
   edv-Alembic eingefroren). Phase 5 = Ablösung der edv-**Features**, nicht des
   Schema-Handovers (der ist 2026-10-01 erfolgt).
4. **Betriebsmodell-Bruch bewusst machen.** edv hat heute portable USB-Builds
   (offline Postgres-Client, Turnierbetrieb ohne Internet). „Web-Server" heißt:
   ein Host bedient mehrere Browser im LAN. Das ist eine *Entscheidung*, keine
   Nebensache — siehe offene Punkte unten.

## 1. Was womit zusammengeführt wird

| Heute (Python) | LOC | Wird in CCR |
|---|---|---|
| `JudgeFrontend/main.py` (FastAPI REST + WS, Live-Propagation) | ~2.6k | `ccr-server` Routen + WS-Handler |
| `JudgeFrontend/app.js`, `fightOrder.js`, PWA | ~2k JS | `frontend/` (Live-Ansicht) |
| `edv/backend/services/*` (tournament, bracket, excel) | ~9k | `ccr-domain` + `ccr-excel` + `ccr-db` |
| `edv/frontend/views/*` (Tkinter-GUI) | ~15k | `frontend/` (Admin-Ansicht) |
| `edv/backend/data/*` (SQLAlchemy-Modelle) | ~2k | `ccr-db` (sqlx) |

## 2. Rust-Stack (begründet)

- **axum + tokio** — Web + WebSocket aus einer Hand; ersetzt FastAPI sauber 1:1.
- **sqlx** (compile-time-geprüftes SQL, async, Postgres) statt ORM — passt auf
  das bestehende, von edv besessene Schema, ohne ein zweites ORM-Weltbild
  aufzuzwingen. (SeaORM als Alternative, falls aktive Schema-Verwaltung gewünscht.)
- **calamine** (lesen `.xls`/`.xlsx`) + **rust_xlsxwriter** (schreiben `.xlsx`).
  ⚠ Legacy-`.xls` (BIFF, die DJB-Orakelblätter) liest Python nur mit xlrd 1.2;
  calamine-Parität ist zu **beweisen**, sonst Python-Sidecar (s. Phase 4).
- **serde** für JSON-Verträge (Ipponboard-Webhook, REST, WS).

## 3. Phasenplan

### Phase 0 — Fundament (✅ in diesem Scaffold angelegt)
- Cargo-Workspace, 4 Crates, lauffähiger `ccr-server` mit `/health` + `/api/version`.
- `.env.example` zeigt auf dieselbe Postgres wie edv/JF.
- **Exit-Kriterium:** `cargo run -p ccr-server` antwortet; `cargo test` grün.

### Phase 0.5 — Excel-Spike (Go/No-Go, ZUERST) — ✅ GO (2026-06-18)
**Ergebnis: GO.** calamine liest alle BIFF-Templates inkl. ko_32 (Werte
vollständig); Details + Caveats in `docs/phase-0.5-excel-spike.md`. Probes:
`cargo run -p ccr-excel --example {xls_probe,ko32_dump}`.

Folgt direkt aus „100 % Rust, kein Sidecar". Bevor irgendetwas anderes startet:
- `calamine` gegen `edv/.../templates/ko_8.xls`, `ko_16.xls`, `ko_32.xls` und die
  Repechage-Blätter (`32_…_doppelter_Trostrunde.xls`, `64_…`).
- Ausgelesene Zellen/`fill_pattern` gegen den Python-`ko_form_decoder`-Output
  diffen (BIFF liest Python nur mit xlrd 1.2 — genau das ist die Wette).
- Schreib-Round-Trip eines DJB-Formulars mit `rust_xlsxwriter` gegen ein
  edv-`excel_form_filler`-Referenz-`.xlsx` prüfen.
- **Exit (GO):** calamine dekodiert die Orakelblätter bitgenau → 100 % Rust steht.
- **Exit (NO-GO):** Parität nicht erreichbar → zurück zur Excel-Entscheidung
  (Sidecar) **bevor** Aufwand in Phase 4 fließt. Rest der Migration unblockiert.

### Phase 1 — DB-Spiegel (read-only)  — ✅ Kern verifiziert (2026-06-18)
- ✅ `ccr-db/src/models.rs`: sqlx-`FromRow`-Structs für ALLE 7 Tabellen 1:1 nach
  `edv/backend/data/models.py`. Typen-Invarianten aus CLAUDE.md eingehalten
  (`INTEGER NULL`, `status VARCHAR(20)`, `doublestart String(10)`,
  `NUMERIC(5,2)`→`rust_decimal`). Quelle der Wahrheit = CCR `migrations/` (Schema-Owner seit 2026-10-01); edv `models.py` ist die Referenz, die wir spiegeln.
- ✅ Read-only Queries (`fights::all_fights`, `participants::resolve`,
  `brackets::resolve`) — runtime-`query_as`, kein DATABASE_URL zum Bauen nötig.
- ✅ `GET /api/matches` spiegelt JFs Match-Dict inkl. Envelope
  `{tournamentName, matches, currentMatchId}` und turnierweiter `fightNr`-
  Laufnummer (JF `main.py:465`).
- ✅ **Verifikation gegen echte edv-Daten:** edv-Seeds (KO+Pool+Doppelpool, 21
  Fights) → CCR vs. JF `/api/matches` feldweise gedifft = **0 Differenzen** auf
  dem gesamten direkt-ableitbaren Subset über alle 3 Bracket-Typen.
- ⏳ **Bewusst aufgeschoben** (in CCR `null`, Topologie/Labels = Phase 2/4):
  `nextMatchId`, `nextMatchPos`, `stageLabel`, `categoryLabel`, `groupLabel`,
  `category`, `bracketTypeLabel`, `p1From`, `p2From`. Noch offen: `/api/brackets`,
  und JFs `get_matches`-Seiteneffekte (Eager-Tree/Bye-Auflösung) bleiben Phase 2.
- Nur SELECT. Read-only-Endpunkte: `GET /api/matches`, `/api/brackets`.
- **Test:** gegen eine mit edv befüllte DB lesen; Ergebnis == JF `/api/matches`.
- **Exit:** CCR zeigt dieselben Matches wie JF, ohne irgendetwas zu schreiben.

### Phase 2 — Live-Pfad (das Herz von JF)  — 🔧 Kern läuft (2026-06-18)
- ✅ WS `/ws` über tokio-`broadcast`-Channel (ersetzt JFs ConnectionManager);
  `tokio::select!` über Socket-recv + Broadcast-recv pro Verbindung.
- ✅ `SCORE_UPDATE`, `SUBSCORE_UPDATE` (native JVP-Subscores), `STATUS_UPDATE`
  (Sieger aus Scores + **WB-Binärbaum-Propagation** mit Lazy-Create,
  `ccr-domain::ko::wb_next/wb_num_rounds`), `REORDER`, `SIGNAL`; Broadcasts
  `SCORE_SYNC`/`REFRESH_LIST`.
- ⛔ **Kein Ipponboard:** kein `POST /api/ippon-score`-Webhook, kein
  `POST /api/push-to-ipponboard/:id`. Ergebnisse werden **nativ über WS** erfasst
  (Score/Subscore/Status) — CCR koppelt bewusst nicht an das Ipponboard.
- ✅ **E2E verifiziert** (echter WS-Client gegen echte DB): Score→Finish→
  Propagation rückt beide HF-Sieger ins Finale; REORDER.
- ⏳ **Phase 4 (aufgeschoben, geloggt, graceful):** Pool-Standings-Finalize
  (DJB-Tiebreaker), Doppelpool-/Doppel-KO-Finalize, LB-Drop/-Advance
  (`_drop_loser_to_lb`/`_advance_lb_winner`), Repechage, Eager-Tree-
  Materialisierung + Bye-Auflösung. STATUS_UPDATE broadcastet diese Phasen
  weiterhin (nur ohne Topologie-Folgeschritte).
- ⏳ Auto-Order (`fightOrder.js` chunked-round-robin) → `ccr-domain`.
- **Exit (offen):** ganzes Turnier live über CCR inkl. Topologie ⇒ braucht Phase 4.

### Phase 3 — Live-Frontend  — 🔧 Slice 1 läuft (2026-06-18)
- **Entscheidung:** Leptos **CSR** (nicht SSR/hydrate) — die WASM-App lädt einmal,
  lebt dann auf dem WS; ccr-server liefert nur Bundle + API/WS. Build via **Trunk**
  (`wasm32-unknown-unknown`); Host-`cargo build`/`test` überspringt das WASM-Crate
  (`default-members`).
- ✅ **Slice 1: Live-Mattenliste** (`crates/ccr-frontend`, Leptos CSR): lädt
  `/api/matches`, hält live über `/ws` (SCORE_SYNC patcht einen Kampf, sonst
  Reload), sortiert nach Matte+Kampfnr, rendert Tabelle. `gloo-net` (fetch+WS),
  `futures::StreamExt`. ccr-server liefert `dist/` via `tower-http ServeDir` +
  SPA-Fallback (übersprungen wenn dist fehlt = API-only).
- ✅ **E2E bis zur Browser-Grenze verifiziert:** Trunk baut WASM (252KB) + JS,
  Server liefert index.html + Assets (200), API koexistiert. **Render selbst
  nur im Browser prüfbar** (`http://localhost:5001`, Server + Daten laufend).
- ✅ **Slice 2: Score-Eingabe** — pro Kampf P1±/P2±/„Ende"-Buttons senden WS
  (`SCORE_UPDATE`/`STATUS_UPDATE`) über einen mpsc→WS-Sink-Kanal; der Server-
  Broadcast aktualisiert alle Clients. WASM 290KB, serviert 200. (Klick→WS-Loop
  selbst nur im Browser prüfbar; WS-Protokoll server-seitig bereits bewiesen.)
- ✅ **Slice 3: Queue-able-Filter** — Liste zeigt nur Kämpfe mit beiden
  Teilnehmern ODER finished/bye (CLAUDE.md); reine Eager-Tree-TBD-Phantome
  ausgeblendet (Beispiel: 485→432 sichtbar, 53 Phantome versteckt).
- ✅ **Slice 4: Matten-Gruppierung** — Liste in Sektionen pro `tableId` (Matte N).
- ✅ **Slice 5: Bracket-Baum-Ansicht** — Modus-Umschalter (Mattenliste/Baum),
  Kategorie-Selektor (alle Brackets), gewähltes Bracket nach (Phase, Runde) als
  Spalten gerendert (WB/LB/rep/pool), TBD-Knoten sichtbar (hier gehören die
  Eager-Tree-Phantome hin), Score-Eingabe inline in beiden Ansichten. WASM 338KB.
- ⏳ Noch offen Slice 6+: echtes Baum-Diagramm mit Verbinderlinien (SVG),
  Doppelpool-Spezialansicht, Auto-Order-Button (`fightOrder.js`-Interleave →
  WS REORDER), Admin-Ansicht (Phase 5).
- **Exit (offen):** Kampfrichter/Anzeige läuft komplett über CCR.

### Phase 4 — Logik-Kern + Excel (das Risiko)  — 🔧 begonnen (2026-06-18)
- ✅ **Pool-Standings + DJB-Tiebreaker** (`ccr-domain::standings::pool_standings`,
  rein) — 3 Orakel-Tests 1:1 aus `JudgeFrontend/tests/test_pool_tiebreaker.py`
  portiert (H2H schlägt Punkte / 3er-Ringschluss→gp.id / transitiv), grün.
  Verdrahtet: `ccr-db::brackets::finalize_pool_if_complete` (Best-of-three-
  Frühabschluss, Zwei-Bronze bei >3, Persistenz) + `live.rs` Pool-Hook →
  `BRACKET_COMPLETED`. **E2E live verifiziert** gegen geseedeten 4er-Pool:
  doppelter Tiebreak [6,5,7,8] korrekt persistiert + gebroadcastet.
- ⏳ Best-of-three-Pfad portiert, aber noch nicht live getestet (keine 2er-Pool-
  Testdaten). Doppelpool-KO-Stage-Erzeugung noch offen.
- ✅ **Modifizierter Doppel-KO-Verliererbaum (8er/16er)** —
  `ccr-domain::doppel_ko` (frozen `_LB_STRUCTURE[3]/[4]`, Judo-Cross-Kanten), 3
  Orakel-Tests gegen die dokumentierten CLAUDE.md-Kanten (= `ko_form_decoder`-
  Spiegel), grün. Live-Engine in `ccr-db::doppel_ko` (`drop_loser_to_lb`,
  `advance_lb_winner`, `finalize`, find-or-create, `loser_id`) + `live.rs`-
  Dispatch (WB-Propagation **und** Loser-Drop; LB-Advance; 2-Bronze-Finalize).
  **E2E live verifiziert** an geseedetem 8er (Bracket 4) UND 16er (Bracket 5):
  je voller Durchlauf, alle Cross-Kanten per LB-Strukturabfrage bestätigt
  (8er-Cross `1−p`, 16er-Cross `p⊕2` + merge), Endplatzierungen handberechnet
  bestätigt. 32er bleibt bewusst ungewired (graceful None).
- ✅ **Doppelpool (`bracket_type='double'`)** — `ccr-db::doppel_pool`
  (`init_ko_stage_if_pools_done` + `finalize`) + `live.rs`-Dispatch. Bei
  Pool-Abschluss 3 WB-Fights eager (HF1=A1×B2, HF2=A2×B1 Crossover, Finale);
  HF→Finale über die normale Binärbaum-Propagation; Finale-Sieg → 1./2. + zwei
  HF-Verlierer als Bronze (kein Bronze-Match). Per-Pool-Standings via
  `ccr_domain::standings` (pool_index 0/1). **E2E live verifiziert** (Bracket 3):
  Crossover + → `{first:9, second:10, third_1:14, third_2:13}`, handberechnet
  bestätigt; Finalize idempotent (genau ein BRACKET_COMPLETED).
- ✅ **32er Doppel-KO (graph-getrieben)** — `ccr-domain::ko32` (frozen
  `_CANONICAL_32`-Feeder-Graph + kf↔node + consumers; abweichende Medaillen-
  Topologie mit Feedback-Rückkreuzung). Orakel-Test = Replay „kleinster Los
  gewinnt" gegen `ko_form_decoder`-Werte aus `ko_32.xls` (Plätze 1,2,4,3 + alle
  Schlüsselkämpfe inkl. kf57/59/61), grün. Live-Engine `ccr-db::doppel_ko`
  (`apply_graph_result_32` schiebt Sieger UND Verlierer pro Kante,
  `finalize_32`: 1./2. aus Finale lb r7, 2 Bronze aus Medaillen-Round-Verlierern
  lb r6; 5. Plätze NICHT persistiert) + `live.rs` (eigener `num_rounds==5`-Zweig
  statt `_LB_STRUCTURE`-Pfad). **E2E live verifiziert** (Bracket 6, 61 Kämpfe) →
  `{41,42,44,43}`, Medaillen-Round-Tabelle deckt sich mit dem Sheet-Orakel.
- ⚠️ **64er Doppel-KO — EXTRAPOLIERT, NICHT zertifiziert** (`ccr-domain::ko_big`).
  Es gibt KEINEN offiziellen 64er-Doppel-KO-Bogen (64 = Repechage) ⇒ kein Orakel.
  Auf Merlins ausdrückliche Wahl „ohne Gewähr" (2026-06-18) gebaut: ein Generator
  schreibt das 32er-Muster gleichförmig fort (WB→SF binär, LB-Leiter Cross
  (XOR nf/2)/Merge alternierend, Feedback-Medaillen-Round). `ko_big` ist Facade:
  num_rounds==5 → delegiert an zertifiziertes `ko32`; ==6 → Generator.
  Live-Engine generalisiert (`apply_graph_result`/`finalize_graph` nehmen
  num_rounds; `is_supported`=5|6). **Nur Intern-Konsistenz geprüft** (Replay:
  125 Kämpfe, jeder auflösbar, 4 verschiedene Medaillen) — KEIN Sheet-Abgleich.
  E2E live (Bracket 7) → {73,74,106,105}, 125 Kämpfe. Medaillen korrekt nur nach
  dieser Konstruktionsregel. Rollback = `ko_big` + num_rounds==6-Zweig löschen.
- ✅ **Repechage (8/16/32/64, „KO mit doppelter Trostrunde")** — `ccr-domain::
  repechage` (Generator: Main-Draw = Binärbaum, 4 Quadranten=Pools; rep-Phase =
  Treppe→Merge→Cross-Bronze; dynamische `plost`-Slots nach Niederlagen-Tiefe).
  32/64 zertifiziert (Generator reproduziert frozen `_REPECHAGE_STRUCTURE[5]/[6]`,
  in Domain-Tests geprüft); 8/16 extrapoliert (gleiche Konstruktion; 8er
  strukturell entartet, sauber behandelt). Live-Engine `ccr-db::repechage`
  (`apply_graph_result`, `trace_pool_victims`, `fill_plost_slots`, `finalize` mit
  Bronze=SIEGER) + `live.rs`-Zweig. **E2E live alle 4 Größen** verifiziert →
  Platzierungs-Offsets `[0, N/2, 1, N/4]` handberechnet bestätigt (43/79/23/11
  Kämpfe). NICHT portiert (defer, graceful bei vollen Pools): `_resolve_repe_byes`
  (Bye-Kaskade) + Eager-Tree.
- ✅ **Eager-Tree-Materialisierung + WB-Freilos-Auflösung** (`ccr-db::reconcile`,
  in `/api/matches` wie JFs `get_matches`-Prelude, idempotent via
  `ON CONFLICT DO NOTHING`). Eager: voller KO-Baum (8/16 Formel + `_LB_ROUND_SIZES`;
  32/64 `ko_big::all_nodes`) + Repechage-Baum (`repechage::all_nodes`). Byes:
  WB-Freilose (`status='bye'`, p1==p2) → `propagate_wb_winner`. **E2E verifiziert**:
  16er DKO 8→27, 32er rep 16→43 (idempotent); 6-TN-Bye-Bracket → Freilos-Sieger
  rücken korrekt in WB r1 vor.
  - ⏳ Defer: LB/repe-Bye-KASKADEN (WB-Bye macht LB-Slot tot — `_resolve_lb_byes`/
    `_resolve_repe_byes`) + `_compute_slot_sources` (p1From/p2From-Labels).
- ✅ **Excel-Datenschicht: `contestants_*.{csv,json}`** (`ccr-excel::contestants`,
  faithful Port von edv `contestants_csv.py`): CSV-Schreiben kanonisch (UTF-8 BOM,
  `;`, CRLF, `.`-Dezimal, lowercase bools, Gender verbatim, Doublestart→standard),
  CSV-Lesen tolerant (`,`/`.`-Gewicht, ja/nein/true/false, legacy `mode`-Spalte),
  JSON via serde (gleiche Schema). 3 Unit-Tests + **gegen echte edv-Daten
  verifiziert** (751 TN, JSON↔CSV verlustfrei). Lesen von `.xls` via calamine in
  Phase 0.5 bewiesen.
- 📦 **Bewusst nach Phase 5 verschoben** (Output/Input-Features, an exakte
  Druck-Layouts + Admin-Workflow gekoppelt, KEIN Live-Pfad-Bedarf): DJB-Formular-
  GENERIERUNG (`excel_form_filler`), Urkunden (`urkunden_export`), Seeding-Reimport
  (`excel_seeding_reimport`). Gehören zur Admin-UI, nicht zum Backend-Kern.
- ✅ **Phase-4-Logik+Daten-Kern KOMPLETT.**

Reihenfolge nach steigendem Risiko (Rest):
1. `ccr-domain::pools` — `_generate_fight_schedule`, Doppelpool-Nummerierung.
   **Orakel:** edv erzeugte Reihenfolgen 1:1 nachrechnen.
2. `ccr-domain::standings` — DJB-Tiebreaker (Direktvergleich, Zyklus → `gp.id`).
3. `ccr-domain::ko` — balancierte KO-Seedung (`bracket_utils`).
4. `ccr-domain::doppel_ko` — `_LB_STRUCTURE` (8/16) + `_KO32_CONSUMERS`, Bye-Kaskade.
5. `ccr-domain::repechage` — `_REPECHAGE_STRUCTURE` (32/64), `plost`-Fill.
6. `ccr-excel` — **zuletzt.** Erst `contestants_*.csv` (einfach), dann DJB-Formulare,
   dann Urkunden, dann Seeding-Reimport.
- **Pflicht:** die maschinengeprüften Tests aus `edv/tests/` (`ko_form_decoder`,
  `repechage_form_decoder`, `test_ko_form_order`) als Rust-Tests mitportieren.
  Ohne dieses Netz kein Merge in `ccr-domain`.
- **Excel-Fallback:** falls calamine die BIFF-Orakelblätter nicht bitgenau liest,
  dünner Python-Sidecar (FastAPI-Microservice nur für Excel), bis Parität steht.
  Das blockiert NICHT den Rest der Migration.
- **Exit:** CCR generiert Brackets + Excel; edv-Backend-Services überflüssig.

### Phase 5 — Admin-Frontend + edv-Ablösung  — 🔧 Slice 1 (2026-06-18)
- ✅ **Slice 1: Admin-Read-Views.** Backend `ccr-server::admin` (read-only):
  `GET /api/participants` (`participants::all`) + `GET /api/brackets`
  (`brackets::all_summaries`, Platzierungs-gp-ids → Namen aufgelöst). Frontend
  3. Modus „Admin" (Leptos): Ergebnis-Übersicht (alle Brackets + Medaillen-Namen)
  + Teilnehmer-Tabelle. **E2E verifiziert**: 984 TN, 14 Brackets, Medaillen über
  ALLE Formate korrekt (Pools/Doppelpool/Doppel-KO 8/16/32/64/Repechage 8/16/32/64).
  Kein Schreibzugriff auf das geteilte Schema.
- ✅ **Slice 2: Contestants-Import + Editor.** Backend: `POST /api/import-contestants`
  (Body = contestants JSON-Array oder CSV; parst via `ccr-excel`, UPSERT in
  `participants` über Natural-Key `ON CONFLICT ON CONSTRAINT uix_participant_identity`
  → updated weight/valid/paid/doublestart/association; edv-Mapping faithful:
  `_normalize_gender` male→m, Birthyear→`date(y,1,1)`, doublestart höher/ja/nein) +
  `PUT /api/participants/:id` (Wiegen-Felder). Frontend: Datei-Upload (`gloo-file`)
  → Import, Such-Filter, sticky Edit-Panel (Gewicht/gültig/bezahlt/Doppelstart →
  PUT → reload). **E2E curl-verifiziert**: echte `contestants_male.json` → 374
  importiert (344 neu/30 upd); **Re-Import idempotent** (0 neu/374 upd); PUT setzt
  Gewicht 42.50/valid/höher (DB bestätigt).
- 🔧 **Slice 3: Bracket-GENERIERUNG (Pools).** Reine Logik `ccr-domain::pools`:
  `recommend_bracket_type(n)` (CLAUDE.md-Schwellen <3 special/3-5 pools/6-10
  double/11-32 ko/33-64 repechage) + `pool_fight_schedule(n)` (= edv
  `_generate_fight_schedule`, 2er best-of-3, 3/4/5er kanonisch, ≥6 Circle) —
  3 Unit-Tests gegen die dokumentierten edv-Werte. Endpunkt `POST /api/brackets/
  :id/generate` (liest group_participants → Typ nach Anzahl → Pool: legt
  Schedule-Fights an, setzt `bracket_type`; Re-Gen-Schutz 409; KO/double/repechage
  = 200 mit „noch nicht"-Note). Frontend: „⚙ Generieren"-Button je Bracket.
  **E2E verifiziert**: 4er → 6 Pool-Fights in exakter Schedule-Reihenfolge.
- ✅ **Slice 4: KO + Repechage-Generierung.** `ccr-domain::ko`: `next_pow2`,
  `seed_order` (edv `_generate_seed_order`-Rekursion) + `generate_round0`
  (`_compute_balanced_bracket`: Round-Robin nach Verein, Byes auf next_pow2,
  Snake-Seed in Slots, Paare). Endpunkt erweitert: ko/repechage → WB-Runde-0
  anlegen (echte Fights pending; Byes p1==p2 status='bye' winner → CCRs
  `_resolve_pending_byes` rückt vor). **E2E verifiziert**: 12→ko (4 echte/4 Byes →
  Eager-Tree 27), 40→repechage (8/24 → 79). (Hinweis: edvs Seeding trennt
  Clubmates NUR best-effort — perfekte R0-Trennung ist KEIN garantiertes edv-
  Verhalten; Docstring-Beispielzahlen sind stale, Code-Rekursion ist maßgeblich.)
- ✅ **Slice 5: Config-getriebene Gruppen-Zuordnung.** `ccr-excel::config`
  (`BracketConfig::load` liest `bracket_config.xlsx` via calamine: AgeEligibility
  → Geburtsjahr→Altersklasse, WeightClasses → (gender,age,gewicht)→Label, Options
  → event_year; `age_group`/`weight_class` mit mathematischem Alters-Fallback) —
  Test gegen die echte xlsx. `ccr-db::groups` (find_or_create + add_member,
  idempotent) + `participants::for_assignment` (Gewicht→f64, Jahr via SQL).
  Endpunkt `POST /api/assign-groups` (klassifiziert alle TN → `gender | age |
  class`-Gruppe, U9/U11 = no-class/gepoolt) + Frontend-Button. **E2E**: 1374
  zugeordnet, 10 skipped, 15 Gruppen, idempotent (2. Lauf 0). **Voller
  Vorbereitungs-Pipeline standalone: Import → Zuordnung → Generierung → live.**
- ✅ **Slice 6: Generierung 'double' + 'special' → ALLE Typen abgedeckt.**
  double = 2 Pools (Round-Robin/Verein + Index-Split) + Pool-Fights, Nummern
  2-by-2 interleaved (`pools::double_pool_fight_numbers`); KO-Stage entsteht live
  bei Pool-Abschluss. special: solo→1.Platz (`brackets::complete_solo`),
  2er→best-of-three-Pool (Typ 'pools'). **E2E**: 8→double (12 Fights, interleaved),
  2→best-of-3 (3), 1→solo (completed). **Bracket-Erzeugung vollständig:
  special/pools/double/ko/repechage.**
- ✅ **Slice 7: Alters-Sperren (`AgeClassLock`).** `ccr-db::locks` (Modell +
  `scope_key` = `"{gender}|{age}"`/`"{age}"`, `is_class_locked`, all/set/remove,
  `locked_keys`). Endpunkte `GET/POST /api/locks` + `DELETE /api/locks/:key`.
  Gating (NUR Admin, nie Live): `update_participant`→423 wenn ALLE Klassen des TN
  gesperrt (Granularität), `generate_bracket`→423 bei gesperrter Bracket-Klasse,
  `assign_groups` skippt gesperrte (`lockedSkipped`). Frontend `<LocksPanel/>`
  (Liste + sperren/entsperren). **E2E**: lock m|U15 → gen 423, edit 423,
  assign skip 141; unlock → 200/200.
- ✅ **Slice 8: Schema-Ownership.** `migrations/0001_baseline.sql` (alle 10
  Tabellen, `CREATE TABLE IF NOT EXISTS` + Constraints inline, benannte
  `uix_participant_identity`/`uix_age_class_lock_scope`/`uix_fight_position`).
  `ccr_db::run_migrations` (`sqlx::migrate!`) läuft beim Server-Start; sqlx-Feature
  `migrate`. **Verifiziert beidseitig**: bestehende DB → Migration No-Op, 1399 TN
  intakt, `_sqlx_migrations`=baseline; **frische DB `ccr_fresh` → CCR erzeugt 10
  Tabellen selbst, Import funktioniert** = CCR besitzt das Schema, edv-Alembic
  kann einfrieren.
- ✅ **Slice 9: Excel-Output.** `ccr-excel::export` (rust_xlsxwriter):
  `results_xlsx` (Kategorie/Typ/1./2./3./3.) + `urkunden_xlsx` (eine Zeile je
  Platzierung: vorname/nachname/platz/klasse/verein, = edv-Format). Endpunkte
  `GET /api/export/{results,urkunden}.xlsx` (Download), Frontend-Links. 2 Tests
  (generieren→via calamine zurücklesen). **E2E**: results 22 Zeilen, urkunden 42,
  gültige .xlsx mit echten Namen. ⚠ Generiert CCRs EIGENES Layout — NICHT die
  pixelgenaue DJB-`.xls`-Vorlage (kein pure-Rust-BIFF-Writer; Template-Fill
  bräuchte den abgelehnten Sidecar).
- ✅ **Slice 10: Doppelstarts.** `config::eligible_age_groups(by)` (alle X im
  Config; Überlappungsjahre 2014→U13/U15, 2012→U15/U18, 2009→U18/18+);
  `assign_groups` expandiert nach doublestart-Flag: `nein`→primär, `höher`→primär
  +nächsthöher, `ja`→alle berechtigten; per-Ziel-Lock-Granularität. **E2E**:
  ja/höher → 2 Gruppen (U13+U15, je eigene Gewichtsklasse), nein → 1.
- ✅ **Slice 11: Voller Kämpfer-Editor + Anlegen + Wiegekarten.** `PUT
  /api/participants/:id` = voller Replace (alle Felder, `parse_edit`+`update_full`);
  `POST /api/participants` (anlegen, `create`); `GET /api/export/wiegekarten.xlsx`
  (eine Zeile/TN, leere Gewicht/Unterschrift-Spalten, Altersklasse via Config).
  Frontend: Edit-Panel auf alle Felder erweitert, „+ Neuer Kämpfer", Wiegekarten-
  Download. **E2E**: create id 1905, voller PUT ändert alle Felder, wiegekarten
  1404 Zeilen.
- ✅ **Slice 12: Urkunden-CSV (Affinity Data Merge).** `export::urkunden_csv`
  (comma, UTF-8, Header = Merge-Feldnamen vorname/nachname/platz/klasse/verein),
  `GET /api/export/urkunden.csv` (text/csv), Frontend-Link. Eine Zeile pro
  Platzierung = ein Zertifikat in Affinity Publishers Datenzusammenführung.
- ✅ **Slice 13: PDF-Druckausgabe (printpdf, pure Rust).** `ccr-excel::pdf`:
  generisches gegittertes `table_pdf` (A4 quer, paginiert) → `wiegekarten_pdf`
  (leere Gewicht/Unterschrift-Zellen = handausfüllbar) + `results_pdf`. Endpunkte
  `GET /api/export/{wiegekarten,results}.pdf` (application/pdf), Frontend-Links.
  **E2E**: gültige PDF 1.3, wiegekarten 1404 TN über ~80 Seiten.
- ✅ **Slice 14: Listen-Erstellung + Teilnehmer löschen.** `POST /api/brackets/
  create-all` (je Gruppe-mit-TN-ohne-Bracket: Bracket anlegen + generieren via
  extrahiertem `generate_fights_for`; Locks skippen). `DELETE /api/participants/:id`
  (gated: in Fight/Platzierung→409 via `is_referenced`, gesperrte Klasse→423;
  sonst Memberships+TN löschen). Frontend: „Listen erstellen"-Button, 🗑 je Zeile
  (mit confirm). **E2E**: create-all → 18 Brackets, delete 200, referenziert → 409.
- ✅ **Slice 15: Cmd+S/Strg+S = Speichern.** Globaler `window_event_listener`
  (keydown): Meta/Ctrl+S → speichert offenes Kämpfer-Edit-Panel, `preventDefault`.
  Save-Logik auf App-Ebene (`save_edit`, von Button + Kürzel genutzt).
- ✅ **Slice 16: Editierbare Klassen-Config (DB-backed).** Migration `0002_app_config`
  (JSONB-Einzelzeile). `ccr-db::app_config::AppConfig` (event_year, age_classes,
  youth_classes, youth_pool_size, adult_methods=Schwellen, birth_years=Jahrgang→Klassen
  inkl. Doppelstart-Überlappung, weight_classes) + Logik `recommend/age_group/
  eligible_age_groups/weight_class`. Seed aus xlsx+Defaults (Merlin: U15+ ≥5 Pool /
  ≥8 Double / größer KO; U9/U11 4er-Pool). `get_config` (load-or-seed) treibt jetzt
  `generate`/`assign-groups`/`wiegekarten`. `GET/PUT /api/config` + Frontend
  `ConfigPanel` (Schwellen, Jugend-Pool, Jahrgang-Tabelle). **E2E**: seed ok, PUT
  round-trip, 6er-U15→pools. ⚠ OFFEN: Jugend-„4er-Pools" = Mehrfach-Pool-Split (>2
  Pools/Bracket) ist NICHT gebaut — Jugend erzeugt aktuell EINEN Pool; braucht
  edv-Jugendformat als Orakel (Split + Finalize-Semantik).
- ✅ **Slice 17: Jugend-Gewichts-Pool-Split + Gewichtsklassen-UI.** `ccr-domain::
  pools::split_into_pools(weights_sorted, pool_size, max_spread)` (2-stufig wie edv
  `split_u9_u11_into_pools`: Spread-Cluster greedy vom leichtesten + `ceil(len/
  pool_size)`-Gleichverteilung). Config-Feld `youth_max_weight_spread` (kg, 0=aus).
  `assign-groups`: Jugend (U9/U11) → EINE gemischte Gruppe je Alter (kein Gender/
  Gewicht). `create-all`: Jugend-Quellgruppe → split in „{Alter} | Pool k"-
  Gruppen + Bracket je Pool (Round-Robin; 1er→Solo); idempotent (Pool-1-Existenz).
  ConfigPanel: Spread-Eingabe + Gewichtsklassen-Tabelle (G/Klasse/≤kg/Label,
  add/remove). **E2E**: edv-Beispiel 18/20/21/23/28/30/31 @ size4/spread5 →
  Pool1[18-23]/Pool2[28-31], 6+3 Kämpfe.
- ✅ **Slice 18: Einzel-Bracket neu generieren.** `POST /api/brackets/:id/generate`
  ist jetzt regenerier-sicher: bei vorhandenen Kämpfen werden sie verworfen
  (`fights::delete_for_bracket` + `brackets::reset`) und neu gebaut — ABER 409
  sobald ein Ergebnis existiert (`fights::any_finished_for_bracket`, status=
  'finished'); Locks weiterhin 423. Antwort enthält `regenerated`. Frontend-Button
  „⚙ Generieren / Neu". **E2E**: 6er→pools(15), neu→regenerated:true, mit
  Ergebnis→409. (Gilt je Bracket = je Gewichtsklasse / Jugend-Pool.)
- ✅ **Slice 19: Docker-Compose (DB + Web).** `Dockerfile` (multi-stage: rust:1-
  bookworm baut Trunk-WASM-Frontend + axum-Server → debian-slim Runtime, serviert
  beides auf :5001), `docker-compose.yaml` (`db` postgres:15 mit healthcheck +
  `web` build . , depends_on db healthy, DATABASE_URL→db:5432, :5001), `.dockerignore`,
  `config/bracket_config.xlsx` ins Repo gebündelt (Image self-contained, CCR_BRACKET_CONFIG).
  Eigene CCR-DB (Volume `ccr_postgres_data`), Schema via Migration beim Start.
- ✅ **Slice 20: Wiegekarten — Affinity-CSV + HTML-Druckansicht (2026-06-25).**
  `export::wiegekarten_csv` (comma, UTF-8, Header = Merge-Feldnamen
  `id,nachname,vorname,verein,geschlecht,jahrgang,altersklasse`, eine Zeile/TN,
  OHNE Gewicht — die Kartenvorlage trägt die Blankozeile; `id` = Merge-Schlüssel /
  QR-Wert), `GET /api/export/wiegekarten.csv` (text/csv) — Spiegel von
  `urkunden_csv`, für Affinity Publisher / LibreOffice-Datenzusammenführung.
  `ccr-excel::print_html::wiegekarten_html` + `GET /print/wiegekarten` (text/html):
  selbsttragende A4-Seite, CSS-Paged-Media-2er-Raster (~10 Karten/Seite,
  `break-inside: avoid`), je Karte Name/Verein/`m · 2014 · U13` + Blankozeilen
  Gewicht/Unterschrift + id; Bildschirm-Toolbar mit „Drucken / Als PDF
  speichern"-Button (`window.print()`, im Druck ausgeblendet); Daten HTML-escaped
  ⇒ In-App-Druck ohne externes Werkzeug. Beide aus `wiegekarte_rows` = ALLE
  Teilnehmer, Altersklasse aus Jahrgang via Config, **gewichtsunabhängig** ⇒
  Import OHNE Gewicht → Karten drucken VOR dem Wiegen. Frontend: Links
  „Wiegekarten (.csv → Affinity)" + „Wiegekarten drucken (HTML)"; xlsx/pdf-Listen
  umbenannt zu „Wiegekarten-Liste". 3 neue ccr-excel-Tests (CSV-Merge-Format,
  HTML-Karte/Escaping, leere Meta). **E2E** (edv-DB, 1420 TN): CSV 1420 Zeilen +
  UTF-8 (`Eßwein`); HTML 1420 Karten, Paged-CSS + Druck-Button, keine
  Platzhalter-Reste; Import-ohne-Gewicht → `weight=NULL` → Karte mit U13 (aus
  Jahrgang) → wieder gelöscht. ⚠ QR-Bild selbst noch nicht gerendert (`id` liegt
  im CSV; Affinity-seitig oder eine Crate server-seitig = offener Einzeiler).
- ✅ **Phase 4 + 5 funktional KOMPLETT** (außer pixelgenaue DJB-Druck-Vorlagen).
- edv-Tkinter-Screens (~15k LOC) → Leptos-Web-Admin: größter UI-Brocken.
- **Exit:** edv + JF abgeschaltet; CCR ist die Suite-Hälfte.

## 4. Was sich NICHT ändert (Verträge wahren)

- **Ipponboard** bleibt unverändert und wird von CCR **nicht** angesprochen: CCR
  kennt weder `POST /fighters` noch `/api/ippon-score`. Ergebnis-Erfassung läuft
  nativ über WS (Score/Subscore/Status) — siehe Phase 2.
- **WeighIn** schreibt weiter `contestants_*.json/.csv` (Schema unverändert);
  CCR übernimmt edv's Re-Import-Seite.
- **Postgres-Schema** während Phase 1–4 unverändert; CCR passt sich an, nicht umgekehrt.

## 5. Risiken (ranggeordnet)

1. **`.xls`-BIFF-Lesen in Rust** — calamine vs. xlrd-1.2-Parität ungewiss, und
   100 % Rust ist gesetzt (kein Sidecar-Netz). → **Phase 0.5 Go/No-Go zuerst.**
2. **Bracket-Topologien** — falsch portiert = falsche Medaillen. → Orakel-Tests Pflicht.
3. **Tkinter→Leptos** (Phase 5) — größter UI-Aufwand, oft unterschätzt; Leptos-
   Ökosystem kleiner als JS → Komponenten ggf. selbst bauen.
4. **LAN-Host bricht USB-Modell** — edv lief offline pro Stick; CCR braucht einen
   erreichbaren Hallen-Host. Ausfall des Hosts = ganze Halle steht. → Host-Redundanz
   / lokales Backup einplanen (Betriebsthema, nicht Code).
5. **Doppel-Schreiber auf eine DB** (Phase 2 Übergang) — JF und CCR dürfen nicht
   gleichzeitig denselben Live-Pfad schreiben. → harter Cutover pro Matte/Turnier.

## 6. Offene Entscheidungen — geklärt (2026-06-18)

Alle vier architektur-forkenden Fragen sind entschieden (s. „Getroffene
Entscheidungen" oben): beide ablösen · Leptos · LAN-Host · 100 % Rust.

Verbleibende kleinere Klärungen, die NICHT die Architektur forken:
- cargo-leptos: SSR vs. CSR/hydrate (Detail zu Phasenbeginn 3).
- Repechage-Bronze-Cross auf dem physischen Blatt verifizieren (CLAUDE.md: „Merlin
  to confirm") — fließt in `ccr-domain::repechage` ein.

> **Nächster Schritt:** Phase 0.5 (Excel-Spike) — die Go/No-Go-Vorbedingung für
> die 100-%-Rust-Excel-Entscheidung. Danach Phase 1.
