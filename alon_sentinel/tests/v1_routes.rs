use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use alon_sentinel::{
    api::{
        app::{AppState, build_router},
        rate_limit::{AuthRateLimiter, StatusPageCache},
    },
    auth::{AuthConfig, AuthService, AuthTokenCache},
    domain::{
        admin_users,
        api_auth::{self, ApiClientType},
        notification_channels::{self, NotificationChannelType},
        notification_deliveries::{self, NotificationEventType},
        permissions, roles, site_monitor_checks, site_monitor_incidents, site_monitors,
        site_notification_channel_overrides, sites,
    },
};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::{Executor, PgPool};
use tokio::{net::TcpListener, task::JoinHandle};

static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

const REQUIRED_SCOPES: [&str; 2] = ["sites:read", "sites:write"];
const TEST_WEBHOOK_SECRET: &str = "route-hook-secret";

#[tokio::test]
async fn issues_token_and_supports_sites_crud_over_http() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app.seed_authenticated_client("sites-smoke").await?;

    let create_response = test_app
        .request(Method::Post, "/v1/sites")
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "name": "Marketing",
            "base_url": "https://test.com",
        }))
        .send()
        .await?;

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created_site: Value = create_response.json().await?;
    assert_eq!(created_site["name"], "Marketing");
    assert_eq!(created_site["http_monitor_status"], "not_configured");
    assert!(created_site.get("account_id").is_none());

    let list_response = test_app
        .request(Method::Get, "/v1/sites")
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(list_response.status(), StatusCode::OK);
    let sites: Vec<Value> = list_response.json().await?;
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0]["base_url"], "https://test.com");
    assert!(sites[0].get("account_id").is_none());

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn list_sites_uses_cursor_pagination() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app.seed_authenticated_client("sites-cursor").await?;
    let first = test_app
        .seed_site("Alpha", "https://alpha.test.com")
        .await?;
    let second = test_app
        .seed_site("Bravo", "https://bravo.test.com")
        .await?;

    let first_page = test_app
        .request_with_query(Method::Get, "/v1/sites", &[("limit", "1")])
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(first_page.status(), StatusCode::OK);
    let cursor = header_string(&first_page, "x-next-cursor");
    let first_page_items: Vec<Value> = first_page.json().await?;
    let expected_cursor = first.id.to_string();
    assert_eq!(first_page_items.len(), 1);
    assert_eq!(first_page_items[0]["id"], Value::from(first.id));
    assert_eq!(cursor.as_deref(), Some(expected_cursor.as_str()));

    let second_page = test_app
        .request_with_query(
            Method::Get,
            "/v1/sites",
            &[
                ("limit", "1"),
                ("cursor", cursor.as_deref().expect("cursor should exist")),
            ],
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(second_page.status(), StatusCode::OK);
    assert!(header_string(&second_page, "x-next-cursor").is_none());
    let second_page_items: Vec<Value> = second_page.json().await?;
    assert_eq!(second_page_items.len(), 1);
    assert_eq!(second_page_items[0]["id"], Value::from(second.id));

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn filters_and_paginates_site_checks_over_http() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app.seed_authenticated_client("checks-route").await?;
    let site = test_app
        .seed_site("Checks Site", "https://checks.test.com")
        .await?;
    let monitor = test_app
        .seed_http_monitor(site.id, "https://checks.test.com/health")
        .await?;

    let _first_failure = test_app
        .insert_check(&monitor, false, Some(500), Some("timeout"))
        .await?;
    let _success = test_app
        .insert_check(&monitor, true, Some(200), None)
        .await?;
    let second_failure = test_app
        .insert_check(&monitor, false, Some(503), Some("service unavailable"))
        .await?;

    let first_page = test_app
        .request(
            Method::Get,
            &format!("/v1/sites/{}/checks?limit=1&outcome=failure", site.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(first_page.status(), StatusCode::OK);
    let next_cursor = header_string(&first_page, "x-next-cursor");
    let first_page_items: Vec<Value> = first_page.json().await?;
    assert_eq!(first_page_items.len(), 1);
    assert_eq!(first_page_items[0]["id"], second_failure.id);
    assert_eq!(first_page_items[0]["is_success"], false);
    assert!(next_cursor.is_some());

    let second_page = test_app
        .request_with_query(
            Method::Get,
            &format!("/v1/sites/{}/checks", site.id),
            &[
                ("limit", "1"),
                ("outcome", "failure"),
                (
                    "cursor",
                    next_cursor.as_deref().expect("cursor should exist"),
                ),
            ],
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(second_page.status(), StatusCode::OK);
    assert!(header_string(&second_page, "x-next-cursor").is_none());
    let second_page_items: Vec<Value> = second_page.json().await?;
    assert_eq!(second_page_items.len(), 1);
    assert_eq!(second_page_items[0]["status_code"], 500);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn summary_and_incidents_routes_derive_state_from_check_history() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app
        .seed_authenticated_client("incidents-route")
        .await?;
    let site = test_app
        .seed_site("Incidents Site", "https://incidents.test.com")
        .await?;
    let monitor = test_app
        .seed_http_monitor(site.id, "https://incidents.test.com/health")
        .await?;

    let resolved_start = test_app
        .insert_check(&monitor, false, Some(500), Some("upstream timeout"))
        .await?;
    let resolved_incident = test_app.open_incident(&monitor, &resolved_start).await?;
    let _resolved_middle = test_app
        .insert_check(&monitor, false, Some(500), Some("upstream timeout"))
        .await?;
    let resolved_end = test_app
        .insert_check(&monitor, true, Some(200), None)
        .await?;
    test_app
        .resolve_incident(resolved_incident.id, &resolved_end)
        .await?;
    let open_incident = test_app
        .insert_check(&monitor, false, Some(503), Some("service unavailable"))
        .await?;
    test_app.open_incident(&monitor, &open_incident).await?;

    let summary_response = test_app
        .request(
            Method::Get,
            &format!("/v1/sites/{}/summary?limit=10", site.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(summary_response.status(), StatusCode::OK);
    let summary: Value = summary_response.json().await?;
    assert_eq!(summary["current_state"], "failing");
    assert_eq!(summary["incident_open"], true);
    assert_eq!(summary["recent_checks"]["total_checks"], 4);
    assert_eq!(summary["latest_check"]["id"], open_incident.id);
    assert_eq!(summary["latest_failure"]["id"], open_incident.id);

    let open_incidents_response = test_app
        .request(
            Method::Get,
            &format!("/v1/sites/{}/incidents?status=open", site.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(open_incidents_response.status(), StatusCode::OK);
    let open_incidents: Vec<Value> = open_incidents_response.json().await?;
    assert_eq!(open_incidents.len(), 1);
    assert_eq!(open_incidents[0]["status"], "open");
    assert_eq!(open_incidents[0]["started_check_id"], open_incident.id);

    let resolved_incidents_response = test_app
        .request(
            Method::Get,
            &format!("/v1/sites/{}/incidents?status=resolved", site.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(resolved_incidents_response.status(), StatusCode::OK);
    let resolved_incidents: Vec<Value> = resolved_incidents_response.json().await?;
    assert_eq!(resolved_incidents.len(), 1);
    assert_eq!(resolved_incidents[0]["status"], "resolved");
    assert_eq!(resolved_incidents[0]["started_check_id"], resolved_start.id);
    assert_eq!(resolved_incidents[0]["resolved_check_id"], resolved_end.id);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn admin_can_acknowledge_open_incident() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let role = test_app
        .seed_role_with_permissions("incidents_writer", &["incidents.write"])
        .await?;
    let auth = test_app
        .seed_authenticated_admin_user("incident-ack", &role.key)
        .await?;
    let site = test_app
        .seed_site("Ack Site", "https://ack.test.com")
        .await?;
    let monitor = test_app
        .seed_http_monitor(site.id, "https://ack.test.com/health")
        .await?;
    let failed_check = test_app
        .insert_check(&monitor, false, Some(503), Some("service unavailable"))
        .await?;
    let incident = test_app.open_incident(&monitor, &failed_check).await?;

    let response = test_app
        .request(
            Method::Post,
            &format!(
                "/v1/admin/sites/{}/incidents/{}/acknowledge",
                site.id, incident.id
            ),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = response.json().await?;
    assert_eq!(payload["id"], Value::from(incident.id));
    assert_eq!(payload["started_check_id"], Value::from(failed_check.id));
    assert_eq!(payload["status"], "open");
    assert!(payload["acknowledged_at"].is_string());

    let admin_user =
        admin_users::repository::get_admin_user_by_email(&test_app.pool, "incident-ack@test.com")
            .await?
            .expect("admin user should exist");
    assert_eq!(payload["acknowledged_by"], Value::from(admin_user.id));

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn filters_and_paginates_notification_deliveries_over_http() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app
        .seed_authenticated_client("deliveries-route")
        .await?;
    let site = test_app
        .seed_site("Deliveries Site", "https://deliveries.test.com")
        .await?;
    let monitor = test_app
        .seed_http_monitor(site.id, "https://deliveries.test.com/health")
        .await?;
    let webhook_secret_ciphertext = alon_sentinel::crypto::WebhookSecretEncryptionKey::from_hex(
        "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
    )
    .expect("test key")
    .encrypt_webhook_secret("test-secret")
    .expect("test secret should encrypt");
    let channel = notification_channels::repository::create_channel(
        &test_app.pool,
        &notification_channels::NotificationChannelParams {
            channel_type: NotificationChannelType::Webhook,
            name: "Pager",
            destination: "https://hooks.test.com/alerts",
            webhook_secret_ciphertext: Some(&webhook_secret_ciphertext),
            notify_on_failure: true,
            notify_on_recovery: true,
            is_active: true,
        },
    )
    .await?;

    let failed_check = test_app
        .insert_check(&monitor, false, Some(500), Some("timeout"))
        .await?;
    let recovered_check = test_app
        .insert_check(&monitor, true, Some(200), None)
        .await?;
    let pending_check = test_app
        .insert_check(&monitor, false, Some(503), Some("overloaded"))
        .await?;

    let failed_delivery_id = test_app
        .enqueue_delivery(
            channel.id,
            monitor.id,
            failed_check.id,
            NotificationEventType::Failure,
            json!({"kind": "failed"}),
        )
        .await?;
    let failed_claim = notification_deliveries::repository::claim_due_deliveries(
        &test_app.pool,
        "worker-routes-failed",
        10,
        60,
    )
    .await?;
    assert!(
        failed_claim
            .iter()
            .any(|delivery| delivery.id == failed_delivery_id)
    );
    notification_deliveries::repository::mark_failed(
        &test_app.pool,
        failed_delivery_id,
        "worker-routes-failed",
        "webhook timeout",
        Some(Utc::now() + Duration::minutes(5)),
    )
    .await?;

    let delivered_delivery_id = test_app
        .enqueue_delivery(
            channel.id,
            monitor.id,
            recovered_check.id,
            NotificationEventType::Recovery,
            json!({"kind": "recovered"}),
        )
        .await?;
    let delivered_claim = notification_deliveries::repository::claim_due_deliveries(
        &test_app.pool,
        "worker-routes-delivered",
        10,
        60,
    )
    .await?;
    assert!(
        delivered_claim
            .iter()
            .any(|delivery| delivery.id == delivered_delivery_id)
    );
    notification_deliveries::repository::mark_delivered(
        &test_app.pool,
        delivered_delivery_id,
        "worker-routes-delivered",
    )
    .await?;

    let _pending_delivery_id = test_app
        .enqueue_delivery(
            channel.id,
            monitor.id,
            pending_check.id,
            NotificationEventType::Failure,
            json!({"kind": "pending"}),
        )
        .await?;

    let filtered_response = test_app
        .request(
            Method::Get,
            &format!(
                "/v1/sites/{}/notifications/deliveries?status=failed&event_type=failure",
                site.id
            ),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(filtered_response.status(), StatusCode::OK);
    let filtered_items: Vec<Value> = filtered_response.json().await?;
    assert_eq!(filtered_items.len(), 1);
    assert_eq!(filtered_items[0]["status"], "failed");
    assert_eq!(filtered_items[0]["event_type"], "failure");
    assert!(filtered_items[0].get("notification_channel_id").is_some());
    assert!(
        filtered_items[0]
            .get("account_notification_channel_id")
            .is_none()
    );

    let page_one = test_app
        .request(
            Method::Get,
            &format!("/v1/sites/{}/notifications/deliveries?limit=1", site.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(page_one.status(), StatusCode::OK);
    let cursor = header_string(&page_one, "x-next-cursor");
    let page_one_items: Vec<Value> = page_one.json().await?;
    assert_eq!(page_one_items.len(), 1);
    assert!(cursor.is_some());

    let page_two = test_app
        .request_with_query(
            Method::Get,
            &format!("/v1/sites/{}/notifications/deliveries", site.id),
            &[
                ("limit", "1"),
                ("cursor", cursor.as_deref().expect("cursor should exist")),
            ],
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(page_two.status(), StatusCode::OK);
    let page_two_items: Vec<Value> = page_two.json().await?;
    assert_eq!(page_two_items.len(), 1);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn notification_channel_routes_do_not_expose_internal_ownership_ids() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app
        .seed_authenticated_client("notification-channels-shape")
        .await?;
    let site = test_app
        .seed_site("Channels Site", "https://channels.test.com")
        .await?;

    let create_response = test_app
        .request(Method::Post, "/v1/notifications/channels")
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "channel_type": "webhook",
            "name": "Pager",
            "destination": "https://1.1.1.1/alerts",
            "webhook_secret": TEST_WEBHOOK_SECRET,
            "notify_on_failure": true,
            "notify_on_recovery": true,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created_channel: Value = create_response.json().await?;
    assert!(created_channel.get("account_id").is_none());
    assert_eq!(created_channel["has_webhook_secret"], true);
    assert!(created_channel.get("webhook_secret").is_none());

    let stored_ciphertext: Option<String> = sqlx::query_scalar(
        r#"
        SELECT webhook_secret_ciphertext
        FROM notification_channels
        WHERE id = $1
        "#,
    )
    .bind(
        created_channel["id"]
            .as_i64()
            .expect("created channel should have an id"),
    )
    .fetch_one(&test_app.pool)
    .await?;
    assert!(stored_ciphertext.is_some());
    assert_ne!(stored_ciphertext.as_deref(), Some(TEST_WEBHOOK_SECRET));

    let list_response = test_app
        .request(Method::Get, "/v1/notifications/channels")
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(list_response.status(), StatusCode::OK);
    let channels: Vec<Value> = list_response.json().await?;
    assert_eq!(channels.len(), 1);
    assert!(channels[0].get("account_id").is_none());
    assert_eq!(channels[0]["has_webhook_secret"], true);
    assert!(channels[0].get("webhook_secret").is_none());

    let site_channels_response = test_app
        .request(
            Method::Get,
            &format!("/v1/sites/{}/notifications/channels", site.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(site_channels_response.status(), StatusCode::OK);
    let site_channels: Vec<Value> = site_channels_response.json().await?;
    assert_eq!(site_channels.len(), 1);
    assert!(site_channels[0].get("account_id").is_none());

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn site_notification_channel_override_upsert_requires_both_permissions_regardless_of_existence()
-> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Override Site", "https://override-perms.test.com")
        .await?;
    let channel = notification_channels::repository::create_channel(
        &test_app.pool,
        &notification_channels::NotificationChannelParams {
            channel_type: NotificationChannelType::Email,
            name: "Ops Email",
            destination: "ops@example.com",
            webhook_secret_ciphertext: None,
            notify_on_failure: true,
            notify_on_recovery: true,
            is_active: true,
        },
    )
    .await?;
    let role = test_app
        .seed_role_with_permissions(
            "override_create_only",
            &["site_notification_channel_overrides.create", "sites.read"],
        )
        .await?;
    let auth = test_app
        .seed_authenticated_admin_user("override-create-only", &role.key)
        .await?;

    let absent_response = test_app
        .request(
            Method::Patch,
            &format!(
                "/v1/sites/{}/notifications/channels/{}",
                site.id, channel.id
            ),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "notify_on_failure": false
        }))
        .send()
        .await?;

    assert_eq!(absent_response.status(), StatusCode::FORBIDDEN);
    let absent_payload: Value = absent_response.json().await?;
    assert_eq!(
        absent_payload["error"],
        "missing required permission: site_notification_channel_overrides.update"
    );

    site_notification_channel_overrides::repository::upsert_for_site(
        &test_app.pool,
        site.id,
        &site_notification_channel_overrides::ChannelOverrideParams {
            notification_channel_id: channel.id,
            notify_on_failure: Some(true),
            notify_on_recovery: Some(true),
            is_active: Some(true),
        },
    )
    .await?;

    let existing_response = test_app
        .request(
            Method::Patch,
            &format!(
                "/v1/sites/{}/notifications/channels/{}",
                site.id, channel.id
            ),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "notify_on_failure": false
        }))
        .send()
        .await?;

    assert_eq!(existing_response.status(), StatusCode::FORBIDDEN);
    let existing_payload: Value = existing_response.json().await?;
    assert_eq!(
        existing_payload["error"],
        "missing required permission: site_notification_channel_overrides.update"
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn notification_channel_routes_reject_loopback_webhook_destinations() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app
        .seed_authenticated_client("notification-channel-ssrf")
        .await?;

    let create_response = test_app
        .request(Method::Post, "/v1/notifications/channels")
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "channel_type": "webhook",
            "name": "Internal Pager",
            "destination": "http://127.0.0.1:8080/webhook",
            "webhook_secret": TEST_WEBHOOK_SECRET,
            "notify_on_failure": true,
            "notify_on_recovery": true,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(create_response.status(), StatusCode::BAD_REQUEST);
    let payload: Value = create_response.json().await?;
    assert_eq!(
        payload["error"],
        "webhook URL must not resolve to a loopback, private, or link-local address"
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn admin_user_can_log_in_and_read_sites_over_http() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let _site = test_app
        .seed_site("Admin Site", "https://admin.test.com")
        .await?;
    let auth = test_app
        .seed_authenticated_admin_user("reader", "viewer")
        .await?;

    let me_response = test_app
        .request(Method::Get, "/v1/admin/auth/me")
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(me_response.status(), StatusCode::OK);
    let me_payload: Value = me_response.json().await?;
    assert_eq!(me_payload["user"]["email"], "reader@test.com");
    assert!(
        me_payload["permissions"]
            .as_array()
            .expect("permissions should be an array")
            .iter()
            .any(|value| value.as_str() == Some("sites.read"))
    );

    let sites_response = test_app
        .request(Method::Get, "/v1/sites")
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(sites_response.status(), StatusCode::OK);
    let sites: Vec<Value> = sites_response.json().await?;
    assert_eq!(sites.len(), 1);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn auth_token_endpoint_rate_limits_repeated_attempts_from_same_ip() -> Result<()> {
    let test_app = TestApp::spawn_with_auth_rate_limit(2, 60).await?;
    let client_secret = "correct-secret";
    let secret_hash = AuthService::hash_client_secret(client_secret)?;
    let client = api_auth::repository::create_api_client(
        &test_app.pool,
        api_auth::repository::NewApiClient {
            name: "Rate Limited Client",
            description: Some("Integration test client"),
            client_type: ApiClientType::InstallationClient,
            client_id: "client_rate_limit",
            client_secret_hash: &secret_hash,
            secret_prefix: &client_secret.chars().take(12).collect::<String>(),
            created_by_user_id: Some("integration-tests"),
        },
    )
    .await?;

    for scope in REQUIRED_SCOPES {
        api_auth::repository::create_api_client_scope(&test_app.pool, client.id, scope).await?;
    }

    for _ in 0..2 {
        let response = test_app
            .request(Method::Post, "/v1/auth/token")
            .json(&json!({
                "client_id": client.client_id,
                "client_secret": "wrong-secret",
            }))
            .send()
            .await?;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    let limited_response = test_app
        .request(Method::Post, "/v1/auth/token")
        .json(&json!({
            "client_id": client.client_id,
            "client_secret": "wrong-secret",
        }))
        .send()
        .await?;

    assert_eq!(limited_response.status(), StatusCode::TOO_MANY_REQUESTS);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn revoked_access_token_is_rejected_after_cache_was_warmed() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app.seed_authenticated_client("revoked-cache").await?;
    let _site = test_app
        .seed_site("Cached Site", "https://cached.test.com")
        .await?;

    let warm_response = test_app
        .request(Method::Get, "/v1/sites")
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(warm_response.status(), StatusCode::OK);

    let revoke_response = test_app
        .request(Method::Post, "/v1/auth/revoke")
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "token": auth.access_token,
            "revoked_reason": "integration-test",
        }))
        .send()
        .await?;
    assert_eq!(revoke_response.status(), StatusCode::OK);

    let rejected_response = test_app
        .request(Method::Get, "/v1/sites")
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(rejected_response.status(), StatusCode::UNAUTHORIZED);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn admin_login_endpoint_rate_limits_repeated_attempts_from_same_ip() -> Result<()> {
    let test_app = TestApp::spawn_with_auth_rate_limit(2, 60).await?;
    let password_hash = AuthService::hash_password("correct-password")?;
    admin_users::repository::create_admin_user(
        &test_app.pool,
        admin_users::repository::NewAdminUser {
            email: "rate-limit-admin@test.com",
            display_name: "Rate Limit Admin",
            password_hash: &password_hash,
        },
    )
    .await?;

    for _ in 0..2 {
        let response = test_app
            .request(Method::Post, "/v1/admin/auth/login")
            .json(&json!({
                "email": "rate-limit-admin@test.com",
                "password": "wrong-password",
            }))
            .send()
            .await?;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    let limited_response = test_app
        .request(Method::Post, "/v1/admin/auth/login")
        .json(&json!({
            "email": "rate-limit-admin@test.com",
            "password": "wrong-password",
        }))
        .send()
        .await?;

    assert_eq!(limited_response.status(), StatusCode::TOO_MANY_REQUESTS);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn admin_login_writes_failure_audit_rows() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let password_hash = AuthService::hash_password("correct-password")?;
    let user = admin_users::repository::create_admin_user(
        &test_app.pool,
        admin_users::repository::NewAdminUser {
            email: "audited-admin@test.com",
            display_name: "Audited Admin",
            password_hash: &password_hash,
        },
    )
    .await?;

    let missing_email_response = test_app
        .request(Method::Post, "/v1/admin/auth/login")
        .header("User-Agent", "admin-audit-failure")
        .json(&json!({
            "email": "missing-admin@test.com",
            "password": "wrong-password",
        }))
        .send()
        .await?;
    assert_eq!(missing_email_response.status(), StatusCode::UNAUTHORIZED);

    let wrong_password_response = test_app
        .request(Method::Post, "/v1/admin/auth/login")
        .header("User-Agent", "admin-audit-failure")
        .json(&json!({
            "email": "audited-admin@test.com",
            "password": "wrong-password",
        }))
        .send()
        .await?;
    assert_eq!(wrong_password_response.status(), StatusCode::UNAUTHORIZED);

    let audit_rows = sqlx::query_as::<
        _,
        (
            Option<i64>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(
        r#"
        SELECT
            admin_user_id,
            action,
            host(ip_address) AS ip_address,
            user_agent,
            meta_json->>'email' AS attempted_email
        FROM admin_auth_audit_logs
        ORDER BY id ASC
        "#,
    )
    .fetch_all(&test_app.pool)
    .await?;

    assert_eq!(audit_rows.len(), 2);
    assert_eq!(
        audit_rows[0],
        (
            None,
            "auth_failed".to_string(),
            Some("127.0.0.1".to_string()),
            Some("admin-audit-failure".to_string()),
            Some("missing-admin@test.com".to_string()),
        )
    );
    assert_eq!(
        audit_rows[1],
        (
            Some(user.id),
            "auth_failed".to_string(),
            Some("127.0.0.1".to_string()),
            Some("admin-audit-failure".to_string()),
            Some("audited-admin@test.com".to_string()),
        )
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn admin_login_writes_success_audit_row() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let password_hash = AuthService::hash_password("correct-password")?;
    let user = admin_users::repository::create_admin_user(
        &test_app.pool,
        admin_users::repository::NewAdminUser {
            email: "success-admin@test.com",
            display_name: "Success Admin",
            password_hash: &password_hash,
        },
    )
    .await?;

    let response = test_app
        .request(Method::Post, "/v1/admin/auth/login")
        .header("User-Agent", "admin-audit-success")
        .json(&json!({
            "email": "success-admin@test.com",
            "password": "correct-password",
        }))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    let audit_row = sqlx::query_as::<
        _,
        (
            Option<i64>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(
        r#"
        SELECT
            admin_user_id,
            action,
            host(ip_address) AS ip_address,
            user_agent,
            meta_json->>'email' AS attempted_email
        FROM admin_auth_audit_logs
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .fetch_one(&test_app.pool)
    .await?;

    assert_eq!(
        audit_row,
        (
            Some(user.id),
            "token_issued".to_string(),
            Some("127.0.0.1".to_string()),
            Some("admin-audit-success".to_string()),
            None,
        )
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn viewer_admin_user_is_forbidden_from_writing_sites() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app
        .seed_authenticated_admin_user("viewer", "viewer")
        .await?;

    let create_response = test_app
        .request(Method::Post, "/v1/sites")
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "name": "Blocked",
            "base_url": "https://blocked.test.com",
        }))
        .send()
        .await?;

    assert_eq!(create_response.status(), StatusCode::FORBIDDEN);
    let payload: Value = create_response.json().await?;
    assert_eq!(
        payload["error"],
        "missing required permission: sites.create"
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn revoke_access_token_requires_authenticated_bearer_token() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app
        .seed_authenticated_client("revoke-auth-required")
        .await?;

    let response = test_app
        .request(Method::Post, "/v1/auth/revoke")
        .json(&json!({
            "token": auth.access_token,
            "revoked_reason": "integration-test",
        }))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let payload: Value = response.json().await?;
    assert_eq!(payload["error"], "missing authorization header");

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn revoke_access_token_rejects_revoking_another_clients_token() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth_a = test_app.seed_authenticated_client("revoke-owner-a").await?;
    let auth_b = test_app.seed_authenticated_client("revoke-owner-b").await?;

    let response = test_app
        .request(Method::Post, "/v1/auth/revoke")
        .bearer_auth(&auth_a.access_token)
        .json(&json!({
            "token": auth_b.access_token,
            "revoked_reason": "integration-test",
        }))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let payload: Value = response.json().await?;
    assert_eq!(payload["error"], "can only revoke the current access token");

    let still_valid_response = test_app
        .request(Method::Get, "/v1/sites")
        .bearer_auth(&auth_b.access_token)
        .send()
        .await?;
    assert_eq!(still_valid_response.status(), StatusCode::OK);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_upsert_requires_both_create_and_update_permissions() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Monitored Site", "https://monitor-perms.test.com")
        .await?;
    let role = test_app
        .seed_role_with_permissions(
            "monitor_create_only",
            &["site_monitors.create", "sites.read"],
        )
        .await?;
    let auth = test_app
        .seed_authenticated_admin_user("monitor-create-only", &role.key)
        .await?;

    let response = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/http", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "https://monitor-perms.test.com/health",
            "check_interval_seconds": 60,
            "expected_status_code": 200,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let payload: Value = response.json().await?;
    assert_eq!(
        payload["error"],
        "missing required permission: site_monitors.update"
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_upsert_checks_permissions_before_url_validation() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Monitored Site", "https://monitor-auth-order.test.com")
        .await?;
    let auth = test_app
        .seed_authenticated_admin_user("viewer-monitor-upsert", "viewer")
        .await?;

    let response = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/http", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "not-a-valid-url",
            "check_interval_seconds": 60,
            "expected_status_code": 200,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let payload: Value = response.json().await?;
    assert_eq!(
        payload["error"],
        "missing required permission: site_monitors.create"
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_collection_put_creates_distinct_monitors_per_target_url() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Monitored Site", "https://multi-monitor.test.com")
        .await?;
    let auth = test_app.seed_authenticated_client("http-monitors").await?;

    let first_response = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/http", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "https://1.1.1.1/health",
            "check_interval_seconds": 60,
            "expected_status_code": 200,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(first_response.status(), StatusCode::OK);
    let first_payload: Value = first_response.json().await?;

    let second_response = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/http", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "https://1.1.1.1/ready",
            "check_interval_seconds": 120,
            "expected_status_code": 204,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(second_response.status(), StatusCode::OK);
    let second_payload: Value = second_response.json().await?;

    assert_ne!(first_payload["id"], second_payload["id"]);

    let monitors =
        site_monitors::repository::list_http_monitors_by_site_id(&test_app.pool, site.id).await?;
    assert_eq!(monitors.len(), 2);
    assert!(
        monitors
            .iter()
            .any(|monitor| monitor.target_url == "https://1.1.1.1/health")
    );
    assert!(
        monitors
            .iter()
            .any(|monitor| monitor.target_url == "https://1.1.1.1/ready")
    );

    let get_response = test_app
        .request(
            Method::Get,
            &format!("/v1/sites/{}/monitoring/http", site.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_payload: Value = get_response.json().await?;
    assert_eq!(
        get_payload["http_monitors"]
            .as_array()
            .expect("http_monitors should be an array")
            .len(),
        2
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_patch_updates_target_url_without_creating_duplicate() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Monitored Site", "https://patch-monitor.test.com")
        .await?;
    let monitor = test_app
        .seed_http_monitor(site.id, "https://1.1.1.1/health")
        .await?;
    let auth = test_app
        .seed_authenticated_client("patch-http-monitor")
        .await?;

    let patch_response = test_app
        .request(
            Method::Patch,
            &format!("/v1/sites/{}/monitoring/http/{}", site.id, monitor.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "https://1.1.1.1/ready",
            "check_interval_seconds": 90,
            "expected_status_code": 204,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(patch_response.status(), StatusCode::OK);
    let patch_payload: Value = patch_response.json().await?;
    assert_eq!(patch_payload["id"], Value::from(monitor.id));
    assert_eq!(
        patch_payload["target_url"],
        Value::from("https://1.1.1.1/ready")
    );

    let monitors =
        site_monitors::repository::list_http_monitors_by_site_id(&test_app.pool, site.id).await?;
    assert_eq!(monitors.len(), 1);
    assert_eq!(monitors[0].id, monitor.id);
    assert_eq!(monitors[0].target_url, "https://1.1.1.1/ready");
    assert_eq!(monitors[0].check_interval_seconds, 90);
    assert_eq!(monitors[0].expected_status_code, 204);
    assert!(
        site_monitors::repository::get_http_monitor_by_site_id_and_target_url(
            &test_app.pool,
            site.id,
            "https://1.1.1.1/health",
        )
        .await?
        .is_none()
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_routes_persist_http_assertions() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Assertion Site", "https://assertions.test.com")
        .await?;
    let auth = test_app
        .seed_authenticated_client("http-monitor-assertions")
        .await?;

    let create_response = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/http", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "http://1.1.1.1/health",
            "check_interval_seconds": 60,
            "expected_status_code": 200,
            "body_must_contain": "healthy",
            "body_must_not_contain": "error",
            "body_must_contain_texts": ["healthy", "ok"],
            "body_must_not_contain_texts": ["error", "panic"],
            "json_path_exists": ["$.status", "$.checks[0].healthy"],
            "json_path_equals": [
                { "path": "$.status", "value": "ok" }
            ],
            "json_path_not_equals": [
                { "path": "$.checks[0].healthy", "value": false }
            ],
            "max_response_time_ms": 1500,
            "required_header_name": "x-health",
            "required_header_value": "healthy",
            "header_assertions": [
                { "name": "x-health" },
                { "name": "x-health", "equals": "healthy" },
                { "name": "cache-control", "contains": "no-" }
            ],
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(create_response.status(), StatusCode::OK);
    let create_payload: Value = create_response.json().await?;
    assert_eq!(create_payload["body_must_contain"], "healthy");
    assert_eq!(create_payload["body_must_not_contain"], "error");
    assert_eq!(
        create_payload["body_must_contain_texts"],
        json!(["healthy", "ok"])
    );
    assert_eq!(
        create_payload["body_must_not_contain_texts"],
        json!(["error", "panic"])
    );
    assert_eq!(
        create_payload["json_path_exists"],
        json!(["$.status", "$.checks[0].healthy"])
    );
    assert_eq!(
        create_payload["json_path_equals"],
        json!([{ "path": "$.status", "value": "ok" }])
    );
    assert_eq!(
        create_payload["json_path_not_equals"],
        json!([{ "path": "$.checks[0].healthy", "value": false }])
    );
    assert_eq!(create_payload["max_response_time_ms"], 1500);
    assert_eq!(create_payload["required_header_name"], "x-health");
    assert_eq!(create_payload["required_header_value"], "healthy");
    assert_eq!(
        create_payload["header_assertions"],
        json!([
            { "name": "x-health", "equals": null, "contains": null },
            { "name": "x-health", "equals": "healthy", "contains": null },
            { "name": "cache-control", "equals": null, "contains": "no-" }
        ])
    );

    let monitor_id = create_payload["id"]
        .as_i64()
        .expect("created monitor id should be present");

    let patch_response = test_app
        .request(
            Method::Patch,
            &format!("/v1/sites/{}/monitoring/http/{}", site.id, monitor_id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "http://1.1.1.1/ready",
            "check_interval_seconds": 60,
            "expected_status_code": 204,
            "body_must_contain": "ready",
            "body_must_not_contain": "maintenance",
            "body_must_contain_texts": ["ready", "steady"],
            "body_must_not_contain_texts": ["maintenance", "outage"],
            "json_path_exists": ["$.state", "$.results[0].name"],
            "json_path_equals": [
                { "path": "$.state", "value": "ready" }
            ],
            "json_path_not_equals": [
                { "path": "$.results[0].name", "value": "failed" }
            ],
            "max_response_time_ms": 900,
            "required_header_name": "x-ready",
            "required_header_value": "yes",
            "header_assertions": [
                { "name": "x-ready" },
                { "name": "x-ready", "equals": "yes" },
                { "name": "cache-control", "contains": "max-age" }
            ],
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(patch_response.status(), StatusCode::OK);
    let patch_payload: Value = patch_response.json().await?;
    assert_eq!(patch_payload["body_must_contain"], "ready");
    assert_eq!(patch_payload["body_must_not_contain"], "maintenance");
    assert_eq!(
        patch_payload["body_must_contain_texts"],
        json!(["ready", "steady"])
    );
    assert_eq!(
        patch_payload["body_must_not_contain_texts"],
        json!(["maintenance", "outage"])
    );
    assert_eq!(
        patch_payload["json_path_exists"],
        json!(["$.state", "$.results[0].name"])
    );
    assert_eq!(
        patch_payload["json_path_equals"],
        json!([{ "path": "$.state", "value": "ready" }])
    );
    assert_eq!(
        patch_payload["json_path_not_equals"],
        json!([{ "path": "$.results[0].name", "value": "failed" }])
    );
    assert_eq!(patch_payload["max_response_time_ms"], 900);
    assert_eq!(patch_payload["required_header_name"], "x-ready");
    assert_eq!(patch_payload["required_header_value"], "yes");
    assert_eq!(
        patch_payload["header_assertions"],
        json!([
            { "name": "x-ready", "equals": null, "contains": null },
            { "name": "x-ready", "equals": "yes", "contains": null },
            { "name": "cache-control", "equals": null, "contains": "max-age" }
        ])
    );

    let monitors =
        site_monitors::repository::list_http_monitors_by_site_id(&test_app.pool, site.id).await?;
    assert_eq!(monitors.len(), 1);
    assert_eq!(monitors[0].body_must_contain.as_deref(), Some("ready"));
    assert_eq!(
        monitors[0].body_must_not_contain.as_deref(),
        Some("maintenance")
    );
    assert_eq!(
        monitors[0].body_must_contain_texts.as_deref(),
        Some(&["ready".to_string(), "steady".to_string()][..])
    );
    assert_eq!(
        monitors[0].body_must_not_contain_texts.as_deref(),
        Some(&["maintenance".to_string(), "outage".to_string()][..])
    );
    assert_eq!(
        monitors[0].json_path_exists.as_deref(),
        Some(&["$.state".to_string(), "$.results[0].name".to_string()][..])
    );
    assert_eq!(
        monitors[0]
            .json_path_equals
            .as_ref()
            .map(|assertions| assertions.0.as_slice()),
        Some(
            &[site_monitors::JsonPathValueAssertion {
                path: "$.state".to_string(),
                value: json!("ready"),
            }][..]
        )
    );
    assert_eq!(
        monitors[0]
            .json_path_not_equals
            .as_ref()
            .map(|assertions| assertions.0.as_slice()),
        Some(
            &[site_monitors::JsonPathValueAssertion {
                path: "$.results[0].name".to_string(),
                value: json!("failed"),
            }][..]
        )
    );
    assert_eq!(monitors[0].max_response_time_ms, Some(900));
    assert_eq!(monitors[0].required_header_name.as_deref(), Some("x-ready"));
    assert_eq!(monitors[0].required_header_value.as_deref(), Some("yes"));
    assert_eq!(
        monitors[0]
            .header_assertions
            .as_ref()
            .map(|assertions| assertions.0.as_slice()),
        Some(
            &[
                site_monitors::HttpHeaderAssertion {
                    name: "x-ready".to_string(),
                    equals: None,
                    contains: None,
                },
                site_monitors::HttpHeaderAssertion {
                    name: "x-ready".to_string(),
                    equals: Some("yes".to_string()),
                    contains: None,
                },
                site_monitors::HttpHeaderAssertion {
                    name: "cache-control".to_string(),
                    equals: None,
                    contains: Some("max-age".to_string()),
                },
            ][..]
        )
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_routes_persist_ssl_certificate_settings() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site(
            "HTTP SSL Assertion Site",
            "https://http-ssl-assertions.test.com",
        )
        .await?;
    let auth = test_app
        .seed_authenticated_client("http-monitor-ssl-certs")
        .await?;

    let create_response = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/http", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "https://1.1.1.1/health",
            "check_interval_seconds": 60,
            "expected_status_code": 200,
            "ssl_certificate_checks_enabled": true,
            "ssl_expiry_warning_days": 21,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(create_response.status(), StatusCode::OK);
    let create_payload: Value = create_response.json().await?;
    assert_eq!(create_payload["monitor_type"], "http");
    assert_eq!(create_payload["ssl_certificate_checks_enabled"], true);
    assert_eq!(create_payload["ssl_expiry_warning_days"], 21);

    let monitor_id = create_payload["id"]
        .as_i64()
        .expect("created monitor id should be present");

    let patch_response = test_app
        .request(
            Method::Patch,
            &format!("/v1/sites/{}/monitoring/http/{}", site.id, monitor_id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "https://1.1.1.1/ready",
            "check_interval_seconds": 60,
            "expected_status_code": 204,
            "ssl_certificate_checks_enabled": true,
            "ssl_expiry_warning_days": 30,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(patch_response.status(), StatusCode::OK);
    let patch_payload: Value = patch_response.json().await?;
    assert_eq!(patch_payload["monitor_type"], "http");
    assert_eq!(patch_payload["ssl_certificate_checks_enabled"], true);
    assert_eq!(patch_payload["ssl_expiry_warning_days"], 30);

    let monitors =
        site_monitors::repository::list_http_monitors_by_site_id(&test_app.pool, site.id).await?;
    assert_eq!(monitors.len(), 1);
    assert!(monitors[0].ssl_certificate_checks_enabled);
    assert_eq!(monitors[0].ssl_expiry_warning_days, Some(30));

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn ssl_monitor_routes_persist_ssl_certificate_settings() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("SSL Assertion Site", "https://ssl-assertions.test.com")
        .await?;
    let auth = test_app
        .seed_authenticated_client("http-monitor-ssl-assertions")
        .await?;

    let create_response = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/ssl", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "https://1.1.1.1/health",
            "check_interval_seconds": 60,
            "ssl_expiry_warning_days": 21,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(create_response.status(), StatusCode::OK);
    let create_payload: Value = create_response.json().await?;
    assert_eq!(create_payload["monitor_type"], "ssl");
    assert_eq!(create_payload["ssl_expiry_warning_days"], 21);

    let monitor_id = create_payload["id"]
        .as_i64()
        .expect("created monitor id should be present");

    let patch_response = test_app
        .request(
            Method::Patch,
            &format!("/v1/sites/{}/monitoring/ssl/{}", site.id, monitor_id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "https://1.1.1.1/ready",
            "check_interval_seconds": 60,
            "ssl_expiry_warning_days": 30,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(patch_response.status(), StatusCode::OK);
    let patch_payload: Value = patch_response.json().await?;
    assert_eq!(patch_payload["monitor_type"], "ssl");
    assert_eq!(patch_payload["ssl_expiry_warning_days"], 30);

    let monitors =
        site_monitors::repository::list_ssl_monitors_by_site_id(&test_app.pool, site.id).await?;
    assert_eq!(monitors.len(), 1);
    assert!(monitors[0].ssl_certificate_checks_enabled);
    assert_eq!(monitors[0].ssl_expiry_warning_days, Some(30));

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn ssl_monitor_routes_reject_non_https_targets() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("SSL Validation Site", "https://ssl-validation.test.com")
        .await?;
    let auth = test_app
        .seed_authenticated_client("http-monitor-ssl-validation")
        .await?;

    let response = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/ssl", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "http://1.1.1.1/health",
            "check_interval_seconds": 60,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: Value = response.json().await?;
    assert_eq!(
        payload["error"],
        "ssl_certificate_checks_enabled requires an https target_url"
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn ssl_monitor_pause_and_resume_toggle_monitor_state() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("SSL Pause Site", "https://pause-ssl-monitor.test.com")
        .await?;
    let monitor = test_app
        .seed_ssl_monitor(site.id, "https://1.1.1.1/health")
        .await?;
    let auth = test_app
        .seed_authenticated_client("pause-ssl-monitor")
        .await?;

    let pause_response = test_app
        .request(
            Method::Post,
            &format!("/v1/sites/{}/monitoring/ssl/{}/pause", site.id, monitor.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(pause_response.status(), StatusCode::OK);
    let paused_payload: Value = pause_response.json().await?;
    assert_eq!(paused_payload["is_active"], false);

    let resume_response = test_app
        .request(
            Method::Post,
            &format!("/v1/sites/{}/monitoring/ssl/{}/resume", site.id, monitor.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(resume_response.status(), StatusCode::OK);
    let resumed_payload: Value = resume_response.json().await?;
    assert_eq!(resumed_payload["is_active"], true);

    let monitors =
        site_monitors::repository::list_ssl_monitors_by_site_id(&test_app.pool, site.id).await?;
    assert_eq!(monitors.len(), 1);
    assert!(monitors[0].is_active);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn heartbeat_monitor_routes_create_and_update_monitor() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Heartbeat Site", "https://heartbeat-monitor.test.com")
        .await?;
    let auth = test_app
        .seed_authenticated_client("heartbeat-monitor-routes")
        .await?;

    let create_response = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/heartbeat", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "check_interval_seconds": 60,
            "heartbeat_grace_seconds": 15,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(create_response.status(), StatusCode::OK);
    let create_payload: Value = create_response.json().await?;
    assert_eq!(create_payload["monitor_type"], "heartbeat");
    assert_eq!(create_payload["heartbeat_grace_seconds"], 15);
    assert!(
        create_payload["ping_path"]
            .as_str()
            .expect("heartbeat ping path should be present")
            .starts_with("/v1/heartbeat/")
    );

    let monitor_id = create_payload["id"]
        .as_i64()
        .expect("created heartbeat monitor id should be present");
    let original_ping_path = create_payload["ping_path"].clone();

    let patch_response = test_app
        .request(
            Method::Patch,
            &format!("/v1/sites/{}/monitoring/heartbeat/{}", site.id, monitor_id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "check_interval_seconds": 90,
            "heartbeat_grace_seconds": 30,
            "is_active": false
        }))
        .send()
        .await?;

    assert_eq!(patch_response.status(), StatusCode::OK);
    let patch_payload: Value = patch_response.json().await?;
    assert_eq!(patch_payload["monitor_type"], "heartbeat");
    assert_eq!(patch_payload["heartbeat_grace_seconds"], 30);
    assert_eq!(patch_payload["is_active"], false);
    assert_eq!(patch_payload["ping_path"], original_ping_path);

    let monitors =
        site_monitors::repository::list_heartbeat_monitors_by_site_id(&test_app.pool, site.id)
            .await?;
    assert_eq!(monitors.len(), 1);
    assert_eq!(monitors[0].check_interval_seconds, 90);
    assert_eq!(monitors[0].heartbeat_grace_seconds, Some(30));
    assert!(!monitors[0].is_active);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn heartbeat_ping_endpoint_updates_last_heartbeat_timestamp() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Heartbeat Ping Site", "https://heartbeat-ping.test.com")
        .await?;
    let monitor = test_app
        .seed_heartbeat_monitor(site.id, "heartbeat-test-token")
        .await?;

    assert!(monitor.last_heartbeat_received_at.is_none());

    let response = test_app
        .request(Method::Post, "/v1/heartbeat/heartbeat-test-token")
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let updated_monitor =
        site_monitors::repository::get_heartbeat_monitor_by_site_id(&test_app.pool, site.id)
            .await?
            .expect("heartbeat monitor should still exist");
    assert!(updated_monitor.last_heartbeat_received_at.is_some());

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_patch_checks_permissions_before_url_validation() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Monitored Site", "https://patch-auth-order.test.com")
        .await?;
    let monitor = test_app
        .seed_http_monitor(site.id, "https://patch-auth-order.test.com/health")
        .await?;
    let auth = test_app
        .seed_authenticated_admin_user("viewer-monitor-patch", "viewer")
        .await?;

    let response = test_app
        .request(
            Method::Patch,
            &format!("/v1/sites/{}/monitoring/http/{}", site.id, monitor.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "not-a-valid-url",
            "check_interval_seconds": 60,
            "expected_status_code": 200,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let payload: Value = response.json().await?;
    assert_eq!(
        payload["error"],
        "missing required permission: site_monitors.update"
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_patch_rejects_target_url_owned_by_another_monitor() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Monitored Site", "https://duplicate-monitor.test.com")
        .await?;
    let first_monitor = test_app
        .seed_http_monitor(site.id, "https://1.1.1.1/health")
        .await?;
    let second_monitor = test_app
        .seed_http_monitor(site.id, "https://1.1.1.1/ready")
        .await?;
    let auth = test_app
        .seed_authenticated_client("duplicate-http-monitor")
        .await?;

    let patch_response = test_app
        .request(
            Method::Patch,
            &format!("/v1/sites/{}/monitoring/http/{}", site.id, first_monitor.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": second_monitor.target_url,
            "check_interval_seconds": 60,
            "expected_status_code": 200,
            "is_active": true
        }))
        .send()
        .await?;

    assert_eq!(patch_response.status(), StatusCode::BAD_REQUEST);
    let payload: Value = patch_response.json().await?;
    assert_eq!(
        payload["error"],
        "target_url already exists for another http monitor on this site"
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_delete_by_id_disables_only_the_target_monitor() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Monitored Site", "https://delete-monitor.test.com")
        .await?;
    let first_monitor = test_app
        .seed_http_monitor(site.id, "https://delete-monitor.test.com/health")
        .await?;
    let second_monitor = test_app
        .seed_http_monitor(site.id, "https://delete-monitor.test.com/ready")
        .await?;
    let auth = test_app
        .seed_authenticated_client("delete-http-monitor")
        .await?;

    let delete_response = test_app
        .request(
            Method::Delete,
            &format!("/v1/sites/{}/monitoring/http/{}", site.id, first_monitor.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(delete_response.status(), StatusCode::OK);
    let payload: Value = delete_response.json().await?;
    assert_eq!(payload["disabled"], Value::from(true));

    let monitors =
        site_monitors::repository::list_http_monitors_by_site_id(&test_app.pool, site.id).await?;
    assert_eq!(monitors.len(), 2);
    assert_eq!(
        monitors
            .iter()
            .find(|monitor| monitor.id == first_monitor.id)
            .expect("first monitor should still exist")
            .is_active,
        false
    );
    assert_eq!(
        monitors
            .iter()
            .find(|monitor| monitor.id == second_monitor.id)
            .expect("second monitor should still exist")
            .is_active,
        true
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_pause_and_resume_toggle_monitor_state() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Pause Site", "https://pause-monitor.test.com")
        .await?;
    let monitor = test_app
        .seed_http_monitor(site.id, "https://1.1.1.1/health")
        .await?;
    let failed_check = test_app
        .insert_check(&monitor, false, Some(503), Some("service unavailable"))
        .await?;
    let incident = test_app.open_incident(&monitor, &failed_check).await?;
    let auth = test_app
        .seed_authenticated_client("pause-http-monitor")
        .await?;

    let pause_response = test_app
        .request(
            Method::Post,
            &format!("/v1/sites/{}/monitoring/http/{}/pause", site.id, monitor.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(pause_response.status(), StatusCode::OK);
    let paused_payload: Value = pause_response.json().await?;
    assert_eq!(paused_payload["is_active"], false);

    let paused_monitor = site_monitors::repository::get_http_monitor_by_site_id_and_target_url(
        &test_app.pool,
        site.id,
        "https://1.1.1.1/health",
    )
    .await?
    .expect("http monitor should still exist");
    assert!(!paused_monitor.is_active);

    let resolved_incident = site_monitor_incidents::repository::get_incident_by_id_and_site_id(
        &test_app.pool,
        incident.id,
        site.id,
    )
    .await?
    .expect("incident should still exist");
    assert_eq!(
        resolved_incident.status,
        site_monitor_incidents::SiteMonitorIncidentStatus::Resolved
    );

    let resume_response = test_app
        .request(
            Method::Post,
            &format!(
                "/v1/sites/{}/monitoring/http/{}/resume",
                site.id, monitor.id
            ),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(resume_response.status(), StatusCode::OK);
    let resumed_payload: Value = resume_response.json().await?;
    assert_eq!(resumed_payload["is_active"], true);

    let resumed_monitor = site_monitors::repository::get_http_monitor_by_site_id_and_target_url(
        &test_app.pool,
        site.id,
        "https://1.1.1.1/health",
    )
    .await?
    .expect("http monitor should still exist");
    assert!(resumed_monitor.is_active);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn health_live_and_ready_endpoints_return_ok() -> Result<()> {
    let test_app = TestApp::spawn().await?;

    let live = test_app.request(Method::Get, "/live").send().await?;
    assert_eq!(live.status(), StatusCode::OK);
    let live_body: Value = live.json().await?;
    assert_eq!(live_body["status"], "ok");

    let health = test_app.request(Method::Get, "/health").send().await?;
    assert_eq!(health.status(), StatusCode::OK);

    let ready = test_app.request(Method::Get, "/ready").send().await?;
    assert_eq!(ready.status(), StatusCode::OK);
    let ready_body: Value = ready.json().await?;
    assert_eq!(ready_body["status"], "ok");

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn metrics_endpoint_returns_prometheus_text() -> Result<()> {
    let test_app = TestApp::spawn().await?;

    let response = test_app.request(Method::Get, "/metrics").send().await?;

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.starts_with("text/plain"),
        "expected text/plain content-type, got: {content_type}"
    );
    let body = response.text().await?;
    assert!(
        body.contains("alon_sentinel_db_pool_connections_max"),
        "expected prometheus metric in body"
    );

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn notification_channel_crud_and_validation() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app
        .seed_authenticated_client("notif-channel-crud")
        .await?;

    let bad_name = test_app
        .request(Method::Post, "/v1/notifications/channels")
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "channel_type": "email",
            "name": "   ",
            "destination": "ops@example.com",
            "notify_on_failure": true,
            "notify_on_recovery": true,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(bad_name.status(), StatusCode::BAD_REQUEST);
    let bad_name_body: Value = bad_name.json().await?;
    assert_eq!(bad_name_body["error"], "name is required");

    let no_events = test_app
        .request(Method::Post, "/v1/notifications/channels")
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "channel_type": "email",
            "name": "Ops",
            "destination": "ops@example.com",
            "notify_on_failure": false,
            "notify_on_recovery": false,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(no_events.status(), StatusCode::BAD_REQUEST);
    let no_events_body: Value = no_events.json().await?;
    assert_eq!(
        no_events_body["error"],
        "at least one notification event must be enabled"
    );

    let created = test_app
        .request(Method::Post, "/v1/notifications/channels")
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "channel_type": "email",
            "name": "Ops Email",
            "destination": "ops@example.com",
            "notify_on_failure": true,
            "notify_on_recovery": false,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let channel: Value = created.json().await?;
    let channel_id = channel["id"]
        .as_i64()
        .expect("channel id should be present");
    assert_eq!(channel["name"], "Ops Email");
    assert_eq!(channel["channel_type"], "email");
    assert_eq!(channel["notify_on_failure"], true);
    assert_eq!(channel["notify_on_recovery"], false);

    let updated = test_app
        .request(
            Method::Patch,
            &format!("/v1/notifications/channels/{channel_id}"),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "channel_type": "email",
            "name": "Primary Ops",
            "destination": "ops@example.com",
            "notify_on_failure": true,
            "notify_on_recovery": true,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(updated.status(), StatusCode::OK);
    let updated_channel: Value = updated.json().await?;
    assert_eq!(updated_channel["name"], "Primary Ops");
    assert_eq!(updated_channel["notify_on_recovery"], true);

    let deleted = test_app
        .request(
            Method::Delete,
            &format!("/v1/notifications/channels/{channel_id}"),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(deleted.status(), StatusCode::OK);
    let deleted_body: Value = deleted.json().await?;
    assert_eq!(deleted_body["deleted"], true);

    let not_found = test_app
        .request(
            Method::Delete,
            &format!("/v1/notifications/channels/{channel_id}"),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(not_found.status(), StatusCode::NOT_FOUND);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_rejects_invalid_interval_and_status_code() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Validation Site", "https://validation.test.com")
        .await?;
    let auth = test_app
        .seed_authenticated_client("http-monitor-validation")
        .await?;

    let short_interval = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/http", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "https://1.1.1.1/health",
            "check_interval_seconds": 29,
            "expected_status_code": 200,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(short_interval.status(), StatusCode::BAD_REQUEST);
    let short_body: Value = short_interval.json().await?;
    assert_eq!(
        short_body["error"],
        "check_interval_seconds must be at least 30"
    );

    let bad_status = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/http", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "https://1.1.1.1/health",
            "check_interval_seconds": 60,
            "expected_status_code": 99,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(bad_status.status(), StatusCode::BAD_REQUEST);
    let bad_status_body: Value = bad_status.json().await?;
    assert_eq!(
        bad_status_body["error"],
        "expected_status_code must be between 100 and 599"
    );

    let bad_status_high = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/http", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_url": "https://1.1.1.1/health",
            "check_interval_seconds": 60,
            "expected_status_code": 600,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(bad_status_high.status(), StatusCode::BAD_REQUEST);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn http_monitor_pause_and_resume_toggle_is_active_flag() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Pause Site", "https://pause-http-monitor.test.com")
        .await?;
    let monitor = test_app
        .seed_http_monitor(site.id, "https://1.1.1.1/health")
        .await?;
    let auth = test_app
        .seed_authenticated_client("pause-http-monitor")
        .await?;

    let pause = test_app
        .request(
            Method::Post,
            &format!("/v1/sites/{}/monitoring/http/{}/pause", site.id, monitor.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(pause.status(), StatusCode::OK);
    let paused: Value = pause.json().await?;
    assert_eq!(paused["is_active"], false);

    let resume = test_app
        .request(
            Method::Post,
            &format!(
                "/v1/sites/{}/monitoring/http/{}/resume",
                site.id, monitor.id
            ),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(resume.status(), StatusCode::OK);
    let resumed: Value = resume.json().await?;
    assert_eq!(resumed["is_active"], true);

    let monitors =
        site_monitors::repository::list_http_monitors_by_site_id(&test_app.pool, site.id).await?;
    assert_eq!(monitors.len(), 1);
    assert!(monitors[0].is_active);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn tcp_monitor_routes_create_update_and_list() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("TCP Site", "https://tcp-monitor.test.com")
        .await?;
    let auth = test_app
        .seed_authenticated_client("tcp-monitor-routes")
        .await?;

    let create = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/tcp", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_host": "1.1.1.1",
            "target_port": 443,
            "check_interval_seconds": 60,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(create.status(), StatusCode::OK);
    let created: Value = create.json().await?;
    assert_eq!(created["monitor_type"], "tcp");
    assert_eq!(created["target_host"], "1.1.1.1");
    assert_eq!(created["target_port"], 443);
    let monitor_id = created["id"]
        .as_i64()
        .expect("tcp monitor id should be present");

    let get = test_app
        .request(
            Method::Get,
            &format!("/v1/sites/{}/monitoring/tcp", site.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(get.status(), StatusCode::OK);
    let get_body: Value = get.json().await?;
    assert_eq!(
        get_body["tcp_monitors"]
            .as_array()
            .expect("tcp_monitors should be an array")
            .len(),
        1
    );

    let patch = test_app
        .request(
            Method::Patch,
            &format!("/v1/sites/{}/monitoring/tcp/{}", site.id, monitor_id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_host": "1.1.1.1",
            "target_port": 80,
            "check_interval_seconds": 120,
            "is_active": false
        }))
        .send()
        .await?;
    assert_eq!(patch.status(), StatusCode::OK);
    let patched: Value = patch.json().await?;
    assert_eq!(patched["target_port"], 80);
    assert_eq!(patched["check_interval_seconds"], 120);
    assert_eq!(patched["is_active"], false);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn dns_monitor_routes_create_update_and_list() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("DNS Site", "https://dns-monitor.test.com")
        .await?;
    let auth = test_app
        .seed_authenticated_client("dns-monitor-routes")
        .await?;

    let create = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/dns", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "hostname": "example.com",
            "record_type": "A",
            "expected_value": "1.1.1.1",
            "check_interval_seconds": 60,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(create.status(), StatusCode::OK);
    let created: Value = create.json().await?;
    assert_eq!(created["monitor_type"], "dns");
    assert_eq!(created["hostname"], "example.com");
    assert_eq!(created["record_type"], "A");
    assert_eq!(created["expected_value"], "1.1.1.1");
    let monitor_id = created["id"]
        .as_i64()
        .expect("dns monitor id should be present");

    let get = test_app
        .request(
            Method::Get,
            &format!("/v1/sites/{}/monitoring/dns", site.id),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(get.status(), StatusCode::OK);
    let get_body: Value = get.json().await?;
    assert_eq!(
        get_body["dns_monitors"]
            .as_array()
            .expect("dns_monitors should be an array")
            .len(),
        1
    );

    let patch = test_app
        .request(
            Method::Patch,
            &format!("/v1/sites/{}/monitoring/dns/{}", site.id, monitor_id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "hostname": "example.com",
            "record_type": "AAAA",
            "check_interval_seconds": 120,
            "is_active": false
        }))
        .send()
        .await?;
    assert_eq!(patch.status(), StatusCode::OK);
    let patched: Value = patch.json().await?;
    assert_eq!(patched["record_type"], "AAAA");
    assert_eq!(patched["check_interval_seconds"], 120);
    assert_eq!(patched["is_active"], false);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn site_uptime_endpoint_returns_daily_buckets() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let auth = test_app.seed_authenticated_client("uptime-route").await?;
    let site = test_app
        .seed_site("Uptime Site", "https://uptime.test.com")
        .await?;

    let response = test_app
        .request(Method::Get, &format!("/v1/sites/{}/uptime/daily", site.id))
        .bearer_auth(&auth.access_token)
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await?;
    assert!(body["days"].is_number());
    assert!(body["buckets"].is_array());

    let days_param = test_app
        .request_with_query(
            Method::Get,
            &format!("/v1/sites/{}/uptime/daily", site.id),
            &[("days", "30")],
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(days_param.status(), StatusCode::OK);
    let days_body: Value = days_param.json().await?;
    assert_eq!(days_body["days"], 30);

    let not_found = test_app
        .request(Method::Get, "/v1/sites/999999/uptime/daily")
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(not_found.status(), StatusCode::NOT_FOUND);

    test_app.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn tcp_and_dns_monitor_pause_and_resume() -> Result<()> {
    let test_app = TestApp::spawn().await?;
    let site = test_app
        .seed_site("Pause Monitor Site", "https://pause-monitors.test.com")
        .await?;
    let auth = test_app
        .seed_authenticated_client("pause-tcp-dns-monitors")
        .await?;

    let tcp = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/tcp", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "target_host": "1.1.1.1",
            "target_port": 53,
            "check_interval_seconds": 60,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(tcp.status(), StatusCode::OK);
    let tcp_monitor_id = tcp.json::<Value>().await?["id"]
        .as_i64()
        .expect("tcp monitor id");

    let pause_tcp = test_app
        .request(
            Method::Post,
            &format!(
                "/v1/sites/{}/monitoring/tcp/{}/pause",
                site.id, tcp_monitor_id
            ),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(pause_tcp.status(), StatusCode::OK);
    assert_eq!(pause_tcp.json::<Value>().await?["is_active"], false);

    let resume_tcp = test_app
        .request(
            Method::Post,
            &format!(
                "/v1/sites/{}/monitoring/tcp/{}/resume",
                site.id, tcp_monitor_id
            ),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(resume_tcp.status(), StatusCode::OK);
    assert_eq!(resume_tcp.json::<Value>().await?["is_active"], true);

    let dns = test_app
        .request(
            Method::Put,
            &format!("/v1/sites/{}/monitoring/dns", site.id),
        )
        .bearer_auth(&auth.access_token)
        .json(&json!({
            "hostname": "example.com",
            "record_type": "A",
            "check_interval_seconds": 60,
            "is_active": true
        }))
        .send()
        .await?;
    assert_eq!(dns.status(), StatusCode::OK);
    let dns_monitor_id = dns.json::<Value>().await?["id"]
        .as_i64()
        .expect("dns monitor id");

    let pause_dns = test_app
        .request(
            Method::Post,
            &format!(
                "/v1/sites/{}/monitoring/dns/{}/pause",
                site.id, dns_monitor_id
            ),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(pause_dns.status(), StatusCode::OK);
    assert_eq!(pause_dns.json::<Value>().await?["is_active"], false);

    let resume_dns = test_app
        .request(
            Method::Post,
            &format!(
                "/v1/sites/{}/monitoring/dns/{}/resume",
                site.id, dns_monitor_id
            ),
        )
        .bearer_auth(&auth.access_token)
        .send()
        .await?;
    assert_eq!(resume_dns.status(), StatusCode::OK);
    assert_eq!(resume_dns.json::<Value>().await?["is_active"], true);

    test_app.cleanup().await?;
    Ok(())
}

#[derive(Clone, Copy)]
enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl From<Method> for reqwest::Method {
    fn from(method: Method) -> Self {
        match method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Patch => reqwest::Method::PATCH,
            Method::Delete => reqwest::Method::DELETE,
        }
    }
}

struct AuthFixture {
    access_token: String,
}

struct TestApp {
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
    base_url: String,
    http: reqwest::Client,
    server_task: JoinHandle<()>,
}

impl TestApp {
    async fn spawn() -> Result<Self> {
        Self::spawn_with_auth_rate_limit(10, 60).await
    }

    async fn spawn_with_auth_rate_limit(
        auth_rate_limit_max_requests: usize,
        auth_rate_limit_window_seconds: usize,
    ) -> Result<Self> {
        dotenvy::dotenv().ok();

        let base_database_url = std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .context("set TEST_DATABASE_URL or DATABASE_URL for integration tests")?;
        let admin_pool = PgPool::connect(&base_database_url).await?;
        let schema = unique_schema_name();
        admin_pool
            .execute(sqlx::query(sqlx::AssertSqlSafe(format!(
                "CREATE SCHEMA {}",
                schema
            ))))
            .await?;

        let schema_database_url = schema_database_url(&base_database_url, &schema)?;
        let pool = PgPool::connect(&schema_database_url).await?;
        apply_migrations(&pool).await?;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = build_router(AppState {
            pool: pool.clone(),
            auth_config: AuthConfig::default(),
            auth_rate_limiter: AuthRateLimiter::new(
                auth_rate_limit_max_requests,
                std::time::Duration::from_secs(auth_rate_limit_window_seconds as u64),
            ),
            auth_token_cache: AuthTokenCache::new(),
            trust_proxy_headers: false,
            trusted_proxy_ips: Vec::new(),
            webhook_secret_encryption_key:
                alon_sentinel::crypto::WebhookSecretEncryptionKey::from_hex(
                    "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
                )
                .expect("test webhook secret key should parse"),
            cookie_secure: false,
            http_monitor_allow_private_targets: false,
            db_max_connections: 5,
            public_rate_limiter: AuthRateLimiter::new(100, std::time::Duration::from_secs(60)),
            status_page_cache: StatusPageCache::new(10, std::time::Duration::from_secs(60)),
        });

        let server_task = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .expect("test server should start");
        });

        Ok(Self {
            admin_pool,
            pool,
            schema,
            base_url: format!("http://{}", address),
            http: reqwest::Client::new(),
            server_task,
        })
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method.into(), format!("{}{}", self.base_url, path))
    }

    fn request_with_query(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
    ) -> reqwest::RequestBuilder {
        let mut url = reqwest::Url::parse(&format!("{}{}", self.base_url, path))
            .expect("request URL should be valid");
        url.query_pairs_mut().extend_pairs(query.iter().copied());
        self.http.request(method.into(), url)
    }

    async fn cleanup(self) -> Result<()> {
        self.server_task.abort();
        self.pool.close().await;
        self.admin_pool
            .execute(sqlx::query(sqlx::AssertSqlSafe(format!(
                "DROP SCHEMA {} CASCADE",
                self.schema
            ))))
            .await?;
        self.admin_pool.close().await;
        Ok(())
    }

    async fn seed_authenticated_client(&self, label: &str) -> Result<AuthFixture> {
        let client_secret = format!("secret_{label}");
        let secret_hash = AuthService::hash_client_secret(&client_secret)?;
        let client = api_auth::repository::create_api_client(
            &self.pool,
            api_auth::repository::NewApiClient {
                name: &format!("Client {label}"),
                description: Some("Integration test client"),
                client_type: ApiClientType::InstallationClient,
                client_id: &format!("client_{label}"),
                client_secret_hash: &secret_hash,
                secret_prefix: &client_secret.chars().take(12).collect::<String>(),
                created_by_user_id: Some("integration-tests"),
            },
        )
        .await?;

        for scope in REQUIRED_SCOPES {
            api_auth::repository::create_api_client_scope(&self.pool, client.id, scope).await?;
        }

        let token_response = self
            .request(Method::Post, "/v1/auth/token")
            .json(&json!({
                "client_id": client.client_id,
                "client_secret": client_secret,
            }))
            .send()
            .await?;

        assert_eq!(token_response.status(), StatusCode::OK);
        let payload: Value = token_response.json().await?;

        Ok(AuthFixture {
            access_token: payload["access_token"]
                .as_str()
                .expect("token response should contain access_token")
                .to_string(),
        })
    }

    async fn seed_authenticated_admin_user(
        &self,
        label: &str,
        role_key: &str,
    ) -> Result<AuthFixture> {
        let email = format!("{label}@test.com");
        let password = format!("password_{label}");
        let password_hash = AuthService::hash_password(&password)?;
        let user = admin_users::repository::create_admin_user(
            &self.pool,
            admin_users::repository::NewAdminUser {
                email: &email,
                display_name: &format!("Admin {label}"),
                password_hash: &password_hash,
            },
        )
        .await?;

        let role = roles::repository::get_role_by_key(&self.pool, role_key)
            .await?
            .expect("role should exist");
        roles::repository::assign_role_to_admin_user(&self.pool, user.id, role.id).await?;

        let token_response = self
            .request(Method::Post, "/v1/admin/auth/login")
            .json(&json!({
                "email": email,
                "password": password,
            }))
            .send()
            .await?;

        assert_eq!(token_response.status(), StatusCode::OK);
        let payload: Value = token_response.json().await?;

        Ok(AuthFixture {
            access_token: payload["access_token"]
                .as_str()
                .expect("admin token response should contain access_token")
                .to_string(),
        })
    }

    async fn seed_site(&self, name: &str, base_url: &str) -> Result<sites::Site> {
        sites::repository::create_site(&self.pool, name, base_url).await
    }

    async fn seed_role_with_permissions(
        &self,
        key: &str,
        permission_keys: &[&str],
    ) -> Result<roles::Role> {
        let role = roles::repository::create_role(
            &self.pool,
            key,
            &format!("Role {key}"),
            Some("Integration test role"),
        )
        .await?;
        let permission_keys = permission_keys
            .iter()
            .map(|key| (*key).to_string())
            .collect::<Vec<_>>();
        let permission_ids =
            permissions::repository::list_permissions_by_keys(&self.pool, &permission_keys)
                .await?
                .into_iter()
                .map(|permission| permission.id)
                .collect::<Vec<_>>();
        permissions::repository::replace_permissions_for_role(&self.pool, role.id, &permission_ids)
            .await?;

        Ok(role)
    }

    async fn seed_http_monitor(
        &self,
        site_id: i64,
        target_url: &str,
    ) -> Result<site_monitors::SiteMonitor> {
        site_monitors::repository::create_http_site_monitor(
            &self.pool,
            site_id,
            &site_monitors::HttpMonitorParams {
                target_url,
                check_interval_seconds: 60,
                expected_status_code: 200,
                body_must_contain: None,
                body_must_not_contain: None,
                body_must_contain_texts: None,
                body_must_not_contain_texts: None,
                json_path_exists: None,
                json_path_equals: None,
                json_path_not_equals: None,
                max_response_time_ms: None,
                required_header_name: None,
                required_header_value: None,
                header_assertions: None,
                ssl_certificate_checks_enabled: false,
                ssl_expiry_warning_days: None,
                http_check_timeout_seconds_override: None,
                http_check_max_attempts_override: None,
                http_check_retry_delays_ms_override: None,
                is_active: true,
            },
        )
        .await
    }

    async fn seed_ssl_monitor(
        &self,
        site_id: i64,
        target_url: &str,
    ) -> Result<site_monitors::SiteMonitor> {
        site_monitors::repository::upsert_ssl_monitor_by_site_id_and_target_url(
            &self.pool,
            site_id,
            &site_monitors::SslMonitorParams {
                target_url,
                check_interval_seconds: 60,
                ssl_expiry_warning_days: Some(14),
                http_check_timeout_seconds_override: None,
                http_check_max_attempts_override: None,
                http_check_retry_delays_ms_override: None,
                is_active: true,
            },
        )
        .await
    }

    async fn seed_heartbeat_monitor(
        &self,
        site_id: i64,
        heartbeat_token: &str,
    ) -> Result<site_monitors::SiteMonitor> {
        let target_url = format!("/v1/heartbeat/{heartbeat_token}");
        site_monitors::repository::create_heartbeat_site_monitor(
            &self.pool,
            site_id,
            &site_monitors::HeartbeatMonitorParams {
                target_url: &target_url,
                heartbeat_token,
                check_interval_seconds: 60,
                heartbeat_grace_seconds: Some(15),
                is_active: true,
            },
        )
        .await
    }

    async fn insert_check(
        &self,
        monitor: &site_monitors::SiteMonitor,
        is_success: bool,
        status_code: Option<i32>,
        error_message: Option<&str>,
    ) -> Result<site_monitor_checks::SiteMonitorCheck> {
        sqlx::query(
            r#"
            UPDATE site_monitors
            SET
                check_claimed_at = NOW(),
                check_lease_until = NOW() + INTERVAL '60 seconds',
                check_claimed_by = 'worker-routes',
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(monitor.id)
        .execute(&self.pool)
        .await?;
        let mut transaction = self.pool.begin().await?;
        let check = site_monitor_checks::repository::create_site_monitor_check(
            &mut transaction,
            monitor.id,
            &site_monitor_checks::CreateMonitorCheckParams {
                monitor_type: monitor.monitor_type,
                url_checked: &monitor.target_url,
                expected_status_code: Some(monitor.expected_status_code),
                is_success,
                status_code,
                response_time_ms: Some(123),
                total_duration_ms: Some(123),
                attempt_count: 1,
                was_retried: false,
                failure_reason: (!is_success).then_some("status_code_mismatch"),
                error_message,
                certificate_expires_at: None,
                certificate_days_remaining: None,
                certificate_issuer: None,
                certificate_subject: None,
                certificate_domain: None,
            },
        )
        .await?;
        site_monitors::repository::update_site_monitor_last_check(
            &mut transaction,
            monitor.id,
            "worker-routes",
            &site_monitors::MonitorLastCheckParams {
                is_success,
                status_code,
                response_time_ms: Some(123),
                failure_reason: (!is_success).then_some("status_code_mismatch"),
                error_message,
                certificate_expires_at: None,
                certificate_days_remaining: None,
                certificate_issuer: None,
                certificate_subject: None,
                certificate_domain: None,
            },
        )
        .await?;
        transaction.commit().await?;

        Ok(check)
    }

    async fn open_incident(
        &self,
        monitor: &site_monitors::SiteMonitor,
        check: &site_monitor_checks::SiteMonitorCheck,
    ) -> Result<site_monitor_incidents::SiteMonitorIncident> {
        let mut transaction = self.pool.begin().await?;
        let incident = site_monitor_incidents::repository::open_incident(
            &mut transaction,
            monitor.site_id,
            &site_monitor_incidents::OpenIncidentParams {
                site_monitor_id: monitor.id,
                monitor_type: monitor.monitor_type,
                target_url: &monitor.target_url,
                expected_status_code: monitor.expected_status_code,
                check_id: check.id,
                checked_at: check.checked_at,
                status_code: check.status_code,
                failure_reason: check.failure_reason.as_deref(),
                error_message: check.error_message.as_deref(),
            },
        )
        .await?;
        transaction.commit().await?;

        Ok(incident)
    }

    async fn resolve_incident(
        &self,
        incident_id: i64,
        check: &site_monitor_checks::SiteMonitorCheck,
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        site_monitor_incidents::repository::resolve_incident(
            &mut transaction,
            incident_id,
            &site_monitor_incidents::ResolveIncidentParams {
                check_id: check.id,
                checked_at: check.checked_at,
                status_code: check.status_code,
                response_time_ms: check.response_time_ms,
            },
        )
        .await?;
        transaction.commit().await?;

        Ok(())
    }

    async fn enqueue_delivery(
        &self,
        channel_id: i64,
        monitor_id: i64,
        check_id: i64,
        event_type: NotificationEventType,
        payload: Value,
    ) -> Result<i64> {
        let mut transaction = self.pool.begin().await?;
        notification_deliveries::repository::enqueue_deliveries(
            &mut transaction,
            &[notification_deliveries::NewNotificationDelivery {
                notification_channel_id: channel_id,
                site_monitor_id: monitor_id,
                site_monitor_check_id: check_id,
                incident_id: None,
                event_type,
                payload: &payload,
            }],
        )
        .await?;
        transaction.commit().await?;

        let delivery_id: i64 = sqlx::query_scalar(
            r#"
            SELECT id
            FROM notification_deliveries
            WHERE notification_channel_id = $1
              AND site_monitor_check_id = $2
              AND event_type = $3
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(check_id)
        .bind(event_type)
        .fetch_one(&self.pool)
        .await?;

        Ok(delivery_id)
    }
}

fn unique_schema_name() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_millis();
    let suffix = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
    format!("sentinel_test_{}_{}", millis, suffix)
}

fn schema_database_url(base_database_url: &str, schema: &str) -> Result<String> {
    let mut url = reqwest::Url::parse(base_database_url)?;
    url.query_pairs_mut()
        .append_pair("options", &format!("-c search_path={},public", schema));
    Ok(url.to_string())
}

async fn apply_migrations(pool: &PgPool) -> Result<()> {
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            id BIGSERIAL PRIMARY KEY,
            filename TEXT NOT NULL,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            CONSTRAINT uq_schema_migrations_filename UNIQUE (filename)
        )",
    )
    .execute(pool)
    .await?;

    for path in migration_paths()? {
        let filename = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or_default()
            .to_string();
        let sql = fs::read_to_string(&path)
            .with_context(|| format!("failed to read migration {}", path.display()))?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(pool)
            .await?;
        sqlx::query("INSERT INTO schema_migrations (filename) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(&filename)
            .execute(pool)
            .await?;
    }

    Ok(())
}

fn migration_paths() -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

fn header_string(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}
