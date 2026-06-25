// SPDX-License-Identifier: GPL-3.0-or-later
//! CompControlRust — unified web server (HTTP + WebSocket) for the WSP suite.
//!
//! Replaces, over the strangler migration (see ../../PLAN.md):
//!   - JudgeFrontend/main.py   (FastAPI REST + WS /ws, live score propagation)
//!   - edv frontend (Tkinter)  (tournament admin, now a web frontend)
//!
//! Phase 1: read-only `/api/matches` (see matches.rs).
//! Phase 2: live path — WS `/ws`, Ipponboard webhook, WB propagation (see live.rs).
//! Topology-heavy propagation (LB / repechage / double / pool standings) is Phase 4.

use std::{env, sync::Arc};

use axum::{
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::{broadcast, Mutex};

use ccr_db::PgPoolHandle as PgPool;

mod admin;
mod live;
mod matches;

/// Shared server state. `tx` fans WS broadcasts out to every client (replaces
/// JF's ConnectionManager.broadcast); `last_pushed` is the Ipponboard pointer
/// (JF's global last_pushed_match_id).
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub tx: broadcast::Sender<String>,
    pub last_pushed: Arc<Mutex<Option<i32>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ccr_server=info,tower_http=info".into()),
        )
        .init();

    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://myuser:mypassword@localhost:5432/mydatabase".to_string()
    });
    let pool = ccr_db::connect(&database_url).await?;
    tracing::info!("connected to Postgres");
    // CCR owns the schema (Phase 5): idempotent baseline creates it on a fresh DB,
    // no-op on the existing edv DB.
    ccr_db::run_migrations(&pool).await?;
    tracing::info!("schema migrations applied");

    let (tx, _rx) = broadcast::channel::<String>(256);
    let state = AppState {
        pool,
        tx,
        last_pushed: Arc::new(Mutex::new(None)),
    };

    let mut app = Router::new()
        .route("/health", get(health))
        .route("/api/version", get(version))
        .route("/api/matches", get(matches::get_matches))
        .route("/api/participants", get(admin::list_participants).post(admin::create_participant))
        .route(
            "/api/participants/:id",
            axum::routing::put(admin::update_participant).delete(admin::delete_participant),
        )
        .route("/api/import-contestants", post(admin::import_contestants))
        .route("/api/brackets", get(admin::list_brackets))
        .route("/api/brackets/:id/generate", post(admin::generate_bracket))
        .route("/api/brackets/create-all", post(admin::create_all_brackets))
        .route("/api/assign-groups", post(admin::assign_groups))
        .route("/api/config", get(admin::get_config_handler).put(admin::put_config))
        .route("/api/locks", get(admin::list_locks).post(admin::add_lock))
        .route("/api/locks/:scope_key", axum::routing::delete(admin::remove_lock))
        .route("/api/export/results.xlsx", get(admin::export_results))
        .route("/api/export/results.pdf", get(admin::export_results_pdf))
        .route("/api/export/wiegekarten.pdf", get(admin::export_wiegekarten_pdf))
        .route("/api/export/urkunden.xlsx", get(admin::export_urkunden))
        .route("/api/export/urkunden.csv", get(admin::export_urkunden_csv))
        .route("/api/export/wiegekarten.xlsx", get(admin::export_wiegekarten))
        .route("/api/export/wiegekarten.csv", get(admin::export_wiegekarten_csv))
        .route("/print/wiegekarten", get(admin::print_wiegekarten))
        .route("/ws", get(live::ws_handler))
        .route("/api/ippon-score", post(live::ippon_score))
        .route("/api/push-to-ipponboard/:match_id", post(live::push_to_ipponboard))
        .with_state(state);

    // Serve the Leptos WASM frontend (Trunk dist) same-origin; SPA fallback to
    // index.html. Skipped if the dist dir is absent (API-only run).
    let dist = env::var("CCR_FRONTEND_DIR")
        .unwrap_or_else(|_| "crates/ccr-frontend/dist".into());
    if std::path::Path::new(&dist).join("index.html").exists() {
        let index = format!("{dist}/index.html");
        app = app.fallback_service(
            tower_http::services::ServeDir::new(&dist)
                .fallback(tower_http::services::ServeFile::new(index)),
        );
        tracing::info!("serving frontend from {dist}");
    } else {
        tracing::info!("no frontend dist at {dist} — API only");
    }

    let addr = env::var("CCR_HTTP_ADDR").unwrap_or_else(|_| "0.0.0.0:5001".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("CompControlRust listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn version() -> Json<Value> {
    Json(json!({ "name": "CompControlRust", "version": env!("CARGO_PKG_VERSION") }))
}

/// HTTP error with an explicit status. sqlx errors map to 500.
pub struct AppError(pub anyhow::Error, pub axum::http::StatusCode);

impl AppError {
    pub fn status(code: axum::http::StatusCode, msg: String) -> Self {
        AppError(anyhow::anyhow!(msg), code)
    }
}

impl From<ccr_db::DbError> for AppError {
    fn from(e: ccr_db::DbError) -> Self {
        AppError(e.into(), axum::http::StatusCode::INTERNAL_SERVER_ERROR)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        if self.1.is_server_error() {
            tracing::error!("handler error: {:#}", self.0);
        }
        (self.1, Json(json!({ "error": self.0.to_string() }))).into_response()
    }
}
