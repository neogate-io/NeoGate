ALTER TABLE "user"
ADD COLUMN IF NOT EXISTS password_changed_at TIMESTAMPTZ;

UPDATE "user"
SET password_changed_at = COALESCE(password_changed_at, now())
WHERE password_hash IS NOT NULL
  AND password_changed_at IS NULL;
