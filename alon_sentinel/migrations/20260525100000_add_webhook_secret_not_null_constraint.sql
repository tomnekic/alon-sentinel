ALTER TABLE notification_channels
ADD CONSTRAINT webhook_channels_require_secret
CHECK (channel_type != 'webhook' OR webhook_secret_ciphertext IS NOT NULL);
