//! Gerenciamento seguro de credenciais GitHub (token PAT para fallback REST e detecção de variáveis de ambiente).
//!
//! Nunca persiste o segredo em texto puro no SQLite ou em logs.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use sha2::{Digest, Sha256};

use crate::error::{GithubError, GithubErrorKind, GithubResult};
use crate::model::RestTokenStatus;

#[derive(Debug, Clone)]
struct StoredTokenEntry {
    token: String,
    source: String,
    account_login: Option<String>,
}

/// Armazenamento seguro de token PAT para o fallback REST da API do GitHub.
///
/// Prioridade de leitura:
/// 1. Variáveis de ambiente (`GH_TOKEN` ou `GITHUB_TOKEN`) quando presentes;
/// 2. Token configurado pelo usuário em arquivo local `0600` ofuscado com chave derivada da máquina/usuário ou em memória.
#[derive(Debug, Clone)]
pub struct TokenStore {
    inner: Arc<RwLock<Option<StoredTokenEntry>>>,
    storage_file: Option<PathBuf>,
    allow_env_lookup: bool,
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl TokenStore {
    /// Cria um `TokenStore` puramente em memória (ideal para testes determinísticos sem ler env global).
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
            storage_file: None,
            allow_env_lookup: false,
        }
    }

    /// Cria um `TokenStore` respaldado por arquivo `0600` no diretório de dados da aplicação e leitura de `GH_TOKEN`/`GITHUB_TOKEN`.
    #[must_use]
    pub fn with_storage_file(path: PathBuf) -> Self {
        let store = Self {
            inner: Arc::new(RwLock::new(None)),
            storage_file: Some(path),
            allow_env_lookup: true,
        };
        store.load_from_disk();
        store
    }

    /// Define o token PAT validando formato mínimo e persistindo em arquivo `0600` quando configurado.
    pub fn set_token(
        &self,
        raw_token: &str,
        account_login: Option<String>,
    ) -> GithubResult<RestTokenStatus> {
        let trimmed = raw_token.trim();
        if trimmed.len() < 8 || trimmed.chars().any(char::is_whitespace) {
            return Err(GithubError::new(
                GithubErrorKind::ValidationFailed,
                "Token do GitHub inválido: informe um Personal Access Token válido sem espaços.",
            ));
        }

        let source = if self.storage_file.is_some() {
            String::from("encrypted_file")
        } else {
            String::from("memory")
        };

        let entry = StoredTokenEntry {
            token: trimmed.to_string(),
            source,
            account_login,
        };

        if let Some(ref file_path) = self.storage_file {
            Self::save_to_disk(file_path, &entry)?;
        }

        *self.inner.write() = Some(entry);
        Ok(self.status())
    }

    /// Atualiza apenas o login da conta validado para o token atual.
    pub fn set_validated_login(&self, login: Option<String>) {
        let mut guard = self.inner.write();
        if let Some(ref mut entry) = *guard {
            entry.account_login = login;
            if let Some(ref file_path) = self.storage_file {
                let _ = Self::save_to_disk(file_path, entry);
            }
        }
    }

    /// Remove qualquer token armazenado em memória e no disco.
    pub fn clear_token(&self) -> GithubResult<RestTokenStatus> {
        *self.inner.write() = None;
        if let Some(ref file_path) = self.storage_file {
            if file_path.exists() {
                let _ = fs::remove_file(file_path);
            }
        }
        Ok(self.status())
    }

    /// Obtém o segredo bruto para envio no cabeçalho `Authorization: Bearer` (nunca logar).
    #[must_use]
    pub fn resolve_secret(&self) -> Option<String> {
        if let Some(ref entry) = *self.inner.read() {
            return Some(entry.token.clone());
        }
        if self.allow_env_lookup {
            for key in ["GH_TOKEN", "GITHUB_TOKEN"] {
                if let Ok(val) = std::env::var(key) {
                    let trimmed = val.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
        None
    }

    /// Retorna o `RestTokenStatus` mascarado e seguro para exibição na UI.
    #[must_use]
    pub fn status(&self) -> RestTokenStatus {
        if let Some(ref entry) = *self.inner.read() {
            return RestTokenStatus {
                configured: true,
                source: entry.source.clone(),
                account_login: entry.account_login.clone(),
                masked_hint: Some(mask_token_hint(&entry.token)),
            };
        }
        if self.allow_env_lookup {
            for key in ["GH_TOKEN", "GITHUB_TOKEN"] {
                if let Ok(val) = std::env::var(key) {
                    let trimmed = val.trim();
                    if !trimmed.is_empty() {
                        return RestTokenStatus {
                            configured: true,
                            source: String::from("env"),
                            account_login: None,
                            masked_hint: Some(mask_token_hint(trimmed)),
                        };
                    }
                }
            }
        }
        RestTokenStatus {
            configured: false,
            source: String::from("none"),
            account_login: None,
            masked_hint: None,
        }
    }

    fn load_from_disk(&self) {
        let Some(ref file_path) = self.storage_file else {
            return;
        };
        let Ok(bytes) = fs::read(file_path) else {
            return;
        };
        let Some(decoded) = deobfuscate_payload(&bytes) else {
            return;
        };
        let Ok(parsed) = serde_json::from_slice::<DiskTokenPayload>(&decoded) else {
            return;
        };
        if parsed.token.trim().len() >= 8 {
            *self.inner.write() = Some(StoredTokenEntry {
                token: parsed.token,
                source: String::from("encrypted_file"),
                account_login: parsed.account_login,
            });
        }
    }

    fn save_to_disk(file_path: &PathBuf, entry: &StoredTokenEntry) -> GithubResult<()> {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                GithubError::new(
                    GithubErrorKind::Internal,
                    "Falha ao criar diretório seguro de credenciais GitHub.",
                )
                .with_details(e.to_string())
            })?;
        }
        let payload = DiskTokenPayload {
            token: entry.token.clone(),
            account_login: entry.account_login.clone(),
        };
        let json_bytes = serde_json::to_vec(&payload).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "Falha ao serializar credencial GitHub.",
            )
            .with_details(e.to_string())
        })?;
        let obfuscated = obfuscate_payload(&json_bytes);
        fs::write(file_path, obfuscated).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "Falha ao gravar credencial GitHub em disco.",
            )
            .with_details(e.to_string())
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(file_path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DiskTokenPayload {
    token: String,
    account_login: Option<String>,
}

/// Mascaramento seguro que preserva apenas os últimos 4 caracteres quando o token possui >= 12 caracteres.
#[must_use]
pub fn mask_token_hint(token: &str) -> String {
    let trimmed = token.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() >= 12 {
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("••••••••{tail}")
    } else {
        String::from("••••••••")
    }
}

fn machine_key_stream(len: usize) -> Vec<u8> {
    let user = std::env::var("USER").unwrap_or_else(|_| String::from("maxterminal"));
    let mut out = Vec::with_capacity(len);
    let mut counter: u64 = 0;
    while out.len() < len {
        let mut hasher = Sha256::new();
        hasher.update(b"maxterminal-github-token-v1:");
        hasher.update(user.as_bytes());
        hasher.update(counter.to_le_bytes());
        let block = hasher.finalize();
        out.extend_from_slice(&block);
        counter += 1;
    }
    out.truncate(len);
    out
}

fn obfuscate_payload(plain: &[u8]) -> Vec<u8> {
    let key = machine_key_stream(plain.len());
    let mut out = Vec::with_capacity(4 + plain.len());
    out.extend_from_slice(b"MTG1");
    for (b, k) in plain.iter().zip(key.iter()) {
        out.push(b ^ k);
    }
    out
}

fn deobfuscate_payload(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 5 || &data[0..4] != b"MTG1" {
        return None;
    }
    let body = &data[4..];
    let key = machine_key_stream(body.len());
    Some(body.iter().zip(key.iter()).map(|(b, k)| b ^ k).collect())
}
