CREATE TABLE conversation_folders (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
    name_key TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

ALTER TABLE conversations ADD COLUMN folder_id TEXT REFERENCES conversation_folders(id) ON DELETE SET NULL;

CREATE INDEX conversations_folder_history_idx ON conversations(folder_id, archived_at, pinned_at DESC, updated_at DESC);
