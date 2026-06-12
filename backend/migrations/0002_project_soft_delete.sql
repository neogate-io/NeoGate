ALTER TABLE project
ADD COLUMN deleted_at TIMESTAMPTZ;

CREATE INDEX idx_project_deleted_created
    ON project(deleted_at, created_at DESC, id DESC);
