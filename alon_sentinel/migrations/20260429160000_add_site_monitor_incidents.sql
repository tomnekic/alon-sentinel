CREATE TYPE site_monitor_incident_status AS ENUM ('open', 'resolved');

CREATE TYPE site_monitor_incident_resolved_reason AS ENUM (
    'recovered',
    'monitoring_disabled',
    'site_deactivated'
);

CREATE TABLE site_monitor_incidents (
    id                        BIGSERIAL PRIMARY KEY,

    site_id                   BIGINT NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    site_monitor_id           BIGINT NOT NULL REFERENCES site_monitors(id) ON DELETE CASCADE,

    status                    site_monitor_incident_status NOT NULL DEFAULT 'open',

    opened_at                 TIMESTAMPTZ NOT NULL,
    resolved_at               TIMESTAMPTZ,

    -- SET NULL so incidents survive check pruning
    opened_check_id           BIGINT REFERENCES site_monitor_checks(id) ON DELETE SET NULL,
    resolved_check_id         BIGINT REFERENCES site_monitor_checks(id) ON DELETE SET NULL,
    last_check_id             BIGINT REFERENCES site_monitor_checks(id) ON DELETE SET NULL,

    -- monitor snapshot at open time (survives monitor edits / deletion)
    monitor_type              site_monitor_type NOT NULL,
    target_url                TEXT NOT NULL,
    expected_status_code      INTEGER NOT NULL,

    -- failure snapshot at open time (survives check pruning)
    opened_status_code        INTEGER,
    opened_failure_reason     TEXT,
    opened_error_message      TEXT,

    -- rolling aggregate updated on each subsequent failing check
    failure_count             INTEGER NOT NULL DEFAULT 1,
    last_checked_at           TIMESTAMPTZ NOT NULL,
    last_status_code          INTEGER,
    last_failure_reason       TEXT,
    last_error_message        TEXT,

    -- resolution snapshot
    resolved_reason           site_monitor_incident_resolved_reason,
    resolved_status_code      INTEGER,
    resolved_response_time_ms INTEGER,
    downtime_seconds          INTEGER,

    -- acknowledgement
    acknowledged_at           TIMESTAMPTZ,
    acknowledged_by           BIGINT REFERENCES admin_users(id) ON DELETE SET NULL,

    created_at                TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at                TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT chk_incident_resolved_fields CHECK (
        (status = 'open'     AND resolved_at IS NULL     AND resolved_reason IS NULL)
        OR
        (status = 'resolved' AND resolved_at IS NOT NULL AND resolved_reason IS NOT NULL)
    )
);

-- Exactly one open incident per monitor at a time
CREATE UNIQUE INDEX uq_site_monitor_incidents_one_open_per_monitor
    ON site_monitor_incidents(site_monitor_id)
    WHERE status = 'open';

CREATE INDEX idx_site_monitor_incidents_site_id_opened_at
    ON site_monitor_incidents(site_id, opened_at DESC);

CREATE INDEX idx_site_monitor_incidents_site_monitor_id_opened_at
    ON site_monitor_incidents(site_monitor_id, opened_at DESC);

CREATE INDEX idx_site_monitor_incidents_status_opened_at
    ON site_monitor_incidents(status, opened_at DESC);

CREATE INDEX idx_site_monitor_incidents_opened_check_id
    ON site_monitor_incidents(opened_check_id)
    WHERE opened_check_id IS NOT NULL;

CREATE INDEX idx_site_monitor_incidents_resolved_check_id
    ON site_monitor_incidents(resolved_check_id)
    WHERE resolved_check_id IS NOT NULL;

CREATE INDEX idx_site_monitor_incidents_acknowledged_by
    ON site_monitor_incidents(acknowledged_by)
    WHERE acknowledged_by IS NOT NULL;

-- Link notification deliveries to the incident that triggered them
ALTER TABLE notification_deliveries
    ADD COLUMN incident_id BIGINT REFERENCES site_monitor_incidents(id) ON DELETE SET NULL;

CREATE INDEX idx_notification_deliveries_incident_id
    ON notification_deliveries(incident_id)
    WHERE incident_id IS NOT NULL;

-- Seed incidents.write permission and assign to operator/admin
INSERT INTO permissions (key, name, description)
VALUES ('incidents.write', 'Write Incidents', 'Acknowledge open and resolved incidents.')
ON CONFLICT (key) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
INNER JOIN permissions p ON p.key = 'incidents.write'
WHERE r.key IN ('operator', 'admin')
ON CONFLICT (role_id, permission_id) DO NOTHING;
