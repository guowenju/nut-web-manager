PRAGMA foreign_keys = ON;

CREATE TABLE hosts (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    address TEXT NOT NULL,
    ssh_port INTEGER NOT NULL CHECK (ssh_port BETWEEN 1 AND 65535),
    username TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('server', 'client')),
    platform_kind TEXT CHECK (
        platform_kind IS NULL OR platform_kind IN (
            'debian', 'proxmox_ve', 'proxmox_backup_server', 'unsupported'
        )
    ),
    os_version TEXT,
    product_version TEXT,
    hostname TEXT,
    nut_version TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (address, ssh_port)
);

CREATE TABLE config_revisions (
    id TEXT PRIMARY KEY NOT NULL,
    host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    revision_number INTEGER NOT NULL CHECK (revision_number > 0),
    manifest_json TEXT NOT NULL,
    backup_path TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (host_id, revision_number)
);

CREATE TABLE nut_servers (
    id TEXT PRIMARY KEY NOT NULL,
    host_id TEXT NOT NULL UNIQUE REFERENCES hosts(id) ON DELETE CASCADE,
    ups_name TEXT NOT NULL,
    listen_address TEXT NOT NULL DEFAULT '0.0.0.0',
    listen_port INTEGER NOT NULL DEFAULT 3493 CHECK (listen_port BETWEEN 1 AND 65535),
    shutdown_host_sync_seconds INTEGER NOT NULL DEFAULT 15 CHECK (shutdown_host_sync_seconds BETWEEN 5 AND 300),
    shutdown_final_delay_seconds INTEGER NOT NULL DEFAULT 5 CHECK (shutdown_final_delay_seconds BETWEEN 0 AND 120),
    shutdown_powerdown_enabled INTEGER NOT NULL DEFAULT 0 CHECK (shutdown_powerdown_enabled IN (0, 1)),
    shutdown_trigger_mode TEXT NOT NULL DEFAULT 'battery_level' CHECK (
        shutdown_trigger_mode IN ('battery_level', 'on_battery_timer')
    ),
    shutdown_battery_level_percent INTEGER NOT NULL DEFAULT 20 CHECK (shutdown_battery_level_percent BETWEEN 5 AND 50),
    shutdown_on_battery_seconds INTEGER NOT NULL DEFAULT 300 CHECK (shutdown_on_battery_seconds BETWEEN 60 AND 7200),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    apply_state TEXT NOT NULL CHECK (
        apply_state IN ('unconfigured', 'pending', 'applying', 'applied', 'removing', 'failed')
    ),
    applied_revision_id TEXT REFERENCES config_revisions(id) ON DELETE SET NULL
);

CREATE TABLE ups_devices (
    id TEXT PRIMARY KEY NOT NULL,
    server_id TEXT NOT NULL UNIQUE REFERENCES nut_servers(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    driver TEXT NOT NULL,
    port TEXT NOT NULL,
    selectors_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE nut_credentials (
    id TEXT PRIMARY KEY NOT NULL,
    server_id TEXT NOT NULL REFERENCES nut_servers(id) ON DELETE CASCADE,
    username TEXT NOT NULL,
    secret_ciphertext BLOB NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (server_id, username)
);

CREATE TABLE nut_client_bindings (
    id TEXT PRIMARY KEY NOT NULL,
    server_id TEXT NOT NULL REFERENCES nut_servers(id) ON DELETE CASCADE,
    client_host_id TEXT NOT NULL UNIQUE REFERENCES hosts(id) ON DELETE CASCADE,
    apply_state TEXT NOT NULL CHECK (
        apply_state IN ('unconfigured', 'pending', 'applying', 'applied', 'removing', 'failed')
    ),
    applied_revision_id TEXT REFERENCES config_revisions(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE operations (
    id TEXT PRIMARY KEY NOT NULL,
    host_id TEXT REFERENCES hosts(id) ON DELETE SET NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'running', 'succeeded', 'failed')),
    progress INTEGER NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 100),
    error_code TEXT,
    error_detail TEXT,
    result_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at TEXT NOT NULL,
    severity TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
    code TEXT NOT NULL,
    entity_type TEXT,
    entity_id TEXT,
    operation_id TEXT REFERENCES operations(id) ON DELETE SET NULL,
    message TEXT NOT NULL,
    details_json TEXT
);

CREATE INDEX idx_bindings_server_id ON nut_client_bindings(server_id);
CREATE INDEX idx_config_revisions_host_id ON config_revisions(host_id, revision_number DESC);
CREATE INDEX idx_events_occurred_at ON events(occurred_at DESC);
CREATE INDEX idx_operations_host_state ON operations(host_id, state);
CREATE UNIQUE INDEX idx_single_enabled_server ON nut_servers(enabled) WHERE enabled = 1;
