ALTER TABLE generation_settings ADD COLUMN context_messages INTEGER;

CREATE INDEX idx_generation_settings_conversation_created_at ON generation_settings(conversation_id, created_at DESC, id DESC);
CREATE INDEX idx_messages_conversation_created_at ON messages(conversation_id, created_at DESC, id DESC);
