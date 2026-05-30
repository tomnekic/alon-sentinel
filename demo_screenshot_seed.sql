BEGIN;

DELETE FROM notification_deliveries;
DELETE FROM site_monitor_incidents;
DELETE FROM site_monitor_checks;
DELETE FROM notification_channels
WHERE name IN ('Ops Slack', 'Status Email', 'Incident Webhook');

INSERT INTO notification_channels (
    channel_type,
    name,
    destination,
    notify_on_failure,
    notify_on_recovery,
    is_active
)
VALUES
    ('slack', 'Ops Slack', 'https://hooks.slack.example/services/sentinel-demo', true, true, false),
    ('email', 'Status Email', 'oncall@example.com', true, true, false),
    ('webhook', 'Incident Webhook', 'https://status.example.com/hooks/sentinel', true, true, false);

CREATE TEMP TABLE demo_checks AS
WITH slots AS (
    SELECT
        sm.id AS site_monitor_id,
        sm.monitor_type,
        sm.target_url,
        sm.expected_status_code,
        sm.ssl_expiry_warning_days,
        sm.ssl_certificate_checks_enabled,
        gs.checked_at,
        row_number() OVER (PARTITION BY sm.id ORDER BY gs.checked_at) AS sample_no
    FROM site_monitors sm
    CROSS JOIN LATERAL generate_series(
        date_trunc('hour', now()) - interval '13 days',
        date_trunc('hour', now()),
        interval '6 hours'
    ) AS gs(checked_at)
),
classified AS (
    SELECT
        s.*,
        CASE
            WHEN s.target_url = 'dns://cloudflare.com/A'
                AND s.checked_at BETWEEN now() - interval '3 days 6 hours' AND now() - interval '2 days 6 hours'
                THEN false
            WHEN s.target_url = 'https://api.github.com'
                AND s.monitor_type = 'http'
                AND s.checked_at BETWEEN now() - interval '7 days 12 hours' AND now() - interval '7 days'
                THEN false
            WHEN s.target_url = 'https://httpbin.org/get'
                AND s.checked_at >= now() - interval '18 hours'
                THEN false
            WHEN s.monitor_type = 'heartbeat'
                AND s.checked_at >= now() - interval '30 hours'
                THEN false
            ELSE true
        END AS is_success
    FROM slots s
),
inserted AS (
    INSERT INTO site_monitor_checks (
        site_monitor_id,
        checked_at,
        monitor_type,
        url_checked,
        expected_status_code,
        is_success,
        status_code,
        response_time_ms,
        failure_reason,
        error_message,
        attempt_count,
        was_retried,
        total_duration_ms,
        certificate_expires_at,
        certificate_days_remaining,
        certificate_issuer,
        certificate_subject,
        certificate_domain
    )
    SELECT
        c.site_monitor_id,
        c.checked_at,
        c.monitor_type,
        c.target_url,
        c.expected_status_code,
        c.is_success,
        CASE
            WHEN c.is_success AND c.monitor_type IN ('http', 'ssl') THEN 200
            WHEN NOT c.is_success AND c.monitor_type = 'http' THEN 503
            ELSE NULL
        END,
        CASE
            WHEN NOT c.is_success THEN NULL
            WHEN c.target_url = 'https://github.com' THEN 118 + (c.sample_no % 6) * 13
            WHEN c.target_url = 'https://www.cloudflare.com' THEN 96 + (c.sample_no % 5) * 11
            WHEN c.target_url = 'https://www.google.com' THEN 82 + (c.sample_no % 4) * 9
            WHEN c.target_url = 'https://news.ycombinator.com' THEN 210 + (c.sample_no % 7) * 21
            WHEN c.target_url = 'https://api.github.com' THEN 145 + (c.sample_no % 5) * 18
            WHEN c.target_url = 'https://jsonplaceholder.typicode.com/posts/1' THEN 132 + (c.sample_no % 5) * 15
            WHEN c.target_url = 'https://httpbin.org/get' THEN 260 + (c.sample_no % 6) * 35
            WHEN c.monitor_type = 'tcp' THEN 38 + (c.sample_no % 5) * 7
            WHEN c.monitor_type = 'dns' THEN 24 + (c.sample_no % 4) * 5
            WHEN c.monitor_type = 'heartbeat' THEN 12
            ELSE 180
        END,
        CASE
            WHEN c.is_success THEN NULL
            WHEN c.monitor_type = 'dns' THEN 'dns_value_mismatch'
            WHEN c.monitor_type = 'heartbeat' THEN 'heartbeat_overdue'
            ELSE 'unexpected_status'
        END,
        CASE
            WHEN c.is_success THEN NULL
            WHEN c.monitor_type = 'dns' THEN 'Expected Cloudflare A record did not match the observed answer set.'
            WHEN c.monitor_type = 'heartbeat' THEN 'No heartbeat received inside the configured grace window.'
            ELSE 'Service returned HTTP 503 during the demo incident window.'
        END,
        CASE WHEN c.is_success THEN 1 ELSE 3 END,
        NOT c.is_success,
        CASE
            WHEN NOT c.is_success THEN 3020
            ELSE 100 + (c.sample_no % 8) * 17
        END,
        CASE
            WHEN c.monitor_type = 'ssl' OR c.ssl_certificate_checks_enabled
                THEN now() + interval '74 days'
            ELSE NULL
        END,
        CASE
            WHEN c.monitor_type = 'ssl' OR c.ssl_certificate_checks_enabled
                THEN 74
            ELSE NULL
        END,
        CASE
            WHEN c.monitor_type = 'ssl' OR c.ssl_certificate_checks_enabled
                THEN 'Let''s Encrypt'
            ELSE NULL
        END,
        CASE
            WHEN c.monitor_type = 'ssl' OR c.ssl_certificate_checks_enabled
                THEN c.target_url
            ELSE NULL
        END,
        CASE
            WHEN c.monitor_type = 'ssl' OR c.ssl_certificate_checks_enabled
                THEN replace(replace(c.target_url, 'https://', ''), '/', '')
            ELSE NULL
        END
    FROM classified c
    RETURNING *
)
SELECT * FROM inserted;

WITH latest AS (
    SELECT DISTINCT ON (site_monitor_id)
        site_monitor_id,
        checked_at,
        is_success,
        status_code,
        response_time_ms,
        error_message,
        failure_reason,
        certificate_expires_at,
        certificate_days_remaining,
        certificate_issuer,
        certificate_subject,
        certificate_domain
    FROM demo_checks
    ORDER BY site_monitor_id, checked_at DESC
),
latest_success AS (
    SELECT site_monitor_id, max(checked_at) AS checked_at
    FROM demo_checks
    WHERE is_success
    GROUP BY site_monitor_id
)
UPDATE site_monitors sm
SET
    last_checked_at = latest.checked_at,
    last_successful_check_at = latest_success.checked_at,
    last_is_success = latest.is_success,
    last_status_code = latest.status_code,
    last_response_time_ms = latest.response_time_ms,
    last_error_message = latest.error_message,
    last_failure_reason = latest.failure_reason,
    last_certificate_expires_at = latest.certificate_expires_at,
    last_certificate_days_remaining = latest.certificate_days_remaining,
    last_certificate_issuer = latest.certificate_issuer,
    last_certificate_subject = latest.certificate_subject,
    last_certificate_domain = latest.certificate_domain,
    last_heartbeat_received_at = CASE
        WHEN sm.monitor_type = 'heartbeat' THEN latest_success.checked_at
        ELSE sm.last_heartbeat_received_at
    END,
    updated_at = now()
FROM latest
LEFT JOIN latest_success ON latest_success.site_monitor_id = latest.site_monitor_id
WHERE sm.id = latest.site_monitor_id;

WITH incident_windows AS (
    SELECT
        sm.site_id,
        sm.id AS site_monitor_id,
        sm.monitor_type,
        sm.target_url,
        sm.expected_status_code,
        min(c.checked_at) AS opened_at,
        max(c.checked_at) AS last_checked_at,
        count(*)::integer AS failure_count,
        min(c.id) AS opened_check_id,
        max(c.id) AS last_check_id,
        max(c.status_code) AS last_status_code,
        max(c.failure_reason) AS last_failure_reason,
        max(c.error_message) AS last_error_message,
        bool_or(sm.target_url = 'https://httpbin.org/get' OR sm.monitor_type = 'heartbeat') AS should_stay_open
    FROM demo_checks c
    JOIN site_monitors sm ON sm.id = c.site_monitor_id
    WHERE NOT c.is_success
    GROUP BY sm.site_id, sm.id, sm.monitor_type, sm.target_url, sm.expected_status_code
),
resolved_check AS (
    SELECT DISTINCT ON (iw.site_monitor_id)
        iw.site_monitor_id,
        c.id AS resolved_check_id,
        c.checked_at AS resolved_at,
        c.status_code AS resolved_status_code,
        c.response_time_ms AS resolved_response_time_ms
    FROM incident_windows iw
    JOIN demo_checks c
        ON c.site_monitor_id = iw.site_monitor_id
       AND c.is_success
       AND c.checked_at > iw.last_checked_at
    WHERE NOT iw.should_stay_open
    ORDER BY iw.site_monitor_id, c.checked_at
),
inserted_incidents AS (
    INSERT INTO site_monitor_incidents (
        site_id,
        site_monitor_id,
        status,
        opened_at,
        resolved_at,
        opened_check_id,
        resolved_check_id,
        last_check_id,
        monitor_type,
        target_url,
        expected_status_code,
        opened_status_code,
        opened_failure_reason,
        opened_error_message,
        failure_count,
        last_checked_at,
        last_status_code,
        last_failure_reason,
        last_error_message,
        resolved_reason,
        resolved_status_code,
        resolved_response_time_ms,
        downtime_seconds,
        created_at,
        updated_at
    )
    SELECT
        iw.site_id,
        iw.site_monitor_id,
        CASE WHEN iw.should_stay_open THEN 'open'::site_monitor_incident_status ELSE 'resolved'::site_monitor_incident_status END,
        iw.opened_at,
        CASE WHEN iw.should_stay_open THEN NULL ELSE rc.resolved_at END,
        iw.opened_check_id,
        CASE WHEN iw.should_stay_open THEN NULL ELSE rc.resolved_check_id END,
        CASE WHEN iw.should_stay_open THEN iw.last_check_id ELSE rc.resolved_check_id END,
        iw.monitor_type,
        iw.target_url,
        iw.expected_status_code,
        iw.last_status_code,
        iw.last_failure_reason,
        iw.last_error_message,
        iw.failure_count,
        CASE WHEN iw.should_stay_open THEN iw.last_checked_at ELSE rc.resolved_at END,
        CASE WHEN iw.should_stay_open THEN iw.last_status_code ELSE rc.resolved_status_code END,
        CASE WHEN iw.should_stay_open THEN iw.last_failure_reason ELSE NULL END,
        CASE WHEN iw.should_stay_open THEN iw.last_error_message ELSE NULL END,
        CASE WHEN iw.should_stay_open THEN NULL ELSE 'recovered'::site_monitor_incident_resolved_reason END,
        CASE WHEN iw.should_stay_open THEN NULL ELSE rc.resolved_status_code END,
        CASE WHEN iw.should_stay_open THEN NULL ELSE rc.resolved_response_time_ms END,
        CASE
            WHEN iw.should_stay_open THEN EXTRACT(EPOCH FROM (now() - iw.opened_at))::integer
            ELSE EXTRACT(EPOCH FROM (rc.resolved_at - iw.opened_at))::integer
        END,
        iw.opened_at,
        now()
    FROM incident_windows iw
    LEFT JOIN resolved_check rc ON rc.site_monitor_id = iw.site_monitor_id
    RETURNING *
)
INSERT INTO notification_deliveries (
    notification_channel_id,
    site_monitor_id,
    site_monitor_check_id,
    event_type,
    payload,
    status,
    attempts,
    next_attempt_at,
    delivered_at,
    last_error,
    incident_id,
    created_at,
    updated_at
)
SELECT
    nc.id,
    ii.site_monitor_id,
    ii.opened_check_id,
    'failure',
    jsonb_build_object(
        'site_id', ii.site_id,
        'monitor_id', ii.site_monitor_id,
        'incident_id', ii.id,
        'status', ii.status,
        'target', ii.target_url,
        'message', ii.opened_error_message
    ),
    CASE WHEN nc.channel_type = 'webhook' THEN 'failed'::notification_delivery_status ELSE 'delivered'::notification_delivery_status END,
    CASE WHEN nc.channel_type = 'webhook' THEN 2 ELSE 1 END,
    CASE WHEN nc.channel_type = 'webhook' THEN now() + interval '15 minutes' ELSE NULL END,
    CASE WHEN nc.channel_type = 'webhook' THEN NULL ELSE ii.opened_at + interval '2 minutes' END,
    CASE WHEN nc.channel_type = 'webhook' THEN 'Demo endpoint returned HTTP 429; retry scheduled.' ELSE NULL END,
    ii.id,
    ii.opened_at,
    now()
FROM site_monitor_incidents ii
CROSS JOIN notification_channels nc
WHERE nc.name IN ('Ops Slack', 'Status Email', 'Incident Webhook')
  AND nc.name <> 'Status Email';

COMMIT;
