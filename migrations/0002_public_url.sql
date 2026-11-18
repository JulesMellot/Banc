-- Phase 1: the server's public address, learned from the first admin's
-- request when the configuration does not give it. A single row.
CREATE TABLE server_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    public_url TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
