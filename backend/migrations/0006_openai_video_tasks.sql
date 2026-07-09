ALTER TABLE task_upstream
    DROP CONSTRAINT IF EXISTS task_upstream_task_type_check;

ALTER TABLE task_upstream
    ADD CONSTRAINT task_upstream_task_type_check CHECK (
        task_type IN (
            'openai_response',
            'openai_video',
            'anthropic_message_batch',
            'neogate_response'
        )
    );

