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

UPDATE provider SET display_name = 'OpenAI 兼容' WHERE code = 'openai' AND display_name = 'OpenAI 官方';
UPDATE provider SET display_name = 'Anthropic 兼容' WHERE code = 'anthropic' AND display_name = 'Anthropic 官方';
UPDATE provider SET default_openai_base_url = '' WHERE code = 'openai';
UPDATE provider SET default_anthropic_base_url = '' WHERE code = 'anthropic';

-- 上游 endpoint 的 adapter 类型 hint，用于在 provider 字段不再携带适配信息时（如"openai 兼容"渠道）
-- 显式标记所需适配层。目前支持 'newapi'，null 表示按 provider 默认规则选取。
ALTER TABLE channel_endpoint ADD COLUMN adapter_hint TEXT;

-- 异步任务快照 endpoint 当时的 adapter_hint，保证任务回捞时使用与创建时一致的适配层。
ALTER TABLE task_upstream ADD COLUMN adapter_hint TEXT;

-- 上游任务 ID 只在调用方 API Key 范围内唯一，不应跨租户按 provider 去重。
ALTER TABLE task_upstream
    DROP CONSTRAINT IF EXISTS task_upstream_task_type_provider_upstream_task_id_key;

UPDATE provider
SET enabled = FALSE,
    updated_at = now()
WHERE code IN ('custom', 'newapi', 'sub2api');

-- 新增模型时记录匹配到的参考价格模型。已有数据不回填。
ALTER TABLE channel_model ADD COLUMN base_model TEXT;
