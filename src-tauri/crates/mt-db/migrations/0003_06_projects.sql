-- =============================================================================
-- Migration 0003 (Fase 06): Projetos, Pastas de Projetos e Auto-descoberta
-- =============================================================================

CREATE TABLE project_roots (
    id              TEXT     PRIMARY KEY,
    path            TEXT     NOT NULL UNIQUE,
    discovery_mode  TEXT     NOT NULL DEFAULT 'suggest'
                            CHECK (discovery_mode IN ('suggest', 'auto_import')),
    watch_enabled   INTEGER NOT NULL DEFAULT 1
                            CHECK (watch_enabled IN (0, 1)),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

CREATE TABLE projects (
    id              TEXT     PRIMARY KEY,
    title           TEXT     NOT NULL CHECK (length(title) BETWEEN 1 AND 120),
    icon            TEXT     NOT NULL,
    color           TEXT     NOT NULL
                            CHECK (color GLOB '#[0-9A-F][0-9A-F][0-9A-F][0-9A-F][0-9A-F][0-9A-F]'),
    path            TEXT     NOT NULL UNIQUE,
    root_id         TEXT    NULL REFERENCES project_roots(id) ON DELETE SET NULL,
    stack           TEXT     NOT NULL DEFAULT 'unknown'
                            CHECK (stack IN (
                                'tauri',
                                'vscode_extension',
                                'laravel',
                                'vue',
                                'rust',
                                'python',
                                'unknown'
                            )),
    identity_source TEXT     NOT NULL DEFAULT 'auto'
                            CHECK (identity_source IN ('auto', 'manual')),
    sort_order      INTEGER NOT NULL,
    missing         INTEGER NOT NULL DEFAULT 0 CHECK (missing IN (0, 1)),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX idx_projects_sort_order
    ON projects (sort_order, id);

CREATE INDEX idx_projects_root_id
    ON projects (root_id)
    WHERE root_id IS NOT NULL;

CREATE TABLE project_root_entries (
    root_id         TEXT     NOT NULL REFERENCES project_roots(id) ON DELETE CASCADE,
    entry_path      TEXT     NOT NULL,
    canonical_path  TEXT     NOT NULL,
    observed_at     INTEGER NOT NULL,
    PRIMARY KEY (root_id, entry_path)
) STRICT, WITHOUT ROWID;

CREATE INDEX idx_root_entries_canonical
    ON project_root_entries (canonical_path, root_id);

CREATE TABLE project_discovery_dismissed (
    root_id         TEXT     NOT NULL REFERENCES project_roots(id) ON DELETE CASCADE,
    path            TEXT     NOT NULL,
    dismissed_at    INTEGER NOT NULL,
    PRIMARY KEY (root_id, path)
) STRICT, WITHOUT ROWID;

CREATE INDEX idx_discovery_dismissed_path
    ON project_discovery_dismissed (path);
