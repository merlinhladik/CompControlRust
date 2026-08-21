// SPDX-License-Identifier: GPL-3.0-or-later
//! Club master data (`clubs`) — feeds the fighter-editor dropdown.
//!
//! `participants.club` stays denormalized text (the contestants JSON/CSV wire
//! format carries the name); a rename here cascades onto participants.

use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Club {
    pub id: i32,
    pub name: String,
    pub association: Option<String>,
}

pub async fn all(pool: &PgPool) -> Result<Vec<Club>, sqlx::Error> {
    sqlx::query_as::<_, Club>("SELECT id, name, association FROM clubs ORDER BY name")
        .fetch_all(pool)
        .await
}

/// Insert (or no-op if the name exists). Returns the club id.
pub async fn create(pool: &PgPool, name: &str, association: Option<&str>) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO clubs (name, association) VALUES ($1, NULLIF($2,'')) \
         ON CONFLICT (name) DO UPDATE SET association = COALESCE(clubs.association, EXCLUDED.association) \
         RETURNING id",
    )
    .bind(name)
    .bind(association.unwrap_or(""))
    .fetch_one(pool)
    .await
}

/// Ensure every distinct club name in `names` exists (import sync).
pub async fn ensure_names(pool: &PgPool, names: &[String]) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO clubs (name) SELECT DISTINCT unnest($1::text[]) ON CONFLICT (name) DO NOTHING",
    )
    .bind(names)
    .execute(pool)
    .await?;
    Ok(())
}

/// Rename + re-associate; the rename cascades onto participants.club.
/// Returns the number of fighters carried along.
pub async fn update(
    pool: &PgPool,
    id: i32,
    name: &str,
    association: Option<&str>,
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let old: Option<String> = sqlx::query_scalar("SELECT name FROM clubs WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(old) = old else { return Err(sqlx::Error::RowNotFound) };
    sqlx::query("UPDATE clubs SET name=$1, association=NULLIF($2,'') WHERE id=$3")
        .bind(name)
        .bind(association.unwrap_or(""))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let carried = if old != name {
        sqlx::query("UPDATE participants SET club=$1 WHERE club=$2")
            .bind(name)
            .bind(&old)
            .execute(&mut *tx)
            .await?
            .rows_affected()
    } else {
        0
    };
    tx.commit().await?;
    Ok(carried)
}

/// Number of fighters still on this club (blocks delete when > 0).
pub async fn fighters_on(pool: &PgPool, id: i32) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM participants WHERE club = (SELECT name FROM clubs WHERE id=$1)",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn delete(pool: &PgPool, id: i32) -> Result<u64, sqlx::Error> {
    let r = sqlx::query("DELETE FROM clubs WHERE id=$1").bind(id).execute(pool).await?;
    Ok(r.rows_affected())
}
