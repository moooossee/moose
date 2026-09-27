CREATE TABLE message_details (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    reasoning TEXT NOT NULL DEFAULT '',
    reasoning_ms INTEGER NOT NULL DEFAULT 0,
    elapsed_ms INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE conversation_drafts (
    conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE conversation_thinking (
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (conversation_id, model)
);

CREATE TABLE workspace_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    conversation_id TEXT REFERENCES conversations(id) ON DELETE SET NULL
);
