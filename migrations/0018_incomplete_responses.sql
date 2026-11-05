DROP VIEW active_messages;

CREATE TABLE messages_new (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
    content TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('streaming', 'complete', 'incomplete', 'cancelled', 'failed')),
    token_count INTEGER,
    created_at TEXT NOT NULL,
    completed_at TEXT,
    parent_id TEXT REFERENCES messages(id) DEFERRABLE INITIALLY DEFERRED,
    is_selected INTEGER NOT NULL DEFAULT 1 CHECK (is_selected IN (0, 1)),
    FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

INSERT INTO messages_new (
    id, conversation_id, role, content, status, token_count,
    created_at, completed_at, parent_id, is_selected
)
SELECT id, conversation_id, role, content, status, token_count,
       created_at, completed_at, parent_id, is_selected
FROM messages;

DROP TABLE messages;
ALTER TABLE messages_new RENAME TO messages;

CREATE INDEX idx_messages_conversation_id ON messages(conversation_id);
CREATE INDEX idx_messages_conversation_created_at ON messages(conversation_id, created_at DESC, id DESC);
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
