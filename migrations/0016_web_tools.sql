ALTER TABLE conversation_tools ADD COLUMN web INTEGER NOT NULL DEFAULT 0 CHECK (web IN (0, 1));
ALTER TABLE conversation_tools ADD COLUMN web_permission TEXT NOT NULL DEFAULT '';

CREATE TABLE web_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    configuration TEXT NOT NULL
);
