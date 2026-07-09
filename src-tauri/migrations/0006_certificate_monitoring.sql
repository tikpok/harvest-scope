ALTER TABLE node_samples ADD COLUMN node_certificate_not_after INTEGER;
ALTER TABLE node_samples ADD COLUMN node_certificate_remaining_days INTEGER;
ALTER TABLE node_samples ADD COLUMN voting_key_start_epoch INTEGER;
ALTER TABLE node_samples ADD COLUMN voting_key_end_epoch INTEGER;
ALTER TABLE node_samples ADD COLUMN finalization_epoch INTEGER;
ALTER TABLE node_samples ADD COLUMN voting_key_remaining_epochs INTEGER;
ALTER TABLE node_samples ADD COLUMN certificate_metric_error TEXT;
