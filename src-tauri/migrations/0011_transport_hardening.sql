ALTER TABLE nodes ADD COLUMN allow_insecure_http INTEGER NOT NULL DEFAULT 0;
ALTER TABLE nodes ADD COLUMN supernode_id INTEGER;

UPDATE nodes
SET enabled = 0
WHERE LOWER(base_url) LIKE 'http://%';
