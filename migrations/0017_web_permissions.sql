CREATE TABLE web_permissions (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    destination TEXT NOT NULL CHECK (length(destination) > 0)
);
