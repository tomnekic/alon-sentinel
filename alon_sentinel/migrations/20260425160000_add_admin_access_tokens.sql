CREATE TABLE admin_access_tokens (
    id BIGSERIAL PRIMARY KEY,
    admin_user_id BIGINT NOT NULL REFERENCES admin_users(id) ON DELETE CASCADE,
    token_hash CHAR(64) NOT NULL,
    token_prefix TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ NULL,
    revoked_reason TEXT NULL,
    last_used_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_admin_access_tokens_token_hash UNIQUE (token_hash),
    CONSTRAINT chk_admin_access_tokens_token_hash_hex
        CHECK (token_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT chk_admin_access_tokens_token_prefix_not_blank
        CHECK (btrim(token_prefix) <> ''),
    CONSTRAINT chk_admin_access_tokens_revocation
        CHECK (
            revoked_at IS NOT NULL
            OR revoked_reason IS NULL
        )
);

CREATE INDEX idx_admin_access_tokens_admin_user_id
    ON admin_access_tokens(admin_user_id);
CREATE INDEX idx_admin_access_tokens_expires_at
    ON admin_access_tokens(expires_at);
CREATE INDEX idx_admin_access_tokens_revoked_at
    ON admin_access_tokens(revoked_at);
CREATE INDEX idx_admin_access_tokens_last_used_at
    ON admin_access_tokens(last_used_at);
