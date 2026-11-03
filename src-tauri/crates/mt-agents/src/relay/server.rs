//! Servidor Unix domain socket autenticado por token de 256 bits e `SO_PEERCRED` (`RelayServer`).

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use mt_core::{AppPaths, MtResult, TabId};
use parking_lot::RwLock;
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::detect::current_unix_ms;
use crate::error;
use crate::launch::PendingRunBinding;
use crate::ports::RunBinding;
use crate::relay::protocol::{
    MAX_RELAY_MESSAGE_BYTES, RELAY_PROTOCOL_VERSION, RelayAck, RelayEnvelope, RelayMessage,
};

const MAX_PENDING_EVENTS_PER_RUN: usize = 256;
const MAX_PENDING_BYTES_PER_RUN: usize = 256 * 1024;
const READ_LINE_TIMEOUT: Duration = Duration::from_secs(1);
const PROBE_CONNECT_TIMEOUT: Duration = Duration::from_millis(100);

/// Token opaco de 256 bits codificado em `base64url` sem padding; `Debug` nunca imprime o segredo.
#[derive(Clone, PartialEq, Eq)]
pub struct RelayToken(String);

impl RelayToken {
    /// Gera um novo token CSPRNG de 256 bits e retorna `(RelayToken, sha256_digest)`.
    #[must_use]
    pub fn generate() -> (Self, [u8; 32]) {
        let mut raw = [0u8; 32];
        rand::rng().fill_bytes(&mut raw);
        let encoded = URL_SAFE_NO_PAD.encode(raw);
        let digest = sha256_bytes(&raw);
        (Self(encoded), digest)
    }

    /// Constrói um `RelayToken` a partir de string existente (para testes).
    #[must_use]
    pub fn from_raw_str(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Retorna a representação textual `base64url` do token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for RelayToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RelayToken([REDACTED])")
    }
}

/// Evento do relay já autenticado e correlacionado a um `RunBinding` ativo.
#[derive(Debug, Clone)]
pub struct AuthenticatedRelayEvent {
    /// Identidade da execução proprietária do token.
    pub binding: RunBinding,
    /// Mensagem recebida do sidecar.
    pub message: RelayMessage,
    /// Timestamp UNIX em ms do recebimento.
    pub received_at_ms: i64,
    /// Indica se houve overflow/perda anterior nesta execução que exige reconciliação.
    pub overflow_reconcile: bool,
}

struct PendingEntry {
    binding: PendingRunBinding,
    queued: VecDeque<(RelayMessage, i64)>,
    queued_bytes: usize,
    overflowed: bool,
}

struct ActiveEntry {
    binding: RunBinding,
    overflowed: Arc<AtomicBool>,
}

/// Servidor Unix domain socket que recebe eventos do binário `mt-agent-relay`.
pub struct RelayServer {
    socket_path: PathBuf,
    tokens: RwLock<HashMap<[u8; 32], ActiveEntry>>,
    pending_tokens: RwLock<HashMap<[u8; 32], PendingEntry>>,
    tx: mpsc::Sender<AuthenticatedRelayEvent>,
    shutdown: CancellationToken,
}

impl RelayServer {
    /// Capacidade padrão do canal de eventos autenticados.
    pub const CHANNEL_CAPACITY: usize = 256;

    /// Inicia o listener em `$XDG_RUNTIME_DIR/maxterminal/agent-relay.sock` (ou fallback em `state_dir/run`).
    pub async fn bind(
        paths: &AppPaths,
        tx: mpsc::Sender<AuthenticatedRelayEvent>,
    ) -> MtResult<Arc<Self>> {
        let base_dir = paths
            .runtime_dir
            .clone()
            .unwrap_or_else(|| paths.state_dir.join("run"));
        Self::bind_in_dir(&base_dir, tx).await
    }

    /// Inicia o listener em um diretório específico (útil para testes).
    pub async fn bind_in_dir(
        base_dir: &Path,
        tx: mpsc::Sender<AuthenticatedRelayEvent>,
    ) -> MtResult<Arc<Self>> {
        fs::create_dir_all(base_dir).map_err(|e| {
            error::relay_unavailable(format!(
                "Falha ao criar diretório do relay {}: {e}",
                base_dir.display()
            ))
        })?;
        fs::set_permissions(base_dir, fs::Permissions::from_mode(0o700)).map_err(|e| {
            error::relay_unavailable(format!(
                "Falha ao ajustar permissão 0700 em {}: {e}",
                base_dir.display()
            ))
        })?;

        let primary_path = base_dir.join("agent-relay.sock");
        let socket_path = prepare_socket_path(&primary_path).await?;

        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            error::relay_unavailable(format!(
                "Falha ao fazer bind em {}: {e}",
                socket_path.display()
            ))
        })?;

        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600)).map_err(|e| {
            error::relay_unavailable(format!(
                "Falha ao ajustar permissão 0600 em {}: {e}",
                socket_path.display()
            ))
        })?;

        let server = Arc::new(Self {
            socket_path,
            tokens: RwLock::new(HashMap::new()),
            pending_tokens: RwLock::new(HashMap::new()),
            tx,
            shutdown: CancellationToken::new(),
        });

        let server_loop = Arc::clone(&server);
        tokio::spawn(async move {
            server_loop.accept_loop(listener).await;
        });

        Ok(server)
    }

    /// Emite um novo token reservado para um `PendingRunBinding` antes de `PaneSpawned`.
    #[must_use]
    pub fn issue(&self, binding: PendingRunBinding) -> RelayToken {
        let (token, digest) = RelayToken::generate();
        self.pending_tokens.write().insert(
            digest,
            PendingEntry {
                binding,
                queued: VecDeque::new(),
                queued_bytes: 0,
                overflowed: false,
            },
        );
        token
    }

    /// Ativa uma reserva pendente quando `PaneSpawned` fornece o `PtyId` real, drenando eventos enfileirados.
    pub fn activate(&self, binding: RunBinding) -> MtResult<()> {
        let found_digest = {
            let pending = self.pending_tokens.read();
            pending
                .iter()
                .find(|(_, entry)| {
                    entry.binding.run_id == binding.run_id
                        && entry.binding.tab_id == binding.tab_id
                        && entry.binding.pane_id == binding.pane_id
                        && entry.binding.session_id == binding.session_id
                        && entry.binding.generation == binding.generation
                })
                .map(|(k, _)| *k)
        };

        let Some(digest) = found_digest else {
            return Err(error::relay_unavailable(
                "Reserva pendente não encontrada ou com identidade divergente ao ativar relay.",
            ));
        };

        let Some(pending_entry) = self.pending_tokens.write().remove(&digest) else {
            return Err(error::relay_unavailable(
                "Reserva pendente removida concorrentemente.",
            ));
        };

        let overflow_flag = Arc::new(AtomicBool::new(pending_entry.overflowed));
        self.tokens.write().insert(
            digest,
            ActiveEntry {
                binding: binding.clone(),
                overflowed: Arc::clone(&overflow_flag),
            },
        );

        for (msg, received_at_ms) in pending_entry.queued {
            let was_overflow = overflow_flag.swap(false, Ordering::SeqCst);
            if self
                .tx
                .try_send(AuthenticatedRelayEvent {
                    binding: binding.clone(),
                    message: msg,
                    received_at_ms,
                    overflow_reconcile: was_overflow,
                })
                .is_err()
            {
                overflow_flag.store(true, Ordering::SeqCst);
            }
        }

        Ok(())
    }

    /// Revoga imediatamente o token associado a `run_id` (ativo ou pendente).
    #[must_use]
    pub fn revoke_run(&self, run_id: &str) -> bool {
        let mut removed = false;
        {
            let mut active = self.tokens.write();
            let before = active.len();
            active.retain(|_, v| v.binding.run_id != run_id);
            if active.len() != before {
                removed = true;
            }
        }
        {
            let mut pending = self.pending_tokens.write();
            let before = pending.len();
            pending.retain(|_, v| v.binding.run_id != run_id);
            if pending.len() != before {
                removed = true;
            }
        }
        removed
    }

    /// Revoga todos os tokens ativos e pendentes pertencentes a `tab`.
    #[must_use]
    pub fn revoke_tab(&self, tab: TabId) -> usize {
        let mut count = 0;
        {
            let mut active = self.tokens.write();
            let before = active.len();
            active.retain(|_, v| v.binding.tab_id != tab);
            count += before.saturating_sub(active.len());
        }
        {
            let mut pending = self.pending_tokens.write();
            let before = pending.len();
            pending.retain(|_, v| v.binding.tab_id != tab);
            count += before.saturating_sub(pending.len());
        }
        count
    }

    /// Retorna o caminho do socket Unix em escuta.
    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Encerra o servidor de relay e remove o arquivo de socket.
    pub async fn shutdown(&self) {
        self.shutdown.cancel();
        let _ = fs::remove_file(&self.socket_path);
    }

    async fn accept_loop(self: Arc<Self>, listener: UnixListener) {
        loop {
            tokio::select! {
                () = self.shutdown.cancelled() => break,
                accepted = listener.accept() => {
                    let Ok((stream, _addr)) = accepted else {
                        break;
                    };
                    let srv = Arc::clone(&self);
                    tokio::spawn(async move {
                        srv.handle_connection(stream).await;
                    });
                }
            }
        }
    }

    async fn handle_connection(&self, mut stream: UnixStream) {
        // Verifica SO_PEERCRED (mesmo UID efetivo do usuário atual)
        let expected_uid = nix::unistd::geteuid().as_raw();
        let Ok(cred) = stream.peer_cred() else {
            return;
        };
        if cred.uid() != expected_uid {
            return;
        }

        let (read_half, mut write_half) = stream.split();
        let mut limited = read_half.take((MAX_RELAY_MESSAGE_BYTES + 1) as u64);
        let mut reader = BufReader::new(&mut limited);
        let mut line = Vec::new();

        let read_res =
            tokio::time::timeout(READ_LINE_TIMEOUT, reader.read_until(b'\n', &mut line)).await;
        let Ok(Ok(bytes_read)) = read_res else {
            return;
        };
        if bytes_read == 0 {
            return;
        }

        if line.len() > MAX_RELAY_MESSAGE_BYTES {
            let _ = write_ack(
                &mut write_half,
                &RelayAck {
                    ok: false,
                    error: Some("payload_too_large".to_string()),
                },
            )
            .await;
            return;
        }

        let Ok(envelope) = serde_json::from_slice::<RelayEnvelope>(&line) else {
            let _ = write_ack(
                &mut write_half,
                &RelayAck {
                    ok: false,
                    error: Some("invalid_json".to_string()),
                },
            )
            .await;
            return;
        };

        if envelope.v != RELAY_PROTOCOL_VERSION {
            let _ = write_ack(
                &mut write_half,
                &RelayAck {
                    ok: false,
                    error: Some("unsupported_version".to_string()),
                },
            )
            .await;
            return;
        }

        let Some(token_digest) = decode_and_hash_token(&envelope.token) else {
            let _ = write_ack(
                &mut write_half,
                &RelayAck {
                    ok: false,
                    error: None,
                },
            )
            .await;
            return;
        };

        let received_at_ms = current_unix_ms();

        // 1. Procura em tokens ativos usando comparação em tempo constante
        let active_match = {
            let guard = self.tokens.read();
            let mut matched: Option<(RunBinding, Arc<AtomicBool>)> = None;
            for (k, entry) in guard.iter() {
                if constant_time_eq_32(k, &token_digest) {
                    matched = Some((entry.binding.clone(), Arc::clone(&entry.overflowed)));
                }
            }
            matched
        };

        if let Some((binding, overflow_flag)) = active_match {
            let was_overflow = overflow_flag.swap(false, Ordering::SeqCst);
            if self
                .tx
                .try_send(AuthenticatedRelayEvent {
                    binding,
                    message: envelope.msg,
                    received_at_ms,
                    overflow_reconcile: was_overflow,
                })
                .is_err()
            {
                overflow_flag.store(true, Ordering::SeqCst);
            }
            let _ = write_ack(
                &mut write_half,
                &RelayAck {
                    ok: true,
                    error: None,
                },
            )
            .await;
            return;
        }

        // 2. Procura em reservas pendentes usando comparação em tempo constante
        let matched_pending = {
            let mut guard = self.pending_tokens.write();
            let mut found_key: Option<[u8; 32]> = None;
            for k in guard.keys() {
                if constant_time_eq_32(k, &token_digest) {
                    found_key = Some(*k);
                }
            }
            if let Some(key) = found_key
                && let Some(entry) = guard.get_mut(&key)
            {
                let msg_size = line.len();
                if entry.queued.len() < MAX_PENDING_EVENTS_PER_RUN
                    && entry.queued_bytes + msg_size <= MAX_PENDING_BYTES_PER_RUN
                {
                    entry.queued_bytes += msg_size;
                    entry.queued.push_back((envelope.msg, received_at_ms));
                } else {
                    entry.overflowed = true;
                }
                true
            } else {
                false
            }
        };

        let _ = write_ack(
            &mut write_half,
            &RelayAck {
                ok: matched_pending,
                error: None,
            },
        )
        .await;
    }
}

async fn prepare_socket_path(primary_path: &Path) -> MtResult<PathBuf> {
    let Ok(meta) = fs::symlink_metadata(primary_path) else {
        return Ok(primary_path.to_path_buf());
    };

    if !meta.file_type().is_socket() {
        return Err(error::relay_unavailable(format!(
            "O caminho {} existe e não é um socket Unix.",
            primary_path.display()
        )));
    }

    let connect_res =
        tokio::time::timeout(PROBE_CONNECT_TIMEOUT, UnixStream::connect(primary_path)).await;
    if matches!(connect_res, Ok(Ok(_))) {
        let pid = std::process::id();
        let parent = primary_path.parent().unwrap_or_else(|| Path::new("/tmp"));
        let alt_path = parent.join(format!("agent-relay-{pid}.sock"));
        if let Ok(alt_meta) = fs::symlink_metadata(&alt_path) {
            if !alt_meta.file_type().is_socket() {
                return Err(error::relay_unavailable(format!(
                    "O caminho {} existe e não é um socket Unix.",
                    alt_path.display()
                )));
            }
            let _ = fs::remove_file(&alt_path);
        }
        return Ok(alt_path);
    }

    fs::remove_file(primary_path).map_err(|e| {
        error::relay_unavailable(format!(
            "Falha ao remover socket órfão {}: {e}",
            primary_path.display()
        ))
    })?;
    Ok(primary_path.to_path_buf())
}

fn sha256_bytes(input: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn decode_and_hash_token(raw_token: &str) -> Option<[u8; 32]> {
    let trimmed = raw_token.trim();
    if trimmed.is_empty() {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| STANDARD.decode(trimmed))
        .ok()?;
    if decoded.len() != 32 {
        return None;
    }
    Some(sha256_bytes(&decoded))
}

fn constant_time_eq_32(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

async fn write_ack<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    ack: &RelayAck,
) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(ack).unwrap_or_else(|_| b"{\"ok\":false}".to_vec());
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.flush().await
}
