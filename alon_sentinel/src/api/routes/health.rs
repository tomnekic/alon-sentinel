use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use serde::Serialize;

use crate::api::state::AppState;

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/live", get(live))
        .route("/ready", get(ready))
        .route("/health", get(live))
}

#[derive(Serialize)]
struct LiveResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct ReadyResponse {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'static str>,
}

async fn live() -> Json<LiveResponse> {
    Json(LiveResponse { status: "ok" })
}

async fn ready(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Err(e) = sqlx::query("SELECT 1").execute(&state.pool).await {
        tracing::warn!(error = ?e, "readiness check: db ping failed");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse {
                status: "not_ready",
                error: Some("db_unavailable"),
            }),
        )
            .into_response();
    }

    let migration_count: Result<i64, _> =
        sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations")
            .fetch_one(&state.pool)
            .await;

    match migration_count {
        Err(e) => {
            tracing::warn!(error = ?e, "readiness check: schema_migrations query failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ReadyResponse {
                    status: "not_ready",
                    error: Some("migrations_not_applied"),
                }),
            )
                .into_response()
        }
        Ok(0) => {
            tracing::warn!("readiness check: schema_migrations is empty");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ReadyResponse {
                    status: "not_ready",
                    error: Some("migrations_not_applied"),
                }),
            )
                .into_response()
        }
        Ok(_) => (
            StatusCode::OK,
            Json(ReadyResponse {
                status: "ok",
                error: None,
            }),
        )
            .into_response(),
    }
}
