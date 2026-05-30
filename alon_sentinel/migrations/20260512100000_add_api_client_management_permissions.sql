INSERT INTO permissions (key, name, description)
VALUES
    ('api_clients.read',  'Read API Clients',  'View API client records, scopes, and last-used timestamps.'),
    ('api_clients.write', 'Write API Clients', 'Create, update, rotate secrets, and delete API clients.')
ON CONFLICT (key) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
INNER JOIN permissions p ON p.key IN ('api_clients.read', 'api_clients.write')
WHERE r.key = 'admin'
ON CONFLICT (role_id, permission_id) DO NOTHING;
