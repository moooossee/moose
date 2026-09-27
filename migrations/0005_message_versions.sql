ALTER TABLE messages ADD COLUMN parent_id TEXT REFERENCES messages(id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE messages ADD COLUMN is_selected INTEGER NOT NULL DEFAULT 1 CHECK (is_selected IN (0, 1));

WITH ordered AS (
    SELECT id, LAG(id) OVER (PARTITION BY conversation_id ORDER BY created_at, id) AS previous_id
    FROM messages
)
UPDATE messages SET parent_id = (SELECT previous_id FROM ordered WHERE ordered.id = messages.id);

CREATE INDEX idx_messages_parent ON messages(conversation_id, parent_id, created_at, id);
CREATE UNIQUE INDEX idx_messages_selected_child ON messages(parent_id) WHERE is_selected = 1 AND parent_id IS NOT NULL;
CREATE UNIQUE INDEX idx_messages_selected_root ON messages(conversation_id) WHERE is_selected = 1 AND parent_id IS NULL;

CREATE VIEW active_messages AS
WITH RECURSIVE selected_path(id, depth) AS (
    SELECT id, 0 FROM messages WHERE parent_id IS NULL AND is_selected = 1
    UNION ALL
    SELECT m.id, p.depth + 1 FROM messages m
    JOIN selected_path p ON m.parent_id = p.id
    WHERE m.is_selected = 1
)
SELECT m.*, p.depth FROM messages m JOIN selected_path p ON m.id = p.id;
