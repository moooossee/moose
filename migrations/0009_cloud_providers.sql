CREATE TABLE providers_new (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('ollama', 'ollama-cloud', 'openai', 'anthropic', 'groq', 'gemini')),
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  is_managed INTEGER NOT NULL DEFAULT 0 CHECK (is_managed IN (0, 1)),
  is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (is_managed = 0 OR kind = 'ollama')
);
INSERT INTO providers_new SELECT * FROM providers;
DROP TABLE providers;
ALTER TABLE providers_new RENAME TO providers;
CREATE UNIQUE INDEX idx_providers_single_default ON providers(is_default) WHERE is_default = 1;

CREATE TABLE privacy_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  local_only INTEGER NOT NULL CHECK (local_only IN (0, 1))
);

INSERT INTO privacy_settings VALUES (1, NOT EXISTS (SELECT 1 FROM providers WHERE is_managed = 0));

CREATE TABLE conversation_remote_permissions (
  conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
  destination TEXT NOT NULL,
  allow_messages INTEGER NOT NULL DEFAULT 0 CHECK (allow_messages IN (0, 1)),
  allow_files INTEGER NOT NULL DEFAULT 0 CHECK (allow_files IN (0, 1))
);
