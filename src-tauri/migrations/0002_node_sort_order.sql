ALTER TABLE nodes ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

WITH ordered AS (
  SELECT
    id,
    ROW_NUMBER() OVER (ORDER BY favorite DESC, display_name COLLATE NOCASE, id) - 1 AS new_order
  FROM nodes
)
UPDATE nodes
SET sort_order = (
  SELECT new_order
  FROM ordered
  WHERE ordered.id = nodes.id
);

CREATE INDEX IF NOT EXISTS idx_nodes_sort_order ON nodes(sort_order);
