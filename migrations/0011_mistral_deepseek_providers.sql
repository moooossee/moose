CREATE TABLE providers_new (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('ollama', 'ollama-cloud', 'openai', 'anthropic', 'groq', 'gemini', 'mistral', 'deepseek')),
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  is_managed INTEGER NOT NULL DEFAULT 0 CHECK (is_managed IN (0, 1)),
  is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (is_managed = 0 OR kind = 'ollama')
);

INSERT INTO providers_new (id, kind, name, base_url, is_managed, is_default, created_at, updated_at)
SELECT id, kind, name, base_url, is_managed, is_default, created_at, updated_at FROM providers;

DROP TABLE providers;
ALTER TABLE providers_new RENAME TO providers;
CREATE UNIQUE INDEX idx_providers_single_default ON providers(is_default) WHERE is_default = 1;
