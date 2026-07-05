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

-- 放宽 pricing_template.pricing_basis 约束,支持更多展示口径。
-- pricing_basis 仅用于参考价展示,不参与实际计费(billing_meter 仍受 token/image 约束)。
ALTER TABLE pricing_template
    DROP CONSTRAINT IF EXISTS pricing_template_pricing_basis_check;
ALTER TABLE pricing_template
    ADD CONSTRAINT pricing_template_pricing_basis_check
    CHECK (pricing_basis IN ('token', 'image', 'call', 'per_10k_token', 'hour', 'second', 'multi_tier_video')) NOT VALID;
ALTER TABLE pricing_template VALIDATE CONSTRAINT pricing_template_pricing_basis_check;
