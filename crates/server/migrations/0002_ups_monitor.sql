CREATE TABLE ups_monitor_sources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    address TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 3493 CHECK (port BETWEEN 1 AND 65535),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    last_discovery_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (address, port)
);

CREATE TABLE ups_monitor_devices (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL REFERENCES ups_monitor_sources(id) ON DELETE CASCADE,
    ups_name TEXT NOT NULL,
    description TEXT,
    online INTEGER NOT NULL DEFAULT 0 CHECK (online IN (0, 1)),
    last_seen_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (source_id, ups_name)
);

CREATE TABLE ups_monitor_snapshots (
    device_id TEXT PRIMARY KEY NOT NULL REFERENCES ups_monitor_devices(id) ON DELETE CASCADE,
    observed_at TEXT NOT NULL,
    status_flags_json TEXT NOT NULL DEFAULT '[]',
    raw_json TEXT NOT NULL DEFAULT '{}',
    charge_percent REAL,
    runtime_seconds INTEGER,
    runtime_capped INTEGER NOT NULL DEFAULT 0 CHECK (runtime_capped IN (0, 1)),
    load_percent REAL,
    input_voltage REAL,
    output_voltage REAL,
    battery_temperature REAL
);

CREATE TABLE ups_monitor_samples (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT NOT NULL REFERENCES ups_monitor_devices(id) ON DELETE CASCADE,
    bucket_at TEXT NOT NULL,
    observed_at TEXT NOT NULL,
    status_flags_json TEXT NOT NULL DEFAULT '[]',
    charge_percent REAL,
    runtime_seconds INTEGER,
    runtime_capped INTEGER NOT NULL DEFAULT 0 CHECK (runtime_capped IN (0, 1)),
    load_percent REAL,
    input_voltage REAL,
    output_voltage REAL,
    battery_temperature REAL,
    UNIQUE (device_id, bucket_at)
);

CREATE TABLE ups_monitor_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT NOT NULL REFERENCES ups_monitor_devices(id) ON DELETE CASCADE,
    occurred_at TEXT NOT NULL,
    kind TEXT NOT NULL,
    severity TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
    message TEXT NOT NULL,
    status_flags_json TEXT NOT NULL DEFAULT '[]'
);

CREATE INDEX idx_ups_monitor_devices_source ON ups_monitor_devices(source_id);
CREATE INDEX idx_ups_monitor_samples_device_time ON ups_monitor_samples(device_id, bucket_at DESC);
CREATE INDEX idx_ups_monitor_events_device_time ON ups_monitor_events(device_id, occurred_at DESC);
