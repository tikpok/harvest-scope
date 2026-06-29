PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS nodes (
  id TEXT PRIMARY KEY NOT NULL,
  display_name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('symbol', 'nis1')),
  base_url TEXT NOT NULL UNIQUE,
  network TEXT,
  version TEXT,
  public_key TEXT,
  roles_json TEXT NOT NULL DEFAULT '[]',
  rest_version TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  favorite INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS node_samples (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  observed_at INTEGER NOT NULL,
  reachable INTEGER NOT NULL,
  status TEXT NOT NULL,
  latency_ms INTEGER,
  height INTEGER,
  finalized_height INTEGER,
  network_height INTEGER,
  lag INTEGER,
  peer_count INTEGER,
  chain_score TEXT,
  api_status TEXT,
  db_status TEXT,
  nis_status_code INTEGER,
  last_error TEXT
);
CREATE INDEX IF NOT EXISTS idx_samples_node_time ON node_samples(node_id, observed_at DESC);
CREATE INDEX IF NOT EXISTS idx_samples_time ON node_samples(observed_at);

CREATE TABLE IF NOT EXISTS hourly_metrics (
  node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  bucket_at INTEGER NOT NULL,
  sample_count INTEGER NOT NULL,
  reachable_count INTEGER NOT NULL,
  latency_avg REAL,
  latency_min INTEGER,
  latency_max INTEGER,
  height_min INTEGER,
  height_max INTEGER,
  finalized_min INTEGER,
  finalized_max INTEGER,
  peer_avg REAL,
  peer_min INTEGER,
  peer_max INTEGER,
  lag_max INTEGER,
  PRIMARY KEY (node_id, bucket_at)
);

CREATE TABLE IF NOT EXISTS peers_current (
  node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  peer_key TEXT NOT NULL,
  name TEXT,
  host TEXT,
  port INTEGER,
  status TEXT NOT NULL,
  version TEXT,
  observed_at INTEGER NOT NULL,
  PRIMARY KEY (node_id, peer_key)
);

CREATE TABLE IF NOT EXISTS alerts (
  id TEXT PRIMARY KEY NOT NULL,
  node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  alert_type TEXT NOT NULL,
  severity TEXT NOT NULL CHECK (severity IN ('warning', 'critical')),
  state TEXT NOT NULL CHECK (state IN ('open', 'acknowledged', 'resolved')),
  message TEXT NOT NULL,
  raised_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  acknowledged_at INTEGER,
  resolved_at INTEGER,
  occurrence_count INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_alerts_state_time ON alerts(state, updated_at DESC);

CREATE TABLE IF NOT EXISTS scan_profiles (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  cidr TEXT NOT NULL,
  ports_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY NOT NULL,
  value_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

