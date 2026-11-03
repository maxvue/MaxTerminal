-- =============================================================================
-- Migration 0006 (Fase 10): Temas, Importador Tabby e Preferências de Aparência
-- =============================================================================

CREATE TABLE IF NOT EXISTS color_schemes (
    id              TEXT    PRIMARY KEY,
    name            TEXT    NOT NULL,
    origin          TEXT    NOT NULL CHECK (origin IN ('builtin', 'imported', 'custom')),
    polarity        TEXT    NOT NULL CHECK (polarity IN ('dark', 'light')),
    author          TEXT,
    source_format   TEXT,
    source_filename TEXT,
    sha256          TEXT    NOT NULL CHECK (length(sha256) = 64),
    payload_json    TEXT    NOT NULL CHECK (json_valid(payload_json)),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_color_schemes_name_nocase
    ON color_schemes (name COLLATE NOCASE);

CREATE INDEX IF NOT EXISTS idx_color_schemes_polarity_name
    ON color_schemes (polarity, name COLLATE NOCASE);

CREATE INDEX IF NOT EXISTS idx_color_schemes_sha256
    ON color_schemes (sha256);

CREATE TABLE IF NOT EXISTS appearance_settings (
    singleton_id             INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    active_theme_id          TEXT    NOT NULL DEFAULT 'max-navy-teal',
    follow_system            INTEGER NOT NULL DEFAULT 0 CHECK (follow_system IN (0, 1)),
    preferred_dark_theme_id  TEXT    NOT NULL DEFAULT 'max-navy-teal',
    preferred_light_theme_id TEXT    NOT NULL DEFAULT 'max-light',
    font_family              TEXT    NOT NULL DEFAULT 'JetBrains Mono, Fira Code, Cascadia Code, monospace',
    font_size                INTEGER NOT NULL DEFAULT 13 CHECK (font_size BETWEEN 8 AND 32),
    line_height              REAL    NOT NULL DEFAULT 1.2 CHECK (line_height BETWEEN 1.0 AND 2.0),
    font_ligatures           INTEGER NOT NULL DEFAULT 1 CHECK (font_ligatures IN (0, 1)),
    cursor_blink             INTEGER NOT NULL DEFAULT 1 CHECK (cursor_blink IN (0, 1)),
    cursor_style_override    TEXT             CHECK (cursor_style_override IN ('block', 'underline', 'bar') OR cursor_style_override IS NULL),
    minimum_contrast_ratio   REAL    NOT NULL DEFAULT 1.0 CHECK (minimum_contrast_ratio BETWEEN 1.0 AND 21.0),
    updated_at               INTEGER NOT NULL
) STRICT;

INSERT OR IGNORE INTO appearance_settings (singleton_id, updated_at)
VALUES (1, 0);

CREATE TABLE IF NOT EXISTS project_theme_overrides (
    project_id TEXT    PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    theme_id   TEXT    NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_project_theme_overrides_theme_id
    ON project_theme_overrides (theme_id);
