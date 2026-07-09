ALTER TABLE task_upstream
    DROP CONSTRAINT IF EXISTS task_upstream_task_type_check;

ALTER TABLE task_upstream
    ADD CONSTRAINT task_upstream_task_type_check CHECK (
        task_type IN (
            'openai_response',
            'openai_video',
            'anthropic_message_batch',
            'neogate_response'
        )
    );

ALTER TABLE provider_price
    ADD COLUMN IF NOT EXISTS video_billing_mode TEXT,
    ADD COLUMN IF NOT EXISTS video_price_tiers JSONB NOT NULL DEFAULT '[]'::JSONB;

ALTER TABLE provider_price
    DROP CONSTRAINT IF EXISTS provider_price_video_billing_mode_check;
ALTER TABLE provider_price
    ADD CONSTRAINT provider_price_video_billing_mode_check
    CHECK (
        video_billing_mode IS NULL
        OR video_billing_mode IN ('official_token', 'per_second')
    ) NOT VALID;
ALTER TABLE provider_price VALIDATE CONSTRAINT provider_price_video_billing_mode_check;

ALTER TABLE provider_price
    DROP CONSTRAINT IF EXISTS provider_price_video_billing_shape_check;
ALTER TABLE provider_price
    ADD CONSTRAINT provider_price_video_billing_shape_check
    CHECK (
        CASE
            WHEN billing_meter = 'video' THEN
                video_billing_mode IS NOT NULL
                AND jsonb_typeof(video_price_tiers) = 'array'
                AND jsonb_array_length(video_price_tiers) > 0
            ELSE
                video_billing_mode IS NULL
                AND video_price_tiers = '[]'::JSONB
        END
    ) NOT VALID;
ALTER TABLE provider_price VALIDATE CONSTRAINT provider_price_video_billing_shape_check;

ALTER TABLE provider_price
    DROP CONSTRAINT IF EXISTS provider_price_billing_meter_check;
ALTER TABLE provider_price
    ADD CONSTRAINT provider_price_billing_meter_check
    CHECK (billing_meter IN ('token', 'image', 'video')) NOT VALID;
ALTER TABLE provider_price VALIDATE CONSTRAINT provider_price_billing_meter_check;

ALTER TABLE provider_model
    DROP CONSTRAINT IF EXISTS provider_model_billing_meter_check;
ALTER TABLE provider_model
    ADD CONSTRAINT provider_model_billing_meter_check
    CHECK (billing_meter IN ('token', 'image', 'video')) NOT VALID;
ALTER TABLE provider_model VALIDATE CONSTRAINT provider_model_billing_meter_check;

ALTER TABLE pricing_template
    DROP CONSTRAINT IF EXISTS pricing_template_billing_meter_check;
ALTER TABLE pricing_template
    ADD CONSTRAINT pricing_template_billing_meter_check
    CHECK (billing_meter IN ('token', 'image', 'video')) NOT VALID;
ALTER TABLE pricing_template VALIDATE CONSTRAINT pricing_template_billing_meter_check;

ALTER TABLE usage
    DROP CONSTRAINT IF EXISTS usage_billing_meter_check;
ALTER TABLE usage
    ADD CONSTRAINT usage_billing_meter_check
    CHECK (billing_meter IN ('token', 'image', 'video')) NOT VALID;
ALTER TABLE usage VALIDATE CONSTRAINT usage_billing_meter_check;

ALTER TABLE usage_daily
    DROP CONSTRAINT IF EXISTS usage_daily_billing_meter_check;
ALTER TABLE usage_daily
    ADD CONSTRAINT usage_daily_billing_meter_check
    CHECK (billing_meter IN ('token', 'image', 'video')) NOT VALID;
ALTER TABLE usage_daily VALIDATE CONSTRAINT usage_daily_billing_meter_check;
