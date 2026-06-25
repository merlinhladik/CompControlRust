// SPDX-License-Identifier: GPL-3.0-or-later
//! Age-class locks (`age_class_locks`). Gate ADMIN ops (edit / assign / (re)gen)
//! of a locked (gender, age) class — NEVER the live path. Mirrors edv
//! `tournament_service.lock_age_class` + `utils.helpers.age_class_scope_key`.

use std::collections::HashSet;

use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct AgeClassLock {
    pub id: i32,
    pub scope_key: String,
    pub age_group: String,
    pub gender: Option<String>,
    pub locked_at: NaiveDateTime,
    pub reason: Option<String>,
}

/// Stable lock key: `"{gender}|{age}"` if gender-scoped, else just `"{age}"`.
/// edv `age_class_scope_key`.
pub fn scope_key(age_group: &str, gender: Option<&str>) -> String {
    match gender {
        Some(g) if !g.is_empty() => format!("{g}|{}", age_group.trim()),
        _ => age_group.trim().to_string(),
    }
}

/// Is the (gender, age) class locked? Matches a gendered OR a gender-neutral lock.
pub fn is_class_locked(locked: &HashSet<String>, gender: Option<&str>, age_group: &str) -> bool {
    locked.contains(&scope_key(age_group, gender)) || locked.contains(&scope_key(age_group, None))
}

pub async fn all(pool: &PgPool) -> Result<Vec<AgeClassLock>, sqlx::Error> {
    sqlx::query_as::<_, AgeClassLock>(
        "SELECT id, scope_key, age_group, gender, locked_at, reason FROM age_class_locks ORDER BY scope_key",
    )
    .fetch_all(pool)
    .await
}

pub async fn locked_keys(pool: &PgPool) -> Result<HashSet<String>, sqlx::Error> {
    let keys: Vec<String> = sqlx::query_scalar("SELECT scope_key FROM age_class_locks")
        .fetch_all(pool)
        .await?;
    Ok(keys.into_iter().collect())
}

/// Lock a class (upsert on scope_key, refresh reason). edv lock_age_class.
pub async fn set(
    pool: &PgPool,
    age_group: &str,
    gender: Option<&str>,
    reason: &str,
) -> Result<String, sqlx::Error> {
    let key = scope_key(age_group, gender);
    sqlx::query(
        "INSERT INTO age_class_locks (scope_key, age_group, gender, locked_at, reason) \
         VALUES ($1,$2,$3,now(),$4) \
         ON CONFLICT ON CONSTRAINT uix_age_class_lock_scope \
         DO UPDATE SET reason = EXCLUDED.reason",
    )
    .bind(&key)
    .bind(age_group)
    .bind(gender)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(key)
}

pub async fn remove(pool: &PgPool, scope_key: &str) -> Result<u64, sqlx::Error> {
    let r = sqlx::query("DELETE FROM age_class_locks WHERE scope_key=$1")
        .bind(scope_key)
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}
