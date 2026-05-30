ALTER TABLE site_monitors
ADD COLUMN http_check_timeout_seconds_override INTEGER,
ADD COLUMN http_check_max_attempts_override INTEGER,
ADD COLUMN http_check_retry_delays_ms_override BIGINT[];
