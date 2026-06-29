CREATE TABLE project_model (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    target_model TEXT NOT NULL,
    target_channel_id BIGINT REFERENCES channel(id) ON DELETE SET NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, model),
    CHECK (length(trim(model)) > 0),
    CHECK (length(trim(target_model)) > 0)
);

CREATE INDEX idx_project_model_project_enabled
    ON project_model(project_id, enabled, model);

ALTER TABLE usage
    ADD COLUMN upstream_model TEXT;

ALTER TABLE task_upstream
    ADD COLUMN upstream_model TEXT;

UPDATE task_upstream
SET upstream_model = model
WHERE upstream_model IS NULL;

ALTER TABLE user_key
    DROP COLUMN IF EXISTS model_limits;
