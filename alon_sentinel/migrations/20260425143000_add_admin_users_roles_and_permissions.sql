CREATE TABLE admin_users (
    id BIGSERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid(),
    email TEXT NOT NULL,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    last_login_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_admin_users_uuid UNIQUE (uuid),
    CONSTRAINT uq_admin_users_email UNIQUE (email),
    CONSTRAINT chk_admin_users_email_not_blank
        CHECK (btrim(email) <> ''),
    CONSTRAINT chk_admin_users_display_name_not_blank
        CHECK (btrim(display_name) <> ''),
    CONSTRAINT chk_admin_users_password_hash_not_blank
        CHECK (btrim(password_hash) <> '')
);

CREATE INDEX idx_admin_users_is_active ON admin_users(is_active);
CREATE INDEX idx_admin_users_last_login_at ON admin_users(last_login_at);

CREATE TABLE roles (
    id BIGSERIAL PRIMARY KEY,
    key TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NULL,
    is_system BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_roles_key UNIQUE (key),
    CONSTRAINT chk_roles_key_format
        CHECK (key ~ '^[a-z0-9._:-]+$'),
    CONSTRAINT chk_roles_name_not_blank
        CHECK (btrim(name) <> '')
);

CREATE INDEX idx_roles_is_system ON roles(is_system);

CREATE TABLE permissions (
    id BIGSERIAL PRIMARY KEY,
    key TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_permissions_key UNIQUE (key),
    CONSTRAINT chk_permissions_key_format
        CHECK (key ~ '^[a-z0-9._:-]+$'),
    CONSTRAINT chk_permissions_name_not_blank
        CHECK (btrim(name) <> '')
);

CREATE TABLE admin_user_roles (
    id BIGSERIAL PRIMARY KEY,
    admin_user_id BIGINT NOT NULL REFERENCES admin_users(id) ON DELETE CASCADE,
    role_id BIGINT NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_admin_user_roles_user_role UNIQUE (admin_user_id, role_id)
);

CREATE INDEX idx_admin_user_roles_admin_user_id
    ON admin_user_roles(admin_user_id);
CREATE INDEX idx_admin_user_roles_role_id
    ON admin_user_roles(role_id);

CREATE TABLE role_permissions (
    id BIGSERIAL PRIMARY KEY,
    role_id BIGINT NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id BIGINT NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_role_permissions_role_permission UNIQUE (role_id, permission_id)
);

CREATE INDEX idx_role_permissions_role_id
    ON role_permissions(role_id);
CREATE INDEX idx_role_permissions_permission_id
    ON role_permissions(permission_id);

INSERT INTO permissions (key, name, description)
VALUES
    ('sites.read', 'Read Sites', 'View site records and current site state.'),
    ('sites.create', 'Create Sites', 'Create new site records.'),
    ('sites.update', 'Update Sites', 'Update existing site records and activation state.'),
    ('sites.delete', 'Delete Sites', 'Delete existing site records.'),
    ('site_monitors.read', 'Read Site Monitors', 'View HTTP monitor configuration and current status for sites.'),
    ('site_monitors.create', 'Create Site Monitors', 'Create HTTP monitor configuration for sites.'),
    ('site_monitors.update', 'Update Site Monitors', 'Update HTTP monitor configuration for sites.'),
    ('site_monitors.delete', 'Delete Site Monitors', 'Disable or remove HTTP monitor configuration for sites.'),
    ('site_checks.read', 'Read Site Checks', 'View individual site check history.'),
    ('site_incidents.read', 'Read Site Incidents', 'View derived incident history for sites.'),
    ('notification_deliveries.read', 'Read Notification Deliveries', 'View notification delivery history.'),
    ('notification_channels.read', 'Read Notification Channels', 'View installation notification channels.'),
    ('notification_channels.create', 'Create Notification Channels', 'Create installation notification channels.'),
    ('notification_channels.update', 'Update Notification Channels', 'Update installation notification channels.'),
    ('notification_channels.delete', 'Delete Notification Channels', 'Delete installation notification channels.'),
    ('site_notification_channel_overrides.read', 'Read Site Notification Overrides', 'View site-level effective notification channel settings.'),
    ('site_notification_channel_overrides.create', 'Create Site Notification Overrides', 'Create site-level notification channel overrides.'),
    ('site_notification_channel_overrides.update', 'Update Site Notification Overrides', 'Update site-level notification channel overrides.'),
    ('site_notification_channel_overrides.delete', 'Delete Site Notification Overrides', 'Delete site-level notification channel overrides.'),
    ('users.read', 'Read Users', 'View Sentinel admin users and their roles.'),
    ('users.write', 'Write Users', 'Create, update, activate, and deactivate Sentinel admin users.'),
    ('roles.read', 'Read Roles', 'View roles and assigned permissions.'),
    ('roles.write', 'Write Roles', 'Create or update roles and permission mappings.')
ON CONFLICT (key) DO NOTHING;

INSERT INTO roles (key, name, description, is_system)
VALUES
    ('viewer', 'Viewer', 'Read-only access to status, checks, incidents, deliveries, and configuration views.', TRUE),
    ('operator', 'Operator', 'Operational access to manage sites, monitoring, and notifications.', TRUE),
    ('admin', 'Admin', 'Full installation administration access.', TRUE)
ON CONFLICT (key) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
INNER JOIN permissions p
    ON p.key IN (
        'sites.read',
        'site_monitors.read',
        'site_checks.read',
        'site_incidents.read',
        'notification_channels.read',
        'site_notification_channel_overrides.read',
        'notification_deliveries.read',
        'users.read',
        'roles.read'
    )
WHERE r.key = 'viewer'
ON CONFLICT (role_id, permission_id) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
INNER JOIN permissions p
    ON p.key IN (
        'sites.read',
        'sites.create',
        'sites.update',
        'sites.delete',
        'site_monitors.read',
        'site_monitors.create',
        'site_monitors.update',
        'site_monitors.delete',
        'site_checks.read',
        'site_incidents.read',
        'notification_channels.read',
        'notification_channels.create',
        'notification_channels.update',
        'notification_channels.delete',
        'site_notification_channel_overrides.read',
        'site_notification_channel_overrides.create',
        'site_notification_channel_overrides.update',
        'site_notification_channel_overrides.delete',
        'notification_deliveries.read',
        'users.read',
        'roles.read'
    )
WHERE r.key = 'operator'
ON CONFLICT (role_id, permission_id) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
INNER JOIN permissions p ON TRUE
WHERE r.key = 'admin'
ON CONFLICT (role_id, permission_id) DO NOTHING;
