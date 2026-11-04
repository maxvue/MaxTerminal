//! Serviço de gerenciamento do ciclo de vida do cofre nativo de credenciais (`VaultService`).
//!
//! Coordena desbloqueio, bloqueio, rotação de senha mestra e criptografia/descriptografia de senhas SSH.

use std::sync::Arc;

use mt_core::error::{MtError, MtResult, codes};
use mt_core::ids::ProjectId;
use mt_db::Db;
use mt_db::repos::{StoredCredentialRow, StoredVaultMetadata};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::credentials::crypto::{
    DEFAULT_M_COST, DEFAULT_P_COST, DEFAULT_T_COST, KDF_SALT_LEN, KEY_LEN, decrypt_credential,
    decrypt_dek, derive_kek, encrypt_credential, encrypt_dek, random_bytes,
};
use crate::workspace::errors::map_db_error;

/// Estado público do cofre retornado aos comandos IPC e exibido na UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum VaultStatus {
    /// O cofre ainda não possui senha mestra configurada.
    Unconfigured,
    /// O cofre está configurado, porém bloqueado (aguarda senha mestra).
    Locked,
    /// O cofre está desbloqueado e sua chave de dados (DEK) está ativa em memória.
    Unlocked,
}

/// Serviço thread-safe do cofre de credenciais.
#[derive(Clone)]
pub struct VaultService {
    db: Db,
    dek: Arc<RwLock<Option<Zeroizing<[u8; KEY_LEN]>>>>,
    op_lock: Arc<TokioMutex<()>>,
}

impl VaultService {
    /// Cria uma nova instância de [`VaultService`].
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self {
            db,
            dek: Arc::new(RwLock::new(None)),
            op_lock: Arc::new(TokioMutex::new(())),
        }
    }

    /// Retorna o estado operacional corrente do cofre.
    pub async fn status(&self) -> MtResult<VaultStatus> {
        if self.dek.read().is_some() {
            return Ok(VaultStatus::Unlocked);
        }

        let meta = self
            .db
            .credentials()
            .get_vault_metadata()
            .await
            .map_err(map_db_error)?;

        if meta.is_some() {
            Ok(VaultStatus::Locked)
        } else {
            Ok(VaultStatus::Unconfigured)
        }
    }

    /// Configura a senha mestra inicial do cofre gerando uma DEK nova.
    pub async fn setup(&self, master_password: &str) -> MtResult<()> {
        let _guard = self.op_lock.lock().await;

        let existing = self
            .db
            .credentials()
            .get_vault_metadata()
            .await
            .map_err(map_db_error)?;

        if existing.is_some() {
            return Err(MtError::new(
                codes::CONFLICT,
                "O cofre de credenciais já está configurado.",
            ));
        }

        let salt = random_bytes::<KDF_SALT_LEN>();
        let dek = random_bytes::<KEY_LEN>();

        let kek = derive_kek(
            master_password,
            &salt,
            DEFAULT_M_COST,
            DEFAULT_T_COST,
            DEFAULT_P_COST,
        )?;

        let (encrypted_dek, dek_nonce) = encrypt_dek(&kek, &dek)?;
        let now_ms = mt_db::time::now_unix_ms();

        let params_json = serde_json::to_string(&serde_json::json!({
            "m_cost": DEFAULT_M_COST,
            "t_cost": DEFAULT_T_COST,
            "p_cost": DEFAULT_P_COST,
        }))
        .unwrap_or_else(|_| String::from("{}"));

        let meta = StoredVaultMetadata {
            id: 1,
            version: 1,
            kdf_algorithm: String::from("argon2id"),
            kdf_salt: salt.to_vec(),
            kdf_params_json: params_json,
            encrypted_dek,
            dek_nonce: dek_nonce.to_vec(),
            created_at: now_ms,
            updated_at: now_ms,
        };

        self.db
            .credentials()
            .save_vault_metadata(meta)
            .await
            .map_err(map_db_error)?;

        *self.dek.write() = Some(Zeroizing::new(dek));
        Ok(())
    }

    /// Desbloqueia o cofre derivando a KEK e decifrando a DEK para a memória.
    pub async fn unlock(&self, master_password: &str) -> MtResult<()> {
        let _guard = self.op_lock.lock().await;

        if self.dek.read().is_some() {
            return Ok(());
        }

        let meta = self
            .db
            .credentials()
            .get_vault_metadata()
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| {
                MtError::new(
                    codes::VAULT_UNCONFIGURED,
                    "O cofre ainda não possui senha mestra configurada.",
                )
            })?;

        let kek = derive_kek(
            master_password,
            &meta.kdf_salt,
            DEFAULT_M_COST,
            DEFAULT_T_COST,
            DEFAULT_P_COST,
        )?;

        let dek = decrypt_dek(&kek, &meta.encrypted_dek, &meta.dek_nonce)?;

        *self.dek.write() = Some(dek);
        Ok(())
    }

    /// Bloqueia o cofre imediatamente, limpando e zeroizando a DEK da memória.
    pub fn lock(&self) {
        let mut w = self.dek.write();
        if let Some(mut existing) = w.take() {
            existing.zeroize();
        }
    }

    /// Troca a senha mestra do cofre, reencapsulando a mesma DEK sob a nova chave KEK.
    pub async fn change_master_password(
        &self,
        current_password: &str,
        new_password: &str,
    ) -> MtResult<()> {
        let _guard = self.op_lock.lock().await;

        let meta = self
            .db
            .credentials()
            .get_vault_metadata()
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| {
                MtError::new(
                    codes::VAULT_UNCONFIGURED,
                    "O cofre ainda não foi configurado.",
                )
            })?;

        let current_kek = derive_kek(
            current_password,
            &meta.kdf_salt,
            DEFAULT_M_COST,
            DEFAULT_T_COST,
            DEFAULT_P_COST,
        )?;

        let dek = decrypt_dek(&current_kek, &meta.encrypted_dek, &meta.dek_nonce)?;

        let new_salt = random_bytes::<KDF_SALT_LEN>();
        let new_kek = derive_kek(
            new_password,
            &new_salt,
            DEFAULT_M_COST,
            DEFAULT_T_COST,
            DEFAULT_P_COST,
        )?;

        let (encrypted_dek, dek_nonce) = encrypt_dek(&new_kek, &dek)?;
        let now_ms = mt_db::time::now_unix_ms();

        let updated_meta = StoredVaultMetadata {
            id: 1,
            version: meta.version + 1,
            kdf_algorithm: String::from("argon2id"),
            kdf_salt: new_salt.to_vec(),
            kdf_params_json: meta.kdf_params_json,
            encrypted_dek,
            dek_nonce: dek_nonce.to_vec(),
            created_at: meta.created_at,
            updated_at: now_ms,
        };

        self.db
            .credentials()
            .update_vault_metadata(updated_meta)
            .await
            .map_err(map_db_error)?;

        *self.dek.write() = Some(dek);
        Ok(())
    }

    /// Cifra e armazena uma senha SSH no cofre sob o projeto informado, retornando seu `credential_id`.
    pub async fn store_credential(&self, project_id: ProjectId, password: &str) -> MtResult<String> {
        let dek_opt = self.dek.read().clone();
        let Some(dek) = dek_opt else {
            return Err(MtError::new(
                codes::VAULT_LOCKED,
                "O cofre de senhas está bloqueado. Desbloqueie-o com a senha mestra para salvar credenciais.",
            ));
        };

        let cred_id = Uuid::now_v7().to_string();
        let proj_str = project_id.to_string();
        let (ciphertext, nonce) = encrypt_credential(&dek, 1, &proj_str, &cred_id, password)?;
        let now_ms = mt_db::time::now_unix_ms();

        let row = StoredCredentialRow {
            id: cred_id.clone(),
            project_id,
            nonce: nonce.to_vec(),
            ciphertext,
            version: 1,
            created_at: now_ms,
            updated_at: now_ms,
        };

        self.db
            .credentials()
            .insert_credential(row)
            .await
            .map_err(map_db_error)?;

        Ok(cred_id)
    }

    /// Atualiza uma credencial existente com nova senha ou cria caso não exista.
    pub async fn update_or_store_credential(
        &self,
        project_id: ProjectId,
        credential_id_opt: Option<&str>,
        password: &str,
    ) -> MtResult<String> {
        let dek_opt = self.dek.read().clone();
        let Some(dek) = dek_opt else {
            return Err(MtError::new(
                codes::VAULT_LOCKED,
                "O cofre de senhas está bloqueado. Desbloqueie-o com a senha mestra.",
            ));
        };

        let cred_id = match credential_id_opt {
            Some(id) if !id.trim().is_empty() => id.trim().to_string(),
            _ => Uuid::now_v7().to_string(),
        };

        let proj_str = project_id.to_string();
        let (ciphertext, nonce) = encrypt_credential(&dek, 1, &proj_str, &cred_id, password)?;
        let now_ms = mt_db::time::now_unix_ms();

        let row = StoredCredentialRow {
            id: cred_id.clone(),
            project_id,
            nonce: nonce.to_vec(),
            ciphertext,
            version: 1,
            created_at: now_ms,
            updated_at: now_ms,
        };

        let existing = self
            .db
            .credentials()
            .get_credential(cred_id.clone())
            .await
            .map_err(map_db_error)?;

        if existing.is_some() {
            self.db
                .credentials()
                .update_credential(row)
                .await
                .map_err(map_db_error)?;
        } else {
            self.db
                .credentials()
                .insert_credential(row)
                .await
                .map_err(map_db_error)?;
        }

        Ok(cred_id)
    }

    /// Decifra e retorna a senha armazenada (exclusivamente para uso nativo do transporte SSH).
    pub async fn retrieve_credential(
        &self,
        project_id: ProjectId,
        credential_id: &str,
    ) -> MtResult<Zeroizing<String>> {
        let dek_opt = self.dek.read().clone();
        let Some(dek) = dek_opt else {
            return Err(MtError::new(
                codes::CREDENTIAL_LOCKED,
                "O cofre de senhas está bloqueado. Desbloqueie com a senha mestra para autenticar conexões SSH.",
            ));
        };

        let cred = self
            .db
            .credentials()
            .get_credential(credential_id.to_string())
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| {
                MtError::new(
                    codes::VAULT_CREDENTIAL_NOT_FOUND,
                    "Credencial de conexão SSH não encontrada no cofre.",
                )
            })?;

        if cred.project_id != project_id {
            return Err(MtError::new(
                codes::PERMISSION_DENIED,
                "Acesso negado: a credencial pertence a outro projeto.",
            ));
        }

        let proj_str = project_id.to_string();
        decrypt_credential(
            &dek,
            cred.version,
            &proj_str,
            credential_id,
            &cred.nonce,
            &cred.ciphertext,
        )
    }

    /// Remove uma credencial armazenada.
    pub async fn delete_credential(&self, credential_id: &str) -> MtResult<bool> {
        self.db
            .credentials()
            .delete_credential(credential_id.to_string())
            .await
            .map_err(map_db_error)
    }
}
