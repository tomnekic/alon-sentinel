ALTER TABLE site_monitors
ADD COLUMN json_path_exists TEXT[],
ADD COLUMN json_path_equals JSONB,
ADD COLUMN json_path_not_equals JSONB;
