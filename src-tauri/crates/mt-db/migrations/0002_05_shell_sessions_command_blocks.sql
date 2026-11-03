-- Fase 05: Shell Integration OSC 133 e Máquina de Estados das Abas.
-- Tabelas base de sessões e blocos de comando + índice FTS5 de conteúdo externo.

CREATE TABLE sessions (
    id                TEXT    PRIMARY KEY,
    tab_id            TEXT,
    pane_id           TEXT,
    project_id        TEXT,
    tab_session_id    TEXT,
    pty_id            TEXT    NOT NULL,
    shell_kind        TEXT    NOT NULL,
    shell_executable  TEXT    NOT NULL,
    initial_cwd       TEXT    NOT NULL,
    last_cwd          TEXT,
    integration_mode  TEXT    NOT NULL DEFAULT 'pending'
                      CHECK (integration_mode IN ('pending', 'osc133', 'fallback_proc', 'disabled')),
    started_at        INTEGER NOT NULL,
    ended_at          INTEGER,
    exit_code         INTEGER,
    exit_signal       TEXT,
    command_count     INTEGER NOT NULL DEFAULT 0,
    error_count       INTEGER NOT NULL DEFAULT 0,
    CHECK (ended_at IS NULL OR ended_at >= started_at)
) STRICT;

CREATE INDEX idx_sessions_tab_started
    ON sessions (tab_id, started_at DESC);

CREATE INDEX idx_sessions_pty
    ON sessions (pty_id);

CREATE INDEX idx_sessions_started_at
    ON sessions (started_at DESC);

CREATE TABLE command_blocks (
    rowid                    INTEGER PRIMARY KEY,
    id                       TEXT    NOT NULL UNIQUE,
    session_id               TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    tab_id                   TEXT,
    pane_id                  TEXT,
    pty_id                   TEXT    NOT NULL,
    command_seq              INTEGER NOT NULL,
    command_text             TEXT,
    command_text_quality     TEXT    NOT NULL DEFAULT 'unavailable'
                             CHECK (command_text_quality IN ('exact', 'argv_reconstructed', 'unavailable')),
    cwd                      TEXT,
    started_at               INTEGER NOT NULL,
    ended_at                 INTEGER,
    duration_ms              INTEGER,
    exit_code                INTEGER,
    signal                   TEXT,
    status                   TEXT    NOT NULL
                             CHECK (status IN (
                                 'running',
                                 'succeeded',
                                 'failed',
                                 'cancelled',
                                 'terminated_by_signal',
                                 'pty_exited',
                                 'interrupted_by_restart'
                             )),
    end_reason               TEXT
                             CHECK (end_reason IS NULL OR end_reason IN (
                                 'exit_zero',
                                 'exit_non_zero',
                                 'ctrl_c',
                                 'killed_by_signal',
                                 'implicit_next_prompt',
                                 'implicit_next_command',
                                 'pty_exited',
                                 'app_shutdown',
                                 'recovered_from_crash'
                             )),
    is_continuous            INTEGER NOT NULL DEFAULT 0 CHECK (is_continuous IN (0, 1)),
    source                   TEXT    NOT NULL CHECK (source IN ('osc133', 'osc633', 'proc_fallback')),
    confidence               TEXT    NOT NULL CHECK (confidence IN ('exact', 'degraded')),
    output_text              TEXT,
    output_kind              TEXT    NOT NULL DEFAULT 'none'
                             CHECK (output_kind IN ('none', 'full', 'head_tail_truncated', 'tail_only')),
    output_bytes             INTEGER NOT NULL DEFAULT 0,
    output_lines             INTEGER NOT NULL DEFAULT 0,
    error_excerpt            TEXT,
    inspectable              INTEGER NOT NULL DEFAULT 0 CHECK (inspectable IN (0, 1)),
    start_pty_seq            INTEGER NOT NULL,
    start_byte_offset        INTEGER NOT NULL,
    output_start_pty_seq     INTEGER,
    output_start_byte_offset INTEGER,
    end_pty_seq              INTEGER,
    end_byte_offset          INTEGER,
    UNIQUE (session_id, command_seq),
    CHECK (
        (status = 'running' AND ended_at IS NULL AND duration_ms IS NULL AND end_reason IS NULL)
        OR (status <> 'running' AND ended_at IS NOT NULL AND ended_at >= started_at AND duration_ms >= 0 AND end_reason IS NOT NULL)
    ),
    CHECK (status <> 'succeeded' OR exit_code = 0),
    CHECK (status <> 'failed' OR exit_code IS NULL OR exit_code <> 0)
) STRICT;

CREATE INDEX idx_command_blocks_session_seq
    ON command_blocks (session_id, command_seq DESC);

CREATE INDEX idx_command_blocks_session_started
    ON command_blocks (session_id, started_at DESC, id DESC);

CREATE INDEX idx_command_blocks_tab_started
    ON command_blocks (tab_id, started_at DESC, id DESC);

CREATE INDEX idx_command_blocks_started_at
    ON command_blocks (started_at DESC, id DESC);

CREATE INDEX idx_command_blocks_status_started
    ON command_blocks (status, started_at DESC);

CREATE VIRTUAL TABLE command_blocks_fts USING fts5(
    command_text,
    output_text,
    error_excerpt,
    cwd,
    content='command_blocks',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2 tokenchars ''_-./:@'''
);

CREATE TRIGGER command_blocks_fts_ai AFTER INSERT ON command_blocks BEGIN
    INSERT INTO command_blocks_fts(rowid, command_text, output_text, error_excerpt, cwd)
    VALUES (new.rowid, new.command_text, new.output_text, new.error_excerpt, new.cwd);
END;

CREATE TRIGGER command_blocks_fts_ad AFTER DELETE ON command_blocks BEGIN
    INSERT INTO command_blocks_fts(command_blocks_fts, rowid, command_text, output_text, error_excerpt, cwd)
    VALUES ('delete', old.rowid, old.command_text, old.output_text, old.error_excerpt, old.cwd);
END;

CREATE TRIGGER command_blocks_fts_au AFTER UPDATE ON command_blocks BEGIN
    INSERT INTO command_blocks_fts(command_blocks_fts, rowid, command_text, output_text, error_excerpt, cwd)
    VALUES ('delete', old.rowid, old.command_text, old.output_text, old.error_excerpt, old.cwd);
    INSERT INTO command_blocks_fts(rowid, command_text, output_text, error_excerpt, cwd)
    VALUES (new.rowid, new.command_text, new.output_text, new.error_excerpt, new.cwd);
END;
