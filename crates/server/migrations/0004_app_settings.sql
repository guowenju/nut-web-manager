CREATE TABLE app_settings (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    default_page TEXT NOT NULL DEFAULT 'overview' CHECK (default_page IN ('overview', 'ups_monitor'))
);

INSERT INTO app_settings (id, default_page) VALUES (1, 'overview');
