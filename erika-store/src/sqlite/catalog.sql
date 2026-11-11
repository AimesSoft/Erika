-- Canonical catalogue. Provider records do not define TMDB content identities.
CREATE TABLE media_node (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('tv','movie','season','episode')),
    parent_id TEXT REFERENCES media_node(id) DEFERRABLE INITIALLY DEFERRED,
    root_id TEXT NOT NULL REFERENCES media_node(id) DEFERRABLE INITIALLY DEFERRED,
    season_number INTEGER CHECK (season_number IS NULL OR season_number >= 0),
    episode_number INTEGER CHECK (episode_number IS NULL OR episode_number >= 0),
    tmdb_kind TEXT,
    tmdb_id TEXT,
    title TEXT NOT NULL,
    data TEXT NOT NULL CHECK (json_valid(data)),
    revision INTEGER NOT NULL DEFAULT 1,
    UNIQUE (tmdb_kind, tmdb_id),
    CHECK ((tmdb_kind IS NULL) = (tmdb_id IS NULL))
) STRICT;
CREATE UNIQUE INDEX media_season_position ON media_node(parent_id, season_number) WHERE kind = 'season' AND season_number IS NOT NULL;
CREATE UNIQUE INDEX media_episode_position ON media_node(parent_id, episode_number) WHERE kind = 'episode' AND episode_number IS NOT NULL;
CREATE INDEX media_root ON media_node(root_id);
CREATE TABLE provider_link (
    media_id TEXT NOT NULL REFERENCES media_node(id),
    provider TEXT NOT NULL,
    scope TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    relation TEXT NOT NULL,
    confirmed INTEGER NOT NULL CHECK(confirmed IN (0,1)),
    data TEXT NOT NULL CHECK(json_valid(data)),
    revision INTEGER NOT NULL,
    PRIMARY KEY(media_id, provider, scope, kind, external_id)
) STRICT;
CREATE UNIQUE INDEX tmdb_exact_identity ON provider_link(provider,scope,kind,external_id) WHERE provider = 'tmdb' AND relation = 'exact' AND confirmed = 1;
CREATE TABLE media_artwork (
    media_id TEXT NOT NULL REFERENCES media_node(id),
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    data TEXT NOT NULL CHECK(json_valid(data)),
    manual INTEGER NOT NULL DEFAULT 0 CHECK(manual IN (0,1)),
    PRIMARY KEY(media_id,kind,path)
) STRICT;
-- Revision survives an undo that restores an unbound file, so an old job
-- cannot mistake that file for the original revision zero.
CREATE TABLE file_revision (
    file_id INTEGER PRIMARY KEY REFERENCES file(id),
    revision INTEGER NOT NULL CHECK(revision > 0)
) STRICT;
CREATE TABLE file_binding (
    file_id INTEGER PRIMARY KEY REFERENCES file(id),
    decision TEXT NOT NULL CHECK(decision IN ('auto','manual','blocked')),
    evidence TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision > 0)
) STRICT;
CREATE TABLE file_binding_item (
    file_id INTEGER NOT NULL REFERENCES file_binding(file_id) ON DELETE CASCADE,
    media_id TEXT NOT NULL REFERENCES media_node(id),
    ordinal INTEGER NOT NULL,
    start_seconds REAL,
    end_seconds REAL,
    PRIMARY KEY(file_id,media_id),
    UNIQUE(file_id,ordinal),
    CHECK(start_seconds IS NULL OR start_seconds >= 0),
    CHECK(end_seconds IS NULL OR end_seconds > coalesce(start_seconds,0))
) STRICT;
CREATE INDEX file_media ON file_binding_item(media_id);
CREATE TABLE danmaku_revision (
    file_id INTEGER PRIMARY KEY REFERENCES file(id),
    revision INTEGER NOT NULL CHECK(revision > 0)
) STRICT;
CREATE TABLE danmaku_binding (
    file_id INTEGER PRIMARY KEY REFERENCES file(id),
    data TEXT NOT NULL CHECK(json_valid(data)),
    revision INTEGER NOT NULL
) STRICT;
CREATE TABLE media_watched (
    media_id TEXT PRIMARY KEY REFERENCES media_node(id),
    watched INTEGER NOT NULL CHECK(watched IN (0,1)),
    manual INTEGER NOT NULL CHECK(manual IN (0,1)),
    updated_at INTEGER NOT NULL,
    revision INTEGER NOT NULL
) STRICT;
CREATE TABLE media_mark (
    media_id TEXT NOT NULL REFERENCES media_node(id),
    reference TEXT NOT NULL,
    data TEXT NOT NULL CHECK(json_valid(data)),
    PRIMARY KEY(media_id,reference)
) STRICT;
CREATE TABLE catalog_job (
    id TEXT PRIMARY KEY,
    file_id INTEGER REFERENCES file(id),
    state TEXT NOT NULL,
    next_attempt INTEGER NOT NULL,
    lease_until INTEGER NOT NULL DEFAULT 0,
    data TEXT NOT NULL CHECK(json_valid(data))
) STRICT;
CREATE INDEX catalog_job_pending ON catalog_job(state,next_attempt);
CREATE TABLE catalog_change (
    id TEXT PRIMARY KEY,
    data TEXT NOT NULL CHECK(json_valid(data)),
    created_at INTEGER NOT NULL,
    undone INTEGER NOT NULL DEFAULT 0 CHECK(undone IN (0,1))
) STRICT;
CREATE TABLE catalog_request (
    id TEXT PRIMARY KEY,
    input TEXT NOT NULL CHECK(json_valid(input)),
    receipt TEXT NOT NULL CHECK(json_valid(receipt))
) STRICT;
CREATE TABLE account_watch_sync (
    operation TEXT PRIMARY KEY,
    media_id TEXT NOT NULL REFERENCES media_node(id),
    account TEXT NOT NULL,
    dedup TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL,
    data TEXT NOT NULL CHECK(json_valid(data))
) STRICT;
-- Local delivery state, deliberately outside catalogue exports and settings.
CREATE TABLE account_sync (
    account TEXT PRIMARY KEY,
    data TEXT NOT NULL CHECK(json_valid(data))
) STRICT;
CREATE TABLE source_scan (
    source_id INTEGER PRIMARY KEY REFERENCES source(id) ON DELETE CASCADE,
    owner TEXT NOT NULL,
    generation INTEGER NOT NULL,
    lease_until INTEGER NOT NULL,
    active INTEGER NOT NULL CHECK(active IN (0,1))
) STRICT;
CREATE TABLE source_scan_item (
    source_id INTEGER NOT NULL REFERENCES source_scan(source_id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY(source_id,path)
) STRICT;
