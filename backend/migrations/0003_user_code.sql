CREATE TABLE user_code (
    id BIGSERIAL PRIMARY KEY,
    email CITEXT NOT NULL,
    code_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_user_code_email_active
    ON user_code(email, expires_at)
    WHERE consumed_at IS NULL;
