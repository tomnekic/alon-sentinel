CREATE TYPE notification_event_type AS ENUM ('failure', 'recovery');
CREATE TYPE notification_delivery_status AS ENUM ('pending', 'delivered', 'failed');
CREATE TYPE notification_channel_type AS ENUM ('webhook', 'email');

CREATE TABLE notification_channels (
    id BIGSERIAL PRIMARY KEY,
    channel_type notification_channel_type NOT NULL,
    name TEXT NOT NULL,
    destination TEXT NOT NULL,
    notify_on_failure BOOLEAN NOT NULL DEFAULT TRUE,
    notify_on_recovery BOOLEAN NOT NULL DEFAULT TRUE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_notification_channels_name UNIQUE (name)
);

CREATE TABLE site_notification_channel_overrides (
    id BIGSERIAL PRIMARY KEY,
    site_id BIGINT NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    notification_channel_id BIGINT NOT NULL REFERENCES notification_channels(id) ON DELETE CASCADE,
    notify_on_failure BOOLEAN,
    notify_on_recovery BOOLEAN,
    is_active BOOLEAN,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_site_notification_channel_overrides_site_id_channel_id
        UNIQUE (site_id, notification_channel_id)
);

CREATE TABLE notification_deliveries (
    id BIGSERIAL PRIMARY KEY,
    notification_channel_id BIGINT NOT NULL REFERENCES notification_channels(id) ON DELETE CASCADE,
    site_monitor_id BIGINT NOT NULL REFERENCES site_monitors(id) ON DELETE CASCADE,
    site_monitor_check_id BIGINT NOT NULL REFERENCES site_monitor_checks(id) ON DELETE CASCADE,
    event_type notification_event_type NOT NULL,
    payload JSONB NOT NULL,
    status notification_delivery_status NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    claimed_by TEXT,
    delivered_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_notification_deliveries_channel_check_event
        UNIQUE (notification_channel_id, site_monitor_check_id, event_type)
);

CREATE INDEX idx_notification_channels_is_active ON notification_channels(is_active);

CREATE INDEX idx_site_notification_channel_overrides_site_id
    ON site_notification_channel_overrides(site_id);
CREATE INDEX idx_site_notification_channel_overrides_channel_id
    ON site_notification_channel_overrides(notification_channel_id);

CREATE INDEX idx_notification_deliveries_status_next_attempt_at
    ON notification_deliveries(status, next_attempt_at);
CREATE INDEX idx_notification_deliveries_lease_until
    ON notification_deliveries(lease_until);
CREATE INDEX idx_notification_deliveries_site_monitor_id
    ON notification_deliveries(site_monitor_id);
