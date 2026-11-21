ALTER TABLE draft_assets ADD COLUMN text_only INTEGER NOT NULL DEFAULT 0 CHECK (text_only IN (0, 1));
ALTER TABLE message_assets ADD COLUMN text_only INTEGER NOT NULL DEFAULT 0 CHECK (text_only IN (0, 1));
CREATE TABLE asset_originals (
    asset_id TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    payload BLOB NOT NULL
);
CREATE TABLE asset_pages (
    asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    page INTEGER NOT NULL CHECK (page > 0),
    method TEXT NOT NULL,
    language TEXT NOT NULL DEFAULT '',
    confidence REAL,
    error TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (asset_id, page)
);
