use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        error::ApiError, extractors::AuthenticatedRequest, permissions::PermissionKey,
        state::AppState,
    },
    auth::AuthService,
    domain::notification_channels::{self, NotificationChannelType},
    notifications,
};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/v1/notifications/channels",
            get(list_notification_channels).post(create_notification_channel),
        )
        .route(
            "/v1/notifications/channels/{channel_id}",
            patch(update_notification_channel).delete(delete_notification_channel),
        )
}

#[derive(Serialize)]
struct NotificationChannelResponse {
    id: i64,
    channel_type: NotificationChannelType,
    name: String,
    destination: String,
    has_webhook_secret: bool,
    notify_on_failure: bool,
    notify_on_recovery: bool,
    is_active: bool,
    created_at: String,
    updated_at: String,
}

impl From<notification_channels::NotificationChannel> for NotificationChannelResponse {
    fn from(channel: notification_channels::NotificationChannel) -> Self {
        Self {
            id: channel.id,
            channel_type: channel.channel_type,
            name: channel.name,
            destination: channel.destination,
            has_webhook_secret: channel.webhook_secret_ciphertext.is_some(),
            notify_on_failure: channel.notify_on_failure,
            notify_on_recovery: channel.notify_on_recovery,
            is_active: channel.is_active,
            created_at: channel.created_at.to_rfc3339(),
            updated_at: channel.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Deserialize)]
struct UpsertNotificationChannelRequest {
    channel_type: NotificationChannelType,
    name: String,
    destination: String,
    webhook_secret: Option<String>,
    notify_on_failure: bool,
    notify_on_recovery: bool,
    is_active: bool,
}

async fn list_notification_channels(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
) -> Result<Json<Vec<NotificationChannelResponse>>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::NotificationChannelsRead,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let channels = notification_channels::repository::list_channels(&state.pool)
        .await
        .map_err(ApiError::internal_error)?;

    Ok(Json(
        channels
            .into_iter()
            .map(NotificationChannelResponse::from)
            .collect(),
    ))
}

async fn create_notification_channel(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Json(payload): Json<UpsertNotificationChannelRequest>,
) -> Result<(StatusCode, Json<NotificationChannelResponse>), ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::NotificationChannelsCreate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    validate_notification_channel_payload(&payload).await?;
    let webhook_secret_ciphertext = resolve_webhook_secret_ciphertext(&state, &payload, None)?;

    let channel = notification_channels::repository::create_channel(
        &state.pool,
        &notification_channels::NotificationChannelParams {
            channel_type: payload.channel_type,
            name: payload.name.trim(),
            destination: payload.destination.trim(),
            webhook_secret_ciphertext: webhook_secret_ciphertext.as_deref(),
            notify_on_failure: payload.notify_on_failure,
            notify_on_recovery: payload.notify_on_recovery,
            is_active: payload.is_active,
        },
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok((
        StatusCode::CREATED,
        Json(NotificationChannelResponse::from(channel)),
    ))
}

async fn update_notification_channel(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(channel_id): Path<i64>,
    Json(payload): Json<UpsertNotificationChannelRequest>,
) -> Result<Json<NotificationChannelResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::NotificationChannelsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    validate_notification_channel_payload(&payload).await?;
    let existing_channel =
        notification_channels::repository::get_channel_by_id(&state.pool, channel_id)
            .await
            .map_err(ApiError::internal_error)?
            .ok_or_else(|| ApiError::not_found("notification channel not found"))?;
    let webhook_secret_ciphertext =
        resolve_webhook_secret_ciphertext(&state, &payload, Some(&existing_channel))?;

    let Some(channel) = notification_channels::repository::update_channel(
        &state.pool,
        channel_id,
        &notification_channels::NotificationChannelParams {
            channel_type: payload.channel_type,
            name: payload.name.trim(),
            destination: payload.destination.trim(),
            webhook_secret_ciphertext: webhook_secret_ciphertext.as_deref(),
            notify_on_failure: payload.notify_on_failure,
            notify_on_recovery: payload.notify_on_recovery,
            is_active: payload.is_active,
        },
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("notification channel not found"));
    };

    Ok(Json(NotificationChannelResponse::from(channel)))
}

#[derive(Serialize)]
struct DeleteNotificationChannelResponse {
    deleted: bool,
}

async fn delete_notification_channel(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(channel_id): Path<i64>,
) -> Result<Json<DeleteNotificationChannelResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::NotificationChannelsDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let deleted = notification_channels::repository::delete_channel(&state.pool, channel_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !deleted {
        return Err(ApiError::not_found("notification channel not found"));
    }

    Ok(Json(DeleteNotificationChannelResponse { deleted }))
}

async fn validate_notification_channel_payload(
    payload: &UpsertNotificationChannelRequest,
) -> Result<(), ApiError> {
    if payload.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }

    if payload.destination.trim().is_empty() {
        return Err(ApiError::bad_request("destination is required"));
    }

    if !payload.notify_on_failure && !payload.notify_on_recovery {
        return Err(ApiError::bad_request(
            "at least one notification event must be enabled",
        ));
    }

    notifications::service::validate_channel_destination(
        payload.channel_type,
        payload.destination.trim(),
    )
    .await
    .map_err(ApiError::bad_request_error)?;

    if matches!(payload.webhook_secret.as_deref(), Some(secret) if secret.trim().is_empty()) {
        return Err(ApiError::bad_request("webhook_secret can not be empty"));
    }

    Ok(())
}

fn resolve_webhook_secret_ciphertext(
    state: &AppState,
    payload: &UpsertNotificationChannelRequest,
    existing_channel: Option<&notification_channels::NotificationChannel>,
) -> Result<Option<String>, ApiError> {
    let provided_secret = payload
        .webhook_secret
        .as_deref()
        .map(str::trim)
        .filter(|secret| !secret.is_empty());

    match payload.channel_type {
        NotificationChannelType::Webhook => {
            if let Some(secret) = provided_secret {
                return state
                    .webhook_secret_encryption_key
                    .encrypt_webhook_secret(secret)
                    .map(Some)
                    .map_err(ApiError::internal_error);
            }

            existing_channel
                .and_then(|channel| channel.webhook_secret_ciphertext.clone())
                .map(Some)
                .ok_or_else(|| {
                    ApiError::bad_request("webhook_secret is required for webhook channels")
                })
        }
        NotificationChannelType::Email
        | NotificationChannelType::Slack
        | NotificationChannelType::Discord => {
            if provided_secret.is_some() {
                return Err(ApiError::bad_request(
                    "webhook_secret is only supported for webhook channels",
                ));
            }

            Ok(None)
        }
    }
}
