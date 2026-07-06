DROP INDEX IF EXISTS idx_usage_provider_created;
DROP INDEX IF EXISTS idx_usage_daily_provider_model_day;
DROP INDEX IF EXISTS idx_usage_daily_user_provider_model_day;
DROP INDEX IF EXISTS idx_channel_model_provider_model;

ALTER TABLE credit_account
    RENAME COLUMN balance_micro_usd TO balance_micros;
ALTER TABLE credit_account
    RENAME COLUMN reserved_micro_usd TO reserved_micros;

ALTER TABLE payment
    RENAME COLUMN amount_micro_usd TO amount_micros;

ALTER TABLE usage
    RENAME COLUMN cost_micro_usd TO cost_micros;

ALTER TABLE provider_price
    RENAME COLUMN input_price_usd_micros TO input_price_micros;
ALTER TABLE provider_price
    RENAME COLUMN output_price_usd_micros TO output_price_micros;
ALTER TABLE provider_price
    RENAME COLUMN cache_read_price_usd_micros TO cache_read_price_micros;
ALTER TABLE provider_price
    RENAME COLUMN cache_write_price_usd_micros TO cache_write_price_micros;
ALTER TABLE provider_price
    RENAME COLUMN unit_price_usd_micros TO unit_price_micros;

ALTER TABLE pricing_template
    RENAME COLUMN input_price_usd_micros TO input_price_micros;
ALTER TABLE pricing_template
    RENAME COLUMN output_price_usd_micros TO output_price_micros;
ALTER TABLE pricing_template
    RENAME COLUMN cache_read_price_usd_micros TO cache_read_price_micros;
ALTER TABLE pricing_template
    RENAME COLUMN cache_write_price_usd_micros TO cache_write_price_micros;
ALTER TABLE pricing_template
    RENAME COLUMN unit_price_usd_micros TO unit_price_micros;

ALTER TABLE credit_allocation
    RENAME COLUMN amount_micro_usd TO amount_micros;
ALTER TABLE credit_allocation
    RENAME COLUMN consumed_micro_usd TO consumed_micros;
ALTER TABLE credit_allocation
    RENAME COLUMN returned_micro_usd TO returned_micros;

ALTER TABLE credit_ledger
    RENAME COLUMN amount_micro_usd TO amount_micros;
ALTER TABLE credit_ledger
    RENAME COLUMN balance_after_micro_usd TO balance_after_micros;

ALTER TABLE usage_daily
    RENAME COLUMN cost_micro_usd TO cost_micros;

ALTER TABLE app_run_log
    RENAME COLUMN cost_micro_usd TO cost_micros;

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
