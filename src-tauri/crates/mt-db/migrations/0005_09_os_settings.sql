-- Fase 09: configurações de integração com o SO (linha única, JSON versionado).
CREATE TABLE IF NOT EXISTS os_settings (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version INTEGER NOT NULL DEFAULT 1 CHECK (schema_version >= 1),
    value_json     TEXT    NOT NULL CHECK (json_valid(value_json) AND json_type(value_json) = 'object'),
    updated_at     INTEGER NOT NULL            -- epoch ms
) STRICT;

INSERT OR IGNORE INTO os_settings (id, schema_version, value_json, updated_at)
VALUES (1, 1, '{}', CAST(strftime('%s','now') AS INTEGER) * 1000);
