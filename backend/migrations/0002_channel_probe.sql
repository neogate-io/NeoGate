CREATE TABLE channel_probe (
    id BIGSERIAL PRIMARY KEY,
    channel_id BIGINT NOT NULL REFERENCES channel(id) ON DELETE CASCADE,
    channel_endpoint_id BIGINT REFERENCES channel_endpoint(id) ON DELETE SET NULL,
    channel_key_id BIGINT REFERENCES channel_key(id) ON DELETE SET NULL,
    provider TEXT NOT NULL,
    protocol TEXT NOT NULL,
    model TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK (status IN ('ok', 'failed', 'skipped')),
    latency_ms BIGINT,
    status_code INTEGER,
    error_summary TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_channel_probe_channel_created
    ON channel_probe(channel_id, created_at DESC, id DESC);

CREATE INDEX idx_channel_probe_created
    ON channel_probe(created_at DESC, id DESC);
