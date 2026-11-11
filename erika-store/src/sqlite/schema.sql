    CREATE TABLE source (
        id INTEGER PRIMARY KEY,
        kind TEXT NOT NULL,
        name TEXT NOT NULL,
        location TEXT NOT NULL,
        added_at INTEGER NOT NULL,
        content TEXT NOT NULL DEFAULT 'anime'
    ) STRICT;
    INSERT INTO source (id, kind, name, location, added_at) VALUES (1, 'opened', '', '', 0);

    CREATE TABLE anime (
        id INTEGER PRIMARY KEY,
        dandanplay_id INTEGER UNIQUE,
        bangumi_id INTEGER UNIQUE,
        title TEXT NOT NULL,
        image TEXT NOT NULL DEFAULT '',
        updated_at INTEGER NOT NULL DEFAULT 0
    ) STRICT;

    CREATE TABLE episode (
        id INTEGER PRIMARY KEY,
        anime_id INTEGER NOT NULL REFERENCES anime (id) ON DELETE CASCADE,
        number INTEGER,
        dandanplay_id INTEGER UNIQUE,
        bangumi_id INTEGER UNIQUE,
        title TEXT NOT NULL DEFAULT ''
    ) STRICT;

    CREATE TABLE file (
        id INTEGER PRIMARY KEY,
        md5 TEXT,
        size INTEGER,
        server_item TEXT UNIQUE,
        episode_id INTEGER REFERENCES episode (id) ON DELETE SET NULL,
        matched_by TEXT,
        UNIQUE (md5, size)
    ) STRICT;

    CREATE TABLE address (
        source_id INTEGER NOT NULL REFERENCES source (id) ON DELETE CASCADE,
        path TEXT NOT NULL,
        file_id INTEGER REFERENCES file (id) ON DELETE SET NULL,
        size INTEGER,
        modified INTEGER,
        seen_at INTEGER NOT NULL,
        PRIMARY KEY (source_id, path)
    ) STRICT;

    CREATE TABLE progress (
        file_id INTEGER PRIMARY KEY REFERENCES file (id) ON DELETE CASCADE,
        position REAL NOT NULL,
        duration REAL NOT NULL,
        danmaku_offset REAL NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL
    ) STRICT;

    CREATE TABLE watched (
        episode_id INTEGER PRIMARY KEY REFERENCES episode (id) ON DELETE CASCADE,
        at INTEGER NOT NULL,
        manual INTEGER NOT NULL
    ) STRICT;

    CREATE TABLE recent (
        source_id INTEGER NOT NULL,
        path TEXT NOT NULL,
        title TEXT NOT NULL,
        still TEXT,
        position REAL NOT NULL,
        duration REAL NOT NULL,
        played_at INTEGER NOT NULL,
        PRIMARY KEY (source_id, path),
        FOREIGN KEY (source_id, path) REFERENCES address (source_id, path) ON DELETE CASCADE
    ) STRICT;
    CREATE INDEX recent_by_time ON recent (played_at DESC);

    CREATE TABLE setting (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) STRICT;
    -- Where the user is with an anime (the anime module's word for it)
    -- and the score they gave it, 1 to 10 or 0.
    CREATE TABLE anime_mark (
        anime_id INTEGER PRIMARY KEY REFERENCES anime (id) ON DELETE CASCADE,
        status TEXT,
        rating INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL
    ) STRICT;
    CREATE TABLE extension (
        id TEXT PRIMARY KEY,
        value TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    ) STRICT;
CREATE TABLE media_playback (
    file_id INTEGER PRIMARY KEY REFERENCES file (id) ON DELETE CASCADE,
    binding_revision INTEGER NOT NULL,
    intervals TEXT NOT NULL
) STRICT;
