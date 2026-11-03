//! Arquivos atômicos de descoberta de endpoint (`maxterminal.endpoint.json` e `maxcode.endpoint.json`)
//! com permissões estritas `0700`/`0600`, verificação de UID e checagem de vivacidade do PID.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use nix::libc;
use nix::unistd::Uid;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{BridgeError, BridgeErrorCode};
use crate::protocol::PeerRole;

/// Nome do subdiretório dentro de `$XDG_RUNTIME_DIR` ou `~/.config` onde os endpoints são publicados.
pub const BRIDGE_SUBDIR: &str = "max-bridge";
/// Nome do arquivo publicado pelo `MaxTerminal`.
pub const MAXTERMINAL_ENDPOINT_FILE: &str = "maxterminal.endpoint.json";
/// Nome do arquivo publicado pela extensão `MaxCode`.
pub const MAXCODE_ENDPOINT_FILE: &str = "maxcode.endpoint.json";

/// Conteúdo serializado em `maxterminal.endpoint.json` e `maxcode.endpoint.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryEndpoint {
    /// Versão do protocolo (`1`).
    pub protocol_version: u32,
    /// Papel do processo que publicou o arquivo (`max_terminal` ou `max_code`).
    pub role: PeerRole,
    /// Identificador único da instância em execução.
    pub instance_id: String,
    /// PID do processo no sistema operacional.
    pub pid: u32,
    /// Endereço IP de escuta (obrigatoriamente `"127.0.0.1"`).
    pub host: String,
    /// Porta TCP de escuta (`1..=65535`).
    pub port: u16,
    ///Caminho do endpoint WebSocket (`"/v1/ws"`).
    pub ws_path: String,
    /// Caminho do endpoint HTTP RPC (`"/v1/rpc"`).
    pub http_rpc_path: String,
    /// Token secreto em hexadecimal (64 chars = 256 bits) exigido em `Authorization: Bearer`.
    pub token: String,
    /// Timestamp UNIX em milissegundos de início da instância.
    pub started_at_ms: i64,
    /// Lista de capacidades suportadas pela instância.
    pub capabilities: Vec<String>,
}

/// Resolve o diretório de descoberta preferindo `$XDG_RUNTIME_DIR/max-bridge` e caindo para
/// `~/.config/max-bridge`, garantindo modo `0700` e propriedade do UID atual.
pub fn resolve_discovery_dir(override_base: Option<&Path>) -> Result<PathBuf, BridgeError> {
    let base = if let Some(custom) = override_base {
        custom.to_path_buf()
    } else if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR")
        && !xdg.trim().is_empty()
    {
        PathBuf::from(xdg).join(BRIDGE_SUBDIR)
    } else {
        let cfg = dirs::config_dir().ok_or_else(|| {
            BridgeError::new(
                BridgeErrorCode::TransportIo,
                "Não foi possível determinar o diretório de configuração do usuário.",
            )
        })?;
        cfg.join(BRIDGE_SUBDIR)
    };

    ensure_secure_dir(&base)?;
    Ok(base)
}

/// Garante que o diretório existe, não é symlink, pertence ao UID atual e possui permissão `0700`.
pub fn ensure_secure_dir(dir: &Path) -> Result<(), BridgeError> {
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|e| {
            BridgeError::new(
                BridgeErrorCode::TransportIo,
                format!("Falha ao criar diretório de descoberta da ponte: {e}"),
            )
        })?;
    }

    let meta = fs::symlink_metadata(dir).map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao inspecionar diretório de descoberta: {e}"),
        )
    })?;

    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(BridgeError::new(
            BridgeErrorCode::DiscoveryInsecurePermissions,
            "O caminho do diretório de descoberta não pode ser um link simbólico.",
        ));
    }

    let current_uid = Uid::effective().as_raw();
    if meta.uid() != current_uid {
        return Err(BridgeError::new(
            BridgeErrorCode::DiscoveryInsecurePermissions,
            "O diretório de descoberta pertence a outro usuário do sistema.",
        ));
    }

    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o700 {
        let mut perms = meta.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(dir, perms).map_err(|e| {
            BridgeError::new(
                BridgeErrorCode::DiscoveryInsecurePermissions,
                format!("Não foi possível aplicar permissão 0700 ao diretório de descoberta: {e}"),
            )
        })?;
    }

    Ok(())
}

/// Escreve atomicamente um `DiscoveryEndpoint` usando arquivo temporário `.tmp.<pid>` com modo `0600` e `rename`.
pub fn write_atomic(target_path: &Path, endpoint: &DiscoveryEndpoint) -> Result<(), BridgeError> {
    validate_endpoint_fields(endpoint)?;
    let parent = target_path.parent().ok_or_else(|| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            "Caminho de arquivo de descoberta sem diretório pai.",
        )
    })?;
    ensure_secure_dir(parent)?;

    let file_name = target_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("endpoint.json");
    let tmp_path = parent.join(format!(".{file_name}.tmp.{}", std::process::id()));

    let serialized = serde_json::to_vec_pretty(endpoint).map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao serializar arquivo de descoberta: {e}"),
        )
    })?;

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp_path)
        .map_err(|e| {
            BridgeError::new(
                BridgeErrorCode::TransportIo,
                format!("Falha ao criar arquivo temporário de descoberta: {e}"),
            )
        })?;

    file.write_all(&serialized).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao gravar arquivo de descoberta: {e}"),
        )
    })?;
    file.sync_all().map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha no fsync do arquivo de descoberta: {e}"),
        )
    })?;
    drop(file);

    fs::rename(&tmp_path, target_path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao renomear arquivo de descoberta atomicamente: {e}"),
        )
    })?;

    Ok(())
}

/// Lê e valida um `DiscoveryEndpoint`:
/// 1. Não pode ser symlink;
/// 2. Deve pertencer ao UID atual;
/// 3. Não pode ter bits de leitura/escrita para grupo ou outros (`mode & 0o077 == 0`);
/// 4. `host` deve ser `"127.0.0.1"`;
/// 5. O `pid` registrado deve estar vivo no sistema operacional (`/proc/<pid>` ou `kill(pid, 0)`).
pub fn read_verified(path: &Path) -> Result<DiscoveryEndpoint, BridgeError> {
    let meta = fs::symlink_metadata(path).map_err(|_| {
        BridgeError::new(
            BridgeErrorCode::DiscoveryNotFound,
            "Arquivo de descoberta não encontrado.",
        )
    })?;

    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(BridgeError::new(
            BridgeErrorCode::DiscoveryInsecurePermissions,
            "O arquivo de descoberta não é um arquivo regular ou é um link simbólico.",
        ));
    }

    let current_uid = Uid::effective().as_raw();
    if meta.uid() != current_uid {
        return Err(BridgeError::new(
            BridgeErrorCode::DiscoveryInsecurePermissions,
            "O arquivo de descoberta pertence a outro UID.",
        ));
    }

    let mode = meta.permissions().mode() & 0o777;
    if (mode & 0o077) != 0 {
        return Err(BridgeError::new(
            BridgeErrorCode::DiscoveryInsecurePermissions,
            format!("Permissões inseguras no arquivo de descoberta ({mode:04o}); esperado 0600."),
        ));
    }

    if meta.len() > 32 * 1024 {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            "Arquivo de descoberta excede o limite de 32 KiB.",
        ));
    }

    let raw = fs::read_to_string(path).map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao ler arquivo de descoberta: {e}"),
        )
    })?;

    let endpoint: DiscoveryEndpoint = serde_json::from_str(&raw).map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            format!("JSON inválido no arquivo de descoberta: {e}"),
        )
    })?;

    validate_endpoint_fields(&endpoint)?;

    if !is_pid_alive(endpoint.pid) {
        let _ = fs::remove_file(path);
        return Err(BridgeError::new(
            BridgeErrorCode::DiscoveryNotFound,
            format!(
                "O processo registrado no arquivo de descoberta (PID {}) não está mais ativo.",
                endpoint.pid
            ),
        ));
    }

    Ok(endpoint)
}

/// Remove o arquivo de descoberta no encerramento somente se ele ainda pertencer à `instance_id` informada.
pub fn remove_if_owned(path: &Path, expected_instance_id: &str) {
    if let Ok(raw) = fs::read_to_string(path)
        && let Ok(ep) = serde_json::from_str::<DiscoveryEndpoint>(&raw)
        && ep.instance_id == expected_instance_id
    {
        let _ = fs::remove_file(path);
    }
}

/// Verifica se um PID está vivo no Linux.
#[must_use]
pub fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    Path::new(&format!("/proc/{pid}")).exists()
}

fn validate_endpoint_fields(ep: &DiscoveryEndpoint) -> Result<(), BridgeError> {
    let ip: IpAddr = ep.host.parse().map_err(|_| {
        BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            "Endereço host inválido no endpoint de descoberta.",
        )
    })?;
    if ip != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            "O endpoint de descoberta deve usar exclusivamente 127.0.0.1.",
        ));
    }
    if ep.port == 0 {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            "A porta do endpoint de descoberta não pode ser 0.",
        ));
    }
    if ep.token.len() < 32 || !ep.token.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            "Token de descoberta inválido (esperado hexadecimal >= 32 caracteres).",
        ));
    }
    Ok(())
}

/// Retorna os caminhos candidatos (primário em `$XDG_RUNTIME_DIR` e secundário em `~/.config`) para `maxcode.endpoint.json`.
#[must_use]
pub fn candidate_maxcode_endpoint_paths(override_dir: Option<&Path>) -> Vec<PathBuf> {
    if let Some(dir) = override_dir {
        return vec![dir.join(MAXCODE_ENDPOINT_FILE)];
    }
    let mut paths = Vec::new();
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR")
        && !xdg.trim().is_empty()
    {
        paths.push(
            PathBuf::from(xdg)
                .join(BRIDGE_SUBDIR)
                .join(MAXCODE_ENDPOINT_FILE),
        );
    }
    if let Some(cfg) = dirs::config_dir() {
        let fallback = cfg.join(BRIDGE_SUBDIR).join(MAXCODE_ENDPOINT_FILE);
        if !paths.contains(&fallback) {
            paths.push(fallback);
        }
    }
    paths
}
