-- Migration 0012_18_remote_credentials_vault.sql (Fase 18)
-- Suporte a templates remotos por SSH, metadados do cofre de senhas e credenciais cifradas.

-- 1. Coluna de conexão em tab_templates (padrão local com retrocompatibilidade total)
ALTER TABLE tab_templates
    ADD COLUMN connection_json TEXT NOT NULL DEFAULT '{"kind":"local"}'
    CHECK (json_valid(connection_json) AND json_type(connection_json) = 'object');

-- 2. Metadados do cofre singleton de credenciais (Argon2id KDF + chave DEK encapsulada)
CREATE TABLE vault_metadata (
    id              INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    version         INTEGER NOT NULL,
    kdf_algorithm   TEXT    NOT NULL,
    kdf_salt        BLOB    NOT NULL,
    kdf_params_json TEXT    NOT NULL CHECK (json_valid(kdf_params_json) AND json_type(kdf_params_json) = 'object'),
    encrypted_dek   BLOB    NOT NULL,
    dek_nonce       BLOB    NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

-- 3. Repositório de credenciais cifradas (isoladas por projeto)
CREATE TABLE credentials (
    id         TEXT    PRIMARY KEY NOT NULL,
    project_id TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    nonce      BLOB    NOT NULL,
    ciphertext BLOB    NOT NULL,
    version    INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE INDEX ix_credentials_project
    ON credentials(project_id, id);
