ALTER TABLE node_samples ADD COLUMN supernode_id INTEGER;
ALTER TABLE node_samples ADD COLUMN supernode_status TEXT;
ALTER TABLE node_samples ADD COLUMN supernode_last_round INTEGER;
ALTER TABLE node_samples ADD COLUMN supernode_last_round_at INTEGER;
ALTER TABLE node_samples ADD COLUMN supernode_checks_passed INTEGER;
ALTER TABLE node_samples ADD COLUMN supernode_failed_checks_json TEXT;
ALTER TABLE node_samples ADD COLUMN supernode_metric_error TEXT;
