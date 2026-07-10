ALTER TABLE nodes ADD COLUMN monitor_mode TEXT NOT NULL DEFAULT 'api' CHECK (monitor_mode IN ('api', 'peer'));
ALTER TABLE peers_current ADD COLUMN roles_json TEXT NOT NULL DEFAULT '[]';
