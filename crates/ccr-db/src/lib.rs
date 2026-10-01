// SPDX-License-Identifier: GPL-3.0-or-later
//! Postgres data access (sqlx).
//!
//! CRITICAL invariant (CLAUDE.md): the shared Postgres `:5432` is owned by CCR
//! (decision 2026-10-01, todo B3) — `migrations/` here is the canonical schema.
//! edv + JF are consumers (declare models, own no migrations; edv's Alembic is
//! frozen). The baseline is idempotent (`CREATE TABLE IF NOT EXISTS`), so it is
//! a no-op on the existing edv DB and the full schema on a fresh one. Column
//! types must still match what edv/JF expect:
//!   fights.score1/score2/duration/table_id : INTEGER NULL
//!   fights.status                           : VARCHAR(20) (pending|finished|bye)
//!   fights.ippon1/.../shido2                : INTEGER NOT NULL DEFAULT 0
//!   participants.doublestart                : String(10)
//!
//! New schema changes go in `migrations/` (this crate), not edv. All queries
//! here are runtime-checked (`query_as`), not the compile-time `query!` macro,
//! so the crate builds without DATABASE_URL.

use sqlx::postgres::{PgPool, PgPoolOptions};

pub use sqlx::PgPool as PgPoolHandle; // re-export so dependents need not depend on sqlx
pub use sqlx::Error as DbError;

pub mod models;
pub mod app_config;
pub mod clubs;
pub mod groups;
pub mod participants;
pub mod fights;
pub mod brackets;
pub mod locks;
pub mod doppel_ko;
pub mod doppel_pool;
pub mod repechage;
pub mod reconcile;

/// Connect a pooled Postgres handle. Same DB as edv/JF during migration.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
}

/// Run CCR's schema migrations (workspace `migrations/`). Idempotent: the
/// baseline is CREATE TABLE IF NOT EXISTS, a no-op on the existing edv DB and
/// the full schema on a fresh one. CCR owns the schema (decision 2026-10-01).
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations").run(pool).await
}
