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

CREATE INDEX idx_task_upstream_owner
    ON task_upstream(user_key_id, task_type, provider, upstream_task_id);

CREATE INDEX idx_task_upstream_polling
    ON task_upstream(next_poll_at)
    WHERE terminal = FALSE;
