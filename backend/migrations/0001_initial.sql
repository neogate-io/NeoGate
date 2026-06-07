CREATE EXTENSION IF NOT EXISTS citext;

CREATE TABLE user_group (
    id BIGSERIAL PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_user_group_single_default
    ON user_group(is_default)
    WHERE is_default = TRUE;

INSERT INTO user_group (code, name, is_default, enabled)
VALUES ('default', '默认', TRUE, TRUE);

CREATE TABLE "user" (
    id BIGSERIAL PRIMARY KEY,
    email CITEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled')),
    password_hash TEXT,
    user_group_id BIGINT NOT NULL REFERENCES user_group(id),
    last_active_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT user_email_unique UNIQUE (email)
);

CREATE TABLE user_code (
    id BIGSERIAL PRIMARY KEY,
    email CITEXT NOT NULL,
    code_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE admin (
    id BIGSERIAL PRIMARY KEY,
    username CITEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled')),
    role TEXT NOT NULL DEFAULT 'owner' CHECK (role IN ('owner', 'admin', 'viewer')),
    last_login_at TIMESTAMPTZ,
    failed_login_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ,
    password_changed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE user_key (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT 'API Key',
    key_prefix TEXT NOT NULL,
    secret_ciphertext TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled')),
    last_active_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    model_limits TEXT[],
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE user_key_model (
    id BIGSERIAL PRIMARY KEY,
    user_key_id BIGINT NOT NULL REFERENCES user_key(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_key_id, model),
    CHECK (length(trim(model)) > 0)
);

CREATE TABLE credit_account (
    id BIGSERIAL PRIMARY KEY,
    owner_type TEXT NOT NULL CHECK (owner_type IN ('user', 'user_key', 'user_key_model')),
    owner_id BIGINT NOT NULL,
    balance_micro_usd BIGINT NOT NULL DEFAULT 0,
    reserved_micro_usd BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (owner_type, owner_id),
    CONSTRAINT credit_account_balance_non_negative CHECK (
        balance_micro_usd >= 0
        AND reserved_micro_usd >= 0
        AND reserved_micro_usd <= balance_micro_usd
    )
);

CREATE TABLE payment (
    id UUID PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    credit_account_id BIGINT NOT NULL REFERENCES credit_account(id),
    provider TEXT NOT NULL,
    provider_order_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'paid', 'failed', 'canceled', 'expired')),
    currency TEXT NOT NULL,
    amount_micro_usd BIGINT NOT NULL CHECK (amount_micro_usd > 0),
    payable_amount_minor BIGINT NOT NULL CHECK (payable_amount_minor > 0),
    checkout_url TEXT,
    return_url TEXT,
    notify_payload JSONB,
    paid_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE payment_event (
    id BIGSERIAL PRIMARY KEY,
    payment_id UUID REFERENCES payment(id) ON DELETE SET NULL,
    provider TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE provider (
    id BIGSERIAL PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    name TEXT NOT NULL,
    default_models TEXT[] NOT NULL DEFAULT '{}',
    default_openai_base_url TEXT NOT NULL DEFAULT '',
    default_openai_oauth_base_url TEXT NOT NULL DEFAULT '',
    default_anthropic_base_url TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO provider
    (code, display_name, name, default_models,
     default_openai_base_url, default_openai_oauth_base_url, default_anthropic_base_url, sort_order)
VALUES
    ('custom', '自定义', 'Custom', ARRAY[]::TEXT[], '', '', '', 0),
    ('openai', 'OpenAI 官方', 'OpenAI', ARRAY['gpt-4o', 'gpt-4o-mini'], 'https://api.openai.com', 'https://chatgpt.com/backend-api/codex', '', 10),
    ('anthropic', 'Anthropic 官方', 'Anthropic', ARRAY['claude-3-5-sonnet-latest', 'claude-3-5-haiku-latest'], '', '', 'https://api.anthropic.com', 20),
    ('google', '谷歌 Gemini', 'Google Gemini', ARRAY['gemini-2.0-flash', 'gemini-1.5-pro'], 'https://generativelanguage.googleapis.com/v1beta/openai', '', '', 30),
    ('deepseek', '深度求索 DeepSeek', 'DeepSeek', ARRAY['deepseek-v4-flash', 'deepseek-v4-pro'], 'https://api.deepseek.com', '', 'https://api.deepseek.com/anthropic', 100),
    ('qwen', '通义千问 Qwen', 'Qwen', ARRAY['qwen3.7-max', 'qwen3.6-plus', 'qwen3.6-flash'], 'https://dashscope.aliyuncs.com/compatible-mode/v1', '', 'https://dashscope.aliyuncs.com/apps/anthropic', 110),
    ('moonshot', '月之暗面 Kimi', 'Moonshot/Kimi', ARRAY['kimi-k2.6', 'kimi-k2.5', 'moonshot-v1-128k'], 'https://api.moonshot.ai/v1', '', 'https://api.moonshot.ai/anthropic', 120),
    ('zhipu', '智谱 GLM', 'Zhipu GLM', ARRAY['glm-4.7', 'glm-4.5-air'], 'https://open.bigmodel.cn/api/paas/v4', '', 'https://api.z.ai/api/anthropic', 130),
    ('doubao', '火山方舟 豆包', 'Volcengine Ark/Doubao', ARRAY['doubao-seed-2-0-pro-260215', 'doubao-seed-2-0-lite-260428', 'doubao-seed-2-0-mini-260428'], 'https://ark.cn-beijing.volces.com/api/v3', '', 'https://ark.cn-beijing.volces.com/api/compatible', 140),
    ('baidu', '百度千帆 ERNIE', 'Baidu Qianfan', ARRAY['ernie-5.0', 'ernie-4.0-turbo-8k'], 'https://api.baiduqianfan.ai/v1', '', '', 150),
    ('tencent', '腾讯混元 TokenHub', 'Tencent Hunyuan', ARRAY['hy3-preview', 'hunyuan-role-latest'], 'https://tokenhub.tencentmaas.com/v1', '', 'https://tokenhub.tencentmaas.com', 160),
    ('minimax', 'MiniMax 稀宇科技', 'MiniMax', ARRAY['MiniMax-M2.7', 'MiniMax-M2.7-highspeed', 'MiniMax-M2.5'], 'https://api.minimax.io/v1', '', 'https://api.minimax.io/anthropic', 170),
    ('stepfun', '阶跃星辰 StepFun', 'StepFun', ARRAY['step-3.5-flash', 'step-2-mini', 'step-2-16k'], 'https://api.stepfun.com/v1', '', 'https://api.stepfun.com/step_plan', 180),
    ('baichuan', '百川智能 Baichuan', 'Baichuan AI', ARRAY['Baichuan4', 'Baichuan3-Turbo'], 'https://api.baichuan-ai.com/v1', '', '', 190),
    ('iflytek', '讯飞星火 Spark', 'iFlytek Spark', ARRAY['4.0Ultra', 'generalv3.5'], 'https://spark-api-open.xf-yun.com/v1', '', '', 200),
    ('sensenova', '商汤日日新 SenseNova', 'SenseNova', ARRAY['SenseChat-5', 'SenseNova-V6-5-Pro', 'SenseNova-V6-Pro'], 'https://api.sensenova.cn/compatible-mode/v2', '', '', 210),
    ('siliconflow', '硅基流动 SiliconFlow', 'SiliconFlow', ARRAY['deepseek-ai/DeepSeek-V4-Flash', 'deepseek-ai/DeepSeek-V4-Pro', 'Qwen/Qwen3.6-35B-A3B'], 'https://api.siliconflow.cn/v1', '', 'https://api.siliconflow.cn', 220);

CREATE TABLE channel (
    id BIGSERIAL PRIMARY KEY,
    provider TEXT NOT NULL REFERENCES provider(code),
    name TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    priority INTEGER NOT NULL DEFAULT 0,
    weight INTEGER NOT NULL DEFAULT 1,
    key_selection_mode TEXT NOT NULL DEFAULT 'polling' CHECK (key_selection_mode IN ('polling', 'random')),
    use_credentials BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE channel_endpoint (
    id BIGSERIAL PRIMARY KEY,
    channel_id BIGINT NOT NULL REFERENCES channel(id) ON DELETE CASCADE,
    protocol TEXT NOT NULL CHECK (protocol IN ('openai', 'openai_oauth', 'anthropic')),
    base_url TEXT NOT NULL,
    models TEXT[] NOT NULL DEFAULT '{}',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    healthy BOOLEAN NOT NULL DEFAULT TRUE,
    last_error TEXT,
    cooldown_until TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (channel_id, protocol)
);

CREATE TABLE channel_key (
    id BIGSERIAL PRIMARY KEY,
    channel_id BIGINT NOT NULL REFERENCES channel(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    key_prefix TEXT NOT NULL,
    secret_ciphertext TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    healthy BOOLEAN NOT NULL DEFAULT TRUE,
    cooldown_until TIMESTAMPTZ,
    last_error TEXT,
    last_used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE credential (
    id BIGSERIAL PRIMARY KEY,
    provider TEXT NOT NULL REFERENCES provider(code) ON DELETE CASCADE,
    identity_hash TEXT NOT NULL,
    identity_label TEXT,
    filename TEXT NOT NULL,
    content_ciphertext TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    auth_mode TEXT,
    api_key_preview TEXT,
    has_oauth_tokens BOOLEAN NOT NULL DEFAULT FALSE,
    has_refresh_token BOOLEAN NOT NULL DEFAULT FALSE,
    has_id_token BOOLEAN NOT NULL DEFAULT FALSE,
    email TEXT,
    account_id TEXT,
    plan_type TEXT,
    last_refresh TEXT,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, identity_hash)
);

CREATE TABLE usage (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT REFERENCES "user"(id) ON DELETE SET NULL,
    user_key_id BIGINT REFERENCES user_key(id) ON DELETE SET NULL,
    channel_id BIGINT REFERENCES channel(id) ON DELETE SET NULL,
    channel_key_id BIGINT REFERENCES channel_key(id) ON DELETE SET NULL,
    credential_id BIGINT REFERENCES credential(id) ON DELETE SET NULL,
    provider TEXT NOT NULL,
    model TEXT,
    status_code INTEGER,
    streamed BOOLEAN NOT NULL DEFAULT FALSE,
    latency_ms BIGINT NOT NULL DEFAULT 0,
    first_response_ms BIGINT,
    output_tokens_per_second DOUBLE PRECISION,
    error_summary TEXT,
    input_tokens BIGINT,
    output_tokens BIGINT,
    total_tokens BIGINT,
    cache_in_tokens BIGINT,
    cache_create_in_tokens BIGINT,
    cache_create_5m_in_tokens BIGINT,
    cache_create_1h_in_tokens BIGINT,
    reason_out_tokens BIGINT,
    audio_in_tokens BIGINT,
    audio_out_tokens BIGINT,
    cost_micro_usd BIGINT,
    billing_status TEXT NOT NULL DEFAULT 'not_billed' CHECK (billing_status IN ('not_billed', 'billed', 'usage_missing', 'undercharged')),
    billing_transaction_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE billing (
    id BIGSERIAL PRIMARY KEY,
    transaction_id UUID NOT NULL UNIQUE,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processed', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at TIMESTAMPTZ
);

CREATE TABLE setting (
    id BIGSERIAL PRIMARY KEY,
    key TEXT NOT NULL UNIQUE CHECK (length(trim(key)) > 0),
    value JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE provider_model (
    id BIGSERIAL PRIMARY KEY,
    provider TEXT NOT NULL REFERENCES provider(code) ON DELETE CASCADE,
    model TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT 'upstream' CHECK (source IN ('seed', 'upstream', 'channel')),
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    discovered_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, model)
);

CREATE TABLE provider_plan (
    id BIGSERIAL PRIMARY KEY,
    provider TEXT NOT NULL REFERENCES provider(code) ON DELETE CASCADE,
    protocol TEXT NOT NULL CHECK (protocol IN ('openai', 'openai_oauth', 'anthropic')),
    plan_type TEXT NOT NULL,
    model TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    source TEXT NOT NULL DEFAULT 'seed' CHECK (source IN ('seed', 'upstream', 'manual')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, protocol, plan_type, model),
    CONSTRAINT provider_plan_provider_model_fk
        FOREIGN KEY (provider, model) REFERENCES provider_model(provider, model) ON DELETE CASCADE
);

CREATE TABLE provider_price (
    id BIGSERIAL PRIMARY KEY,
    provider TEXT NOT NULL REFERENCES provider(code) ON DELETE CASCADE,
    model TEXT NOT NULL,
    input_price_usd_micros BIGINT NOT NULL CHECK (input_price_usd_micros >= 0),
    output_price_usd_micros BIGINT NOT NULL CHECK (output_price_usd_micros >= 0),
    cache_read_price_usd_micros BIGINT CHECK (cache_read_price_usd_micros >= 0),
    cache_write_price_usd_micros BIGINT CHECK (cache_write_price_usd_micros >= 0),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, model),
    CONSTRAINT provider_price_provider_model_fk
        FOREIGN KEY (provider, model) REFERENCES provider_model(provider, model) ON DELETE CASCADE
);

CREATE TABLE pricing_template (
    id BIGSERIAL PRIMARY KEY,
    provider TEXT NOT NULL REFERENCES provider(code) ON DELETE CASCADE,
    model TEXT NOT NULL,
    input_price_usd_micros BIGINT NOT NULL CHECK (input_price_usd_micros >= 0),
    output_price_usd_micros BIGINT NOT NULL CHECK (output_price_usd_micros >= 0),
    cache_read_price_usd_micros BIGINT CHECK (cache_read_price_usd_micros >= 0),
    cache_write_price_usd_micros BIGINT CHECK (cache_write_price_usd_micros >= 0),
    source TEXT NOT NULL DEFAULT 'seed',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, model),
    CONSTRAINT pricing_template_provider_model_fk
        FOREIGN KEY (provider, model) REFERENCES provider_model(provider, model) ON DELETE CASCADE
);

CREATE TABLE credential_model (
    id BIGSERIAL PRIMARY KEY,
    credential_id BIGINT NOT NULL REFERENCES credential(id) ON DELETE CASCADE,
    channel_endpoint_id BIGINT NOT NULL REFERENCES channel_endpoint(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'available' CHECK (status IN ('available', 'unavailable')),
    unavailable_until TIMESTAMPTZ,
    last_error TEXT,
    last_status_code INTEGER,
    last_seen_at TIMESTAMPTZ,
    success_count BIGINT NOT NULL DEFAULT 0,
    failure_count BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (credential_id, channel_endpoint_id, model)
);

CREATE TABLE pricing_policy (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    user_group TEXT NOT NULL REFERENCES user_group(code) ON DELETE CASCADE,
    multiplier_micros BIGINT NOT NULL DEFAULT 1000000 CHECK (multiplier_micros >= 0),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    priority INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE credit_allocation (
    id BIGSERIAL PRIMARY KEY,
    credit_account_id BIGINT NOT NULL REFERENCES credit_account(id),
    amount_micro_usd BIGINT NOT NULL CHECK (amount_micro_usd > 0),
    consumed_micro_usd BIGINT NOT NULL DEFAULT 0 CHECK (consumed_micro_usd >= 0),
    returned_micro_usd BIGINT NOT NULL DEFAULT 0 CHECK (returned_micro_usd >= 0),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'settled', 'recovered')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (consumed_micro_usd + returned_micro_usd <= amount_micro_usd)
);

CREATE TABLE credit_ledger (
    id BIGSERIAL PRIMARY KEY,
    credit_account_id BIGINT NOT NULL REFERENCES credit_account(id),
    amount_micro_usd BIGINT NOT NULL,
    balance_after_micro_usd BIGINT,
    reason TEXT NOT NULL CHECK (reason IN ('recharge', 'gift', 'adjustment', 'usage', 'refund', 'allocation_recover')),
    usage_id BIGINT REFERENCES usage(id) ON DELETE SET NULL,
    allocation_id BIGINT REFERENCES credit_allocation(id) ON DELETE SET NULL,
    transaction_id UUID NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE usage_daily (
    day DATE NOT NULL,
    user_id BIGINT REFERENCES "user"(id) ON DELETE SET NULL,
    user_key_id BIGINT REFERENCES user_key(id) ON DELETE SET NULL,
    channel_id BIGINT REFERENCES channel(id) ON DELETE SET NULL,
    channel_key_id BIGINT REFERENCES channel_key(id) ON DELETE SET NULL,
    credential_id BIGINT REFERENCES credential(id) ON DELETE SET NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL DEFAULT '',
    request_count BIGINT NOT NULL DEFAULT 0 CHECK (request_count >= 0),
    success_count BIGINT NOT NULL DEFAULT 0 CHECK (success_count >= 0),
    error_count BIGINT NOT NULL DEFAULT 0 CHECK (error_count >= 0),
    streamed_count BIGINT NOT NULL DEFAULT 0 CHECK (streamed_count >= 0),
    latency_ms_total BIGINT NOT NULL DEFAULT 0 CHECK (latency_ms_total >= 0),
    first_response_ms_total BIGINT NOT NULL DEFAULT 0 CHECK (first_response_ms_total >= 0),
    first_response_count BIGINT NOT NULL DEFAULT 0 CHECK (first_response_count >= 0),
    input_tokens BIGINT NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens BIGINT NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    total_tokens BIGINT NOT NULL DEFAULT 0 CHECK (total_tokens >= 0),
    cache_in_tokens BIGINT NOT NULL DEFAULT 0 CHECK (cache_in_tokens >= 0),
    cache_create_in_tokens BIGINT NOT NULL DEFAULT 0 CHECK (cache_create_in_tokens >= 0),
    cache_create_5m_in_tokens BIGINT NOT NULL DEFAULT 0 CHECK (cache_create_5m_in_tokens >= 0),
    cache_create_1h_in_tokens BIGINT NOT NULL DEFAULT 0 CHECK (cache_create_1h_in_tokens >= 0),
    reason_out_tokens BIGINT NOT NULL DEFAULT 0 CHECK (reason_out_tokens >= 0),
    audio_in_tokens BIGINT NOT NULL DEFAULT 0 CHECK (audio_in_tokens >= 0),
    audio_out_tokens BIGINT NOT NULL DEFAULT 0 CHECK (audio_out_tokens >= 0),
    cost_micro_usd BIGINT NOT NULL DEFAULT 0 CHECK (cost_micro_usd >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE task_upstream (
    id BIGSERIAL PRIMARY KEY,
    task_type TEXT NOT NULL CHECK (
        task_type IN ('openai_response', 'anthropic_message_batch')
    ),
    upstream_task_id TEXT NOT NULL,

    user_id BIGINT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    user_key_id BIGINT NOT NULL REFERENCES user_key(id) ON DELETE CASCADE,

    protocol TEXT NOT NULL CHECK (protocol IN ('openai', 'anthropic')),
    provider TEXT NOT NULL,
    model TEXT,

    channel_id BIGINT REFERENCES channel(id) ON DELETE SET NULL,
    channel_endpoint_id BIGINT REFERENCES channel_endpoint(id) ON DELETE SET NULL,
    channel_key_id BIGINT REFERENCES channel_key(id) ON DELETE SET NULL,
    credential_id BIGINT REFERENCES credential(id) ON DELETE SET NULL,
    upstream_base_url TEXT NOT NULL,

    status TEXT NOT NULL,
    terminal BOOLEAN NOT NULL DEFAULT FALSE,

    billing_hold JSONB,
    billing_status TEXT NOT NULL DEFAULT 'held' CHECK (
        billing_status IN ('held', 'settled', 'released', 'failed')
    ),
    usage_summary JSONB NOT NULL DEFAULT '{}',
    upstream_metadata JSONB NOT NULL DEFAULT '{}',

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_polled_at TIMESTAMPTZ,
    next_poll_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,

    UNIQUE (task_type, provider, upstream_task_id),
    UNIQUE (user_key_id, task_type, upstream_task_id)
);

CREATE INDEX idx_user_key_user_id ON user_key(user_id);
CREATE INDEX idx_user_key_key_prefix ON user_key(key_prefix);
CREATE INDEX idx_user_key_created_id ON user_key(created_at DESC, id DESC);
CREATE INDEX idx_user_key_user_created_id ON user_key(user_id, created_at DESC, id DESC);
CREATE INDEX idx_user_key_model_enabled_key
    ON user_key_model(user_key_id)
    WHERE enabled = TRUE;
CREATE INDEX idx_user_user_group ON "user"(user_group_id);
CREATE INDEX idx_user_created_id ON "user"(created_at DESC, id DESC);
CREATE INDEX idx_admin_status ON admin(status);
CREATE INDEX idx_user_code_email_active
    ON user_code(email, expires_at)
    WHERE consumed_at IS NULL;
CREATE UNIQUE INDEX idx_payment_provider_order
    ON payment(provider, provider_order_id)
    WHERE provider_order_id IS NOT NULL;
CREATE INDEX idx_payment_user_created ON payment(user_id, created_at DESC);
CREATE INDEX idx_payment_pending ON payment(provider, created_at ASC)
    WHERE status = 'pending';
CREATE INDEX idx_payment_event_payment_created ON payment_event(payment_id, created_at DESC);
CREATE INDEX idx_channel_provider_priority ON channel(provider, priority DESC);
CREATE INDEX idx_channel_relay_ready ON channel(provider, priority DESC, created_at ASC)
    WHERE enabled = TRUE;
CREATE INDEX idx_channel_endpoint_relay_ready ON channel_endpoint(protocol, channel_id)
    WHERE enabled = TRUE AND healthy = TRUE;
CREATE INDEX idx_channel_key_channel_id ON channel_key(channel_id);
CREATE INDEX idx_channel_key_relay_ready ON channel_key(channel_id, created_at ASC)
    WHERE enabled = TRUE AND healthy = TRUE;
CREATE INDEX idx_credential_provider_enabled
    ON credential(provider, enabled);
CREATE INDEX idx_credential_provider_plan
    ON credential(provider, plan_type)
    WHERE enabled = TRUE AND plan_type IS NOT NULL;
CREATE INDEX idx_usage_created ON usage(created_at DESC);
CREATE INDEX idx_usage_created_id ON usage(created_at DESC, id DESC);
CREATE INDEX idx_usage_user_created ON usage(user_id, created_at DESC);
CREATE INDEX idx_usage_user_billable_created
    ON usage(user_id, created_at DESC)
    WHERE billing_status IN ('billed', 'undercharged')
      AND cost_micro_usd IS NOT NULL;
CREATE INDEX idx_usage_user_billable_cursor
    ON usage(user_id, created_at DESC, id DESC)
    WHERE billing_status IN ('billed', 'undercharged')
      AND cost_micro_usd IS NOT NULL;
CREATE INDEX idx_usage_channel_created ON usage(channel_id, created_at DESC);
CREATE INDEX idx_usage_provider_created ON usage(provider, created_at DESC, id DESC);
CREATE INDEX idx_usage_model_created ON usage(model, created_at DESC, id DESC)
    WHERE model IS NOT NULL;
CREATE INDEX idx_billing_pending_created ON billing(created_at ASC)
    WHERE status = 'pending';
CREATE INDEX idx_billing_pending_attempts_created ON billing(attempts ASC, created_at ASC)
    WHERE status = 'pending';
CREATE INDEX idx_provider_model_provider_enabled ON provider_model(provider, enabled, model);
CREATE INDEX idx_provider_plan_lookup
    ON provider_plan(provider, protocol, plan_type, model)
    WHERE enabled = TRUE;
CREATE INDEX idx_credential_model_unavailable
    ON credential_model(channel_endpoint_id, model, unavailable_until)
    WHERE status = 'unavailable';
CREATE INDEX idx_pricing_template_provider_model ON pricing_template(provider, model) WHERE enabled = TRUE;
CREATE INDEX idx_pricing_policy_user_group_enabled
    ON pricing_policy(user_group, enabled, priority DESC);
CREATE UNIQUE INDEX idx_usage_billing_transaction_id ON usage(billing_transaction_id)
    WHERE billing_transaction_id IS NOT NULL;
CREATE INDEX idx_credit_allocation_account ON credit_allocation(credit_account_id, status, created_at ASC);
CREATE INDEX idx_credit_allocation_active_stale ON credit_allocation(created_at ASC, id ASC)
    WHERE status = 'active';
CREATE INDEX idx_credit_ledger_account_created ON credit_ledger(credit_account_id, created_at DESC);
CREATE INDEX idx_credit_ledger_usage ON credit_ledger(usage_id);
CREATE INDEX idx_credit_ledger_allocation ON credit_ledger(allocation_id);
CREATE UNIQUE INDEX idx_credit_ledger_transaction_allocation
    ON credit_ledger(transaction_id, allocation_id, credit_account_id)
    WHERE allocation_id IS NOT NULL;
CREATE UNIQUE INDEX idx_usage_daily_identity ON usage_daily(
    day,
    COALESCE(user_id, '-1'::BIGINT),
    COALESCE(user_key_id, '-1'::BIGINT),
    COALESCE(channel_id, '-1'::BIGINT),
    COALESCE(channel_key_id, '-1'::BIGINT),
    COALESCE(credential_id, '-1'::BIGINT),
    provider,
    model
);
CREATE INDEX idx_usage_daily_day ON usage_daily(day DESC);
CREATE INDEX idx_usage_daily_user_day ON usage_daily(user_id, day DESC);
CREATE INDEX idx_usage_daily_user_key_day ON usage_daily(user_id, user_key_id, day DESC);
CREATE INDEX idx_usage_daily_provider_model_day ON usage_daily(provider, model, day DESC);
CREATE INDEX idx_task_upstream_owner
    ON task_upstream(user_key_id, task_type, provider, upstream_task_id);
CREATE INDEX idx_task_upstream_owner_created
    ON task_upstream(user_key_id, task_type, created_at DESC, id DESC);
CREATE INDEX idx_task_upstream_polling
    ON task_upstream(next_poll_at)
    WHERE terminal = FALSE;
CREATE INDEX idx_task_upstream_expired_terminal
    ON task_upstream(expires_at ASC, id ASC)
    WHERE terminal = TRUE
      AND billing_status IN ('settled', 'released', 'failed')
      AND expires_at IS NOT NULL;
CREATE INDEX idx_task_upstream_stale_held
    ON task_upstream(updated_at ASC, id ASC)
    WHERE terminal = TRUE
      AND billing_status = 'held'
      AND usage_summary = '{}'::JSONB;

INSERT INTO provider_model (provider, model, display_name, source, enabled)
SELECT provider.code, model, model, 'seed', FALSE
FROM provider
CROSS JOIN LATERAL unnest(provider.default_models) AS model
ON CONFLICT (provider, model) DO NOTHING;

INSERT INTO pricing_policy (name, user_group, multiplier_micros, enabled, priority)
VALUES ('Default price', 'default', 1000000, TRUE, 0);

-- Enable existing OpenAI seed models so they are available for OAuth channels.
UPDATE provider_model
SET enabled = TRUE, updated_at = now()
WHERE provider = 'openai' AND source = 'seed' AND enabled = FALSE;

-- Insert Codex/OAuth models discovered from OpenAI's Codex client catalog.
INSERT INTO provider_model (provider, model, display_name, source, enabled)
VALUES
    ('openai', 'codex-auto-review',    'codex-auto-review',    'seed', TRUE),
    ('openai', 'gpt-5.2',              'gpt-5.2',              'seed', TRUE),
    ('openai', 'gpt-5.3-codex',         'gpt-5.3-codex',         'seed', TRUE),
    ('openai', 'gpt-5.3-codex-spark',   'gpt-5.3-codex-spark',   'seed', TRUE),
    ('openai', 'gpt-5.4',              'gpt-5.4',              'seed', TRUE),
    ('openai', 'gpt-5.4-mini',         'gpt-5.4-mini',         'seed', TRUE),
    ('openai', 'gpt-5.5',              'gpt-5.5',              'seed', TRUE)
ON CONFLICT (provider, model) DO NOTHING;

INSERT INTO provider_plan (provider, protocol, plan_type, model, enabled, source)
SELECT 'openai', 'openai_oauth', plan_type, model, TRUE, 'seed'
FROM (
    VALUES
        ('free',     'codex-auto-review'),
        ('free',     'gpt-5.2'),
        ('free',     'gpt-5.3-codex'),
        ('free',     'gpt-5.4'),
        ('free',     'gpt-5.4-mini'),
        ('free',     'gpt-5.5'),
        ('team',     'codex-auto-review'),
        ('team',     'gpt-5.2'),
        ('team',     'gpt-5.3-codex'),
        ('team',     'gpt-5.4'),
        ('team',     'gpt-5.4-mini'),
        ('team',     'gpt-5.5'),
        ('business', 'codex-auto-review'),
        ('business', 'gpt-5.2'),
        ('business', 'gpt-5.3-codex'),
        ('business', 'gpt-5.4'),
        ('business', 'gpt-5.4-mini'),
        ('business', 'gpt-5.5'),
        ('go',       'codex-auto-review'),
        ('go',       'gpt-5.2'),
        ('go',       'gpt-5.3-codex'),
        ('go',       'gpt-5.4'),
        ('go',       'gpt-5.4-mini'),
        ('go',       'gpt-5.5'),
        ('plus',     'gpt-5.2'),
        ('plus',     'gpt-5.3-codex'),
        ('plus',     'gpt-5.3-codex-spark'),
        ('plus',     'gpt-5.4'),
        ('plus',     'gpt-5.4-mini'),
        ('plus',     'gpt-5.5'),
        ('pro',      'gpt-5.2'),
        ('pro',      'gpt-5.3-codex'),
        ('pro',      'gpt-5.3-codex-spark'),
        ('pro',      'gpt-5.4'),
        ('pro',      'gpt-5.4-mini'),
        ('pro',      'gpt-5.5')
) AS plan_models(plan_type, model)
ON CONFLICT (provider, protocol, plan_type, model)
DO UPDATE SET
    enabled = EXCLUDED.enabled,
    source = EXCLUDED.source,
    updated_at = now();
