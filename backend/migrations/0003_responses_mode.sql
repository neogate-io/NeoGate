ALTER TABLE channel_endpoint
    ADD COLUMN responses_capability TEXT NOT NULL DEFAULT 'unknown'
        CHECK (responses_capability IN ('unknown', 'native', 'chat_fallback', 'disabled')),
    ADD COLUMN responses_checked_at TIMESTAMPTZ,
    ADD COLUMN responses_probe JSONB NOT NULL DEFAULT '{}'::JSONB;
