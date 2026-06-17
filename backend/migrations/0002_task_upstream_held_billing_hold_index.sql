CREATE INDEX IF NOT EXISTS idx_task_upstream_held_billing_hold
    ON task_upstream USING GIN (billing_hold jsonb_path_ops)
    WHERE billing_status = 'held'
      AND billing_hold IS NOT NULL;
