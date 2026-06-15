CREATE TABLE chat_profiles (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL COLLATE NOCASE UNIQUE,
  description TEXT NOT NULL,
  temperature REAL NOT NULL CHECK (temperature >= 0.0 AND temperature <= 2.0),
  system_prompt TEXT NOT NULL,
  is_builtin INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

INSERT INTO chat_profiles (id, name, description, temperature, system_prompt, is_builtin, created_at, updated_at)
VALUES
  ('builtin-general', 'General', 'Everyday questions, explanations and practical help', 0.7, 'Be clear, helpful and direct. Give practical answers and explain important details without unnecessary complexity.', 1, '2026-06-15T00:00:00Z', '2026-06-15T00:00:00Z'),
  ('builtin-code', 'Code', 'Programming, debugging and technical explanations', 0.2, 'Respond with technical precision. Ask for missing context when it affects the solution, prioritize verifiable approaches and clearly identify assumptions.', 1, '2026-06-15T00:00:00Z', '2026-06-15T00:00:00Z'),
  ('builtin-writing', 'Writing', 'Drafting, rewriting, ideas and tone', 0.9, 'Help write with clarity, purpose and a consistent style. Preserve the intended voice while improving structure and wording.', 1, '2026-06-15T00:00:00Z', '2026-06-15T00:00:00Z'),
  ('builtin-translation', 'Translation', 'Translation and language adaptation', 0.3, 'Translate while preserving meaning, tone, formatting and relevant cultural context. Flag genuine ambiguities instead of silently guessing.', 1, '2026-06-15T00:00:00Z', '2026-06-15T00:00:00Z'),
  ('builtin-reasoning', 'Reasoning', 'Complex problems, analysis and decisions', 0.4, 'Analyze carefully, break complex problems into useful parts and distinguish conclusions from assumptions and uncertainty.', 1, '2026-06-15T00:00:00Z', '2026-06-15T00:00:00Z');

ALTER TABLE generation_settings ADD COLUMN profile_id TEXT REFERENCES chat_profiles(id) ON DELETE SET NULL;

CREATE INDEX idx_generation_settings_profile_id ON generation_settings(profile_id);
