# CCR — Handlungsplan (was nicht passt / was fehlt)

Stand: Review gegen **PLAN.md** (Absichten/Phasen) und **WSP/CLAUDE.md** (Suite-Invarianten);
Orakel = **JudgeFrontend (`main.py`, `app.js`)** und **edv** (`models.py`, `pool_renderer`, `data_transformation_pipeline`).

**Gesamturteil:** Kernlogik (Pool-Spielplan, 8er/16er Doppel-KO-Topologie, DJB-Tiebreaker,
KO32-Graph, CSV/JSON-Format, Locks/Doublestart) ist solide und die 40 Tests (domain 29, excel 11) sind grün.
Die Lücken liegen **außerhalb des reinen Live-Kerns**: der Live-/DB-Pfad hat **keine
automatisierten Tests**, es gibt **eine Schema-Entscheidung offen** (geteilte edv-DB) und mehrere
**veraltete „Phase-4"-Labels** auf bereits implementierten Pfade. Ipponboard-Integration ist
**bewusst entfernt** (Entscheidung — siehe A1).

Priorität: 🔴 kritisch · 🟠 hoch · 🟡 mittel · 🔵 gering.

---

## A. Funktionslücken (Parität zu JF/edv-Orakel)

### A1 · ⛔ Ipponboard-Integration — bewusst entfernt (Entscheidung, 2026-10-01)
**Entscheidung:** CCR koppelt **nicht** an das Ipponboard. Ergebnis-Erfassung läuft
nativ über WS (`SCORE_UPDATE` / `SUBSCORE_UPDATE` / `STATUS_UPDATE`). Die dafür
vorgesehenen Komponenten wurden entfernt:
- `crates/ccr-server/src/live.rs` — `push_to_ipponboard`, `ippon_score` (Webhook), `parse_subscores`.
- `crates/ccr-server/src/main.rs` — `last_pushed`-Pointer, `CCR_MATS`-Flag, `/api/features`, Routen `/api/ippon-score` + `/api/push-to-ipponboard/:id`.
- `crates/ccr-db/src/fights.rs` — `set_subscores` (nur vom Webhook genutzt) + totes `apply_winner`.
- `crates/ccr-frontend/src/main.rs` — `mats`-Signal + `/api/features`-Boot; **Mattenliste jetzt immer sichtbar** (natives Scoring, kein Ipponboard).
- Doku: `docker-compose.yaml`, `DOCKER.md`, `PLAN.md`, `migrations/0003`-Kommentar.

**Geblieben (nativ, kein Ipponboard):** `add_subscore`, `set_result`, `jvp_finish_broadcast`,
JVP-Modul (`ccr-domain/jvp.rs`), Subscore-Spalten in `fights`, Mattenliste + Baum-Scoring.
**Status:** `cargo check` (native + wasm) grün; 40 Tests (domain 29, excel 11) grün.

### A3 · 🟠 „Ende"/Ergebnis-Propagation nur für Pools, nicht für KO/Repechage/Doppel
**Problem:** `jvp_finish_broadcast` finalisiert **nur** `bracket_phase == "pool"`.
- `live.rs:279-298` — `if fight.bracket_phase == "pool"` (Zeile 285) → `finalize_pool_if_complete`.
- `live.rs:278` — „KO propagation via the webhook stays **deferred**".
- `live.rs:209` — Log „repechage **deferred** to Phase 4" (Tatsache: Repechage-Byes **sind** implementiert, A4/D).

**Folge:** Ein abgeschlossener KO-/Repechage-/Doppel-Kampf avanciert **nicht** automatisch in die nächste Runde
(nächster Kampf, Bronze). In JF tut das „Ende". Das ist die zentrale Live-Lücke.

**Fix:** KO/Repechage-Propagation bauen: Gewinner → nachfolgende Instanz (`fights`-Feed), dann ggf. `BRACKET_COMPLETED`
für Bronze. `ccr-db/src/fights.rs:245` („LB/repechage/double topology … Phase 4") und `brackets.rs:223,236` auflösen.
**Verifikation:** E2E 8er/16er DKO: Finale → Bronzerunde wird besetzt; 32er KO: WB-Finale → LB-Slots füllen.

### A4 · 🟠 Bye-Kaskade deckt nur `bracket_type = 'ko'` ab — nicht Repechage
**Problem:** `resolve_lb_byes` iteriert **nur** `ko`-Brackets.
- `crates/ccr-db/src/reconcile.rs:239-241` — `for bid in bracket_ids_of_type(pool, "ko")`.
- phases werden korrekt geparst (`wb`/`lb`/`rep`, Zeile 258-262), aber der Bracket-Typ-Filter schließt `repechage` aus.

**Folge:** Repechage-Listen **mit** Freilosen können Bronze nie abschließen (Trost-Runde hängt offen).

**Fix:** Typ-Filter auf `{"ko","repechage"}` (bzw. alle LB-führenden Typen) erweitern; `ko_feeders`/`feeder_dead`
für Rep-Struktur prüfen.
**Verifikation:** Rep-Bracket mit WB-Freilos → Trostrunde-Bye-Cascade läuft, Bronze wird gesetzt.

### A5 · 🟡 Automatische Kampf-Reihenfolge (Mattenliste) fehlt im UI
**Problem:** Server kann REORDER, aber es gibt **keine UI** und **keine** Auto-Sequenz.
- `live.rs:247-255` — WS-Event `REORDER` → `fights::reorder` (funktioniert).
- Kein „Auto-Reihenfolge"-Button, kein Drag&Drop der Mattenliste im Frontend.
- Fehlend: JF `fightOrder.js` (chunk=2, Round-Robin der Mattenbelegung).

**Fix:** Mattenliste-Panel mit Auto-Sequenz + Drag&Drop; REORDER-Event feuern.
**Verifikation:** „Auto-Reihenfolge" → `fight_number`-Spalte belegt; Drag&Drop → persistiert via `fights::reorder`.

### A6 · 🟡 Kampf-Protokoll fehlt; `duration` nie gesetzt
**Problem:** Kein Pendant zu JF `logs/fights.jsonl`; der Live-Pfad (WS) loggt keine abgeschlossenen Kämpfe.
- `fights.duration` wird nie geschrieben (weder über WS noch sonst wo).

**Fix:** Kampflog (JSONL oder Tabelle) pro abgeschlossenem Kampf (Score/Subscore/Status → Finish); `finished_at` setzen.
**Verifikation:** abgeschlossener Kampf erzeugt Log-Zeile mit Dauer; CSV-Export (1 Zeile/Kampf) bleibt intakt.

---

## B. Korrektheits- & Drift-Risiken

### B1 · ✅ Frontend hardcodet Jugend = U9/U11, Server nutzt konfigurierbare `youth_classes` — ** umgesetzt (2026-10-01)**
**Problem:**
- `ccr-frontend/src/api.rs:83-84` — `is_youth() => age_group == "U9" || "U11"` (hart).
- `ccr-db/src/app_config.rs:69-71` — `is_youth()` via `youth_classes` (konfigurierbar).
- `ccr-frontend/src/admin.rs:899` — UI-Feld `youth_classes` ist **editierbar**.

**Folge:** Ändert man `youth_classes` (z. B. U13 dazunehmen), weicht UI-Logik (JVP-Additiv vs. 1/0) vom Server ab.
Zwei „Wahrheiten" über Jugend-Regeln.

**Fix:** Frontend liest `youth_classes` aus `/api/config` statt hardcoden; `is_youth`-Helfer serverseitig spiegeln.
**Verifikation:** `youth_classes` auf U13 stellen → U13-Kampf zeigt JVP-Additiv in UI **und** Server.

**Status:** umgesetzt. Frontend liest jetzt `youth_classes` aus `/api/config`
(`api.rs::fetch_youth_classes`) und `Match::is_youth(&[String])` spiegelt die Server-Regel
(`AppConfig::is_youth`) 1:1. `youth`-Signal in `main.rs` wird beim Start geladen und nach
`PUT /api/config` (`ConfigPanel`) aktualisiert → Live-View reagiert auf Konfig-Änderung ohne
Reload. Default-Seed bleibt `["U9","U11"]` = identisches Verhalten wie zuvor. `cargo check
-p ccr-frontend --target wasm32-unknown-unknown` + `cargo test --workspace` (domain 29, excel 11) grün.

### B2 · ✅ Doppelte Threshold-Tabelle (eine davon tot) — ** umgesetzt (2026-10-01)**
**Problem:**
- `ccr-domain/src/pools.rs:14-26` — `recommend_bracket_type` (hart: <3 special, 3–5 pools, 6–10 double, 11–32 ko, 33–64 rep).
- Genutzt wird stattdessen `AppConfig::recommend`: `ccr-db/src/app_config.rs:50-66` (via `admin.rs:671`),
  getrieben von `adult_methods` + `youth_classes`.

**Folge:** `pools.rs::recommend_bracket_type` ist **totes Code** (nur eigene Tests, `pools.rs:139-147`);
zwei Schwellwert-Tabellen können sich unabhängig voneinander verschieben.

**Fix:** `pools.rs::recommend_bracket_type` **löschen** (inkl. Tests) **oder** als Single-Source-Truth etablieren und
`AppConfig::recommend` darauf aufbauen. Eines der beiden, nicht beide.
**Verifikation:** `cargo test --workspace` grün; nur eine Funktion entscheidet über Bracket-Typ.

**Status:** umgesetzt. `pools.rs::recommend_bracket_type` (hartkodiert) + Test `type_thresholds_match_claudemd`
gelöscht. Single-Source-Truth bleibt die konfig-getriebene `AppConfig::recommend` (`adult_methods` +
`youth_classes`, via `admin.rs::generate_fights_for`), weil sie UI-editierbar ist und die B1-Richtung
(fortlaufend konfig-getrieben) fortsetzt — die hartkodierte Tabelle wäre ein Rückbau in Hardcoding.
`cargo test --workspace` grün (domain 28, excel 11); nur noch `AppConfig::recommend` entscheidet über den Typ.

### B3 · ✅ Schema-Abweichung auf der geteilten edv-DB — **umgesetzt (2026-10-01, Option A: CCR-Ownership)**
**Problem:** `.env.example` zielt explizit auf die **gemeinsame** edv/JF-DB („Same Postgres as edv/JF during migration"),
CCR-Migrations laufen dort auf — und ändern edvs Schema:
- `migrations/0003_fight_subscores.sql` — **8 neue Spalten** auf `fights` (ippon1/wazari1/…/shido2).
- `migrations/0002_app_config.sql` — neue Tabelle `app_config`.
- `migrations/0004_clubs.sql` — neue Tabelle `clubs`.

**Kollision mit Invariante** `WSP/CLAUDE.md:29` — Subscores: „**NOT persisted to the DB** (no `fights` columns, no
edv/Alembic change)", und `:21` — „`edv` is schema owner". CCR:0003 tut **genau** das, was die Invariante verbietet.
(In der Standalone-Docker-Moode — `docker-compose.yaml:3-5`, eigene DB auf :5433 — ist das CCR-eigenes Schema und OK.)

**Entscheidung (wählen):**
1. **Akzeptieren** CCR-Ownership für die geteilte DB → `CLAUDE.md:21` **und** `:29` aktualisieren (Subscores in `fights`,
   neue `app_config`/`clubs`-Tabellen) + edv-Modelle (`models.py`) um die 8 Spalten ergänzen, damit edv sie nicht ignoriert/brechen.
2. **Rollback** in der Migrations-Moode: Subscores aus `fights` raus (nur JSONL-Log, wie JF), `app_config`/`clubs` bleiben
   CCR-privat in der Standalone-DB — dann muss `.env.example`-Migrations-Modus auf Standalone-DB umziehen.

**Verifikation:** whichever gewählt: `edv` und CCR können parallel dieselbe DB lesen; `cargo test` + edv-Smoke grün;
`CLAUDE.md` widerspricht dem tatsächlichen Schema nicht.

**Status:** umgesetzt (2026-10-01). **Option A gewählt** (Merlin): CCR wird **Schema-Owner** der geteilten `:5432`-DB; edv + JF sind Consumer (deklarieren Modelle, eigene Migrationen: keine; **edv-Alembic eingefroren**). Die 3 CCR-Migrations (`app_config` / 8 Subscore-Spalten / `clubs`) sind legale additive Erweiterungen — idempotent (`CREATE TABLE IF NOT EXISTS` / `ADD COLUMN IF NOT EXISTS`), edv bricht nicht. Umgesetzt: (1) `WSP/CLAUDE.md` — CCR in Workspace-Liste + Invarianten `:5432`/`fights`-Typen/Subscores umgeschrieben (CCR-Owner, Subscores jetzt in `fights`, Supersedes der JF-JSONL-Regel); (2) CCR-Selbstwiderspruch aufgelöst — `ccr-db/src/lib.rs`, `models.rs`, `main.rs`, `README.md`, `PLAN.md` (Leitentsch. 3 + Phase-1-Zeile): „kein Schema-Owner bis Phase 5" → „CCR-Owner seit 2026-10-01"; (3) edv `models.py::Fight` + 8 Subscore-Spalten (`default=0`, ORM-sicher, Konvention `bracket_phase`). **Phase 5 bleibt = edv-Feature-Ablösung, nicht Schema-Handover** (der ist 2026-10-01 erfolgt). **Verifiziert:** `cargo test --workspace` grün (domain 28, excel 11 — 39 gesamt) + edv-Model-Import/DDL grün; `CLAUDE.md` widerspricht dem Ist-Schema nicht.

### B4 · 🟡 Web-Port 5001 kollidiert mit JF-Backend (Strangler-Parallelbetrieb unmöglich)
**Problem:** CCR bindet Host-Port **5001** — derselbe wie JF-Backend.
- `docker-compose.yaml:39` `CCR_HTTP_ADDR=0.0.0.0:5001`, `:43` `5001:5001`.
- `.env.example` `CCR_HTTP_ADDR=0.0.0.0:5001`.
- `WSP/CLAUDE.md:10` — JF-Backend läuft auf `:5001`.

**Folge:** CCR + JF **parallel auf einem Host** (der Sinn der Strangler-Migration) → Port-Kollision.
Der DB-Port wurde explizit abgestimmt (`docker-compose.yaml:21` „avoids edv's :5432", Host :5433) — der **Web-Port nicht**.

**Fix:** CCR-Web auf einen freien Port (z. B. 5002) in `docker-compose.yaml` + `.env.example` + `DOCKER.md`;
bzw. Doku klarstellen: „CCR ersetzt JF → Port 5001 erst freigeben, wenn JF runter ist".
**Verifikation:** CCR-Container + JF-uvicorn laufen gleichzeitig; beide `/health` erreichbar.

### B5 · 🟡 Pool-Remis: CCR speichert JVP-Gesamte, Referenz speichert 0/0
**Problem:** `live.rs:361-368` — `record_draw` → `set_result(id, None, t1, t2)` mit `t1/t2` = JVP-Additiv-Gesamte.
JF/CLAUDE.md-Pool-Remis-Idiom ist `score1 == score2 == 0` (Remis als neutral, kein Punktestand).
**Aktion:** gegen JF `_compute_pool_standings` verifizieren, ob gleiche Nicht-Null-Scores den Tiebreaker
(Siege zählt, nie Punkt-Diff) stören; falls JF 0/0 nutzt → CCR auf 0/0 angleichen.
**Verifikation:** Pool mit Remis → Standings-Test (domain `standings.rs`) bleibt identisch.

---

## C. Architektur-/Entscheidungen & Sicherheit

### C1 · 🟠 Kein Auth auf LAN-exponierten Write/Delete-Endpoints
**Problem:** Alle mutating Endpoints sind ungeschützt:
`main.rs:73-92` — `PUT/DELETE /api/participants/:id`, `PUT/DELETE /api/clubs/:id`,
`PUT /api/config`, `POST /api/brackets/:id/generate`, `POST /api/brackets/create-all`, `DELETE /api/locks/:scope_key`.
Keine Auth-Middleware im Router.

**Folge:** Jedes Gerät im LAN kann Kämpfer/Clubs/Config/Brackets löschen.
JF hat dasselbe Risiko (LAN-Werkzeug), aber CCR **neu** einführt DELETEs, die JF so nicht hatte.

**Entscheidung:** LAN-Trust-Doku klarmachen (wie JF) **oder** minimale Token-/Header-Auth für `PUT/DELETE` +
`POST generate/create-all` ergänzen.
**Verifikation:** ohne Token → 401/403 auf mutating Routen; mit Token → 2xx.

### C2 · 🟠 Zero automatisierte Tests im Live-/DB-Pfad
**Problem:** `cargo test --workspace`: **ccr_server 0, ccr_db 0** — nur `ccr_domain 29` + `ccr_excel 11` decken Logik.
Der gesamte Live-Pfad (WS-Scoring, REORDER, `resolve_lb_byes`, Pool-Finalize, `set_result`) ist **ungetestet**
und bisher nur per manueller E2E validiert (PLAN.md).

**Fix:** Mindestens: `SUBSCORE_UPDATE` (JVP-Additiv + Sore-Made-Abschluss), `STATUS_UPDATE` (Propagation),
`resolve_lb_byes` (ko + rep, A4), `jvp_finish_broadcast`/Pool-Finalize, `reorder` als Integrationstests (sqlx + Testcontainer/Mock).
**Verifikation:** `cargo test --workspace` > 40; rot, wenn A3/A4-Regressionen entstehen.

### C3 · 🔵 `fights`-Reihenfolge & Reconcile-Schreiben bei jedem `GET`
**Problem:** `matches.rs:118` — `reconcile::resolve_lb_byes` läuft in `GET /api/matches` (Lesepfad schreibt).
**Folge:** Lese-Requests haben Seiteneffekte (DB-Writes), schwer zu testen/skalieren.
**Fix:** Reconcile auf den schreibenden Pfade (Webhook/REORDER/`set_result`) verlegen; `GET` bleibt rein lesend.
**Verifikation:** `GET /api/matches` ändert `fights`-Tabellen nicht (Snapshot-Vergleich im Test).

---

## D. Doku-/Hygiene-Drift (veraltete Angaben)

### D1 · 🟡 Veraltete „Phase-4 / DEFERRED / STUBBED"-Labels auf bereits implementierten Pfade
Betroffen (alle **sagen**, es sei vertagt, **Tatsache** ist es gebaut):
- `ccr-server/src/live.rs:7` — „DEFERRED to Phase 4 … pool standings finalize" → aber `live.rs:285-295` finalisiert Pools.
- `ccr-server/src/main.rs:10` — „Topology-heavy propagation … is Phase 4".
- `ccr-server/src/matches.rs:17` — „Phase-1 subset … owned by Phase 4".
- `ccr-db/src/fights.rs:245` — „LB/repechage/double topology … Phase 4".
- `ccr-db/src/brackets.rs:223,236` — „Doppelpool … KO-stage is Phase 4".
- `ccr-domain/src/ko.rs:4` — „Balanced-seeding + LB topology stay STUBBED until Phase 4" → aber `resolve_lb_byes` + `ko32`-Orakel existieren.
- `ccr-domain/src/lib.rs:24` — `TODO(Phase 4)`.

**Achtung:** Das ist **gemischt** — manche STUBs sind echt, manche überholt. Prüfen, was live ist:
- **Echt (noch) STUB** (verifizieren!): `ccr-excel/src/form_filler.rs:2`, `seeding_reimport.rs:2`, `urkunden.rs:2` („STUB — see PLAN.md Phase 4").
- **Überholt** (gebaut, aber als vertagt gelabelt): die obigen `live.rs/main.rs/matches.rs/fights.rs/brackets.rs/ko.rs`.

**Fix:** Labels aufräumen: gebaute Pfade als „implemented" markieren, echte Stubs mit konkretem Scope-Vermerk.
**Verifikation:** `grep -rn "Phase 4\|DEFERRED\|STUB"` → jede Treffer-Zeile entweder korrekt oder entfernt.

### D2 · 🟡 `admin.rs`-Header ist falsch
**Problem:** `ccr-server/src/admin.rs:2-4` — „Read-only — no writes to the edv-owned schema yet (import + bracket generation are later slices)".
**Tatsache:** `admin.rs` tut Import, CURD (Participant/Club), Config-PUT, Bracket-Generierung, Seeding, Places.
**Fix:** Modul-Doku auf den Ist-Zustand aktualisieren.

### D3 · ✅ `ccr-db/src/lib.rs`-Invariante vs. Realität — **umgesetzt (2026-10-01, via B3)**
**Problem:** „Do NOT introduce migrations that diverge" (in `ccr-db/src/lib.rs`) — aber Migrations 0002/0003/0004 tun genau das (siehe B3).
**Fix:** mit B3-Entscheidung auflösen (Text anpassen **oder** Migrations rollen).
**Status:** aufgelöst via **B3 (Option A)** — `ccr-db/src/lib.rs` sagt jetzt „CCR owns the schema (decision 2026-10-01) … New schema changes go in `migrations/` (this crate), not edv" (statt „Do NOT diverge until Phase 5"). Text angepasst; keine Migration gerollt. Siehe `WSP/CLAUDE.md` (Invariant `:5432`).

### D4 · 🔵 Triviales / Cleanup
- `crates/ccr-frontend/dist/index 2.html` (946 B) — macOS-„Kopie"-Artefakt; `dist/` ist **git-ignoriert** (`.gitignore:6`), also kein Repo-Problem, aber lokales Clutter → löschen.
- `ccr-db/src/participants.rs:120` — `update_weighin` ist **totes Code** (keine Aufrufe) → löschen oder nutzen.
- `main.rs:120-128` — `no-cache`-Header wird über den **gesamten** Router gelegt (auch alle `/api/*`-Antworten); API-Endpunkte brauchen das nicht → auf die statischen Assets beschränken.
- `pools.rs`-Pool-Draw-Scores vs. JF 0/0 (siehe B5) — Angleichen.
- 64er-DKO + 8er/16er-Repechage sind **extrapoliert** (PLAN.md „ohne Gewähr") → Orakel-Tests (wie `ko32`) ergänzen, bevor in Produktion.
- `SIGNAL`-WS-Event vorhanden (`live.rs:257`), aber **kein UI** → Button ergänzen oder entfernen.
- Keine SVG-Tree-Linien / Doppelpool-Spezialansicht (bekanntes Deferral) → in PLAN.md als offener Punkt sichtbar halten.

---

## Bestätigt OK (nicht erneut anfechten)
- 8er/16er Doppel-KO-Struktur == JF `_LB_STRUCTURE` (`JudgeFrontend/main.py:1287-1312`).
- `standings.rs` == DJB-Tiebreaker (Siege zählt, nie Punkt-Diff).
- `ko32`-Graph + Replay-Orakel grün; `resolve_lb_byes` **ist** implementiert (trotz D1-Label).
- contestants CSV/JSON-Format treu zu edv; Locks/Doublestart/Reseding korrekt.
- Config-`.xlsx`-Parsing validiert (Test- vs. reale edv-Config).
- **40 Tests grün** (domain 29, excel 11); Git-Baum sauber (2 Commits).

> Hinweis: `config/bracket_config.xlsx` (CCR) und `edv/config/bracket_config.xlsx` haben unterschiedliche
> Hashes — vermutlich eine veraltete Bundled-Kopie. Prüfen, ob CCR die edv-Quelle referenzieren soll
> (Sonst: eine Zeile in Doku: „CCR-Config ist der kanonische Import".)
