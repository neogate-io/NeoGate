DROP INDEX IF EXISTS idx_usage_provider_created;
DROP INDEX IF EXISTS idx_usage_daily_provider_model_day;
DROP INDEX IF EXISTS idx_usage_daily_user_provider_model_day;
DROP INDEX IF EXISTS idx_channel_model_provider_model;

ALTER TABLE usage
    DROP COLUMN IF EXISTS provider;

DROP INDEX IF EXISTS idx_usage_daily_identity;
ALTER TABLE usage_daily
    DROP COLUMN IF EXISTS provider;
CREATE UNIQUE INDEX idx_usage_daily_identity ON usage_daily(
    day,
    COALESCE(user_id, -1),
    COALESCE(project_id, -1),
    COALESCE(user_key_id, -1),
    COALESCE(channel_id, -1),
    COALESCE(channel_key_id, -1),
    COALESCE(credential_id, -1),
    model,
    billing_meter
);

CREATE INDEX IF NOT EXISTS idx_usage_daily_user_model_day
    ON usage_daily(user_id, model, day DESC);

ALTER TABLE channel_model
    DROP CONSTRAINT IF EXISTS channel_model_provider_model_fk,
    DROP COLUMN IF EXISTS provider;

ALTER TABLE channel_probe
    DROP COLUMN IF EXISTS provider;
