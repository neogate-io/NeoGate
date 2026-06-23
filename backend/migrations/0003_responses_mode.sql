ALTER TABLE channel_endpoint
    ADD COLUMN responses_mode TEXT NOT NULL DEFAULT 'native'
        CHECK (responses_mode IN ('native', 'chat_fallback', 'disabled')),
    ADD COLUMN responses_mode_source TEXT NOT NULL DEFAULT 'auto'
        CHECK (responses_mode_source IN ('auto', 'manual', 'probed')),
    ADD COLUMN responses_probe JSONB NOT NULL DEFAULT '{}'::JSONB;
