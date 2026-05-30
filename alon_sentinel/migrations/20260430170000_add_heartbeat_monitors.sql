ALTER TYPE site_monitor_type ADD VALUE IF NOT EXISTS 'heartbeat';

ALTER TABLE site_monitors
    ADD COLUMN heartbeat_token TEXT,
    ADD COLUMN heartbeat_grace_seconds INTEGER,
    ADD COLUMN last_heartbeat_received_at TIMESTAMPTZ;

CREATE UNIQUE INDEX uq_site_monitors_heartbeat_token
    ON site_monitors(heartbeat_token)
    WHERE heartbeat_token IS NOT NULL;
