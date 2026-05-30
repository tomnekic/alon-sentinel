ALTER TABLE site_monitors
ADD COLUMN body_must_contain_texts TEXT[],
ADD COLUMN body_must_not_contain_texts TEXT[];
