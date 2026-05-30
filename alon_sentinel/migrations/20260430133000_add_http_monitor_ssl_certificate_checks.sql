ALTER TABLE site_monitors
    ADD COLUMN ssl_certificate_checks_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN ssl_expiry_warning_days INTEGER,
    ADD COLUMN last_certificate_expires_at TIMESTAMPTZ,
    ADD COLUMN last_certificate_days_remaining INTEGER,
    ADD COLUMN last_certificate_issuer TEXT,
    ADD COLUMN last_certificate_subject TEXT,
    ADD COLUMN last_certificate_domain TEXT;

ALTER TABLE site_monitor_checks
    ADD COLUMN certificate_expires_at TIMESTAMPTZ,
    ADD COLUMN certificate_days_remaining INTEGER,
    ADD COLUMN certificate_issuer TEXT,
    ADD COLUMN certificate_subject TEXT,
    ADD COLUMN certificate_domain TEXT;
