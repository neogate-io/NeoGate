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

CREATE INDEX idx_user_key_model_enabled_key
    ON user_key_model(user_key_id)
    WHERE enabled = TRUE;

ALTER TABLE credit_account
    DROP CONSTRAINT credit_account_owner_type_check;

ALTER TABLE credit_account
    ADD CONSTRAINT credit_account_owner_type_check
    CHECK (owner_type IN ('user', 'user_key', 'user_key_model'));
