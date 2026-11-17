-- Schema obtained after all migrations.
-- Written by `cargo xtask schema`, do not edit by hand.

CREATE TABLE admin_sessions (
    -- BLAKE3 of the cookie value: a database leak does not give sessions.
    token_hash BLOB PRIMARY KEY,
    admin_id INTEGER NOT NULL REFERENCES admins (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX admin_sessions_admin ON admin_sessions (admin_id);

CREATE TABLE admins (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    -- argon2id, PHC string format.
    password_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE events (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    -- `admin:<id>`, `machine:<id>` or `server`.
    actor TEXT NOT NULL,
    kind TEXT NOT NULL,
    machine_id INTEGER REFERENCES machines (id) ON DELETE SET NULL,
    -- JSON.
    detail TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX events_at ON events (at);

CREATE TABLE invites (
    id INTEGER PRIMARY KEY,
    -- BLAKE3 of the token.
    token_hash BLOB NOT NULL UNIQUE,
    -- Who the invitation is for, shown in the admin.
    user_name TEXT NOT NULL,
    person_role TEXT NOT NULL CHECK (person_role IN ('admin', 'editor', 'colorist')),
    machine_role TEXT NOT NULL CHECK (machine_role IN ('workstation', 'depot', 'render')),
    -- Admin key the invitation was made for (its fingerprint is in the link).
    admin_public_key BLOB NOT NULL,
    created_by INTEGER REFERENCES admins (id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at INTEGER,
    revoked_at INTEGER,
    machine_id INTEGER REFERENCES machines (id) ON DELETE SET NULL
);

CREATE TABLE machines (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    -- Ed25519 key, also the iroh identifier.
    public_key BLOB NOT NULL UNIQUE CHECK (length(public_key) = 32),
    os TEXT NOT NULL CHECK (os IN ('macos', 'windows', 'linux')),
    arch TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('workstation', 'depot', 'render')),
    user_name TEXT NOT NULL,
    person_role TEXT NOT NULL CHECK (person_role IN ('admin', 'editor', 'colorist')),
    enrolled_at INTEGER NOT NULL,
    last_seen_at INTEGER,
    agent_version TEXT NOT NULL,
    os_version TEXT,
    hostname TEXT,
    resolve_edition TEXT CHECK (resolve_edition IN ('studio', 'free')),
    resolve_version TEXT,
    -- Last inventory received, as JSON.
    last_report TEXT,
    revoked_at INTEGER
);

CREATE TABLE team (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    admin_public_key BLOB NOT NULL CHECK (length(admin_public_key) = 32),
    updated_at INTEGER NOT NULL
);
