CREATE TABLE site_status_pages (
    id              BIGSERIAL PRIMARY KEY,
    site_id         BIGINT NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    is_enabled      BOOLEAN NOT NULL DEFAULT FALSE,
    slug            TEXT NOT NULL,
    page_title      TEXT,
    show_monitor_details    BOOLEAN NOT NULL DEFAULT TRUE,
    show_uptime_percentages BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX idx_site_status_pages_site_id ON site_status_pages(site_id);
CREATE UNIQUE INDEX idx_site_status_pages_slug    ON site_status_pages(slug);
