-- Phase 2: the package store and the signed manifests.

-- Files uploaded by the admins. The content is in `data_dir/packages/<blake3>`.
CREATE TABLE package_files (
    blake3 TEXT PRIMARY KEY,
    -- Name of the file when it was uploaded.
    name TEXT NOT NULL,
    size INTEGER NOT NULL,
    uploaded_by INTEGER REFERENCES admins (id) ON DELETE SET NULL,
    uploaded_at INTEGER NOT NULL
) WITHOUT ROWID;

-- Published manifests, as signed by the admin's application. The latest
-- version applies. The server relays them: workstations check the signature.
CREATE TABLE manifests (
    version INTEGER PRIMARY KEY,
    -- The exact bytes that were signed.
    payload TEXT NOT NULL,
    -- JSON.
    signature TEXT NOT NULL,
    published_by INTEGER REFERENCES admins (id) ON DELETE SET NULL,
    published_at INTEGER NOT NULL
);

-- The manifest being edited in the web admin, not signed. A single row.
CREATE TABLE manifest_draft (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    -- JSON.
    content TEXT NOT NULL,
    updated_by INTEGER REFERENCES admins (id) ON DELETE SET NULL,
    updated_at INTEGER NOT NULL
);

-- Where each workstation stands against the manifest, as it last reported.
CREATE TABLE machine_packages (
    machine_id INTEGER PRIMARY KEY REFERENCES machines (id) ON DELETE CASCADE,
    -- JSON.
    report TEXT NOT NULL,
    reported_at INTEGER NOT NULL
);
