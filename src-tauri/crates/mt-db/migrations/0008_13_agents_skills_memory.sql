-- Fase 13: agentes CLI, isolamento, telemetria, MaxSkills e memória contextual

-- Templates dedicados já semeados por projeto (idempotência de agent_templates_ensure, respeitando exclusões do usuário)
CREATE TABLE agent_template_seeds (
    project_id  TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    agent_id    TEXT    NOT NULL CHECK (agent_id IN ('claude-code', 'codex', 'agy', 'opencode')),
    template_id TEXT    NULL REFERENCES tab_templates(id) ON DELETE SET NULL,
    seeded_at   INTEGER NOT NULL,
    PRIMARY KEY (project_id, agent_id)
) STRICT;

-- Opções específicas de agente por template (environment_kind = 'agent')
CREATE TABLE agent_template_options (
    template_id       TEXT    PRIMARY KEY REFERENCES tab_templates(id) ON DELETE CASCADE,
    model             TEXT    NULL CHECK (model IS NULL OR length(model) BETWEEN 1 AND 128),
    extra_args_json   TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(extra_args_json) AND json_type(extra_args_json) = 'array'),
    isolation_default INTEGER NOT NULL DEFAULT 0 CHECK (isolation_default IN (0, 1)),
    location          TEXT    NOT NULL DEFAULT 'project' CHECK (location IN ('project', 'max_code')),
    updated_at        INTEGER NOT NULL
) STRICT;

-- Vínculo aba ↔ worktree (CA-12, CA-14, CA-15)
CREATE TABLE agent_tab_links (
    id             INTEGER PRIMARY KEY,
    tab_id         TEXT    NOT NULL,
    project_id     TEXT    NOT NULL,
    agent_id       TEXT    NOT NULL CHECK (agent_id IN ('claude-code', 'codex', 'agy', 'opencode')),
    worktree_path  TEXT    NULL,
    branch         TEXT    NULL,
    base_ref       TEXT    NULL,
    created_by_app INTEGER NOT NULL DEFAULT 0 CHECK (created_by_app IN (0, 1)),
    opened_at      INTEGER NOT NULL,
    tab_closed_at  INTEGER NULL,
    resolution     TEXT    NULL CHECK (resolution IN ('keep', 'remove', 'remove_and_delete_branch')),
    resolved_at    INTEGER NULL,
    CHECK ((resolution IS NULL) = (resolved_at IS NULL)),
    CHECK (resolution IS NULL OR tab_closed_at IS NOT NULL),
    CHECK (resolution <> 'remove_and_delete_branch' OR created_by_app = 1),
    CHECK (worktree_path IS NOT NULL OR (branch IS NULL AND created_by_app = 0))
) STRICT;
CREATE UNIQUE INDEX ux_agent_links_open_tab ON agent_tab_links(tab_id) WHERE tab_closed_at IS NULL;
CREATE INDEX ix_agent_links_pending ON agent_tab_links(project_id)
    WHERE tab_closed_at IS NOT NULL AND resolution IS NULL AND worktree_path IS NOT NULL;

-- Operações duráveis para reconciliar Git/SQLite/PTY após crash; não guardar tokens ou argv secretos
CREATE TABLE agent_launch_operations (
    launch_id     TEXT    PRIMARY KEY,
    project_id    TEXT    NOT NULL,
    tab_id        TEXT    NULL,
    agent_id      TEXT    NOT NULL CHECK (agent_id IN ('claude-code', 'codex', 'agy', 'opencode')),
    repo_path     TEXT    NOT NULL,
    worktree_path TEXT    NULL,
    branch        TEXT    NULL,
    base_oid      TEXT    NULL,
    stage         TEXT    NOT NULL CHECK (stage IN ('reserved', 'worktree_ready', 'opening', 'opened', 'recovery_required', 'resolved')),
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;
CREATE INDEX ix_agent_launch_pending ON agent_launch_operations(updated_at)
    WHERE stage IN ('reserved', 'worktree_ready', 'opening', 'recovery_required');

-- Último snapshot de telemetria por aba (restauração da barra após reinício; sem conteúdo de conversa)
CREATE TABLE agent_telemetry_snapshots (
    tab_id     TEXT    PRIMARY KEY REFERENCES tabs(id) ON DELETE CASCADE,
    agent_id   TEXT    NOT NULL,
    payload    TEXT    NOT NULL CHECK (json_valid(payload)),
    updated_at INTEGER NOT NULL
) STRICT;

-- Alvos de sincronização de MaxSkills
CREATE TABLE skill_sync_targets (
    id             INTEGER PRIMARY KEY,
    agent_id       TEXT    NOT NULL CHECK (agent_id IN ('claude-code', 'codex', 'agy', 'opencode')),
    scope          TEXT    NOT NULL CHECK (scope IN ('project', 'global')),
    project_id     TEXT    NULL REFERENCES projects(id) ON DELETE CASCADE,
    mode           TEXT    NOT NULL CHECK (mode IN ('symlink', 'copy')),
    auto_sync      INTEGER NOT NULL DEFAULT 0 CHECK (auto_sync IN (0, 1)),
    enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    selection_json TEXT    NOT NULL DEFAULT '{"kind":"recommended"}' CHECK (json_valid(selection_json)),
    dest_canonical TEXT    NOT NULL,
    project_root   TEXT    NULL,
    dest_override  TEXT    NULL,
    last_synced_at INTEGER NULL,
    created_at     INTEGER NOT NULL,
    CHECK ((scope = 'project') = (project_id IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX ux_skill_targets_dest ON skill_sync_targets(dest_canonical);

-- Entradas gerenciadas por alvo (fonte da verdade do que o app criou)
CREATE TABLE skill_sync_entries (
    target_id   INTEGER NOT NULL REFERENCES skill_sync_targets(id) ON DELETE CASCADE,
    skill_name  TEXT    NOT NULL,
    dest_path   TEXT    NOT NULL,
    mode        TEXT    NOT NULL CHECK (mode IN ('symlink', 'copy')),
    source_hash TEXT    NOT NULL CHECK (length(source_hash) = 64),
    source_path TEXT    NOT NULL,
    synced_hash TEXT    NOT NULL CHECK (length(synced_hash) = 64),
    synced_at   INTEGER NOT NULL,
    PRIMARY KEY (target_id, skill_name)
) STRICT;

-- Memória contextual de comandos recorrentes (texto já sanitizado — Fase 05 — e re-sanitizado no INSERT)
CREATE TABLE command_memory (
    project_id     TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    command_norm   TEXT    NOT NULL CHECK (length(command_norm) BETWEEN 1 AND 1024),
    command        TEXT    NOT NULL,
    use_count      INTEGER NOT NULL DEFAULT 0,
    success_count  INTEGER NOT NULL DEFAULT 0,
    failure_count  INTEGER NOT NULL DEFAULT 0,
    first_used_at  INTEGER NOT NULL,
    last_used_at   INTEGER NOT NULL,
    last_cwd       TEXT    NULL,
    last_exit_code INTEGER NULL,
    pinned         INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    hidden         INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1)),
    PRIMARY KEY (project_id, command_norm),
    CHECK (instr(command, '[REDACTED:') = 0)
) STRICT, WITHOUT ROWID;
CREATE INDEX ix_command_memory_recent ON command_memory(project_id, last_used_at DESC) WHERE hidden = 0;

-- Replay at-least-once: atualizar memória somente quando o bloco for inserido aqui na mesma transação
CREATE TABLE command_memory_processed (
    block_id     TEXT    PRIMARY KEY,
    project_id   TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    processed_at INTEGER NOT NULL
) STRICT;
CREATE INDEX ix_memory_processed_project ON command_memory_processed(project_id);

-- Preferências sobrevivem ao rebuild; tombstone impede que "Esquecer" ressuscite histórico antigo
CREATE TABLE command_memory_preferences (
    project_id   TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    command_hash TEXT    NOT NULL CHECK (length(command_hash) = 64),
    pinned       INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    hidden       INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1)),
    forgotten    INTEGER NOT NULL DEFAULT 0 CHECK (forgotten IN (0, 1)),
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (project_id, command_hash)
) STRICT, WITHOUT ROWID;
