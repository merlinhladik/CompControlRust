// SPDX-License-Identifier: GPL-3.0-or-later
//! Group find-or-create + membership (config-driven assignment, Phase 5).

use sqlx::PgPool;

/// Find or create a group by its unique name; returns the group id.
pub async fn find_or_create(
    pool: &PgPool,
    name: &str,
    gender: Option<&str>,
    age_group: Option<&str>,
    weight_class: Option<&str>,
) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO groups (name, gender, age_group, weight_class) VALUES ($1,$2,$3,$4) \
         ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING id",
    )
    .bind(name)
    .bind(gender)
    .bind(age_group)
    .bind(weight_class)
    .fetch_one(pool)
    .await
}

/// Members of a group as (participant_id, weight_kg), sorted lightest first
/// (NULL weight last) — the input for youth weight-pool splitting.
pub async fn members_with_weight(
    pool: &PgPool,
    group_id: i32,
) -> Result<Vec<(i32, f64)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT gp.participant_id, COALESCE(p.weight, 0)::float8 \
         FROM group_participants gp JOIN participants p ON p.id = gp.participant_id \
         WHERE gp.group_id = $1 ORDER BY p.weight ASC NULLS LAST, gp.participant_id",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await
}

/// True if a group with this exact name exists (idempotency guard for splits).
pub async fn exists_named(pool: &PgPool, name: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM groups WHERE name = $1)")
        .bind(name)
        .fetch_one(pool)
        .await
}

/// Add a participant to a group unless already a member. Returns true if added.
pub async fn add_member(pool: &PgPool, group_id: i32, participant_id: i32) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        "INSERT INTO group_participants (group_id, participant_id) \
         SELECT $1, $2 WHERE NOT EXISTS ( \
            SELECT 1 FROM group_participants WHERE group_id=$1 AND participant_id=$2)",
    )
    .bind(group_id)
    .bind(participant_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}
