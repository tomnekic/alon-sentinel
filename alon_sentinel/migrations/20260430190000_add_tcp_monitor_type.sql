ALTER TYPE site_monitor_type ADD VALUE IF NOT EXISTS 'tcp';

ALTER TABLE site_monitors
    ADD COLUMN IF NOT EXISTS tcp_target_host TEXT,
    ADD COLUMN IF NOT EXISTS tcp_target_port INTEGER;
