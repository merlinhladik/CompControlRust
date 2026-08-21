<!-- SPDX-License-Identifier: CC0-1.0 -->
# CCR — Docker install & startup

Standalone deployment: one compose stack with Postgres + the CCR web server
(HTTP + WebSocket + frontend, same-origin on `:5001`). CCR owns this database —
it is **not** the shared edv/JF instance; schema migrations run automatically on
startup.

## Prerequisites

- Docker with the Compose plugin (`docker compose version`). Nothing else — no
  Rust toolchain needed; the image builds frontend (Trunk/WASM) and backend in a
  multi-stage build.

## Start

```sh
cd CompControlRust
docker compose up -d --build
open http://localhost:5001        # frontend ("Admin" for config/lists)
```

First build takes a while (Rust release build + WASM); subsequent builds are
cached. `docker compose ps` shows both services `healthy` when ready.

With Mattenliste + Ipponboard coupling enabled:

```sh
CCR_MATS=1 docker compose up -d --build
```

## Ports

| Port | What | Note |
|---|---|---|
| `5001` | CCR web (HTTP + WS + frontend) | health probe: `curl localhost:5001/health` |
| `127.0.0.1:5433` | Postgres (host access, psql/seeds) | 5433 on purpose — coexists with edv's `:5432` |

Containers talk to the DB internally on `db:5432`; the host mapping is only for
manual inspection: `psql postgres://myuser:mypassword@localhost:5433/mydatabase`.

## Everyday operations

```sh
docker compose logs -f web        # server logs (RUST_LOG=info)
docker compose down               # stop, keep data
docker compose up -d --build      # update after a code change
```

## Reset data

Tournament data lives in the named volume `ccr_postgres_data` and survives
`down`/`up`.

> **Warning:** the following permanently deletes all CCR tournament data
> (participants, brackets, results). It does not touch the edv/JF database.

```sh
docker compose down -v
```
