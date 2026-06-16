ALTER TABLE conversations ADD COLUMN pinned_at TEXT;

CREATE INDEX idx_conversations_pinned_updated_at ON conversations(pinned_at DESC, updated_at DESC);
CREATE INDEX idx_conversations_archived_updated_at ON conversations(archived_at, updated_at DESC);
