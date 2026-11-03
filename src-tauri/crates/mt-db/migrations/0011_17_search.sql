-- Migration 0011_17_search.sql (Fase 17)
-- Projeção auxiliar sanitizada e reconstruível para fontes de metadados sem FTS dedicado.

CREATE TABLE IF NOT EXISTS search_metadata (
    rowid INTEGER PRIMARY KEY,
    source TEXT NOT NULL CHECK (source IN (
        'sessions',
        'projects',
        'templates',
        'groups',
        'group_runs',
        'issues',
        'comments'
    )),
    entity_key TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    tab_id TEXT,
    session_id TEXT,
    pane_id TEXT,
    exit_code INTEGER,
    source_truncated INTEGER NOT NULL DEFAULT 0 CHECK (source_truncated IN (0, 1)),
    fetched_at INTEGER,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    target_json TEXT NOT NULL,
    occurred_at INTEGER,
    source_revision TEXT NOT NULL,
    UNIQUE (source, entity_key)
) STRICT;

CREATE INDEX IF NOT EXISTS ix_search_metadata_project_source
    ON search_metadata(project_id, source, occurred_at DESC, entity_key ASC);

CREATE INDEX IF NOT EXISTS ix_search_metadata_source_time
    ON search_metadata(source, occurred_at DESC, entity_key ASC);

CREATE VIRTUAL TABLE IF NOT EXISTS search_metadata_fts USING fts5(
    title,
    body,
    content='search_metadata',
    content_rowid='rowid',
    tokenize="unicode61 remove_diacritics 2",
    prefix='2 3'
);

CREATE TRIGGER IF NOT EXISTS search_metadata_fts_ai AFTER INSERT ON search_metadata BEGIN
    INSERT INTO search_metadata_fts(rowid, title, body) VALUES (new.rowid, new.title, new.body);
END;

CREATE TRIGGER IF NOT EXISTS search_metadata_fts_ad AFTER DELETE ON search_metadata BEGIN
    INSERT INTO search_metadata_fts(search_metadata_fts, rowid, title, body)
    VALUES ('delete', old.rowid, old.title, old.body);
END;

CREATE TRIGGER IF NOT EXISTS search_metadata_fts_au AFTER UPDATE ON search_metadata BEGIN
    INSERT INTO search_metadata_fts(search_metadata_fts, rowid, title, body)
    VALUES ('delete', old.rowid, old.title, old.body);
    INSERT INTO search_metadata_fts(rowid, title, body)
    VALUES (new.rowid, new.title, new.body);
END;

-- Índices B-tree auxiliares nas tabelas das Fases 05/07 (criados com IF NOT EXISTS caso ausentes)
CREATE INDEX IF NOT EXISTS ix_sessions_project_started_id
    ON sessions(project_id, started_at DESC, id ASC);

CREATE INDEX IF NOT EXISTS ix_command_blocks_session_started_id
    ON command_blocks(session_id, started_at DESC, id ASC);

CREATE INDEX IF NOT EXISTS ix_command_blocks_exit_started_id
    ON command_blocks(exit_code, started_at DESC, id ASC);
