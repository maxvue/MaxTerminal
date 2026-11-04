//! Repositório transacional de metadados do cofre (`vault_metadata`) e credenciais cifradas (`credentials`).
//!
//! Garante armazenamento seguro e isolado por projeto de credenciais SSH cifradas.
//! NUNCA armazena senhas em texto puro ou chaves mestras descriptografadas.

use std::str::FromStr;

use mt_core::ids::ProjectId;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::Db;
use crate::error::{DbError, DbResult};

/// Linha persistida da tabela `credentials`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCredentialRow {
    /// Identificador único da credencial (UUID v7 textual).
    pub id: String,
    /// Projeto proprietário da credencial.
    pub project_id: ProjectId,
    /// Nonce aleatório de 24 bytes (XChaCha20-Poly1305).
    pub nonce: Vec<u8>,
    /// Texto cifrado incluindo a tag de autenticação Poly1305.
    pub ciphertext: Vec<u8>,
    /// Versão do formato criptográfico da credencial (`>= 1`).
    pub version: i64,
    /// Timestamp UNIX em ms de criação.
    pub created_at: i64,
    /// Timestamp UNIX em ms da última atualização.
    pub updated_at: i64,
}

/// Metadados persistidos do cofre singleton (`vault_metadata`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredVaultMetadata {
    /// Identificador fixo singleton (`1`).
    pub id: i64,
    /// Versão do cofre (`>= 1`).
    pub version: i64,
    /// Identificador do algoritmo KDF (ex.: `"argon2id"`).
    pub kdf_algorithm: String,
    /// Salt criptográfico aleatório utilizado no KDF (16 bytes).
    pub kdf_salt: Vec<u8>,
    /// Parâmetros do KDF em JSON (ex.: `{"m_cost":65536,"t_cost":3,"p_cost":1}`).
    pub kdf_params_json: String,
    /// Chave DEK encapsulada/cifrada com a KEK derivada da senha mestra.
    pub encrypted_dek: Vec<u8>,
    /// Nonce aleatório de 24 bytes para a DEK cifrada.
    pub dek_nonce: Vec<u8>,
    /// Timestamp UNIX em ms de criação do cofre.
    pub created_at: i64,
    /// Timestamp UNIX em ms da última atualização de senha mestra.
    pub updated_at: i64,
}

/// Repositório para operações sobre as tabelas `vault_metadata` e `credentials`.
#[derive(Clone, Debug)]
pub struct CredentialsRepo {
    db: Db,
}

impl CredentialsRepo {
    /// Cria uma nova instância de [`CredentialsRepo`] acoplada à fachada [`Db`].
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    /// Busca os metadados do cofre singleton (se configurado).
    pub async fn get_vault_metadata(&self) -> DbResult<Option<StoredVaultMetadata>> {
        self.db.read(get_vault_metadata_conn).await
    }

    /// Salva os metadados iniciais do cofre singleton (configuração da senha mestra).
    pub async fn save_vault_metadata(&self, meta: StoredVaultMetadata) -> DbResult<()> {
        self.db
            .write_sized("vault_metadata_save", 1024, move |tx| {
                save_vault_metadata_tx(tx, &meta)
            })
            .await
    }

    /// Atualiza a senha mestra do cofre reencapsulando a DEK com nova KEK/salt/nonce.
    pub async fn update_vault_metadata(&self, meta: StoredVaultMetadata) -> DbResult<()> {
        self.db
            .write_sized("vault_metadata_update", 1024, move |tx| {
                update_vault_metadata_tx(tx, &meta)
            })
            .await
    }

    /// Busca uma credencial pelo seu ID.
    pub async fn get_credential(&self, id: String) -> DbResult<Option<StoredCredentialRow>> {
        self.db
            .read(move |conn| get_credential_conn(conn, &id))
            .await
    }

    /// Lista todas as credenciais pertencentes a um projeto.
    pub async fn list_by_project(
        &self,
        project_id: ProjectId,
    ) -> DbResult<Vec<StoredCredentialRow>> {
        let proj_str = project_id.to_string();
        self.db
            .read(move |conn| list_credentials_by_project_conn(conn, &proj_str))
            .await
    }

    /// Insere uma nova credencial cifrada.
    pub async fn insert_credential(&self, cred: StoredCredentialRow) -> DbResult<()> {
        self.db
            .write_sized("credentials_insert", 1024, move |tx| {
                insert_credential_tx(tx, &cred)
            })
            .await
    }

    /// Atualiza uma credencial cifrada existente.
    pub async fn update_credential(&self, cred: StoredCredentialRow) -> DbResult<()> {
        self.db
            .write_sized("credentials_update", 1024, move |tx| {
                update_credential_tx(tx, &cred)
            })
            .await
    }

    /// Exclui uma credencial pelo seu ID.
    pub async fn delete_credential(&self, id: String) -> DbResult<bool> {
        self.db
            .write_sized("credentials_delete", 512, move |tx| {
                delete_credential_tx(tx, &id)
            })
            .await
    }
}

/// Busca metadados do cofre na conexão SQLite.
pub fn get_vault_metadata_conn(conn: &Connection) -> DbResult<Option<StoredVaultMetadata>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, version, kdf_algorithm, kdf_salt, kdf_params_json,
                    encrypted_dek, dek_nonce, created_at, updated_at
             FROM vault_metadata
             WHERE id = 1;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "vault_metadata_get"))?;
    stmt.query_row([], map_vault_metadata_row)
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "vault_metadata_get"))
}

/// Salva novos metadados do cofre em transação.
pub fn save_vault_metadata_tx(tx: &Transaction<'_>, meta: &StoredVaultMetadata) -> DbResult<()> {
    tx.execute(
        "INSERT INTO vault_metadata (
            id, version, kdf_algorithm, kdf_salt, kdf_params_json,
            encrypted_dek, dek_nonce, created_at, updated_at
         ) VALUES (
            1, ?1, ?2, ?3, ?4,
            ?5, ?6, ?7, ?8
         );",
        params![
            meta.version,
            meta.kdf_algorithm,
            meta.kdf_salt,
            meta.kdf_params_json,
            meta.encrypted_dek,
            meta.dek_nonce,
            meta.created_at,
            meta.updated_at,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "vault_metadata_insert"))?;
    Ok(())
}

/// Atualiza metadados do cofre (troca de senha mestra) em transação.
pub fn update_vault_metadata_tx(tx: &Transaction<'_>, meta: &StoredVaultMetadata) -> DbResult<()> {
    let updated = tx
        .execute(
            "UPDATE vault_metadata
             SET version = ?1,
                 kdf_algorithm = ?2,
                 kdf_salt = ?3,
                 kdf_params_json = ?4,
                 encrypted_dek = ?5,
                 dek_nonce = ?6,
                 updated_at = ?7
             WHERE id = 1;",
            params![
                meta.version,
                meta.kdf_algorithm,
                meta.kdf_salt,
                meta.kdf_params_json,
                meta.encrypted_dek,
                meta.dek_nonce,
                meta.updated_at,
            ],
        )
        .map_err(|e| DbError::from_rusqlite(e, "vault_metadata_update"))?;
    if updated != 1 {
        return Err(DbError::ConstraintViolation {
            constraint: String::from("vault_metadata.not_found"),
        });
    }
    Ok(())
}

/// Busca uma credencial pelo seu ID.
pub fn get_credential_conn(conn: &Connection, id: &str) -> DbResult<Option<StoredCredentialRow>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, project_id, nonce, ciphertext, version, created_at, updated_at
             FROM credentials
             WHERE id = ?1;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "credentials_get"))?;
    stmt.query_row([id], map_credential_row)
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "credentials_get"))
}

/// Lista credenciais de um projeto.
pub fn list_credentials_by_project_conn(
    conn: &Connection,
    project_id: &str,
) -> DbResult<Vec<StoredCredentialRow>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, project_id, nonce, ciphertext, version, created_at, updated_at
             FROM credentials
             WHERE project_id = ?1
             ORDER BY created_at ASC, id ASC;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "credentials_list_by_project"))?;
    let rows = stmt
        .query_map([project_id], map_credential_row)
        .map_err(|e| DbError::from_rusqlite(e, "credentials_list_by_project"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| DbError::from_rusqlite(e, "credentials_list_by_project"))
}

/// Insere uma credencial cifrada em transação.
pub fn insert_credential_tx(tx: &Transaction<'_>, cred: &StoredCredentialRow) -> DbResult<()> {
    let proj_str = cred.project_id.to_string();
    tx.execute(
        "INSERT INTO credentials (
            id, project_id, nonce, ciphertext, version, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7
         );",
        params![
            cred.id,
            proj_str,
            cred.nonce,
            cred.ciphertext,
            cred.version,
            cred.created_at,
            cred.updated_at,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "credentials_insert"))?;
    Ok(())
}

/// Atualiza uma credencial cifrada em transação.
pub fn update_credential_tx(tx: &Transaction<'_>, cred: &StoredCredentialRow) -> DbResult<()> {
    let updated = tx
        .execute(
            "UPDATE credentials
             SET nonce = ?2,
                 ciphertext = ?3,
                 version = ?4,
                 updated_at = ?5
             WHERE id = ?1;",
            params![
                cred.id,
                cred.nonce,
                cred.ciphertext,
                cred.version,
                cred.updated_at,
            ],
        )
        .map_err(|e| DbError::from_rusqlite(e, "credentials_update"))?;
    if updated != 1 {
        return Err(DbError::ConstraintViolation {
            constraint: String::from("credentials.not_found"),
        });
    }
    Ok(())
}

/// Exclui uma credencial pelo ID em transação.
pub fn delete_credential_tx(tx: &Transaction<'_>, id: &str) -> DbResult<bool> {
    let count = tx
        .execute("DELETE FROM credentials WHERE id = ?1;", [id])
        .map_err(|e| DbError::from_rusqlite(e, "credentials_delete"))?;
    Ok(count > 0)
}

fn map_vault_metadata_row(row: &Row<'_>) -> rusqlite::Result<StoredVaultMetadata> {
    Ok(StoredVaultMetadata {
        id: row.get(0)?,
        version: row.get(1)?,
        kdf_algorithm: row.get(2)?,
        kdf_salt: row.get(3)?,
        kdf_params_json: row.get(4)?,
        encrypted_dek: row.get(5)?,
        dek_nonce: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn map_credential_row(row: &Row<'_>) -> rusqlite::Result<StoredCredentialRow> {
    let id: String = row.get(0)?;
    let proj_raw: String = row.get(1)?;
    let project_id = ProjectId::from_str(&proj_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::from("ProjectId inválido"),
        )
    })?;
    let nonce: Vec<u8> = row.get(2)?;
    let ciphertext: Vec<u8> = row.get(3)?;
    let version: i64 = row.get(4)?;
    let created_at: i64 = row.get(5)?;
    let updated_at: i64 = row.get(6)?;

    Ok(StoredCredentialRow {
        id,
        project_id,
        nonce,
        ciphertext,
        version,
        created_at,
        updated_at,
    })
}
