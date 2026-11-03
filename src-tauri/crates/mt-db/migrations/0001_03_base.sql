-- Fase 03: Persistência SQLite e Sanitização de Segredos (schema base).
-- Este arquivo é imutável após merge; novas fases adicionam 0002_*, 0003_*, etc.

CREATE TABLE IF NOT EXISTS _schema_migrations (
    version      INTEGER PRIMARY KEY,
    name         TEXT    NOT NULL,
    checksum     TEXT    NOT NULL,
    applied_at   INTEGER NOT NULL,
    duration_ms  INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS settings (
    key         TEXT    PRIMARY KEY,
    value_json  TEXT    NOT NULL CHECK (json_valid(value_json)),
    updated_at  INTEGER NOT NULL,
    revision    INTEGER NOT NULL DEFAULT 1
) STRICT;

CREATE INDEX IF NOT EXISTS idx_settings_updated_at
    ON settings (updated_at DESC);

CREATE TABLE IF NOT EXISTS _db_meta (
    key         TEXT    PRIMARY KEY,
    value_text  TEXT    NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;
