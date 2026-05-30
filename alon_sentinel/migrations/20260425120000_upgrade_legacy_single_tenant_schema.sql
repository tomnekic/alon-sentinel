DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.tables
        WHERE table_schema = current_schema()
          AND table_name = 'accounts'
    ) THEN
        IF (
            SELECT COUNT(*)
            FROM accounts
        ) > 1 THEN
            RAISE EXCEPTION
                'Cannot automatically migrate a multi-account Sentinel database to the single-tenant schema.';
        END IF;
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_type t
        JOIN pg_enum e ON e.enumtypid = t.oid
        WHERE t.typnamespace = current_schema()::regnamespace
          AND t.typname = 'api_client_type'
          AND e.enumlabel = 'tenant_client'
    ) THEN
        IF EXISTS (
            SELECT 1
            FROM pg_type t
            JOIN pg_enum e ON e.enumtypid = t.oid
            WHERE t.typnamespace = current_schema()::regnamespace
              AND t.typname = 'api_client_type'
              AND e.enumlabel = 'installation_client'
        ) THEN
            UPDATE api_clients
            SET type = 'installation_client'
            WHERE type::text = 'tenant_client';
        ELSE
            ALTER TYPE api_client_type RENAME VALUE 'tenant_client' TO 'installation_client';
        END IF;
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'api_clients'
          AND column_name = 'account_id'
    ) THEN
        ALTER TABLE api_clients
            DROP CONSTRAINT IF EXISTS chk_api_clients_account_rule;

        ALTER TABLE api_clients
            DROP CONSTRAINT IF EXISTS api_clients_account_id_fkey;

        ALTER TABLE api_clients
            DROP COLUMN account_id;
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'api_client_audit_logs'
          AND column_name = 'account_id'
    ) THEN
        ALTER TABLE api_client_audit_logs
            DROP COLUMN account_id;
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'sites'
          AND column_name = 'account_id'
    ) THEN
        ALTER TABLE sites
            DROP CONSTRAINT IF EXISTS sites_account_id_fkey;

        ALTER TABLE sites
            DROP COLUMN account_id;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint c
        JOIN pg_class r ON r.oid = c.conrelid
        JOIN pg_namespace n ON n.oid = r.relnamespace
        WHERE n.nspname = current_schema()
          AND r.relname = 'sites'
          AND c.conname = 'uq_sites_base_url'
    ) THEN
        ALTER TABLE sites
            ADD CONSTRAINT uq_sites_base_url UNIQUE (base_url);
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.tables
        WHERE table_schema = current_schema()
          AND table_name = 'account_notification_channels'
    )
    AND NOT EXISTS (
        SELECT 1
        FROM information_schema.tables
        WHERE table_schema = current_schema()
          AND table_name = 'notification_channels'
    ) THEN
        ALTER TABLE account_notification_channels
            DROP CONSTRAINT IF EXISTS account_notification_channels_account_id_fkey;

        ALTER TABLE account_notification_channels
            DROP COLUMN IF EXISTS account_id;

        ALTER TABLE account_notification_channels
            RENAME TO notification_channels;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM information_schema.tables
        WHERE table_schema = current_schema()
          AND table_name = 'notification_channels'
    )
    AND NOT EXISTS (
        SELECT 1
        FROM pg_constraint c
        JOIN pg_class r ON r.oid = c.conrelid
        JOIN pg_namespace n ON n.oid = r.relnamespace
        WHERE n.nspname = current_schema()
          AND r.relname = 'notification_channels'
          AND c.conname = 'uq_notification_channels_name'
    ) THEN
        ALTER TABLE notification_channels
            ADD CONSTRAINT uq_notification_channels_name UNIQUE (name);
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'site_notification_channel_overrides'
          AND column_name = 'account_notification_channel_id'
    ) THEN
        ALTER TABLE site_notification_channel_overrides
            RENAME COLUMN account_notification_channel_id TO notification_channel_id;
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'notification_deliveries'
          AND column_name = 'account_notification_channel_id'
    ) THEN
        ALTER TABLE notification_deliveries
            RENAME COLUMN account_notification_channel_id TO notification_channel_id;
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.tables
        WHERE table_schema = current_schema()
          AND table_name = 'accounts'
    ) THEN
        DROP TABLE accounts;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_type
        WHERE typnamespace = current_schema()::regnamespace
          AND typname = 'account_status'
    ) THEN
        DROP TYPE account_status;
    END IF;
END
$$;
