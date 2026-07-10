CREATE INDEX IF NOT EXISTS idx_samples_node_time_id
  ON node_samples(node_id, observed_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_samples_latest_harvest
  ON node_samples(node_id, observed_at DESC, id DESC)
  WHERE delegated_harvester_count IS NOT NULL
     OR total_importance IS NOT NULL
     OR total_balance_micro_xym IS NOT NULL
     OR harvest_metric_error IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_samples_latest_certificate
  ON node_samples(node_id, observed_at DESC, id DESC)
  WHERE node_certificate_not_after IS NOT NULL
     OR node_certificate_remaining_days IS NOT NULL
     OR voting_key_start_epoch IS NOT NULL
     OR voting_key_end_epoch IS NOT NULL
     OR finalization_epoch IS NOT NULL
     OR voting_key_remaining_epochs IS NOT NULL
     OR certificate_metric_error IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_samples_latest_supernode
  ON node_samples(node_id, observed_at DESC, id DESC)
  WHERE supernode_id IS NOT NULL
     OR supernode_status IS NOT NULL
     OR supernode_last_round IS NOT NULL
     OR supernode_checks_passed IS NOT NULL
     OR supernode_metric_error IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_harvest_events_node_time_id
  ON harvest_events(node_id, observed_at DESC, id DESC);
