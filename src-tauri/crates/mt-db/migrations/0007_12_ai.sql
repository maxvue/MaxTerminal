-- =============================================================================
-- Migration 0007 (Fase 12): Configurações, Caches e Histórico de IA (MaxAiManager)
-- =============================================================================

CREATE TABLE IF NOT EXISTS ai_settings (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    payload     TEXT    NOT NULL CHECK (json_valid(payload)),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS ai_catalog_cache (
    gateway_key TEXT    PRIMARY KEY,
    payload     TEXT    NOT NULL CHECK (json_valid(payload)),
    etag        TEXT,
    fetched_at  TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_diagnosis_cache (
    error_hash        TEXT    PRIMARY KEY CHECK (length(error_hash) = 64),
    gateway_key       TEXT    NOT NULL,
    project_id        TEXT    REFERENCES projects (id) ON DELETE CASCADE,
    summary           TEXT    NOT NULL,
    fix_command       TEXT,
    fix_risk          TEXT    CHECK (fix_risk IN ('safe', 'caution', 'destructive')),
    harness_id        TEXT    NOT NULL,
    model_id          TEXT    NOT NULL,
    reasoning_id      TEXT,
    prompt_version    INTEGER NOT NULL,
    sanitizer_version INTEGER NOT NULL,
    hits              INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_hit_at       TEXT,
    expires_at        TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ai_diagnosis_cache_last_hit ON ai_diagnosis_cache (last_hit_at);
CREATE INDEX IF NOT EXISTS idx_ai_diagnosis_cache_expiry   ON ai_diagnosis_cache (expires_at);
CREATE INDEX IF NOT EXISTS idx_ai_diagnosis_cache_project  ON ai_diagnosis_cache (project_id, gateway_key);

CREATE TABLE IF NOT EXISTS ai_prompt_history (
    id                 TEXT    PRIMARY KEY,
    kind               TEXT    NOT NULL CHECK (kind IN ('suggest', 'diagnosis', 'rag', 'structured')),
    trigger            TEXT    CHECK (trigger IN ('at', 'question')),
    project_id         TEXT    REFERENCES projects (id) ON DELETE SET NULL,
    tab_id             TEXT,
    pane_id            TEXT,
    session_id         TEXT,
    block_id           TEXT,
    harness_id         TEXT,
    model_id           TEXT,
    reasoning_id       TEXT,
    rag_slug           TEXT,
    schema_id          TEXT,
    prompt_sanitized   TEXT    NOT NULL,
    response_sanitized TEXT,
    risk_level         TEXT    CHECK (risk_level IN ('safe', 'caution', 'destructive')),
    status             TEXT    NOT NULL CHECK (status IN ('completed', 'failed', 'cancelled')),
    error_code         TEXT,
    latency_ms         INTEGER,
    created_at         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_ai_history_project      ON ai_prompt_history (project_id, id DESC);
CREATE INDEX IF NOT EXISTS idx_ai_history_kind         ON ai_prompt_history (kind, id DESC);
CREATE INDEX IF NOT EXISTS idx_ai_history_project_kind ON ai_prompt_history (project_id, kind, id DESC);
