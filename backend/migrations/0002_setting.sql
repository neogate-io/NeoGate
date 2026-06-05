CREATE TABLE setting (
    id BIGSERIAL PRIMARY KEY,
    key TEXT NOT NULL UNIQUE CHECK (length(trim(key)) > 0),
    value JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
