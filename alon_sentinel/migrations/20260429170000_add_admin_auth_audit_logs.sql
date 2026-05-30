CREATE TABLE admin_auth_audit_logs (
    id BIGSERIAL PRIMARY KEY,
    admin_user_id BIGINT NULL REFERENCES admin_users(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    ip_address INET NULL,
    user_agent TEXT NULL,
    meta_json JSONB NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_admin_auth_audit_logs_admin_user_id
    ON admin_auth_audit_logs(admin_user_id);
CREATE INDEX idx_admin_auth_audit_logs_created_at
    ON admin_auth_audit_logs(created_at);
