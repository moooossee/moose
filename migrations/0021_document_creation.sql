CREATE TABLE document_requests (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    format TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL DEFAULT 'waiting',
    error TEXT NOT NULL DEFAULT ''
);

CREATE TABLE created_documents (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    format TEXT NOT NULL,
    content TEXT NOT NULL,
    payload BLOB NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(message_id, format)
);
