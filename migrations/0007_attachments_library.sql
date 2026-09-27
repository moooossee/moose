CREATE TABLE assets (
    id TEXT PRIMARY KEY,
    digest TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('image', 'document', 'pdf')),
    mime_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    payload BLOB NOT NULL,
    page_count INTEGER NOT NULL,
    in_library INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE TABLE document_chunks (
    id INTEGER PRIMARY KEY,
    asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    page INTEGER NOT NULL,
    position INTEGER NOT NULL,
    content TEXT NOT NULL
);
CREATE INDEX document_chunks_asset ON document_chunks(asset_id, page, position);
CREATE VIRTUAL TABLE document_search USING fts5(content, content='document_chunks', content_rowid='id', tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER document_chunks_insert AFTER INSERT ON document_chunks BEGIN
    INSERT INTO document_search(rowid, content) VALUES (new.id, new.content);
END;
CREATE TRIGGER document_chunks_delete AFTER DELETE ON document_chunks BEGIN
    INSERT INTO document_search(document_search, rowid, content) VALUES ('delete', old.id, old.content);
END;
CREATE TABLE draft_assets (
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    asset_id TEXT NOT NULL REFERENCES assets(id),
    position INTEGER NOT NULL,
    PRIMARY KEY(conversation_id, asset_id)
);
CREATE TABLE message_assets (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    asset_id TEXT NOT NULL REFERENCES assets(id),
    position INTEGER NOT NULL,
    PRIMARY KEY(message_id, asset_id)
);
CREATE TABLE conversation_library (
    conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE response_sources (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    number INTEGER NOT NULL,
    asset_id TEXT NOT NULL REFERENCES assets(id),
    name TEXT NOT NULL,
    page INTEGER NOT NULL,
    content TEXT NOT NULL,
    PRIMARY KEY(message_id, number)
);
CREATE INDEX draft_assets_asset ON draft_assets(asset_id);
CREATE INDEX message_assets_asset ON message_assets(asset_id);
CREATE INDEX response_sources_asset ON response_sources(asset_id);
