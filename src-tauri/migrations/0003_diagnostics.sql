CREATE TABLE IF NOT EXISTS diagnostic_runs (
  id TEXT PRIMARY KEY NOT NULL,
  node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  alert_id TEXT REFERENCES alerts(id) ON DELETE SET NULL,
  started_at INTEGER NOT NULL,
  completed_at INTEGER NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('passed', 'warning', 'failed')),
  summary TEXT NOT NULL,
  checks_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_diagnostic_runs_node_time
  ON diagnostic_runs(node_id, completed_at DESC);
CREATE INDEX IF NOT EXISTS idx_diagnostic_runs_alert_time
  ON diagnostic_runs(alert_id, completed_at DESC);
