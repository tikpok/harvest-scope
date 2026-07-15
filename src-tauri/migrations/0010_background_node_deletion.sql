ALTER TABLE nodes ADD COLUMN deleted_at INTEGER;

CREATE INDEX IF NOT EXISTS idx_nodes_deleted_at
  ON nodes(deleted_at);
