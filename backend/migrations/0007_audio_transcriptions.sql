ALTER TABLE task_upstream
    DROP CONSTRAINT IF EXISTS task_upstream_task_type_check;
ALTER TABLE task_upstream
    ADD CONSTRAINT task_upstream_task_type_check CHECK (
        task_type IN (
            'openai_response',
            'openai_video',
            'audio_transcription',
            'anthropic_message_batch',
            'neogate_response'
        )
    );

ALTER TABLE channel_price DROP CONSTRAINT IF EXISTS channel_price_billing_meter_check;
ALTER TABLE channel_price ADD CONSTRAINT channel_price_billing_meter_check
    CHECK (billing_meter IN ('token', 'image', 'video', 'audio'));
ALTER TABLE channel_price ADD CONSTRAINT channel_price_audio_billing_shape_check
    CHECK (billing_meter <> 'audio' OR unit_price_micros > 0) NOT VALID;
ALTER TABLE channel_price VALIDATE CONSTRAINT channel_price_audio_billing_shape_check;

ALTER TABLE provider_model DROP CONSTRAINT IF EXISTS provider_model_billing_meter_check;
ALTER TABLE provider_model ADD CONSTRAINT provider_model_billing_meter_check
    CHECK (billing_meter IN ('token', 'image', 'video', 'audio')) NOT VALID;
ALTER TABLE provider_model VALIDATE CONSTRAINT provider_model_billing_meter_check;

ALTER TABLE pricing_template DROP CONSTRAINT IF EXISTS pricing_template_billing_meter_check;
ALTER TABLE pricing_template ADD CONSTRAINT pricing_template_billing_meter_check
    CHECK (billing_meter IN ('token', 'image', 'video', 'audio')) NOT VALID;
ALTER TABLE pricing_template VALIDATE CONSTRAINT pricing_template_billing_meter_check;

ALTER TABLE usage DROP CONSTRAINT IF EXISTS usage_billing_meter_check;
ALTER TABLE usage ADD CONSTRAINT usage_billing_meter_check
    CHECK (billing_meter IN ('token', 'image', 'video', 'audio')) NOT VALID;
ALTER TABLE usage VALIDATE CONSTRAINT usage_billing_meter_check;

ALTER TABLE usage_daily DROP CONSTRAINT IF EXISTS usage_daily_billing_meter_check;
ALTER TABLE usage_daily ADD CONSTRAINT usage_daily_billing_meter_check
    CHECK (billing_meter IN ('token', 'image', 'video', 'audio')) NOT VALID;
ALTER TABLE usage_daily VALIDATE CONSTRAINT usage_daily_billing_meter_check;

UPDATE provider_model
SET billing_meter = 'audio', updated_at = now()
WHERE lower(model) IN ('fun-asr', 'paraformer-v2');

UPDATE pricing_template
SET billing_meter = 'audio', updated_at = now()
WHERE lower(model) IN ('fun-asr', 'paraformer-v2')
  AND pricing_basis = 'second'
  AND unit_price_micros > 0;
