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

ALTER TABLE provider
    DROP COLUMN IF EXISTS default_models;

ALTER TABLE provider_price
    DROP CONSTRAINT IF EXISTS provider_price_billing_meter_check;
ALTER TABLE provider_price
    ADD CONSTRAINT provider_price_billing_meter_check
    CHECK (billing_meter IN ('token', 'image', 'video')) NOT VALID;
ALTER TABLE provider_price VALIDATE CONSTRAINT provider_price_billing_meter_check;

ALTER TABLE provider_price
    ADD COLUMN IF NOT EXISTS video_billing_mode TEXT,
    ADD COLUMN IF NOT EXISTS video_price_tiers JSONB NOT NULL DEFAULT '[]'::JSONB;

CREATE TABLE channel_price (
    id BIGSERIAL PRIMARY KEY,
    channel_id BIGINT NOT NULL REFERENCES channel(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    input_price_micros BIGINT NOT NULL CHECK (input_price_micros >= 0),
    output_price_micros BIGINT NOT NULL CHECK (output_price_micros >= 0),
    cache_read_price_micros BIGINT CHECK (cache_read_price_micros >= 0),
    cache_write_price_micros BIGINT CHECK (cache_write_price_micros >= 0),
    billing_meter TEXT NOT NULL DEFAULT 'token',
    unit_price_micros BIGINT CHECK (unit_price_micros >= 0),
    video_billing_mode TEXT,
    video_price_tiers JSONB NOT NULL DEFAULT '[]'::JSONB,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (channel_id, model),
    CONSTRAINT channel_price_channel_model_fk
        FOREIGN KEY (channel_id, model) REFERENCES channel_model(channel_id, model) ON DELETE CASCADE,
    CONSTRAINT channel_price_billing_meter_check
        CHECK (billing_meter IN ('token', 'image', 'video')),
    CONSTRAINT channel_price_video_billing_mode_check
        CHECK (
            video_billing_mode IS NULL
            OR video_billing_mode IN ('official_token', 'per_second')
        ),
    CONSTRAINT channel_price_video_billing_shape_check
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
        )
);

INSERT INTO channel_price (
    channel_id, model, input_price_micros, output_price_micros,
    cache_read_price_micros, cache_write_price_micros,
    billing_meter, unit_price_micros, video_billing_mode, video_price_tiers,
    enabled, created_at, updated_at
)
SELECT
    cm.channel_id,
    pp.model,
    pp.input_price_micros,
    pp.output_price_micros,
    pp.cache_read_price_micros,
    pp.cache_write_price_micros,
    pp.billing_meter,
    pp.unit_price_micros,
    pp.video_billing_mode,
    pp.video_price_tiers,
    pp.enabled,
    pp.created_at,
    pp.updated_at
FROM provider_price pp
JOIN channel c ON c.provider = pp.provider
JOIN channel_model cm ON cm.channel_id = c.id AND cm.model = pp.model
ON CONFLICT (channel_id, model) DO NOTHING;

DROP TABLE provider_price;

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

UPDATE provider_model
SET billing_meter = 'video',
    updated_at = now()
WHERE billing_meter <> 'video'
  AND jsonb_path_exists(capabilities, '$.modalities.output[*] ? (@ like_regex "^video$" flag "i")');

UPDATE pricing_template
SET billing_meter = 'video',
    updated_at = now()
WHERE billing_meter <> 'video'
  AND pricing_basis = 'multi_tier_video';

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
