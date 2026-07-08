CREATE TABLE IF NOT EXISTS bridges (
  id TEXT PRIMARY KEY NOT NULL,
  display_name TEXT NOT NULL,
  api_url TEXT NOT NULL UNIQUE,
  direction TEXT NOT NULL CHECK (direction IN ('auto', 'wrap_only', 'bidirectional')),
  enabled INTEGER NOT NULL DEFAULT 1,
  bridge_enabled INTEGER,
  native_network_json TEXT,
  wrapped_network_json TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS bridge_samples (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  bridge_id TEXT NOT NULL REFERENCES bridges(id) ON DELETE CASCADE,
  observed_at INTEGER NOT NULL,
  reachable INTEGER NOT NULL,
  status TEXT NOT NULL,
  latency_ms INTEGER,
  bridge_enabled INTEGER,
  wrap_unprocessed INTEGER NOT NULL DEFAULT 0,
  wrap_unprocessed_capped INTEGER NOT NULL DEFAULT 0,
  wrap_failed INTEGER NOT NULL DEFAULT 0,
  wrap_failed_capped INTEGER NOT NULL DEFAULT 0,
  unwrap_unprocessed INTEGER NOT NULL DEFAULT 0,
  unwrap_unprocessed_capped INTEGER NOT NULL DEFAULT 0,
  unwrap_failed INTEGER NOT NULL DEFAULT 0,
  unwrap_failed_capped INTEGER NOT NULL DEFAULT 0,
  oldest_unprocessed_at INTEGER,
  last_error TEXT
);

CREATE INDEX IF NOT EXISTS idx_bridge_samples_target_time
  ON bridge_samples(bridge_id, observed_at DESC);
CREATE INDEX IF NOT EXISTS idx_bridge_samples_time
  ON bridge_samples(observed_at);
