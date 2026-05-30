use std::time::{Duration, Instant};

#[derive(Debug)]
struct MissingWebhookSecret;

impl std::fmt::Display for MissingWebhookSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("webhook channel is missing a signing secret")
    }
}

impl std::error::Error for MissingWebhookSecret {}

use anyhow::{Context, Result, anyhow};
use chrono::{Duration as ChronoDuration, Utc};
use futures::{StreamExt, stream};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, address::AddressError,
    error::Error as MessageError, message::Mailbox, transport::smtp::Error as SmtpError,
    transport::smtp::authentication::Credentials,
};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, Postgres};
use tokio::sync::watch;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::{
    config::{Config, SmtpConfig},
    crypto,
    domain::{
        notification_channels::{self, NotificationChannelType},
        notification_deliveries::{
            self, ClaimedNotificationDelivery, NewNotificationDelivery, NotificationEventType,
        },
        site_monitor_checks::SiteMonitorCheck,
        site_monitors::{SiteMonitor, SiteMonitorType},
    },
    net,
};

const NOTIFICATION_PAYLOAD_SCHEMA_VERSION: i32 = 1;

#[derive(Clone)]
pub struct SmtpMailer {
    mailer: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

#[derive(Debug, sqlx::FromRow)]
struct NotificationContext {
    site_id: i64,
    site_name: String,
    site_base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotificationPayload {
    #[serde(default = "notification_payload_schema_version")]
    schema_version: i32,
    event_type: NotificationEventType,
    occurred_at: String,
    summary: String,
    email_subject: String,
    email_text: String,
    site: NotificationSitePayload,
    monitor: NotificationMonitorPayload,
    previous_check: NotificationPreviousCheckPayload,
    current_check: NotificationCurrentCheckPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotificationSitePayload {
    id: i64,
    name: String,
    base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotificationMonitorPayload {
    id: i64,
    monitor_type: String,
    target_url: String,
    expected_status_code: i32,
    check_interval_seconds: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotificationPreviousCheckPayload {
    checked_at: Option<String>,
    is_success: Option<bool>,
    status_code: Option<i32>,
    response_time_ms: Option<i32>,
    failure_reason: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotificationCurrentCheckPayload {
    id: i64,
    checked_at: String,
    is_success: bool,
    status_code: Option<i32>,
    response_time_ms: Option<i32>,
    failure_reason: Option<String>,
    error_message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NotificationDeliveryLeasePlan {
    initial_lease: Duration,
    heartbeat_interval: Duration,
}

fn notification_payload_schema_version() -> i32 {
    NOTIFICATION_PAYLOAD_SCHEMA_VERSION
}

pub async fn enqueue_site_monitor_notifications(
    transact: &mut sqlx::Transaction<'_, Postgres>,
    site_monitor: &SiteMonitor,
    site_monitor_check: &SiteMonitorCheck,
    incident_id: Option<i64>,
) -> Result<usize> {
    let Some(event_type) =
        determine_event_type(site_monitor.last_is_success, site_monitor_check.is_success)
    else {
        return Ok(0);
    };

    let channels = notification_channels::repository::list_effective_by_site_id(
        transact.as_mut(),
        site_monitor.site_id,
    )
    .await?;

    let channels = channels
        .into_iter()
        .filter(|channel| channel.is_active)
        .filter(|channel| match event_type {
            NotificationEventType::Failure => channel.notify_on_failure,
            NotificationEventType::Recovery => channel.notify_on_recovery,
        })
        .collect::<Vec<_>>();

    if channels.is_empty() {
        return Ok(0);
    }

    let context = load_notification_context(transact.as_mut(), site_monitor.site_id).await?;
    let payload = serde_json::to_value(build_payload(
        &context,
        site_monitor,
        site_monitor_check,
        event_type,
    ))?;

    let deliveries = channels
        .iter()
        .map(|channel| NewNotificationDelivery {
            notification_channel_id: channel.id,
            site_monitor_id: site_monitor.id,
            site_monitor_check_id: site_monitor_check.id,
            incident_id,
            event_type,
            payload: &payload,
        })
        .collect::<Vec<_>>();

    notification_deliveries::repository::enqueue_deliveries(transact, &deliveries).await?;

    Ok(deliveries.len())
}

pub async fn deliver_due_notifications(
    pool: &sqlx::PgPool,
    smtp_mailer: Option<&SmtpMailer>,
    config: &Config,
    worker_id: &str,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<usize> {
    let lease_plan = derive_notification_delivery_lease_plan(config);
    let claim_limit = notification_delivery_claim_limit(config);

    if *shutdown_rx.borrow() {
        return Ok(0);
    }

    let deliveries = notification_deliveries::repository::claim_due_deliveries(
        pool,
        worker_id,
        claim_limit,
        duration_to_lease_seconds(lease_plan.initial_lease),
    )
    .await?;
    let claimed_count = deliveries.len();

    if deliveries.is_empty() {
        return Ok(0);
    }

    stream::iter(deliveries.into_iter().map(|delivery| {
        let delivery_shutdown_rx = shutdown_rx.clone();
        let lease_plan = lease_plan.clone();

        async move {
            if *delivery_shutdown_rx.borrow() {
                release_cancelled_delivery_claim(
                    pool,
                    delivery.id,
                    worker_id,
                    "before delivery start",
                )
                .await;
                return;
            }

            let result = deliver_one_notification(
                pool,
                delivery,
                DeliverOneCtx {
                    smtp_mailer,
                    config,
                    worker_id,
                    lease_plan,
                    shutdown_rx: delivery_shutdown_rx,
                },
            )
            .await;

            if let Err(error) = result {
                error!("Error delivering notification: {error:?}");
            }
        }
    }))
    .buffer_unordered(config.max_notification_concurrent_deliveries)
    .for_each(|_| async {})
    .await;

    Ok(claimed_count)
}

enum NotificationDeliveryExecution {
    Completed,
    Cancelled,
}

enum NotificationDeliveryLoopOutcome {
    Completed(Result<()>),
    ShutdownCancelled(&'static str),
    LeaseLost,
}

fn determine_event_type(
    previous_is_success: Option<bool>,
    current_is_success: bool,
) -> Option<NotificationEventType> {
    match (previous_is_success, current_is_success) {
        (Some(true), false) | (None, false) => Some(NotificationEventType::Failure),
        (Some(false), true) => Some(NotificationEventType::Recovery),
        _ => None,
    }
}

fn build_payload(
    context: &NotificationContext,
    site_monitor: &SiteMonitor,
    site_monitor_check: &SiteMonitorCheck,
    event_type: NotificationEventType,
) -> NotificationPayload {
    let summary = build_summary(context, site_monitor, site_monitor_check, event_type);
    let email_subject = build_email_subject(context, event_type);
    let email_text = build_email_text(&summary, context, site_monitor, site_monitor_check);

    NotificationPayload {
        schema_version: notification_payload_schema_version(),
        event_type,
        occurred_at: site_monitor_check.checked_at.to_rfc3339(),
        summary,
        email_subject,
        email_text,
        site: NotificationSitePayload {
            id: context.site_id,
            name: context.site_name.clone(),
            base_url: context.site_base_url.clone(),
        },
        monitor: NotificationMonitorPayload {
            id: site_monitor.id,
            monitor_type: site_monitor.monitor_type.as_str().to_string(),
            target_url: site_monitor.target_url.clone(),
            expected_status_code: site_monitor.expected_status_code,
            check_interval_seconds: site_monitor.check_interval_seconds,
        },
        previous_check: NotificationPreviousCheckPayload {
            checked_at: site_monitor.last_checked_at.map(|value| value.to_rfc3339()),
            is_success: site_monitor.last_is_success,
            status_code: site_monitor.last_status_code,
            response_time_ms: site_monitor.last_response_time_ms,
            failure_reason: site_monitor.last_failure_reason.clone(),
            error_message: site_monitor.last_error_message.clone(),
        },
        current_check: NotificationCurrentCheckPayload {
            id: site_monitor_check.id,
            checked_at: site_monitor_check.checked_at.to_rfc3339(),
            is_success: site_monitor_check.is_success,
            status_code: site_monitor_check.status_code,
            response_time_ms: site_monitor_check.response_time_ms,
            failure_reason: site_monitor_check.failure_reason.clone(),
            error_message: site_monitor_check.error_message.clone(),
        },
    }
}

fn build_summary(
    context: &NotificationContext,
    site_monitor: &SiteMonitor,
    _site_monitor_check: &SiteMonitorCheck,
    event_type: NotificationEventType,
) -> String {
    match (site_monitor.monitor_type, event_type) {
        (SiteMonitorType::Http, NotificationEventType::Failure) => format!(
            "Site '{}' is DOWN for HTTP monitor {} ({})",
            context.site_name, site_monitor.id, site_monitor.target_url
        ),
        (SiteMonitorType::Http, NotificationEventType::Recovery) => format!(
            "Site '{}' recovered for HTTP monitor {} ({})",
            context.site_name, site_monitor.id, site_monitor.target_url
        ),
        (SiteMonitorType::Ssl, NotificationEventType::Failure) => format!(
            "Site '{}' has an SSL issue for monitor {} ({})",
            context.site_name, site_monitor.id, site_monitor.target_url
        ),
        (SiteMonitorType::Ssl, NotificationEventType::Recovery) => format!(
            "Site '{}' SSL monitor recovered for monitor {} ({})",
            context.site_name, site_monitor.id, site_monitor.target_url
        ),
        (SiteMonitorType::Heartbeat, NotificationEventType::Failure) => format!(
            "Site '{}' missed a heartbeat for monitor {} ({})",
            context.site_name, site_monitor.id, site_monitor.target_url
        ),
        (SiteMonitorType::Heartbeat, NotificationEventType::Recovery) => format!(
            "Site '{}' heartbeat monitor recovered for monitor {} ({})",
            context.site_name, site_monitor.id, site_monitor.target_url
        ),
        (SiteMonitorType::Tcp, NotificationEventType::Failure) => format!(
            "Site '{}' TCP port unreachable for monitor {} ({})",
            context.site_name, site_monitor.id, site_monitor.target_url
        ),
        (SiteMonitorType::Tcp, NotificationEventType::Recovery) => format!(
            "Site '{}' TCP port reachable again for monitor {} ({})",
            context.site_name, site_monitor.id, site_monitor.target_url
        ),
        (SiteMonitorType::Dns, NotificationEventType::Failure) => format!(
            "Site '{}' DNS check failed for monitor {} ({})",
            context.site_name, site_monitor.id, site_monitor.target_url
        ),
        (SiteMonitorType::Dns, NotificationEventType::Recovery) => format!(
            "Site '{}' DNS check recovered for monitor {} ({})",
            context.site_name, site_monitor.id, site_monitor.target_url
        ),
    }
}

fn build_email_subject(context: &NotificationContext, event_type: NotificationEventType) -> String {
    match event_type {
        NotificationEventType::Failure => {
            format!("[Alon Sentinel] {} is DOWN", context.site_name)
        }
        NotificationEventType::Recovery => {
            format!("[Alon Sentinel] {} recovered", context.site_name)
        }
    }
}

fn build_email_text(
    summary: &str,
    context: &NotificationContext,
    site_monitor: &SiteMonitor,
    site_monitor_check: &SiteMonitorCheck,
) -> String {
    match site_monitor.monitor_type {
        SiteMonitorType::Http => format!(
            "{summary}\n\nSite: {}\nBase URL: {}\nMonitor URL: {}\nExpected status: {}\nOccurred at: {}\nCurrent status: {}\nResponse time: {}\nError: {}\n",
            context.site_name,
            context.site_base_url,
            site_monitor.target_url,
            site_monitor.expected_status_code,
            site_monitor_check.checked_at.to_rfc3339(),
            site_monitor_check
                .status_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            site_monitor_check
                .response_time_ms
                .map(|value| format!("{value} ms"))
                .unwrap_or_else(|| "n/a".to_string()),
            site_monitor_check
                .error_message
                .clone()
                .unwrap_or_else(|| "n/a".to_string()),
        ),
        SiteMonitorType::Ssl => format!(
            "{summary}\n\nSite: {}\nBase URL: {}\nMonitor URL: {}\nOccurred at: {}\nCertificate expires at: {}\nDays remaining: {}\nIssuer: {}\nSubject: {}\nError: {}\n",
            context.site_name,
            context.site_base_url,
            site_monitor.target_url,
            site_monitor_check.checked_at.to_rfc3339(),
            site_monitor_check
                .certificate_expires_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_else(|| "n/a".to_string()),
            site_monitor_check
                .certificate_days_remaining
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            site_monitor_check
                .certificate_issuer
                .clone()
                .unwrap_or_else(|| "n/a".to_string()),
            site_monitor_check
                .certificate_subject
                .clone()
                .unwrap_or_else(|| "n/a".to_string()),
            site_monitor_check
                .error_message
                .clone()
                .unwrap_or_else(|| "n/a".to_string()),
        ),
        SiteMonitorType::Heartbeat => format!(
            "{summary}\n\nSite: {}\nBase URL: {}\nHeartbeat path: {}\nOccurred at: {}\nLast heartbeat received at: {}\nError: {}\n",
            context.site_name,
            context.site_base_url,
            site_monitor.target_url,
            site_monitor_check.checked_at.to_rfc3339(),
            site_monitor
                .last_heartbeat_received_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_else(|| "n/a".to_string()),
            site_monitor_check
                .error_message
                .clone()
                .unwrap_or_else(|| "n/a".to_string()),
        ),
        SiteMonitorType::Tcp => format!(
            "{summary}\n\nSite: {}\nBase URL: {}\nHost: {}\nPort: {}\nOccurred at: {}\nConnect time: {}\nError: {}\n",
            context.site_name,
            context.site_base_url,
            site_monitor.tcp_target_host.as_deref().unwrap_or("n/a"),
            site_monitor
                .tcp_target_port
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            site_monitor_check.checked_at.to_rfc3339(),
            site_monitor_check
                .response_time_ms
                .map(|value| format!("{value} ms"))
                .unwrap_or_else(|| "n/a".to_string()),
            site_monitor_check
                .error_message
                .clone()
                .unwrap_or_else(|| "n/a".to_string()),
        ),
        SiteMonitorType::Dns => format!(
            "{summary}\n\nSite: {}\nBase URL: {}\nHostname: {}\nRecord type: {}\nExpected value: {}\nOccurred at: {}\nQuery time: {}\nError: {}\n",
            context.site_name,
            context.site_base_url,
            site_monitor.dns_hostname.as_deref().unwrap_or("n/a"),
            site_monitor.dns_record_type.as_deref().unwrap_or("n/a"),
            site_monitor.dns_expected_value.as_deref().unwrap_or("any"),
            site_monitor_check.checked_at.to_rfc3339(),
            site_monitor_check
                .response_time_ms
                .map(|value| format!("{value} ms"))
                .unwrap_or_else(|| "n/a".to_string()),
            site_monitor_check
                .error_message
                .clone()
                .unwrap_or_else(|| "n/a".to_string()),
        ),
    }
}

async fn load_notification_context<'a, E>(executor: E, site_id: i64) -> Result<NotificationContext>
where
    E: Executor<'a, Database = Postgres>,
{
    let context = sqlx::query_as::<_, NotificationContext>(
        r#"
        SELECT
            s.id AS site_id,
            s.name AS site_name,
            s.base_url AS site_base_url
        FROM sites s
        WHERE s.id = $1
        "#,
    )
    .bind(site_id)
    .fetch_one(executor)
    .await?;

    Ok(context)
}

struct DeliverOneCtx<'a> {
    smtp_mailer: Option<&'a SmtpMailer>,
    config: &'a Config,
    worker_id: &'a str,
    lease_plan: NotificationDeliveryLeasePlan,
    shutdown_rx: watch::Receiver<bool>,
}

async fn deliver_one_notification(
    pool: &sqlx::PgPool,
    delivery: ClaimedNotificationDelivery,
    ctx: DeliverOneCtx<'_>,
) -> Result<NotificationDeliveryExecution> {
    let DeliverOneCtx {
        smtp_mailer,
        config,
        worker_id,
        lease_plan,
        mut shutdown_rx,
    } = ctx;
    if *shutdown_rx.borrow() {
        release_cancelled_delivery_claim(pool, delivery.id, worker_id, "before send").await;
        return Ok(NotificationDeliveryExecution::Cancelled);
    }

    let payload = serde_json::from_value::<NotificationPayload>(delivery.payload.clone())?;

    let send_future = async {
        match delivery.channel_type {
            NotificationChannelType::Webhook => {
                send_webhook_notification(
                    config,
                    delivery.id,
                    &delivery.destination,
                    delivery.webhook_secret_ciphertext.as_deref(),
                    &delivery.payload,
                )
                .await
            }
            NotificationChannelType::Email => {
                send_email_notification(smtp_mailer, &delivery.destination, &payload).await
            }
            NotificationChannelType::Slack => {
                send_simple_webhook_notification(
                    config,
                    &delivery.destination,
                    "text",
                    &payload.summary,
                )
                .await
            }
            NotificationChannelType::Discord => {
                send_simple_webhook_notification(
                    config,
                    &delivery.destination,
                    "content",
                    &payload.summary,
                )
                .await
            }
        }
    };
    tokio::pin!(send_future);

    let lease_seconds = duration_to_lease_seconds(lease_plan.initial_lease);
    let mut lease_deadline = Instant::now() + lease_plan.initial_lease;

    let outcome = loop {
        tokio::select! {
            result = &mut send_future => break NotificationDeliveryLoopOutcome::Completed(result),
            changed = shutdown_rx.changed() => {
                match changed {
                    Ok(_) if *shutdown_rx.borrow() => break NotificationDeliveryLoopOutcome::ShutdownCancelled("during send"),
                    Ok(_) => continue,
                    Err(_) => break NotificationDeliveryLoopOutcome::ShutdownCancelled("during send"),
                }
            }
            _ = sleep(lease_plan.heartbeat_interval) => {
                match notification_deliveries::repository::extend_delivery_claim(
                    pool,
                    delivery.id,
                    worker_id,
                    lease_seconds,
                )
                .await
                {
                    Ok(true) => {
                        lease_deadline = Instant::now() + lease_plan.initial_lease;
                    }
                    Ok(false) => {
                        warn!(
                            "Notification {} lease extension failed because ownership was lost",
                            delivery.id
                        );
                        break NotificationDeliveryLoopOutcome::LeaseLost;
                    }
                    Err(error) => {
                        error!(
                            "Failed to extend notification {} lease: {error:?}",
                            delivery.id
                        );

                        if Instant::now() >= lease_deadline {
                            warn!(
                                "Notification {} lease expired after renewal failures; cancelling in-flight delivery",
                                delivery.id
                            );
                            break NotificationDeliveryLoopOutcome::LeaseLost;
                        }
                    }
                }
            }
        }
    };

    match outcome {
        NotificationDeliveryLoopOutcome::Completed(Ok(())) => {
            if notification_deliveries::repository::mark_delivered(pool, delivery.id, worker_id)
                .await?
            {
                info!(
                    "Delivered notification {} to channel {}",
                    delivery.id, delivery.channel_name
                );
            } else {
                warn!(
                    "Notification {} completed delivery but lost ownership before state update",
                    delivery.id
                );
            }
            Ok(NotificationDeliveryExecution::Completed)
        }
        NotificationDeliveryLoopOutcome::Completed(Err(error)) => {
            let should_retry = should_retry_delivery_error(&error);
            handle_delivery_failure(
                pool,
                &delivery,
                &DeliveryFailureCtx {
                    config,
                    worker_id,
                    error_message: &error.to_string(),
                    should_retry,
                },
            )
            .await?;
            Ok(NotificationDeliveryExecution::Completed)
        }
        NotificationDeliveryLoopOutcome::ShutdownCancelled(phase) => {
            release_cancelled_delivery_claim(pool, delivery.id, worker_id, phase).await;
            Ok(NotificationDeliveryExecution::Cancelled)
        }
        NotificationDeliveryLoopOutcome::LeaseLost => Ok(NotificationDeliveryExecution::Cancelled),
    }
}

async fn send_webhook_notification(
    config: &Config,
    delivery_id: i64,
    destination: &str,
    webhook_secret_ciphertext: Option<&str>,
    payload: &serde_json::Value,
) -> Result<()> {
    send_webhook_notification_internal(
        config,
        &WebhookParams {
            delivery_id,
            destination,
            webhook_secret_ciphertext,
            payload,
        },
        !cfg!(test),
    )
    .await
}

struct WebhookParams<'a> {
    delivery_id: i64,
    destination: &'a str,
    webhook_secret_ciphertext: Option<&'a str>,
    payload: &'a serde_json::Value,
}

async fn send_webhook_notification_internal(
    config: &Config,
    p: &WebhookParams<'_>,
    validate_public_target: bool,
) -> Result<()> {
    let body = serde_json::to_vec(p.payload)?;
    let webhook_secret_ciphertext = p.webhook_secret_ciphertext.ok_or(MissingWebhookSecret)?;
    let webhook_secret = config
        .webhook_secret_encryption_key
        .decrypt_webhook_secret(webhook_secret_ciphertext)
        .context("failed to decrypt webhook signing secret")?;
    let timestamp = Utc::now().timestamp().to_string();
    let signature = crypto::build_webhook_signature(&webhook_secret, &timestamp, &body);
    let client = build_webhook_runtime_client(
        p.destination,
        Duration::from_secs(config.notification_delivery_timeout_seconds as u64),
        validate_public_target,
    )
    .await?;

    let response = client
        .post(p.destination)
        .header("content-type", "application/json")
        .header("user-agent", "alon-sentinel/notification-channel")
        .header("x-alon-delivery-id", p.delivery_id.to_string())
        .header("x-sentinel-delivery-id", p.delivery_id.to_string())
        .header("x-sentinel-timestamp", &timestamp)
        .header("x-sentinel-signature", signature)
        .body(body)
        .send()
        .await?;

    if response.status().is_redirection() {
        anyhow::bail!(
            "webhook URL redirected with HTTP status {}",
            response.status().as_u16()
        );
    }

    response.error_for_status()?;

    Ok(())
}

async fn build_webhook_runtime_client(
    webhook_url: &str,
    timeout: Duration,
    validate_public_target: bool,
) -> Result<reqwest::Client> {
    let url = reqwest::Url::parse(webhook_url)?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(anyhow!("unsupported webhook URL scheme: {scheme}")),
    };

    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("webhook URL must have a host"))?;
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(timeout);

    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if validate_public_target && !net::check_ip_is_public(ip) {
            anyhow::bail!(
                "webhook URL must not resolve to a loopback, private, or link-local address"
            );
        }

        return builder.build().context("failed to build webhook client");
    }

    let port = url.port_or_known_default().unwrap_or(80);
    let resolved: Vec<_> = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .map_err(|_| anyhow!("webhook URL hostname could not be resolved"))?
        .collect();
    if resolved.is_empty() {
        anyhow::bail!("webhook URL hostname could not be resolved");
    }

    for addr in &resolved {
        if validate_public_target && !net::check_ip_is_public(addr.ip()) {
            anyhow::bail!(
                "webhook URL must not resolve to a loopback, private, or link-local address"
            );
        }
    }

    builder = builder.resolve_to_addrs(host, &resolved);
    builder.build().context("failed to build webhook client")
}

// Slack and Discord incoming webhooks are pre-authenticated by URL (the token is embedded in the
// path). No request signing or auth header is added here — those platforms don't expect it, and
// adding one would break delivery. This is intentionally different from the signed webhook channel
// type, which uses X-Sentinel-Signature. Do not conflate the two.
async fn send_simple_webhook_notification(
    config: &Config,
    destination: &str,
    message_key: &str,
    message: &str,
) -> Result<()> {
    let body = serde_json::to_vec(&serde_json::json!({ message_key: message }))?;
    let client = build_webhook_runtime_client(
        destination,
        Duration::from_secs(config.notification_delivery_timeout_seconds as u64),
        !cfg!(test),
    )
    .await?;

    let response = client
        .post(destination)
        .header("content-type", "application/json")
        .header("user-agent", "alon-sentinel/notification-channel")
        .body(body)
        .send()
        .await?;

    if response.status().is_redirection() {
        anyhow::bail!(
            "webhook URL redirected with HTTP status {}",
            response.status().as_u16()
        );
    }

    response.error_for_status()?;
    Ok(())
}

async fn send_email_notification(
    smtp_mailer: Option<&SmtpMailer>,
    destination: &str,
    payload: &NotificationPayload,
) -> Result<()> {
    let smtp_mailer = smtp_mailer.ok_or_else(|| anyhow!("SMTP is not configured"))?;
    let to = parse_email_mailbox(destination)?;
    let email = Message::builder()
        .from(smtp_mailer.from.clone())
        .to(to)
        .subject(&payload.email_subject)
        .body(payload.email_text.clone())?;

    smtp_mailer.mailer.send(email).await?;
    Ok(())
}

pub fn build_smtp_mailer(config: &Config) -> Result<Option<SmtpMailer>> {
    let Some(smtp) = config.smtp.as_ref() else {
        return Ok(None);
    };

    let timeout = Some(Duration::from_secs(
        config.notification_delivery_timeout_seconds as u64,
    ));
    let builder = match smtp.port {
        // In lettre 0.11, relay() means implicit TLS/SMTPS from the first byte.
        465 => AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host)?,
        // Port 587 and most modern submission ports expect STARTTLS upgrade.
        _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)?,
    }
    .port(smtp.port)
    .timeout(timeout);

    let builder = if let (Some(username), Some(password)) = (&smtp.username, &smtp.password) {
        builder.credentials(Credentials::new(username.clone(), password.clone()))
    } else {
        builder
    };

    Ok(Some(SmtpMailer {
        mailer: builder.build(),
        from: build_from_mailbox(smtp)?,
    }))
}

fn build_from_mailbox(smtp: &SmtpConfig) -> Result<Mailbox> {
    let address = smtp.from_email.parse()?;
    Ok(Mailbox::new(smtp.from_name.clone(), address))
}

fn parse_email_mailbox(destination: &str) -> Result<Mailbox> {
    Ok(Mailbox::new(None, destination.parse()?))
}

struct DeliveryFailureCtx<'a> {
    config: &'a Config,
    worker_id: &'a str,
    error_message: &'a str,
    should_retry: bool,
}

async fn handle_delivery_failure(
    pool: &sqlx::PgPool,
    delivery: &ClaimedNotificationDelivery,
    ctx: &DeliveryFailureCtx<'_>,
) -> Result<()> {
    let attempt_number = delivery.attempts + 1;
    let should_retry =
        ctx.should_retry && (attempt_number as usize) < ctx.config.notification_max_attempts;
    let next_attempt_at = should_retry.then(|| {
        Utc::now()
            + ChronoDuration::seconds(compute_retry_delay_seconds(
                ctx.config.notification_retry_base_seconds,
                attempt_number,
            ))
    });

    let updated = notification_deliveries::repository::mark_failed(
        pool,
        delivery.id,
        ctx.worker_id,
        ctx.error_message,
        next_attempt_at,
    )
    .await?;

    if !updated {
        warn!(
            "Notification {} failed delivery but lost ownership before state update",
            delivery.id
        );
        return Ok(());
    }

    if should_retry {
        warn!(
            "Notification {} failed attempt {} and will retry: {}",
            delivery.id, attempt_number, ctx.error_message
        );
    } else {
        error!(
            "Notification {} exhausted retries after {} attempts: {}",
            delivery.id, attempt_number, ctx.error_message
        );
    }

    Ok(())
}

async fn release_cancelled_delivery_claim(
    pool: &sqlx::PgPool,
    delivery_id: i64,
    worker_id: &str,
    phase: &str,
) {
    match notification_deliveries::repository::release_delivery_claim(pool, delivery_id, worker_id)
        .await
    {
        Ok(true) => info!(
            "Released notification {} claim after shutdown cancellation {}",
            delivery_id, phase
        ),
        Ok(false) => warn!(
            "Notification {} claim was already lost before shutdown cleanup {}",
            delivery_id, phase
        ),
        Err(error) => error!(
            "Failed to release notification {} claim during shutdown cleanup {}: {error:?}",
            delivery_id, phase
        ),
    }
}

fn derive_notification_delivery_lease_plan(config: &Config) -> NotificationDeliveryLeasePlan {
    let timeout = Duration::from_secs(config.notification_delivery_timeout_seconds as u64);
    let minimum_lease = Duration::from_secs(config.lease_notification_delivery_seconds as u64);
    let safety_buffer = (timeout / 2).max(Duration::from_secs(2));
    let initial_lease = minimum_lease.max(timeout.saturating_add(safety_buffer));

    NotificationDeliveryLeasePlan {
        initial_lease,
        heartbeat_interval: derive_heartbeat_interval(initial_lease),
    }
}

fn notification_delivery_claim_limit(config: &Config) -> i64 {
    config
        .due_notification_batch_size
        .min(config.max_notification_concurrent_deliveries) as i64
}

fn derive_heartbeat_interval(lease: Duration) -> Duration {
    (lease / 3).max(Duration::from_millis(250))
}

fn duration_to_lease_seconds(duration: Duration) -> i64 {
    duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() > 0))
        .max(1) as i64
}

fn should_retry_delivery_error(error: &anyhow::Error) -> bool {
    if let Some(error) = error.downcast_ref::<SmtpError>() {
        return !error.is_permanent();
    }

    if error.downcast_ref::<AddressError>().is_some()
        || error.downcast_ref::<MessageError>().is_some()
        || error.downcast_ref::<MissingWebhookSecret>().is_some()
    {
        return false;
    }

    true
}

fn compute_retry_delay_seconds(base_seconds: usize, attempt_number: i32) -> i64 {
    let exponent = attempt_number.saturating_sub(1).clamp(0, 6) as u32;
    let multiplier = 2_i64.pow(exponent);
    (base_seconds as i64).saturating_mul(multiplier)
}

pub async fn validate_webhook_url(webhook_url: &str) -> Result<()> {
    let url = reqwest::Url::parse(webhook_url)?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(anyhow!("unsupported webhook URL scheme: {scheme}")),
    };

    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("webhook URL must have a host"))?;
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if !net::check_ip_is_public(ip) {
            anyhow::bail!(
                "webhook URL must not resolve to a loopback, private, or link-local address"
            );
        }
        return Ok(());
    }

    let port = url.port_or_known_default().unwrap_or(80);
    let resolved = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .map_err(|_| anyhow!("webhook URL hostname could not be resolved"))?;
    for addr in resolved {
        if !net::check_ip_is_public(addr.ip()) {
            anyhow::bail!(
                "webhook URL must not resolve to a loopback, private, or link-local address"
            );
        }
    }

    Ok(())
}

pub fn validate_email_address(email_address: &str) -> Result<()> {
    parse_email_mailbox(email_address)?;
    Ok(())
}

pub async fn validate_channel_destination(
    channel_type: NotificationChannelType,
    destination: &str,
) -> Result<()> {
    match channel_type {
        NotificationChannelType::Webhook
        | NotificationChannelType::Slack
        | NotificationChannelType::Discord => validate_webhook_url(destination).await,
        NotificationChannelType::Email => validate_email_address(destination),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NotificationContext, NotificationPayload, WebhookParams, build_payload, build_smtp_mailer,
        compute_retry_delay_seconds, deliver_due_notifications, derive_heartbeat_interval,
        derive_notification_delivery_lease_plan, determine_event_type, duration_to_lease_seconds,
        notification_payload_schema_version, send_webhook_notification_internal,
        should_retry_delivery_error, validate_channel_destination, validate_email_address,
        validate_webhook_url,
    };
    use crate::{
        config::{Config, SmtpConfig},
        crypto::{WebhookSecretEncryptionKey, build_webhook_signature},
        domain::{
            notification_channels::{self, NotificationChannelType},
            notification_deliveries::{self, NotificationDelivery, NotificationEventType},
            site_monitor_checks, site_monitors, sites,
        },
    };
    use anyhow::{Context, Result, anyhow};
    use chrono::Utc;
    use lettre::{address::AddressError, error::Error as MessageError};
    use sqlx::{Executor, PgPool};
    use std::{
        fs,
        path::PathBuf,
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::{Notify, watch},
        time::{sleep, timeout},
    };

    const TEST_WEBHOOK_SECRET_ENCRYPTION_KEY_HEX: &str =
        "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const TEST_WEBHOOK_SECRET: &str = "hook-secret";

    #[test]
    fn determine_event_type_returns_failure_for_first_failed_check() {
        let event_type = determine_event_type(None, false);

        assert_eq!(event_type, Some(NotificationEventType::Failure));
    }

    #[test]
    fn determine_event_type_returns_recovery_after_failure() {
        let event_type = determine_event_type(Some(false), true);

        assert_eq!(event_type, Some(NotificationEventType::Recovery));
    }

    #[test]
    fn determine_event_type_returns_none_for_steady_state() {
        assert_eq!(determine_event_type(Some(true), true), None);
        assert_eq!(determine_event_type(Some(false), false), None);
    }

    #[test]
    fn build_smtp_mailer_uses_wrapper_tls_for_port_465() -> Result<()> {
        let mut config = test_config();
        config.smtp = Some(SmtpConfig {
            host: "smtp.example.com".to_string(),
            port: 465,
            username: Some("mailer".to_string()),
            password: Some("secret".to_string()),
            from_name: Some("Alon Sentinel".to_string()),
            from_email: "alerts@example.com".to_string(),
        });

        let smtp_mailer = build_smtp_mailer(&config)?.expect("smtp mailer should be built");
        let debug = format!("{:?}", smtp_mailer.mailer);

        assert!(debug.contains("port: 465"), "debug={debug}");
        assert!(debug.contains("tls: Wrapper"), "debug={debug}");
        assert!(!debug.contains("tls: Required"), "debug={debug}");

        Ok(())
    }

    #[test]
    fn build_smtp_mailer_uses_required_starttls_for_port_587() -> Result<()> {
        let mut config = test_config();
        config.smtp = Some(SmtpConfig {
            host: "smtp.example.com".to_string(),
            port: 587,
            username: Some("mailer".to_string()),
            password: Some("secret".to_string()),
            from_name: Some("Alon Sentinel".to_string()),
            from_email: "alerts@example.com".to_string(),
        });

        let smtp_mailer = build_smtp_mailer(&config)?.expect("smtp mailer should be built");
        let debug = format!("{:?}", smtp_mailer.mailer);

        assert!(debug.contains("port: 587"), "debug={debug}");
        assert!(debug.contains("tls: Required"), "debug={debug}");
        assert!(!debug.contains("tls: Wrapper"), "debug={debug}");

        Ok(())
    }

    #[test]
    fn compute_retry_delay_seconds_uses_exponential_backoff() {
        assert_eq!(compute_retry_delay_seconds(30, 1), 30);
        assert_eq!(compute_retry_delay_seconds(30, 2), 60);
        assert_eq!(compute_retry_delay_seconds(30, 3), 120);
    }

    #[test]
    fn should_retry_delivery_error_retries_unknown_errors() {
        assert!(should_retry_delivery_error(&anyhow!(
            "transient unknown failure"
        )));
    }

    #[test]
    fn should_retry_delivery_error_stops_retrying_invalid_addresses() {
        let error = anyhow!(AddressError::MissingParts);

        assert!(!should_retry_delivery_error(&error));
    }

    #[test]
    fn should_retry_delivery_error_stops_retrying_message_build_errors() {
        let error = anyhow!(MessageError::EmailMissingDomain);

        assert!(!should_retry_delivery_error(&error));
    }

    #[tokio::test]
    async fn validate_webhook_url_accepts_http_and_https() {
        assert!(
            validate_webhook_url("https://1.1.1.1/hooks/site")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn validate_webhook_url_rejects_non_http_schemes() {
        assert!(validate_webhook_url("ftp://test.com/hook").await.is_err());
    }

    #[tokio::test]
    async fn validate_webhook_url_rejects_loopback_and_private_targets() {
        assert!(
            validate_webhook_url("http://127.0.0.1:8080/webhook")
                .await
                .is_err()
        );
        assert!(
            validate_webhook_url("http://localhost:8080/webhook")
                .await
                .is_err()
        );
        assert!(
            validate_webhook_url("http://192.168.1.10:8080/webhook")
                .await
                .is_err()
        );
    }

    #[test]
    fn validate_email_address_accepts_simple_email() {
        assert!(validate_email_address("alerts@test.com").is_ok());
    }

    #[tokio::test]
    async fn validate_channel_destination_dispatches_by_type() {
        assert!(
            validate_channel_destination(
                NotificationChannelType::Webhook,
                "https://1.1.1.1/hooks/site",
            )
            .await
            .is_ok()
        );
        assert!(
            validate_channel_destination(NotificationChannelType::Email, "alerts@test.com",)
                .await
                .is_ok()
        );
    }

    #[test]
    fn build_payload_includes_top_level_schema_version() -> Result<()> {
        let site_monitor = build_test_monitor(9, "https://notify.test.com/health", 200);
        let site_monitor_check = site_monitor_checks::SiteMonitorCheck {
            id: 44,
            site_monitor_id: site_monitor.id,
            checked_at: Utc::now(),
            monitor_type: site_monitor.monitor_type,
            url_checked: site_monitor.target_url.clone(),
            expected_status_code: Some(site_monitor.expected_status_code),
            is_success: false,
            status_code: Some(500),
            response_time_ms: Some(250),
            total_duration_ms: Some(250),
            attempt_count: 1,
            was_retried: false,
            failure_reason: Some("timeout".to_string()),
            error_message: Some("timed out".to_string()),
            certificate_expires_at: None,
            certificate_days_remaining: None,
            certificate_issuer: None,
            certificate_subject: None,
            certificate_domain: None,
        };
        let payload = serde_json::to_value(build_payload(
            &NotificationContext {
                site_id: 7,
                site_name: "Notify Site".to_string(),
                site_base_url: "https://notify.test.com".to_string(),
            },
            &site_monitor,
            &site_monitor_check,
            NotificationEventType::Failure,
        ))?;

        assert_eq!(
            payload["schema_version"],
            serde_json::Value::from(notification_payload_schema_version())
        );

        Ok(())
    }

    #[test]
    fn notification_payload_deserialization_defaults_schema_version_for_legacy_rows() -> Result<()>
    {
        let legacy_payload = serde_json::json!({
            "event_type": "failure",
            "occurred_at": "2026-04-28T12:00:00Z",
            "summary": "Site 'Notify Site' is DOWN for monitor 9 (https://notify.test.com/health)",
            "email_subject": "[Alon Sentinel] Notify Site is DOWN",
            "email_text": "body",
            "site": {
                "id": 7,
                "name": "Notify Site",
                "base_url": "https://notify.test.com"
            },
            "monitor": {
                "id": 9,
                "monitor_type": "http",
                "target_url": "https://notify.test.com/health",
                "expected_status_code": 200,
                "check_interval_seconds": 60
            },
            "previous_check": {
                "checked_at": null,
                "is_success": null,
                "status_code": null,
                "response_time_ms": null,
                "failure_reason": null,
                "error_message": null
            },
            "current_check": {
                "id": 44,
                "checked_at": "2026-04-28T12:00:00Z",
                "is_success": false,
                "status_code": 500,
                "response_time_ms": 250,
                "failure_reason": "timeout",
                "error_message": "timed out"
            }
        });

        let payload = serde_json::from_value::<NotificationPayload>(legacy_payload)?;

        assert_eq!(
            payload.schema_version,
            notification_payload_schema_version()
        );

        Ok(())
    }

    #[tokio::test]
    async fn deliver_due_notifications_signs_webhook_payloads() -> Result<()> {
        let test_db = TestDb::spawn().await?;
        let (webhook_url, server) =
            spawn_recording_webhook_server(Duration::ZERO, Duration::ZERO).await?;
        let delivery_id = test_db.seed_webhook_delivery(&webhook_url).await?;
        let config = test_config();
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let smtp_mailer = build_smtp_mailer(&config)?;

        let claimed = deliver_due_notifications(
            &test_db.pool,
            smtp_mailer.as_ref(),
            &config,
            "worker-signed",
            shutdown_rx,
        )
        .await?;

        assert_eq!(claimed, 1);
        server.wait_for_request_count(1).await?;

        let delivery = test_db.get_delivery(delivery_id).await?;
        let requests = server.recorded_requests();
        assert_eq!(requests.len(), 1);

        let request = &requests[0];
        assert_eq!(request.delivery_id, delivery_id.to_string());
        assert_eq!(request.sentinel_delivery_id, delivery_id.to_string());
        assert!(!request.timestamp.is_empty());
        assert_eq!(
            request.signature,
            build_webhook_signature(TEST_WEBHOOK_SECRET, &request.timestamp, &request.body)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&request.body)?,
            delivery.payload
        );
        assert_eq!(
            delivery.payload["schema_version"],
            serde_json::Value::from(notification_payload_schema_version())
        );

        test_db.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn send_webhook_notification_rejects_loopback_destination_at_delivery_time() -> Result<()>
    {
        let config = test_config();
        let error = send_webhook_notification_internal(
            &config,
            &WebhookParams {
                delivery_id: 42,
                destination: "http://127.0.0.1:8080/hooks/site",
                webhook_secret_ciphertext: Some(&test_webhook_secret_ciphertext()?),
                payload: &serde_json::json!({"kind": "test"}),
            },
            true,
        )
        .await
        .expect_err("loopback destination should be rejected at send time");

        assert!(
            error
                .to_string()
                .contains("must not resolve to a loopback, private, or link-local address")
        );

        Ok(())
    }

    #[tokio::test]
    async fn delivery_state_updates_require_current_owner() -> Result<()> {
        let test_db = TestDb::spawn().await?;
        let delivery_id = test_db
            .seed_webhook_delivery("https://test.com/hooks/ownership")
            .await?;

        let claimed = notification_deliveries::repository::claim_due_deliveries(
            &test_db.pool,
            "worker-a",
            10,
            60,
        )
        .await?;
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, delivery_id);

        let delivered_by_wrong_worker = notification_deliveries::repository::mark_delivered(
            &test_db.pool,
            delivery_id,
            "worker-b",
        )
        .await?;
        assert!(!delivered_by_wrong_worker);

        let failed_by_wrong_worker = notification_deliveries::repository::mark_failed(
            &test_db.pool,
            delivery_id,
            "worker-b",
            "wrong owner",
            Some(Utc::now()),
        )
        .await?;
        assert!(!failed_by_wrong_worker);

        let delivery = test_db.get_delivery(delivery_id).await?;
        assert_eq!(
            delivery.status,
            notification_deliveries::NotificationDeliveryStatus::Pending
        );
        assert_eq!(delivery.attempts, 0);
        assert_eq!(delivery.claimed_by.as_deref(), Some("worker-a"));

        test_db.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn deliver_due_notifications_extends_claim_while_send_is_in_flight() -> Result<()> {
        let test_db = TestDb::spawn().await?;
        let webhook_url = spawn_slow_webhook_server(Duration::from_millis(1_500)).await?;
        let delivery_id = test_db.seed_webhook_delivery(&webhook_url).await?;
        let mut config = test_config();
        config.lease_notification_delivery_seconds = 1;
        config.notification_delivery_timeout_seconds = 5;
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let smtp_mailer = build_smtp_mailer(&config)?;

        let delivery_task = tokio::spawn({
            let pool = test_db.pool.clone();
            let config = config.clone();
            let smtp_mailer = smtp_mailer.clone();
            async move {
                deliver_due_notifications(
                    &pool,
                    smtp_mailer.as_ref(),
                    &config,
                    "worker-a",
                    shutdown_rx,
                )
                .await
            }
        });

        test_db
            .wait_for_delivery_claim(delivery_id, "worker-a")
            .await?;
        sleep(Duration::from_millis(1_200)).await;
        let competing_claim = notification_deliveries::repository::claim_due_deliveries(
            &test_db.pool,
            "worker-b",
            10,
            1,
        )
        .await?;
        assert!(competing_claim.is_empty());

        let _ = timeout(Duration::from_secs(5), delivery_task).await??;

        let delivery = test_db.get_delivery(delivery_id).await?;
        assert_eq!(
            delivery.status,
            notification_deliveries::NotificationDeliveryStatus::Delivered
        );
        assert_eq!(delivery.attempts, 1);
        assert!(delivery.claimed_by.is_none());
        assert!(delivery.lease_until.is_none());

        test_db.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn deliver_due_notifications_releases_claim_when_shutdown_cancels_in_flight_send()
    -> Result<()> {
        let test_db = TestDb::spawn().await?;
        let webhook_url = spawn_slow_webhook_server(Duration::from_secs(5)).await?;
        let delivery_id = test_db.seed_webhook_delivery(&webhook_url).await?;
        let mut config = test_config();
        config.lease_notification_delivery_seconds = 5;
        config.notification_delivery_timeout_seconds = 10;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let smtp_mailer = build_smtp_mailer(&config)?;

        let delivery_task = tokio::spawn({
            let pool = test_db.pool.clone();
            let config = config.clone();
            let smtp_mailer = smtp_mailer.clone();
            async move {
                deliver_due_notifications(
                    &pool,
                    smtp_mailer.as_ref(),
                    &config,
                    "worker-a",
                    shutdown_rx,
                )
                .await
            }
        });

        test_db
            .wait_for_delivery_claim(delivery_id, "worker-a")
            .await?;
        let _ = shutdown_tx.send(true);
        let _ = timeout(Duration::from_secs(2), delivery_task).await??;

        let delivery = test_db.get_delivery(delivery_id).await?;
        assert_eq!(
            delivery.status,
            notification_deliveries::NotificationDeliveryStatus::Pending
        );
        assert_eq!(delivery.attempts, 0);
        assert!(delivery.claimed_by.is_none());
        assert!(delivery.lease_until.is_none());

        let reclaimed = notification_deliveries::repository::claim_due_deliveries(
            &test_db.pool,
            "worker-b",
            10,
            60,
        )
        .await?;
        assert_eq!(reclaimed.len(), 1);
        assert_eq!(reclaimed[0].id, delivery_id);

        test_db.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn deliver_due_notifications_cancels_when_lease_is_lost_mid_send() -> Result<()> {
        let test_db = TestDb::spawn().await?;
        let webhook_url = spawn_slow_webhook_server(Duration::from_secs(5)).await?;
        let delivery_id = test_db.seed_webhook_delivery(&webhook_url).await?;
        let mut config = test_config();
        config.lease_notification_delivery_seconds = 1;
        config.notification_delivery_timeout_seconds = 2;
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let smtp_mailer = build_smtp_mailer(&config)?;

        let delivery_task = tokio::spawn({
            let pool = test_db.pool.clone();
            let config = config.clone();
            let smtp_mailer = smtp_mailer.clone();
            async move {
                deliver_due_notifications(
                    &pool,
                    smtp_mailer.as_ref(),
                    &config,
                    "worker-a",
                    shutdown_rx,
                )
                .await
            }
        });

        test_db
            .wait_for_delivery_claim(delivery_id, "worker-a")
            .await?;
        sqlx::query(
            r#"
            UPDATE notification_deliveries
            SET
                claimed_at = NOW(),
                lease_until = NOW() + INTERVAL '60 seconds',
                claimed_by = 'worker-b',
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(delivery_id)
        .execute(&test_db.pool)
        .await?;

        let _ = timeout(Duration::from_secs(3), delivery_task).await??;

        let delivery = test_db.get_delivery(delivery_id).await?;
        assert_eq!(
            delivery.status,
            notification_deliveries::NotificationDeliveryStatus::Pending
        );
        assert_eq!(delivery.attempts, 0);
        assert_eq!(delivery.claimed_by.as_deref(), Some("worker-b"));
        assert!(delivery.lease_until.is_some());

        test_db.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn deliver_due_notifications_reclaims_after_lease_loss_and_reuses_idempotency_header()
    -> Result<()> {
        let test_db = TestDb::spawn().await?;
        let (webhook_url, server) =
            spawn_recording_webhook_server(Duration::from_secs(5), Duration::ZERO).await?;
        let delivery_id = test_db.seed_webhook_delivery(&webhook_url).await?;
        let mut config_a = test_config();
        config_a.lease_notification_delivery_seconds = 1;
        config_a.notification_delivery_timeout_seconds = 2;
        let mut config_b = config_a.clone();
        config_b.notification_delivery_timeout_seconds = 5;

        let (_shutdown_tx_a, shutdown_rx_a) = watch::channel(false);
        let smtp_mailer_a = build_smtp_mailer(&config_a)?;
        let worker_a = tokio::spawn({
            let pool = test_db.pool.clone();
            let config = config_a.clone();
            let smtp_mailer = smtp_mailer_a.clone();
            async move {
                deliver_due_notifications(
                    &pool,
                    smtp_mailer.as_ref(),
                    &config,
                    "worker-a",
                    shutdown_rx_a,
                )
                .await
            }
        });

        test_db
            .wait_for_delivery_claim(delivery_id, "worker-a")
            .await?;
        server.wait_for_request_count(1).await?;

        sqlx::query(
            r#"
            UPDATE notification_deliveries
            SET
                claimed_at = NULL,
                lease_until = NOW() - INTERVAL '1 second',
                claimed_by = NULL,
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(delivery_id)
        .execute(&test_db.pool)
        .await?;

        let (_shutdown_tx_b, shutdown_rx_b) = watch::channel(false);
        let smtp_mailer_b = build_smtp_mailer(&config_b)?;
        let worker_b = tokio::spawn({
            let pool = test_db.pool.clone();
            let config = config_b.clone();
            let smtp_mailer = smtp_mailer_b.clone();
            async move {
                deliver_due_notifications(
                    &pool,
                    smtp_mailer.as_ref(),
                    &config,
                    "worker-b",
                    shutdown_rx_b,
                )
                .await
            }
        });

        let worker_b_claimed = timeout(Duration::from_secs(3), worker_b).await???;
        let worker_a_claimed = timeout(Duration::from_secs(3), worker_a).await???;
        assert_eq!(worker_b_claimed, 1);
        assert_eq!(worker_a_claimed, 1);

        server.wait_for_request_count(2).await?;
        let recorded_delivery_ids = server.recorded_delivery_ids();
        let delivery = test_db.get_delivery(delivery_id).await?;
        assert_eq!(
            delivery.status,
            notification_deliveries::NotificationDeliveryStatus::Delivered,
            "unexpected delivery state: attempts={}, last_error={:?}, headers={recorded_delivery_ids:?}",
            delivery.attempts,
            delivery.last_error,
        );
        assert_eq!(delivery.attempts, 1);
        assert!(delivery.claimed_by.is_none());
        assert!(delivery.lease_until.is_none());
        assert_eq!(
            recorded_delivery_ids,
            vec![delivery_id.to_string(), delivery_id.to_string()]
        );

        test_db.cleanup().await?;
        Ok(())
    }

    #[test]
    fn heartbeat_interval_uses_one_third_of_lease_with_one_second_floor() {
        assert_eq!(
            derive_heartbeat_interval(Duration::from_secs(1)),
            Duration::from_nanos(333_333_333)
        );
        assert_eq!(
            derive_heartbeat_interval(Duration::from_secs(6)),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn derive_notification_delivery_lease_plan_covers_timeout_and_buffer() {
        let mut config = test_config();
        config.lease_notification_delivery_seconds = 1;
        config.notification_delivery_timeout_seconds = 5;

        let lease_plan = derive_notification_delivery_lease_plan(&config);

        assert_eq!(lease_plan.initial_lease, Duration::from_millis(7_500));
        assert_eq!(lease_plan.heartbeat_interval, Duration::from_millis(2_500));
        assert_eq!(duration_to_lease_seconds(lease_plan.initial_lease), 8);
    }

    struct TestDb {
        admin_pool: PgPool,
        pool: PgPool,
        schema: String,
    }

    #[derive(Clone)]
    struct RecordedWebhookRequest {
        delivery_id: String,
        sentinel_delivery_id: String,
        timestamp: String,
        signature: String,
        body: Vec<u8>,
    }

    #[derive(Clone)]
    struct RecordingWebhookServer {
        request_count: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<RecordedWebhookRequest>>>,
        request_notify: Arc<Notify>,
    }

    impl RecordingWebhookServer {
        async fn wait_for_request_count(&self, expected: usize) -> Result<()> {
            for _ in 0..80 {
                if self.request_count.load(Ordering::Relaxed) >= expected
                    && self
                        .requests
                        .lock()
                        .expect("lock recorded webhook requests")
                        .len()
                        >= expected
                {
                    return Ok(());
                }

                let notified = self.request_notify.notified();
                tokio::select! {
                    _ = notified => {}
                    _ = sleep(Duration::from_millis(50)) => {}
                }
            }

            anyhow::bail!("timed out waiting for {expected} webhook requests")
        }

        fn recorded_requests(&self) -> Vec<RecordedWebhookRequest> {
            self.requests
                .lock()
                .expect("lock recorded webhook requests")
                .clone()
        }

        fn recorded_delivery_ids(&self) -> Vec<String> {
            self.recorded_requests()
                .into_iter()
                .map(|request| request.delivery_id)
                .collect()
        }
    }

    impl TestDb {
        async fn spawn() -> Result<Self> {
            dotenvy::dotenv().ok();

            let base_database_url = std::env::var("TEST_DATABASE_URL")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .context("set TEST_DATABASE_URL or DATABASE_URL for notification tests")?;
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

            Ok(Self {
                admin_pool,
                pool,
                schema,
            })
        }

        async fn cleanup(self) -> Result<()> {
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

        async fn seed_webhook_delivery(&self, webhook_url: &str) -> Result<i64> {
            let site = sites::repository::create_site(
                &self.pool,
                "Notify Site",
                "https://notify.test.com",
            )
            .await?;
            let monitor = site_monitors::repository::create_http_site_monitor(
                &self.pool,
                site.id,
                &site_monitors::HttpMonitorParams {
                    target_url: "https://notify.test.com/health",
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
            .await?;
            let webhook_secret_ciphertext = test_webhook_secret_ciphertext()?;
            let channel = notification_channels::repository::create_channel(
                &self.pool,
                &notification_channels::NotificationChannelParams {
                    channel_type: NotificationChannelType::Webhook,
                    name: &format!("webhook-{}", unique_schema_name()),
                    destination: webhook_url,
                    webhook_secret_ciphertext: Some(&webhook_secret_ciphertext),
                    notify_on_failure: true,
                    notify_on_recovery: true,
                    is_active: true,
                },
            )
            .await?;

            let mut transaction = self.pool.begin().await?;
            let check = site_monitor_checks::repository::create_site_monitor_check(
                &mut transaction,
                monitor.id,
                &site_monitor_checks::CreateMonitorCheckParams {
                    monitor_type: site_monitors::SiteMonitorType::Http,
                    url_checked: &monitor.target_url,
                    expected_status_code: Some(200),
                    is_success: false,
                    status_code: Some(500),
                    response_time_ms: Some(250),
                    total_duration_ms: Some(250),
                    attempt_count: 1,
                    was_retried: false,
                    failure_reason: Some("timeout"),
                    error_message: Some("webhook timeout"),
                    certificate_expires_at: None,
                    certificate_days_remaining: None,
                    certificate_issuer: None,
                    certificate_subject: None,
                    certificate_domain: None,
                },
            )
            .await?;
            let payload = serde_json::to_value(build_payload(
                &NotificationContext {
                    site_id: site.id,
                    site_name: site.name.clone(),
                    site_base_url: site.base_url.clone(),
                },
                &monitor,
                &check,
                NotificationEventType::Failure,
            ))?;
            notification_deliveries::repository::enqueue_deliveries(
                &mut transaction,
                &[notification_deliveries::NewNotificationDelivery {
                    notification_channel_id: channel.id,
                    site_monitor_id: monitor.id,
                    site_monitor_check_id: check.id,
                    incident_id: None,
                    event_type: NotificationEventType::Failure,
                    payload: &payload,
                }],
            )
            .await?;
            transaction.commit().await?;

            let delivery_id = sqlx::query_scalar(
                r#"
                SELECT id
                FROM notification_deliveries
                WHERE notification_channel_id = $1
                  AND site_monitor_check_id = $2
                LIMIT 1
                "#,
            )
            .bind(channel.id)
            .bind(check.id)
            .fetch_one(&self.pool)
            .await?;

            Ok(delivery_id)
        }

        async fn get_delivery(&self, delivery_id: i64) -> Result<NotificationDelivery> {
            let delivery = sqlx::query_as::<_, NotificationDelivery>(
                r#"
                SELECT
                    id,
                    notification_channel_id,
                    site_monitor_id,
                    site_monitor_check_id,
                    incident_id,
                    event_type,
                    payload,
                    status,
                    attempts,
                    next_attempt_at,
                    claimed_at,
                    lease_until,
                    claimed_by,
                    delivered_at,
                    last_error,
                    created_at,
                    updated_at
                FROM notification_deliveries
                WHERE id = $1
                "#,
            )
            .bind(delivery_id)
            .fetch_one(&self.pool)
            .await?;

            Ok(delivery)
        }

        async fn wait_for_delivery_claim(&self, delivery_id: i64, claimed_by: &str) -> Result<()> {
            for _ in 0..40 {
                let delivery = self.get_delivery(delivery_id).await?;
                if delivery.claimed_by.as_deref() == Some(claimed_by) {
                    return Ok(());
                }

                sleep(Duration::from_millis(50)).await;
            }

            anyhow::bail!(
                "timed out waiting for delivery {delivery_id} to be claimed by {claimed_by}"
            )
        }
    }

    fn test_config() -> Config {
        Config {
            database_url: "postgresql://unused".to_string(),
            api_bind_address: "127.0.0.1:3000".to_string(),
            access_token_ttl_seconds: 3600,
            cors_allowed_origins: Vec::new(),
            db_max_connections: 20,
            db_min_connections: 2,
            db_acquire_timeout_seconds: 5,
            db_idle_timeout_seconds: 600,
            db_max_lifetime_seconds: 1800,
            api_db_max_connections: 20,
            api_db_min_connections: 2,
            worker_db_max_connections: 20,
            worker_db_min_connections: 2,
            worker_max_poll_interval_ms: 5_000,
            site_worker_count: 1,
            max_http_concurrent_checks: 1,
            due_sites_batch_size: 10,
            lease_site_check_seconds: 60,
            http_check_timeout_seconds: 1,
            http_check_max_response_body_bytes: 64 * 1024,
            http_check_max_attempts: 1,
            http_check_retry_delays_ms: Vec::new(),
            http_check_retry_jitter_percent: 0,
            site_monitor_check_retention_days: 90,
            site_monitor_check_retention_interval_seconds: 3600,
            site_monitor_check_retention_batch_size: 5000,
            due_notification_batch_size: 10,
            max_notification_concurrent_deliveries: 1,
            lease_notification_delivery_seconds: 60,
            notification_delivery_timeout_seconds: 5,
            notification_max_attempts: 3,
            notification_retry_base_seconds: 30,
            auth_rate_limit_max_requests: 10,
            auth_rate_limit_window_seconds: 60,
            trust_proxy_headers: false,
            trusted_proxy_ips: Vec::new(),
            cookie_secure: false,
            http_monitor_allow_private_targets: false,
            webhook_secret_encryption_key: test_webhook_secret_encryption_key(),
            smtp: None,
        }
    }

    fn test_webhook_secret_encryption_key() -> WebhookSecretEncryptionKey {
        WebhookSecretEncryptionKey::from_hex(TEST_WEBHOOK_SECRET_ENCRYPTION_KEY_HEX)
            .expect("test webhook secret encryption key should parse")
    }

    fn test_webhook_secret_ciphertext() -> Result<String> {
        test_webhook_secret_encryption_key().encrypt_webhook_secret(TEST_WEBHOOK_SECRET)
    }

    fn unique_schema_name() -> String {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_millis();
        let suffix = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        format!("sentinel_notifications_test_{}_{}", millis, suffix)
    }

    fn build_test_monitor(
        id: i64,
        target_url: &str,
        expected_status_code: i32,
    ) -> site_monitors::SiteMonitor {
        let timestamp = Utc::now();

        site_monitors::SiteMonitor {
            id,
            site_id: 1,
            monitor_type: site_monitors::SiteMonitorType::Http,
            target_url: target_url.to_string(),
            check_interval_seconds: 60,
            expected_status_code,
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
            tcp_target_host: None,
            tcp_target_port: None,
            dns_hostname: None,
            dns_record_type: None,
            dns_expected_value: None,
            dns_nameserver: None,
            heartbeat_token: None,
            heartbeat_grace_seconds: None,
            http_check_timeout_seconds_override: None,
            http_check_max_attempts_override: None,
            http_check_retry_delays_ms_override: None,
            is_active: true,
            check_claimed_at: None,
            check_lease_until: None,
            check_claimed_by: None,
            last_checked_at: None,
            last_successful_check_at: None,
            last_is_success: None,
            last_status_code: None,
            last_response_time_ms: None,
            last_failure_reason: None,
            last_error_message: None,
            last_heartbeat_received_at: None,
            last_certificate_expires_at: None,
            last_certificate_days_remaining: None,
            last_certificate_issuer: None,
            last_certificate_subject: None,
            last_certificate_domain: None,
            created_at: timestamp,
            updated_at: timestamp,
        }
    }

    fn schema_database_url(base_database_url: &str, schema: &str) -> Result<String> {
        let mut url = reqwest::Url::parse(base_database_url)?;
        url.query_pairs_mut()
            .append_pair("options", &format!("-c search_path={},public", schema));
        Ok(url.to_string())
    }

    async fn apply_migrations(pool: &PgPool) -> Result<()> {
        for path in migration_paths()? {
            let sql = fs::read_to_string(&path)
                .with_context(|| format!("failed to read migration {}", path.display()))?;
            sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
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

    async fn spawn_slow_webhook_server(response_delay: Duration) -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept webhook connection");
            let _ = read_webhook_request(&mut stream).await;
            sleep(response_delay).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .expect("write webhook response");
        });

        Ok(format!("http://{}", address))
    }

    async fn spawn_recording_webhook_server(
        first_response_delay: Duration,
        subsequent_response_delay: Duration,
    ) -> Result<(String, RecordingWebhookServer)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = RecordingWebhookServer {
            request_count: Arc::new(AtomicUsize::new(0)),
            requests: Arc::new(Mutex::new(Vec::new())),
            request_notify: Arc::new(Notify::new()),
        };
        let request_count = server.request_count.clone();
        let requests = server.requests.clone();
        let request_notify = server.request_notify.clone();

        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener
                    .accept()
                    .await
                    .expect("accept recording webhook connection");
                let request_count = request_count.clone();
                let requests = requests.clone();
                let request_notify = request_notify.clone();

                tokio::spawn(async move {
                    let request_index = request_count.fetch_add(1, Ordering::Relaxed);
                    let buffer = read_webhook_request(&mut stream)
                        .await
                        .expect("read recording webhook request");
                    let request = parse_recorded_webhook_request(&buffer)
                        .expect("parse recorded webhook request");
                    requests
                        .lock()
                        .expect("lock recorded webhook requests")
                        .push(request);
                    request_notify.notify_waiters();

                    if request_index == 0 {
                        sleep(first_response_delay).await;
                    } else {
                        sleep(subsequent_response_delay).await;
                    }

                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
                        )
                        .await
                        .expect("write recording webhook response");
                });
            }
        });

        Ok((format!("http://{}", address), server))
    }

    fn parse_recorded_webhook_request(buffer: &[u8]) -> Result<RecordedWebhookRequest> {
        let header_end = buffer
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|position| position + 4)
            .context("recorded webhook request is missing a header terminator")?;
        let header_text = String::from_utf8_lossy(&buffer[..header_end]);
        let header_value = |name: &str| {
            header_text.lines().find_map(|line| {
                let (header_name, value) = line.split_once(':')?;
                header_name
                    .eq_ignore_ascii_case(name)
                    .then(|| value.trim().to_string())
            })
        };

        Ok(RecordedWebhookRequest {
            delivery_id: header_value("x-alon-delivery-id").unwrap_or_default(),
            sentinel_delivery_id: header_value("x-sentinel-delivery-id").unwrap_or_default(),
            timestamp: header_value("x-sentinel-timestamp").unwrap_or_default(),
            signature: header_value("x-sentinel-signature").unwrap_or_default(),
            body: buffer[header_end..].to_vec(),
        })
    }

    async fn read_webhook_request(stream: &mut tokio::net::TcpStream) -> Result<Vec<u8>> {
        let mut buffer = Vec::with_capacity(2048);
        let header_end = loop {
            let mut chunk = [0_u8; 1024];
            let bytes_read = stream.read(&mut chunk).await?;
            if bytes_read == 0 {
                anyhow::bail!("webhook client closed connection before sending headers");
            }

            buffer.extend_from_slice(&chunk[..bytes_read]);
            if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };

        let header_text = String::from_utf8_lossy(&buffer[..header_end]);
        let content_length = header_text
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let mut body_bytes_read = buffer.len().saturating_sub(header_end);

        while body_bytes_read < content_length {
            let mut chunk = [0_u8; 1024];
            let bytes_read = stream.read(&mut chunk).await?;
            if bytes_read == 0 {
                anyhow::bail!(
                    "webhook client closed connection before sending full body ({body_bytes_read}/{content_length} bytes)"
                );
            }

            buffer.extend_from_slice(&chunk[..bytes_read]);
            body_bytes_read += bytes_read;
        }

        Ok(buffer)
    }
}
