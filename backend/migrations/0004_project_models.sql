CREATE TABLE project_model (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    target_model TEXT NOT NULL,
    target_channel_id BIGINT REFERENCES channel(id) ON DELETE SET NULL,
    route_mode TEXT NOT NULL DEFAULT 'direct',
    routing_config JSONB NOT NULL DEFAULT '{}'::JSONB,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, model),
    CHECK (route_mode IN ('direct', 'smart')),
    CHECK (length(trim(model)) > 0),
    CHECK (length(trim(target_model)) > 0)
);

CREATE INDEX idx_project_model_project_enabled
    ON project_model(project_id, enabled, model);

CREATE TABLE project_model_candidate (
    id BIGSERIAL PRIMARY KEY,
    project_model_id BIGINT NOT NULL REFERENCES project_model(id) ON DELETE CASCADE,
    target_model TEXT NOT NULL,
    target_channel_id BIGINT REFERENCES channel(id) ON DELETE SET NULL,
    tier TEXT NOT NULL CHECK (tier IN ('simple', 'standard', 'advanced')),
    priority INTEGER NOT NULL DEFAULT 0,
    weight INTEGER NOT NULL DEFAULT 1 CHECK (weight >= 1),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (length(trim(target_model)) > 0)
);

CREATE INDEX idx_project_model_candidate_parent
    ON project_model_candidate(project_model_id, enabled, tier, priority DESC, id ASC);

CREATE TABLE routing_decision (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    project_model_id BIGINT REFERENCES project_model(id) ON DELETE SET NULL,
    requested_model TEXT NOT NULL,
    selected_model TEXT NOT NULL,
    selected_channel_id BIGINT REFERENCES channel(id) ON DELETE SET NULL,
    decision_source TEXT NOT NULL CHECK (decision_source IN ('rules', 'classifier', 'fallback')),
    tier TEXT NOT NULL CHECK (tier IN ('simple', 'standard', 'advanced')),
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0,
    reason TEXT NOT NULL DEFAULT '',
    latency_ms BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_routing_decision_project_created
    ON routing_decision(project_id, created_at DESC, id DESC);

ALTER TABLE usage
    ADD COLUMN upstream_model TEXT,
    ADD COLUMN routing_phase TEXT NOT NULL DEFAULT 'relay',
    ADD CONSTRAINT usage_routing_phase_check CHECK (routing_phase IN ('relay', 'classifier'));

ALTER TABLE task_upstream
    ADD COLUMN upstream_model TEXT;

UPDATE task_upstream
SET upstream_model = model
WHERE upstream_model IS NULL;

ALTER TABLE user_key
    DROP COLUMN IF EXISTS model_limits;
