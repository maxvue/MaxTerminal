-- Fase 07: Abas, Templates, Split Panes e Sessões
-- Migration exclusivamente aditiva sobre o schema das Fases 03, 05 e 06.

-- 1. Templates de Abas por Projeto
CREATE TABLE tab_templates (
    id               TEXT    PRIMARY KEY NOT NULL,
    project_id       TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name             TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 64),
    icon             TEXT    NOT NULL,
    cwd              TEXT,
    initial_command  TEXT    CHECK (initial_command IS NULL OR length(CAST(initial_command AS BLOB)) <= 4096),
    add_to_workspace INTEGER NOT NULL DEFAULT 0 CHECK (add_to_workspace IN (0, 1)),
    continuous       INTEGER NOT NULL DEFAULT 0 CHECK (continuous IN (0, 1)),
    environment_kind TEXT    NOT NULL CHECK (environment_kind IN ('shell', 'agent')),
    agent_id         TEXT,
    env_json         TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(env_json) AND json_type(env_json) = 'object'),
    is_default       INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    position         INTEGER NOT NULL DEFAULT 0,
    revision         INTEGER NOT NULL DEFAULT 1,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    CHECK ((environment_kind = 'agent') = (agent_id IS NOT NULL))
) STRICT;

CREATE UNIQUE INDEX ux_tab_templates_default
    ON tab_templates(project_id)
    WHERE is_default = 1;

CREATE INDEX ix_tab_templates_project
    ON tab_templates(project_id, position, id);

CREATE INDEX ix_tab_templates_workspace
    ON tab_templates(project_id, id)
    WHERE add_to_workspace = 1;

-- 2. Abas e Folhas (Painéis)
CREATE TABLE tabs (
    id               TEXT    PRIMARY KEY NOT NULL,
    project_id       TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    template_id      TEXT    REFERENCES tab_templates(id) ON DELETE SET NULL,
    origin           TEXT    NOT NULL CHECK (origin IN ('manual', 'workspace')),
    title            TEXT    NOT NULL,
    icon             TEXT    NOT NULL,
    position         INTEGER NOT NULL,
    continuous       INTEGER NOT NULL CHECK (continuous IN (0, 1)),
    launch_json      TEXT    NOT NULL CHECK (json_valid(launch_json)),
    layout_json      TEXT    NOT NULL CHECK (json_valid(layout_json)),
    active_pane_id   TEXT    NOT NULL,
    primary_pane_id  TEXT    NOT NULL,
    revision         INTEGER NOT NULL DEFAULT 1,
    layout_revision  INTEGER NOT NULL DEFAULT 1,
    last_session_seq INTEGER NOT NULL DEFAULT 0,
    opened_at        INTEGER NOT NULL,
    closed_at        INTEGER,
    last_focused_at  INTEGER
) STRICT;

CREATE INDEX ix_tabs_project_open
    ON tabs(project_id, position, id)
    WHERE closed_at IS NULL;

CREATE UNIQUE INDEX ux_tabs_workspace_template
    ON tabs(template_id)
    WHERE origin = 'workspace' AND template_id IS NOT NULL;

CREATE TABLE panes (
    id         TEXT    PRIMARY KEY NOT NULL,
    tab_id     TEXT    NOT NULL REFERENCES tabs(id) ON DELETE CASCADE,
    cwd        TEXT    NOT NULL,
    generation INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    closed_at  INTEGER
) STRICT;

CREATE INDEX ix_panes_tab
    ON panes(tab_id, id)
    WHERE closed_at IS NULL;

-- 3. Extensão aditiva da tabela sessions (Fase 05) e agrupamentos de sessão por aba
ALTER TABLE sessions ADD COLUMN status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'closed', 'archived'));
ALTER TABLE sessions ADD COLUMN next_seq INTEGER NOT NULL DEFAULT 1 CHECK (next_seq > 0);
ALTER TABLE sessions ADD COLUMN archived_at INTEGER;
ALTER TABLE sessions ADD COLUMN next_scrollback_seq INTEGER NOT NULL DEFAULT 1 CHECK (next_scrollback_seq > 0);
ALTER TABLE sessions ADD COLUMN scrollback_bytes INTEGER NOT NULL DEFAULT 0 CHECK (scrollback_bytes >= 0);
ALTER TABLE sessions ADD COLUMN scrollback_truncated INTEGER NOT NULL DEFAULT 0 CHECK (scrollback_truncated IN (0, 1));

CREATE INDEX ix_sessions_archived
    ON sessions(archived_at, id)
    WHERE archived_at IS NOT NULL;

CREATE TRIGGER IF NOT EXISTS command_blocks_sync_session_seq_ai AFTER INSERT ON command_blocks BEGIN
    UPDATE sessions
    SET next_seq = MAX(next_seq, new.command_seq + 1)
    WHERE id = new.session_id;
END;

CREATE TABLE tab_sessions (
    id          TEXT    PRIMARY KEY NOT NULL,
    tab_id      TEXT    NOT NULL REFERENCES tabs(id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL CHECK (seq > 0),
    started_at  INTEGER NOT NULL,
    archived_at INTEGER,
    end_reason  TEXT    CHECK (end_reason IN (
        'new_session',
        'tab_closed',
        'app_exit',
        'crash_recovered',
        'project_removed',
        'spawn_failed'
    )),
    CHECK ((archived_at IS NULL) = (end_reason IS NULL)),
    UNIQUE (tab_id, seq)
) STRICT;

CREATE UNIQUE INDEX ux_tab_sessions_active
    ON tab_sessions(tab_id)
    WHERE archived_at IS NULL;

CREATE INDEX ix_tab_sessions_history
    ON tab_sessions(tab_id, seq DESC);

CREATE TABLE tab_session_panes (
    session_id       TEXT    PRIMARY KEY NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    tab_session_id   TEXT    NOT NULL REFERENCES tab_sessions(id) ON DELETE CASCADE,
    pane_id          TEXT    NOT NULL REFERENCES panes(id) ON DELETE RESTRICT,
    generation       INTEGER NOT NULL,
    joined_at        INTEGER NOT NULL,
    closed_at        INTEGER,
    initial_dispatch TEXT    NOT NULL DEFAULT 'none'
                     CHECK (initial_dispatch IN ('none', 'pending', 'waiting', 'claimed', 'sent')),
    UNIQUE (tab_session_id, pane_id, generation)
) STRICT;

CREATE UNIQUE INDEX ux_tab_session_pane_active
    ON tab_session_panes(tab_session_id, pane_id)
    WHERE closed_at IS NULL;

CREATE INDEX ix_tab_session_panes_group
    ON tab_session_panes(tab_session_id, pane_id, generation);

-- 4. Scrollback sanitizado, índice FTS5 e Ledger de Operações do Workspace
CREATE TABLE session_scrollback (
    rowid      INTEGER PRIMARY KEY,
    session_id TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL CHECK (seq > 0),
    ts         INTEGER NOT NULL,
    text       TEXT    NOT NULL,
    kind       TEXT    NOT NULL CHECK (kind IN ('output', 'gap')),
    byte_len   INTEGER NOT NULL CHECK (byte_len >= 0 AND byte_len <= 65536),
    UNIQUE (session_id, seq)
) STRICT;

CREATE VIRTUAL TABLE IF NOT EXISTS session_scrollback_fts USING fts5(
    text,
    content='session_scrollback',
    content_rowid='rowid',
    tokenize="unicode61 remove_diacritics 2",
    prefix='2 3'
);

CREATE TRIGGER IF NOT EXISTS session_scrollback_fts_ai AFTER INSERT ON session_scrollback BEGIN
    INSERT INTO session_scrollback_fts(rowid, text) VALUES (new.rowid, new.text);
END;

CREATE TRIGGER IF NOT EXISTS session_scrollback_fts_ad AFTER DELETE ON session_scrollback BEGIN
    INSERT INTO session_scrollback_fts(session_scrollback_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
END;

CREATE TRIGGER IF NOT EXISTS session_scrollback_fts_au AFTER UPDATE ON session_scrollback BEGIN
    INSERT INTO session_scrollback_fts(session_scrollback_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
    INSERT INTO session_scrollback_fts(rowid, text) VALUES (new.rowid, new.text);
END;

CREATE TRIGGER IF NOT EXISTS session_scrollback_sync_session_ai AFTER INSERT ON session_scrollback BEGIN
    UPDATE sessions
    SET next_scrollback_seq = MAX(next_scrollback_seq, new.seq + 1),
        scrollback_bytes = scrollback_bytes + new.byte_len,
        scrollback_truncated = CASE WHEN new.kind = 'gap' THEN 1 ELSE scrollback_truncated END
    WHERE id = new.session_id;
END;

CREATE TABLE workspace_operations (
    request_id  TEXT    PRIMARY KEY NOT NULL,
    kind        TEXT    NOT NULL,
    args_hash   TEXT    NOT NULL,
    tab_id      TEXT    REFERENCES tabs(id) ON DELETE CASCADE,
    boot_id     TEXT    NOT NULL,
    stage       TEXT    NOT NULL CHECK (stage IN ('prepared', 'stopping', 'committed', 'completed', 'blocked', 'interrupted')),
    result_json TEXT    CHECK (result_json IS NULL OR json_valid(result_json)),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX ix_workspace_operations_pending
    ON workspace_operations(stage, updated_at);
