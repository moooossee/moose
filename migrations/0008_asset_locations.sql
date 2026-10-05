CREATE TABLE asset_locations (
    asset_id TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    source_uri TEXT NOT NULL
);
