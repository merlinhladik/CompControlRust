// SPDX-License-Identifier: GPL-3.0-or-later
//! Resolve group_participants.id → real athlete data (Phase 1).
//! Mirrors JF main.py `_resolve_participants` (gp JOIN participants).

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;

/// Fields for a contestants→participant UPSERT (mirrors edv tournament_service).
/// `birthyear`/`weight_kg` are converted to birth_date (Jan 1) / Decimal here so
/// callers need no chrono/decimal deps.
pub struct ParticipantUpsert {
    pub first_name: String,
    pub last_name: String,
    pub gender: Option<String>,
    pub birthyear: Option<i32>,
    pub weight_kg: Option<f64>,
    pub club: String,
    pub association: String,
    pub valid: bool,
    pub paid: bool,
    pub doublestart: String,
}

fn weight_to_decimal(w: Option<f64>) -> Option<Decimal> {
    w.and_then(Decimal::from_f64_retain)
}
fn birthyear_to_date(y: Option<i32>) -> Option<NaiveDate> {
    y.and_then(|y| NaiveDate::from_ymd_opt(y, 1, 1))
}

/// UPSERT a participant by the natural-key constraint
/// `uix_participant_identity (first_name,last_name,gender,birth_date,club)`.
/// On conflict updates the weigh-in fields (weight/valid/paid/doublestart) +
/// association — exactly edv's import semantics. Returns true if INSERTED (new).
pub async fn upsert(pool: &PgPool, p: ParticipantUpsert) -> Result<bool, sqlx::Error> {
    let inserted: bool = sqlx::query_scalar(
        "INSERT INTO participants \
             (first_name,last_name,gender,birth_date,weight,club,association,valid,paid,doublestart) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
         ON CONFLICT ON CONSTRAINT uix_participant_identity DO UPDATE SET \
             weight=EXCLUDED.weight, valid=EXCLUDED.valid, paid=EXCLUDED.paid, \
             doublestart=EXCLUDED.doublestart, association=EXCLUDED.association \
         RETURNING (xmax = 0)",
    )
    .bind(&p.first_name)
    .bind(&p.last_name)
    .bind(&p.gender)
    .bind(birthyear_to_date(p.birthyear))
    .bind(weight_to_decimal(p.weight_kg))
    .bind(&p.club)
    .bind(&p.association)
    .bind(p.valid)
    .bind(p.paid)
    .bind(&p.doublestart)
    .fetch_one(pool)
    .await?;
    Ok(inserted)
}

/// Full editable participant record (the in-app fighter editor / create).
pub struct ParticipantEdit {
    pub first_name: String,
    pub last_name: String,
    pub gender: Option<String>,
    pub birthyear: Option<i32>,
    pub weight_kg: Option<f64>,
    pub club: String,
    pub association: String,
    pub valid: bool,
    pub paid: bool,
    pub doublestart: String,
}

/// Create a new participant; returns the new id.
pub async fn create(pool: &PgPool, p: &ParticipantEdit) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO participants \
             (first_name,last_name,gender,birth_date,weight,club,association,valid,paid,doublestart) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id",
    )
    .bind(&p.first_name).bind(&p.last_name).bind(&p.gender)
    .bind(birthyear_to_date(p.birthyear)).bind(weight_to_decimal(p.weight_kg))
    .bind(&p.club).bind(&p.association).bind(p.valid).bind(p.paid).bind(&p.doublestart)
    .fetch_one(pool)
    .await
}

/// Full update of one participant (all fields). Returns rows affected.
pub async fn update_full(pool: &PgPool, id: i32, p: &ParticipantEdit) -> Result<u64, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE participants SET first_name=$2,last_name=$3,gender=$4,birth_date=$5,weight=$6, \
             club=$7,association=$8,valid=$9,paid=$10,doublestart=$11 WHERE id=$1",
    )
    .bind(id)
    .bind(&p.first_name).bind(&p.last_name).bind(&p.gender)
    .bind(birthyear_to_date(p.birthyear)).bind(weight_to_decimal(p.weight_kg))
    .bind(&p.club).bind(&p.association).bind(p.valid).bind(p.paid).bind(&p.doublestart)
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}

/// (id, last_name, first_name, club, gender, birth_year) for weigh-in cards.
/// No weight filter — ALL participants, so cards can be printed pre-weigh-in.
pub async fn for_wiegekarten(
    pool: &PgPool,
) -> Result<Vec<(i32, String, String, String, String, Option<i32>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, last_name, first_name, COALESCE(club,''), COALESCE(gender,''), \
                EXTRACT(YEAR FROM birth_date)::int \
         FROM participants ORDER BY club NULLS LAST, last_name, first_name",
    )
    .fetch_all(pool)
    .await
}

/// Update the weigh-in fields of one participant (the editor). Returns rows affected.
pub async fn update_weighin(
    pool: &PgPool,
    id: i32,
    weight_kg: Option<f64>,
    valid: bool,
    paid: bool,
    doublestart: &str,
) -> Result<u64, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE participants SET weight=$2, valid=$3, paid=$4, doublestart=$5 WHERE id=$1",
    )
    .bind(id)
    .bind(weight_to_decimal(weight_kg))
    .bind(valid)
    .bind(paid)
    .bind(doublestart)
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}

/// Resolved fighter info for one `group_participants.id`.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ResolvedParticipant {
    pub gp_id: i32,
    pub participant_id: i32,
    pub first_name: String,
    pub last_name: String,
    pub club: Option<String>,
}

/// True if the participant is referenced by any fight (p1/p2/winner) or bracket
/// placement — i.e. competing/placed, so must NOT be deleted.
pub async fn is_referenced(pool: &PgPool, id: i32) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM group_participants gp WHERE gp.participant_id=$1 AND ( \
            EXISTS(SELECT 1 FROM fights f WHERE gp.id IN (f.participant1_id, f.participant2_id, f.winner_id)) \
            OR EXISTS(SELECT 1 FROM brackets b WHERE gp.id IN \
                (b.first_place, b.second_place, b.third_place_1, b.third_place_2)) ))",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

/// Delete a participant + its group memberships. Caller must check is_referenced.
pub async fn delete(pool: &PgPool, id: i32) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM group_participants WHERE participant_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let r = sqlx::query("DELETE FROM participants WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(r.rows_affected())
}

/// (gender, age_group) of every group a participant belongs to (lock check).
pub async fn groups_of(
    pool: &PgPool,
    participant_id: i32,
) -> Result<Vec<(Option<String>, Option<String>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT g.gender, g.age_group FROM group_participants gp \
         JOIN groups g ON g.id = gp.group_id WHERE gp.participant_id = $1",
    )
    .bind(participant_id)
    .fetch_all(pool)
    .await
}

/// Participants reduced to what group assignment needs: (id, gender, birth_year,
/// weight_kg, doublestart). Weight→f64 + year extracted in SQL (no Decimal).
pub async fn for_assignment(
    pool: &PgPool,
) -> Result<Vec<(i32, Option<String>, Option<i32>, Option<f64>, Option<String>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, gender, EXTRACT(YEAR FROM birth_date)::int, weight::float8, doublestart \
         FROM participants",
    )
    .fetch_all(pool)
    .await
}

/// All participants (admin list). Ordered by club then name.
pub async fn all(pool: &PgPool) -> Result<Vec<crate::models::Participant>, sqlx::Error> {
    sqlx::query_as::<_, crate::models::Participant>(
        "SELECT id, first_name, last_name, gender, birth_date, weight, club, \
                association, valid, paid, doublestart \
         FROM participants ORDER BY club NULLS LAST, last_name, first_name",
    )
    .fetch_all(pool)
    .await
}

/// Batch-resolve a set of group_participants ids.
pub async fn resolve(
    pool: &PgPool,
    gp_ids: &[i32],
) -> Result<Vec<ResolvedParticipant>, sqlx::Error> {
    if gp_ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as::<_, ResolvedParticipant>(
        "SELECT gp.id AS gp_id, p.id AS participant_id, \
                p.first_name, p.last_name, p.club \
         FROM group_participants gp \
         JOIN participants p ON p.id = gp.participant_id \
         WHERE gp.id = ANY($1)",
    )
    .bind(gp_ids)
    .fetch_all(pool)
    .await
}
