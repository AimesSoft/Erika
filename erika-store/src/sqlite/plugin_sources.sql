-- Current-schema initialization only. No upgrades or conversion of old data.
CREATE TABLE plugin_source (
    source_id INTEGER PRIMARY KEY REFERENCES source(id) ON DELETE CASCADE,
    owner TEXT NOT NULL,
    external_id TEXT NOT NULL,
    server_id TEXT NOT NULL,
    generation TEXT NOT NULL DEFAULT '',
    UNIQUE(owner, external_id)
) STRICT;
CREATE TABLE plugin_source_scan (
    source_id INTEGER PRIMARY KEY REFERENCES plugin_source(source_id) ON DELETE CASCADE,
    id TEXT NOT NULL UNIQUE,
    data TEXT NOT NULL
) STRICT;
CREATE TABLE plugin_source_page (
    scan_id TEXT NOT NULL REFERENCES plugin_source_scan(id) ON DELETE CASCADE,
    page INTEGER NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY(scan_id, page)
) STRICT;
CREATE TABLE plugin_source_stage (
    scan_id TEXT NOT NULL REFERENCES plugin_source_scan(id) ON DELETE CASCADE,
    slot TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY(scan_id, slot)
) STRICT;
CREATE TABLE plugin_source_item (
    source_id INTEGER NOT NULL REFERENCES plugin_source(source_id) ON DELETE CASCADE,
    slot TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    file_id INTEGER NOT NULL REFERENCES file(id),
    generation TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY(source_id, slot)
) STRICT;
