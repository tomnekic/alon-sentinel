CREATE TYPE api_client_type AS ENUM ('internal_service', 'installation_client');

CREATE TABLE api_clients (
    id BIGSERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    description TEXT NULL,
    type api_client_type NOT NULL,
    client_id TEXT NOT NULL,
    client_secret_hash TEXT NOT NULL,
    secret_prefix TEXT NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    last_used_at TIMESTAMPTZ NULL,
    created_by_user_id TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_api_clients_uuid UNIQUE (uuid),
    CONSTRAINT uq_api_clients_client_id UNIQUE (client_id),
    CONSTRAINT chk_api_clients_secret_prefix_not_blank
        CHECK (btrim(secret_prefix) <> '')
);

CREATE INDEX idx_api_clients_type ON api_clients(type);
CREATE INDEX idx_api_clients_is_active ON api_clients(is_active);

CREATE TABLE api_client_scopes (
    id BIGSERIAL PRIMARY KEY,
    api_client_id BIGINT NOT NULL REFERENCES api_clients(id) ON DELETE CASCADE,
    scope TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_api_client_scopes_client_scope UNIQUE (api_client_id, scope)
);

CREATE TABLE access_tokens (
    id BIGSERIAL PRIMARY KEY,
    api_client_id BIGINT NOT NULL REFERENCES api_clients(id) ON DELETE CASCADE,
    token_hash CHAR(64) NOT NULL,
    token_prefix TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ NULL,
    revoked_reason TEXT NULL,
    last_used_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_access_tokens_token_hash UNIQUE (token_hash),
    CONSTRAINT chk_access_tokens_token_hash_hex
        CHECK (token_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT chk_access_tokens_revocation
        CHECK (
            revoked_at IS NOT NULL
            OR revoked_reason IS NULL
        )
);

CREATE INDEX idx_access_tokens_api_client_id ON access_tokens(api_client_id);
CREATE INDEX idx_access_tokens_expires_at ON access_tokens(expires_at);
CREATE INDEX idx_access_tokens_revoked_at ON access_tokens(revoked_at);
CREATE INDEX idx_access_tokens_last_used_at ON access_tokens(last_used_at);

CREATE TABLE api_client_audit_logs (
    id BIGSERIAL PRIMARY KEY,
    api_client_id BIGINT NULL REFERENCES api_clients(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    ip_address INET NULL,
    user_agent TEXT NULL,
    meta_json JSONB NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_api_client_audit_logs_api_client_id
    ON api_client_audit_logs(api_client_id);
CREATE INDEX idx_api_client_audit_logs_created_at
    ON api_client_audit_logs(created_at);
