CREATE INDEX IF NOT EXISTS idx_task_upstream_held_billing_hold
    ON task_upstream USING GIN (billing_hold jsonb_path_ops)
    WHERE billing_status = 'held'
      AND billing_hold IS NOT NULL;

DROP INDEX IF EXISTS idx_billing_pending_created;
DROP INDEX IF EXISTS idx_billing_pending_attempts_created;

CREATE INDEX idx_billing_pending_created ON billing(created_at ASC)
    WHERE status IN ('pending', 'failed');
CREATE INDEX idx_billing_pending_attempts_created ON billing(attempts ASC, created_at ASC)
    WHERE status IN ('pending', 'failed');
