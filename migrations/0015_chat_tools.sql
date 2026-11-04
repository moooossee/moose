CREATE TABLE conversation_tools (
    conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    calculator INTEGER NOT NULL DEFAULT 0 CHECK (calculator IN (0, 1))
);

CREATE TABLE message_tool_activity (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    calls TEXT NOT NULL DEFAULT '[]',
    notice TEXT NOT NULL DEFAULT ''
);
