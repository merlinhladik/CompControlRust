<!-- SPDX-License-Identifier: CC0-1.0 -->
# CompControlRust (CCR)

Unified Rust web server merging **JudgeFrontend** (live scoring API + PWA) and
**edv** (tournament admin) into one backend with a single web frontend, part of
the "TOP Team Combat Control" Judo suite.

> **Status:** backend functionally complete — live scoring, all bracket types,
> standings, admin, and exports are ported and tested; the Tkinter→Leptos admin
> UI is the remaining large piece. See [PLAN.md](PLAN.md) for the slice-by-slice
> log and the strangler-migration strategy.

## Why a sibling repo, not a fork

CCR grows *beside* the running Python suite, sharing the existing Postgres
`:5432`. The live system keeps working while CCR takes over one bounded context
at a time. No big-bang cutover.

## Workspace layout

```
crates/
  ccr-server/   axum HTTP + WebSocket; serves the web frontend (thin)
  ccr-domain/   pure logic: pools, KO, doppel-KO, repechage, standings (crown jewel)
  ccr-db/       sqlx Postgres access (matches edv schema until edv retires)
  ccr-excel/    .xls/.xlsx/.csv I/O + Urkunden (HIGHEST RISK — 100% Rust, Go/No-Go spike first)
  ccr-frontend/ Leptos (Rust/WASM) web UI: live + admin
migrations/     sqlx migrations (only after edv schema ownership transfers)
docs/
```

## Build

```sh
cd CompControlRust
cargo build
cargo run -p ccr-server      # GET /health, /api/version on :5001
cargo test
```

Requires a Rust toolchain (`rustup`, ≥1.80) and — for DB work — a reachable
Postgres (`docker compose -f ../edv/docker-compose.yaml up -d db`).

## Export & printing

All exports are `GET` endpoints (download unless noted); the frontend admin links them.

| Endpoint | Output |
|---|---|
| `/api/export/results.xlsx` · `.pdf` | Ergebnisliste (results) |
| `/api/export/urkunden.xlsx` | Urkunden (certificates) — one row per placement |
| `/api/export/urkunden.csv` | Urkunden as a **merge source** (Affinity Publisher / LibreOffice data merge) |
| `/api/export/wiegekarten.xlsx` · `.pdf` | Wiegekarten-**Liste** — weigh-in roster, blank Gewicht/Unterschrift |
| `/api/export/wiegekarten.csv` | Wiegekarten as a **merge source** — design the card once in Affinity, data merge → print |
| `/print/wiegekarten` | Print-ready HTML cards (2-up, ~10/page); open in a browser, Cmd/Ctrl+P (also saves a PDF) |

Weigh-in cards are **weight-independent** — the age class is derived from the birth
year, so the roster can be imported and the cards printed *before* the weigh-in (the
Gewicht/Unterschrift lines are filled by hand at the scale). The `.csv` merge sources
carry the participant `id` as the merge key (and a QR/barcode value for the WeighIn
scanner).

> CCR's exports use its own clean layout, **not** the pixel-exact DJB `.xls`
> templates (no pure-Rust BIFF writer; faithful template-fill would need the
> rejected Python sidecar). For branded, pixel-controlled cards/certificates, use
> the `.csv` merge path in Affinity Publisher / LibreOffice.

## License

Code GPL-3.0-or-later; docs/data CC0-1.0 (REUSE/SPDX, suite convention).
