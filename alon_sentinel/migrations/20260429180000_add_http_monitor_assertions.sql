ALTER TABLE site_monitors
ADD COLUMN body_must_contain TEXT,
ADD COLUMN body_must_not_contain TEXT,
ADD COLUMN max_response_time_ms INTEGER;
