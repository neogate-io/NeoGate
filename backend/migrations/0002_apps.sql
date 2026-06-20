CREATE TABLE app (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    description TEXT NOT NULL DEFAULT '',
    app_type TEXT NOT NULL CHECK (app_type IN ('wecom', 'webhook', 'widget', 'feishu', 'dingtalk')),
    status TEXT NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled')),
    model TEXT NOT NULL CHECK (length(trim(model)) > 0),
    system_prompt TEXT NOT NULL DEFAULT '',
    context_turns INTEGER NOT NULL DEFAULT 10 CHECK (context_turns >= 0 AND context_turns <= 50),
    max_output_tokens INTEGER NOT NULL DEFAULT 2048 CHECK (max_output_tokens > 0 AND max_output_tokens <= 128000),
    user_key_id BIGINT NOT NULL REFERENCES user_key(id) ON DELETE RESTRICT,
    metadata JSONB NOT NULL DEFAULT '{}',
    last_active_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE app_endpoint (
    id BIGSERIAL PRIMARY KEY,
    app_id BIGINT NOT NULL REFERENCES app(id) ON DELETE CASCADE,
    endpoint_type TEXT NOT NULL CHECK (endpoint_type IN ('wecom', 'webhook', 'widget', 'feishu', 'dingtalk')),
    name TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    config JSONB NOT NULL DEFAULT '{}',
    secret_ciphertext JSONB NOT NULL DEFAULT '{}',
    last_active_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (app_id, endpoint_type)
);

CREATE TABLE app_conversation (
    id BIGSERIAL PRIMARY KEY,
    app_id BIGINT NOT NULL REFERENCES app(id) ON DELETE CASCADE,
    endpoint_id BIGINT NOT NULL REFERENCES app_endpoint(id) ON DELETE CASCADE,
    external_user_id TEXT NOT NULL,
    external_conversation_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (app_id, endpoint_id, external_user_id, external_conversation_id)
);

CREATE TABLE app_message (
    id BIGSERIAL PRIMARY KEY,
    conversation_id BIGINT NOT NULL REFERENCES app_conversation(id) ON DELETE CASCADE,
    app_id BIGINT NOT NULL REFERENCES app(id) ON DELETE CASCADE,
    endpoint_id BIGINT NOT NULL REFERENCES app_endpoint(id) ON DELETE CASCADE,
    external_message_id TEXT,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
    content TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE app_run_log (
    id BIGSERIAL PRIMARY KEY,
    app_id BIGINT REFERENCES app(id) ON DELETE SET NULL,
    endpoint_id BIGINT REFERENCES app_endpoint(id) ON DELETE SET NULL,
    conversation_id BIGINT REFERENCES app_conversation(id) ON DELETE SET NULL,
    external_user_id TEXT,
    external_conversation_id TEXT,
    external_message_id TEXT,
    trace_id TEXT,
    app_type TEXT NOT NULL,
    model TEXT,
    status TEXT NOT NULL CHECK (status IN ('success', 'failed', 'duplicate', 'ignored')),
    status_code INTEGER,
    latency_ms BIGINT NOT NULL DEFAULT 0,
    input_tokens BIGINT,
    output_tokens BIGINT,
    total_tokens BIGINT,
    cost_micro_usd BIGINT,
    error_summary TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE app_message_delivery (
    id BIGSERIAL PRIMARY KEY,
    endpoint_id BIGINT NOT NULL REFERENCES app_endpoint(id) ON DELETE CASCADE,
    external_message_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (endpoint_id, external_message_id)
);

CREATE INDEX idx_app_type_status ON app(app_type, status, created_at DESC);
CREATE INDEX idx_app_user_key ON app(user_key_id);
CREATE INDEX idx_app_endpoint_type_enabled ON app_endpoint(endpoint_type, enabled);
CREATE INDEX idx_app_message_conversation_created ON app_message(conversation_id, created_at DESC, id DESC);
CREATE INDEX idx_app_message_app_created ON app_message(app_id, created_at DESC, id DESC);
CREATE INDEX idx_app_message_endpoint_created ON app_message(endpoint_id, created_at DESC, id DESC);
CREATE INDEX idx_app_run_log_created ON app_run_log(created_at DESC, id DESC);
CREATE INDEX idx_app_run_log_app_created ON app_run_log(app_id, created_at DESC, id DESC);
CREATE INDEX idx_app_run_log_endpoint_created ON app_run_log(endpoint_id, created_at DESC, id DESC);
CREATE INDEX idx_app_run_log_conversation_created ON app_run_log(conversation_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_task_upstream_held_billing_hold
    ON task_upstream USING GIN (billing_hold jsonb_path_ops)
    WHERE billing_status = 'held'
      AND billing_hold IS NOT NULL;

DROP INDEX IF EXISTS idx_task_upstream_polling;
DROP INDEX IF EXISTS idx_credit_allocation_active_stale;
DROP INDEX IF EXISTS idx_billing_pending_created;
DROP INDEX IF EXISTS idx_billing_pending_attempts_created;

CREATE INDEX idx_task_upstream_polling
    ON task_upstream(next_poll_at ASC, id ASC)
    WHERE terminal = FALSE
      AND next_poll_at IS NOT NULL;
CREATE INDEX idx_credit_allocation_active_stale
    ON credit_allocation(id ASC, created_at ASC)
    WHERE status = 'active'
      AND consumed_micro_usd + returned_micro_usd < amount_micro_usd;
CREATE INDEX idx_billing_pending_created ON billing(created_at ASC)
    WHERE status IN ('pending', 'failed');
CREATE INDEX idx_billing_pending_attempts_created ON billing(attempts ASC, created_at ASC)
    WHERE status IN ('pending', 'failed');

-- Relay trace columns belong logically after usage.credential_id and before provider/model/status_code.
-- PostgreSQL does not support ADD COLUMN ... AFTER, so keep this before later usage column additions.
ALTER TABLE usage
    ADD COLUMN relay_trace_id UUID,
    ADD COLUMN relay_attempt INTEGER NOT NULL DEFAULT 1 CHECK (relay_attempt > 0),
    ADD COLUMN relay_final BOOLEAN NOT NULL DEFAULT TRUE;

CREATE INDEX idx_usage_relay_trace ON usage(relay_trace_id, relay_attempt, id)
    WHERE relay_trace_id IS NOT NULL;

ALTER TABLE usage
    ADD COLUMN billing_meter TEXT NOT NULL DEFAULT 'token',
    ADD COLUMN billable_units BIGINT NOT NULL DEFAULT 0 CHECK (billable_units >= 0),
    ADD CONSTRAINT usage_billing_meter_check CHECK (billing_meter IN ('token', 'image')) NOT VALID;
ALTER TABLE usage VALIDATE CONSTRAINT usage_billing_meter_check;

ALTER TABLE usage_daily
    ADD COLUMN billing_meter TEXT NOT NULL DEFAULT 'token',
    ADD COLUMN billable_units BIGINT NOT NULL DEFAULT 0 CHECK (billable_units >= 0),
    ADD CONSTRAINT usage_daily_billing_meter_check CHECK (billing_meter IN ('token', 'image')) NOT VALID;
ALTER TABLE usage_daily VALIDATE CONSTRAINT usage_daily_billing_meter_check;

DROP INDEX IF EXISTS idx_usage_daily_identity;
CREATE UNIQUE INDEX idx_usage_daily_identity ON usage_daily(
    day,
    COALESCE(user_id, -1),
    COALESCE(project_id, -1),
    COALESCE(user_key_id, -1),
    COALESCE(channel_id, -1),
    COALESCE(channel_key_id, -1),
    COALESCE(credential_id, -1),
    provider,
    model,
    billing_meter
);

ALTER TABLE provider_model
    ADD COLUMN billing_meter TEXT NOT NULL DEFAULT 'token',
    ADD COLUMN capabilities JSONB NOT NULL DEFAULT '{}'::JSONB,
    ADD CONSTRAINT provider_model_billing_meter_check CHECK (billing_meter IN ('token', 'image')) NOT VALID;
ALTER TABLE provider_model VALIDATE CONSTRAINT provider_model_billing_meter_check;

ALTER TABLE provider_price
    ADD COLUMN billing_meter TEXT NOT NULL DEFAULT 'token',
    ADD COLUMN unit_price_usd_micros BIGINT CHECK (unit_price_usd_micros >= 0);
ALTER TABLE provider_price
    ADD CONSTRAINT provider_price_billing_meter_check CHECK (billing_meter IN ('token', 'image')) NOT VALID;
ALTER TABLE provider_price VALIDATE CONSTRAINT provider_price_billing_meter_check;

ALTER TABLE pricing_template
    ADD COLUMN billing_meter TEXT NOT NULL DEFAULT 'token',
    ADD COLUMN unit_price_usd_micros BIGINT CHECK (unit_price_usd_micros >= 0),
    ADD COLUMN pricing_basis TEXT NOT NULL DEFAULT 'token';

UPDATE pricing_template pt
SET billing_meter = 'token',
    unit_price_usd_micros = NULL,
    pricing_basis = 'token'
FROM provider_model pm
WHERE pt.provider = pm.provider
  AND pt.model = pm.model
  AND pt.source = 'models_dev'
  AND pt.pricing_basis = 'image'
  AND pm.capabilities -> 'modalities' -> 'output' ? 'image';

ALTER TABLE pricing_template
    ADD CONSTRAINT pricing_template_billing_meter_check CHECK (billing_meter IN ('token', 'image')) NOT VALID,
    ADD CONSTRAINT pricing_template_pricing_basis_check CHECK (pricing_basis IN ('token', 'image')) NOT VALID;
ALTER TABLE pricing_template VALIDATE CONSTRAINT pricing_template_billing_meter_check;
ALTER TABLE pricing_template VALIDATE CONSTRAINT pricing_template_pricing_basis_check;
