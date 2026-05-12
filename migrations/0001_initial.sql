CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
);

CREATE TABLE providers (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('ollama')),
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  is_managed INTEGER NOT NULL DEFAULT 0 CHECK (is_managed IN (0, 1)),
  is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_providers_single_default ON providers(is_default) WHERE is_default = 1;

CREATE TABLE models (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL,
  name TEXT NOT NULL,
  digest TEXT,
  size_bytes INTEGER,
  family TEXT,
  parameter_size TEXT,
  quantization_level TEXT,
  installed_at TEXT,
  last_seen_at TEXT NOT NULL,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);

CREATE TABLE conversations (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL,
  model_id TEXT,
  title TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE RESTRICT,
  FOREIGN KEY(model_id) REFERENCES models(id) ON DELETE SET NULL
);

CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
  content TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('streaming', 'complete', 'cancelled', 'failed')),
  token_count INTEGER,
  created_at TEXT NOT NULL,
  completed_at TEXT,
  FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE TABLE generation_settings (
  id TEXT PRIMARY KEY,
  conversation_id TEXT,
  model TEXT,
  temperature REAL,
  top_p REAL,
  top_k INTEGER,
  seed INTEGER,
  num_ctx INTEGER,
  system_prompt TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE TABLE download_jobs (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL,
  model_name TEXT NOT NULL,
  status TEXT NOT NULL,
  total_bytes INTEGER,
  completed_bytes INTEGER,
  error_message TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);

CREATE INDEX idx_models_provider_id ON models(provider_id);
CREATE INDEX idx_conversations_updated_at ON conversations(updated_at);
CREATE INDEX idx_messages_conversation_id ON messages(conversation_id);
CREATE INDEX idx_download_jobs_provider_id ON download_jobs(provider_id);
