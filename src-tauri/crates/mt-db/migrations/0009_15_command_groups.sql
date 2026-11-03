-- Fase 15: Grupos de Comandos (Local e Global), Itens, Labels e Histórico de Execuções

CREATE TABLE command_groups (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL CHECK(scope IN ('local','global')),
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 80),
    name_key TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    mode TEXT NOT NULL CHECK(mode IN ('sequential','parallel','multi_tabs')),
    separator TEXT NOT NULL CHECK(separator IN ('and','semicolon')),
    cwd TEXT,
    env_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(env_json)),
    sort_order INTEGER NOT NULL CHECK(sort_order >= 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK((scope = 'local' AND project_id IS NOT NULL) OR (scope = 'global' AND project_id IS NULL))
);

CREATE UNIQUE INDEX ux_groups_local_name ON command_groups(project_id, name_key) WHERE scope = 'local';
CREATE UNIQUE INDEX ux_groups_global_name ON command_groups(name_key) WHERE scope = 'global';
CREATE INDEX ix_groups_order ON command_groups(scope, project_id, sort_order, id);

CREATE TABLE command_group_labels (
    group_id TEXT NOT NULL REFERENCES command_groups(id) ON DELETE CASCADE,
    label_key TEXT NOT NULL,
    label TEXT NOT NULL,
    PRIMARY KEY(group_id, label_key)
);

CREATE INDEX ix_group_labels_lookup ON command_group_labels(label_key, group_id);

CREATE TABLE command_group_items (
    id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL REFERENCES command_groups(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    title TEXT NOT NULL,
    command TEXT NOT NULL CHECK(length(command) > 0),
    cwd TEXT,
    env_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(env_json)),
    continuous INTEGER NOT NULL DEFAULT 0 CHECK(continuous IN (0, 1)),
    UNIQUE(group_id, position)
);

CREATE TABLE command_group_runs (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    request_hash TEXT NOT NULL,
    group_id TEXT REFERENCES command_groups(id) ON DELETE SET NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    origin_session_id TEXT,
    snapshot_json TEXT NOT NULL CHECK(json_valid(snapshot_json)),
    mode TEXT NOT NULL CHECK(mode IN ('sequential','parallel','multi_tabs')),
    status TEXT NOT NULL CHECK(status IN ('queued','running','cancelling','succeeded','failed','partial','cancelled','interrupted')),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
    last_exit_code INTEGER,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    cancel_requested_at INTEGER,
    finished_at INTEGER
);

CREATE INDEX ix_group_runs_project_time ON command_group_runs(project_id, created_at DESC, id);
CREATE INDEX ix_group_runs_group_time ON command_group_runs(group_id, created_at DESC, id);
CREATE INDEX ix_group_runs_origin ON command_group_runs(origin_session_id);
CREATE INDEX ix_group_runs_active ON command_group_runs(status) WHERE finished_at IS NULL;

CREATE TABLE command_group_run_items (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES command_group_runs(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    snapshot_json TEXT NOT NULL CHECK(json_valid(snapshot_json)),
    status TEXT NOT NULL CHECK(status IN ('pending','starting','running','cancelling','succeeded','failed','skipped','cancelled','interrupted')),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
    pty_id TEXT,
    tab_id TEXT,
    pane_id TEXT,
    session_id TEXT,
    command_block_id TEXT,
    exit_code INTEGER,
    signal INTEGER,
    error_code TEXT,
    error_message TEXT,
    started_at INTEGER,
    finished_at INTEGER,
    output_tail TEXT NOT NULL DEFAULT '',
    output_truncated INTEGER NOT NULL DEFAULT 0 CHECK(output_truncated IN (0, 1)),
    UNIQUE(run_id, position)
);

CREATE INDEX ix_group_run_items_pty ON command_group_run_items(pty_id);
CREATE INDEX ix_group_run_items_session ON command_group_run_items(session_id);
CREATE INDEX ix_group_run_items_tab ON command_group_run_items(tab_id);
CREATE INDEX ix_group_run_items_pane ON command_group_run_items(pane_id);
CREATE INDEX ix_group_run_items_block ON command_group_run_items(command_block_id);
