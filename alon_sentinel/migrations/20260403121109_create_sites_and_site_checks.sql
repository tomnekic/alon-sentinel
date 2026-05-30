CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TYPE site_monitor_type AS ENUM ('http');

CREATE TABLE sites (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_sites_base_url UNIQUE (base_url)
);

CREATE TABLE site_monitors (
    id BIGSERIAL PRIMARY KEY,
    site_id BIGINT NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    monitor_type site_monitor_type NOT NULL DEFAULT 'http',
    target_url TEXT NOT NULL,
    check_interval_seconds INTEGER NOT NULL DEFAULT 300,
    expected_status_code INTEGER NOT NULL DEFAULT 200,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    check_claimed_at TIMESTAMPTZ,
    check_lease_until TIMESTAMPTZ,
    check_claimed_by TEXT,
    last_checked_at TIMESTAMPTZ,
    last_successful_check_at TIMESTAMPTZ,
    last_is_success BOOLEAN,
    last_status_code INTEGER,
    last_response_time_ms INTEGER,
    last_error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_site_monitors_site_id_monitor_type_target_url UNIQUE (site_id, monitor_type, target_url)
);

CREATE TABLE site_monitor_checks (
    id BIGSERIAL PRIMARY KEY,
    site_monitor_id BIGINT NOT NULL REFERENCES site_monitors(id) ON DELETE CASCADE,
    checked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    monitor_type site_monitor_type NOT NULL DEFAULT 'http',
    url_checked TEXT NOT NULL,
    expected_status_code INTEGER,
    is_success BOOLEAN NOT NULL,
    status_code INTEGER,
    response_time_ms INTEGER,
    failure_reason TEXT,
    error_message TEXT
);

CREATE INDEX idx_sites_is_active ON sites(is_active);

CREATE INDEX idx_site_monitors_site_id ON site_monitors(site_id);
CREATE INDEX idx_site_monitors_is_active ON site_monitors(is_active);
CREATE INDEX idx_site_monitors_last_checked_at ON site_monitors(last_checked_at);
CREATE INDEX idx_site_monitors_check_lease_until ON site_monitors(check_lease_until);

CREATE INDEX idx_site_monitor_checks_site_monitor_id ON site_monitor_checks(site_monitor_id);
CREATE INDEX idx_site_monitor_checks_checked_at ON site_monitor_checks(checked_at);
CREATE INDEX idx_site_monitor_checks_site_monitor_id_checked_at ON site_monitor_checks(site_monitor_id, checked_at DESC);
