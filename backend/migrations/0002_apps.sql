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
    endpoint_type TEXT NOT NULL CHECK (endpoint_type IN ('wecom', 'webhook', 'widget')),
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
CREATE INDEX idx_app_endpoint_type_enabled ON app_endpoint(endpoint_type, enabled);
CREATE INDEX idx_app_message_conversation_created ON app_message(conversation_id, created_at DESC);
CREATE INDEX idx_app_run_log_created ON app_run_log(created_at DESC, id DESC);
CREATE INDEX idx_app_run_log_app_created ON app_run_log(app_id, created_at DESC);
CREATE INDEX idx_app_run_log_endpoint_created ON app_run_log(endpoint_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_upstream_held_billing_hold
    ON task_upstream USING GIN (billing_hold jsonb_path_ops)
    WHERE billing_status = 'held'
      AND billing_hold IS NOT NULL;

DROP INDEX IF EXISTS idx_billing_pending_created;
DROP INDEX IF EXISTS idx_billing_pending_attempts_created;

CREATE INDEX idx_billing_pending_created ON billing(created_at ASC)
    WHERE status IN ('pending', 'failed');
CREATE INDEX idx_billing_pending_attempts_created ON billing(attempts ASC, created_at ASC)
    WHERE status IN ('pending', 'failed');
