use std::sync::Arc;

use axum::{
    Router,
    http::{HeaderName, HeaderValue},
};
use tower_http::set_header::SetResponseHeaderLayer;

pub use crate::api::state::AppState;

use crate::api::routes::{admin, auth, health, metrics, notifications, public, sites};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .merge(metrics::router())
        .merge(auth::router())
        .merge(admin::router())
        .merge(sites::router())
        .merge(notifications::router())
        .merge(public::router())
        .with_state(Arc::new(state))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static("default-src 'none'"),
        ))
}
