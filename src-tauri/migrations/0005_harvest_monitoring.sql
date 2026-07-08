ALTER TABLE nodes ADD COLUMN show_harvest_metrics INTEGER NOT NULL DEFAULT 0;

ALTER TABLE node_samples ADD COLUMN delegated_harvester_count INTEGER;
ALTER TABLE node_samples ADD COLUMN delegated_harvester_delta INTEGER;
ALTER TABLE node_samples ADD COLUMN total_importance TEXT;
ALTER TABLE node_samples ADD COLUMN total_balance_micro_xym TEXT;
ALTER TABLE node_samples ADD COLUMN harvest_metric_error TEXT;

CREATE TABLE IF NOT EXISTS node_harvesters_current (
  node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  public_key TEXT NOT NULL,
  observed_at INTEGER NOT NULL,
  PRIMARY KEY (node_id, public_key)
);
CREATE INDEX IF NOT EXISTS idx_node_harvesters_public_key
  ON node_harvesters_current(public_key);

CREATE TABLE IF NOT EXISTS harvest_events (
  id TEXT PRIMARY KEY NOT NULL,
  event_key TEXT NOT NULL UNIQUE,
  node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  observer_node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  harvester_public_key TEXT NOT NULL,
  height TEXT,
  hash TEXT,
  observed_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_harvest_events_time
  ON harvest_events(observed_at DESC);
CREATE INDEX IF NOT EXISTS idx_harvest_events_node_time
  ON harvest_events(node_id, observed_at DESC);
