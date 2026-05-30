ALTER TYPE site_monitor_type ADD VALUE IF NOT EXISTS 'dns';

ALTER TABLE site_monitors
    ADD COLUMN IF NOT EXISTS dns_hostname TEXT,
    ADD COLUMN IF NOT EXISTS dns_record_type TEXT,
    ADD COLUMN IF NOT EXISTS dns_expected_value TEXT,
    ADD COLUMN IF NOT EXISTS dns_nameserver TEXT;
