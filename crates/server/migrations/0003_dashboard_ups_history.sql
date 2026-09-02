CREATE TABLE dashboard_ups_samples (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id TEXT NOT NULL REFERENCES nut_servers(id) ON DELETE CASCADE,
    bucket_at TEXT NOT NULL,
    observed_at TEXT NOT NULL,
    load_percent REAL,
    runtime_seconds INTEGER,
    realpower_watts REAL,
    UNIQUE (server_id, bucket_at)
);

CREATE INDEX idx_dashboard_ups_samples_server_time
    ON dashboard_ups_samples(server_id, observed_at DESC);
