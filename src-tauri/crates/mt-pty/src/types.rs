//! Tipos de domínio e DTOs serializáveis do crate `mt-pty`.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use mt_core::ids::{PaneId, PtyId, TabId};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Representação de inteiro de 64 bits sem sinal serializado como string decimal para IPC seguro com JS/TS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, specta::Type)]
#[specta(transparent)]
pub struct U64Decimal(#[specta(type = String)] pub u64);

impl From<u64> for U64Decimal {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<U64Decimal> for u64 {
    fn from(value: U64Decimal) -> Self {
        value.0
    }
}

impl fmt::Display for U64Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for U64Decimal {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u64>().map(Self)
    }
}

impl Serialize for U64Decimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for U64Decimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse::<u64>().map(Self).map_err(serde::de::Error::custom)
    }
}

/// Identificador opaco de uma geração de conexão (`attach`) de output do PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
#[specta(transparent)]
pub struct AttachmentId(#[specta(type = String)] pub uuid::Uuid);

impl AttachmentId {
    /// Cria um novo identificador de conexão baseado em UUID v7.
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }
}

impl Default for AttachmentId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AttachmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for AttachmentId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s).map(Self)
    }
}

/// Dimensões da janela do pseudo-terminal em células e pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySize {
    /// Quantidade de linhas (`1..=1000`).
    pub rows: u16,
    /// Quantidade de colunas (`1..=1000`).
    pub cols: u16,
    /// Largura em pixels (`0` se desconhecida).
    pub pixel_width: u16,
    /// Altura em pixels (`0` se desconhecida).
    pub pixel_height: u16,
}

impl Default for PtySize {
    fn default() -> Self {
        Self {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

impl PtySize {
    /// Valida que `rows` e `cols` estão no intervalo `1..=1000`.
    pub const fn validate(&self) -> Result<(), crate::error::PtyError> {
        if self.rows == 0 || self.rows > 1000 || self.cols == 0 || self.cols > 1000 {
            return Err(crate::error::PtyError::ResizeInvalid {
                rows: self.rows,
                cols: self.cols,
            });
        }
        Ok(())
    }
}

/// Dono opaco do PTY (preenchido pela Fase 07).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyOwner {
    /// Identificador opcional da aba proprietária.
    pub tab_id: Option<TabId>,
    /// Identificador opcional do painel proprietário.
    pub pane_id: Option<PaneId>,
}

/// Especificação para criação de um novo PTY.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpawnSpec {
    /// Caminho absoluto ou nome de executável resolvido no `PATH`.
    pub shell: Option<String>,
    /// Argumentos passados ao executável.
    pub args: Vec<String>,
    /// Define se deve iniciar como login shell quando `args` estiver vazio.
    pub login: bool,
    /// Diretório de trabalho inicial (`None` usa `$HOME`).
    pub cwd: Option<String>,
    /// Variáveis de ambiente adicionais que sobrepõem o ambiente herdado.
    pub env: BTreeMap<String, String>,
    /// Chaves de variáveis de ambiente a remover do ambiente herdado.
    pub env_remove: Vec<String>,
    /// Dimensões iniciais da grade do terminal.
    pub size: PtySize,
    /// Vínculo opcional de aba e painel.
    pub owner: PtyOwner,
}

impl Default for SpawnSpec {
    fn default() -> Self {
        Self {
            shell: None,
            args: Vec::new(),
            login: true,
            cwd: None,
            env: BTreeMap::new(),
            env_remove: Vec::new(),
            size: PtySize::default(),
            owner: PtyOwner::default(),
        }
    }
}

/// Razão de encerramento do processo principal do PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ExitReason {
    /// Processo terminou normalmente com código de saída.
    Exited,
    /// Processo terminou devido a sinal não solicitado pelo usuário.
    Signaled,
    /// Encerrado explicitamente por solicitação do usuário (`pty_kill`).
    KilledByUser,
    /// Encerrado durante o desligamento coordenado do aplicativo.
    Shutdown,
    /// Encerrado devido a falha irrecuperável na leitura do master PTY.
    ReaderFailed,
}

/// Informações detalhadas sobre o término do PTY.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExitInfo {
    /// Código de saída (`None` quando terminado por sinal).
    pub code: Option<i32>,
    /// Número do sinal POSIX se reconhecido (`ex.: 9`).
    pub signal: Option<i32>,
    /// Nome textual do sinal (`ex.: "SIGKILL"`).
    pub signal_name: Option<String>,
    /// Motivo semântico do encerramento.
    pub reason: ExitReason,
    /// Timestamp em milissegundos desde a época Unix.
    pub at_ms: U64Decimal,
}

/// Estado do ciclo de vida do PTY.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PtyStatus {
    /// Processo em execução normal.
    Running,
    /// Em processo de encerramento escalonado.
    #[serde(rename_all = "camelCase")]
    Exiting {
        /// Timestamp em milissegundos do início do encerramento.
        since_ms: U64Decimal,
    },
    /// Falha ao encerrar todos os processos do cgroup dentro do deadline.
    #[serde(rename_all = "camelCase")]
    CleanupFailed {
        /// Descrição do erro de limpeza.
        error: String,
    },
    /// Processo e descendentes completamente finalizados (tombstone).
    #[serde(rename_all = "camelCase")]
    Exited {
        /// Dados do encerramento.
        exit: ExitInfo,
    },
}

/// Modo de encerramento solicitado em `pty_kill`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum KillMode {
    /// Escalonamento gracioso: `SIGHUP → SIGTERM → cgroup.kill`.
    #[default]
    Graceful,
    /// Encerramento imediato via `cgroup.kill` (`SIGKILL`).
    Force,
}

/// Identidade única e anti-TOCTOU de um processo Linux.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProcessIdentity {
    /// Identificador de boot do kernel (`/proc/sys/kernel/random/boot_id`).
    pub boot_id: String,
    /// PID numérico do processo.
    pub pid: u32,
    /// Ticks de clock desde o boot no início do processo (campo 22 de `/proc/<pid>/stat`).
    pub start_time: U64Decimal,
}

/// Identificadores de agrupamento POSIX de processos do PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyPids {
    /// PID do processo principal (shell).
    pub shell_pid: u32,
    /// Process Group ID do shell.
    pub pgid: u32,
    /// Session ID do terminal.
    pub sid: u32,
    /// Process Group ID atualmente em foreground (`tcgetpgrp`).
    pub foreground_pgid: Option<u32>,
}

/// Fotografia resumida do estado e contadores de um PTY.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySummary {
    /// Identificador único do PTY.
    pub id: PtyId,
    /// Vínculo de aba/painel.
    pub owner: PtyOwner,
    /// Caminho resolvido do executável do shell.
    pub shell: String,
    /// Argumentos efetivos passados ao shell.
    pub args: Vec<String>,
    /// Diretório de trabalho inicial canonicalizado.
    pub cwd_initial: String,
    /// Dimensões atuais da grade.
    pub size: PtySize,
    /// Status atual do ciclo de vida.
    pub status: PtyStatus,
    /// Identificadores POSIX do processo e grupo em foreground.
    pub pids: PtyPids,
    /// Indica se há um consumidor (`OutputSink`) anexado.
    pub attached: bool,
    /// Timestamp de criação em milissegundos desde a época Unix.
    pub created_at_ms: U64Decimal,
    /// Total acumulado de bytes produzidos pelo PTY.
    pub bytes_out: U64Decimal,
    /// Último número de sequência de frame `Data` produzido (`0` se nenhum).
    pub seq: U64Decimal,
    /// Revisão monotônica de alteração para reconciliação entre lista e eventos.
    pub revision: U64Decimal,
}

/// Estado do processo segundo `/proc/<pid>/stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ProcState {
    /// Em execução ou executável na fila (`R`).
    Running,
    /// Espera interruptível (`S`).
    Sleeping,
    /// Espera ininterruptível de I/O de disco/kernel (`D`).
    DiskSleep,
    /// Parado por sinal de controle de job (`T`).
    Stopped,
    /// Parado por depurador/ptrace (`t`).
    TracingStop,
    /// Processo zumbi aguardando `waitpid` (`Z`).
    Zombie,
    /// Processo morto (`X` / `x`).
    Dead,
    /// Thread ociosa do kernel (`I`).
    Idle,
    /// Estado desconhecido.
    Unknown,
}

/// Metadados detalhados de um processo lidos de `/proc/<pid>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    /// PID do processo.
    pub pid: u32,
    /// PID do processo pai (`ppid`).
    pub ppid: u32,
    /// Process Group ID (`pgrp`).
    pub pgid: u32,
    /// Session ID (`session`).
    pub sid: u32,
    /// Foreground PGID do terminal controlador (`tpgid`).
    pub tpgid: i32,
    /// Nome curto do comando (`comm`).
    pub comm: String,
    /// Vetor de argumentos da linha de comando (`cmdline`).
    pub cmdline: Vec<String>,
    /// Caminho resolvido do executável (`exe`), se acessível.
    pub exe: Option<String>,
    /// Diretório de trabalho atual (`cwd`), se acessível.
    pub cwd: Option<String>,
    /// Estado atual no escalonador do kernel.
    pub state: ProcState,
    /// Ticks de início desde o boot (`starttime`).
    pub start_time: U64Decimal,
    /// Memória residente (`RSS`) em bytes.
    pub rss_bytes: Option<U64Decimal>,
    /// Quantidade de threads do processo.
    pub threads: u32,
}

/// Nó da árvore hierárquica de processos (`ppid -> filhos`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProcessNode {
    /// Informações do processo deste nó.
    pub info: ProcessInfo,
    /// Processos filhos diretos ordenados por PID.
    pub children: Vec<ProcessNode>,
}

/// Relatório consolidado de supervisão de processos de um PTY.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyProcessReport {
    /// Identificador do PTY inspecionado.
    pub pty_id: PtyId,
    /// Agrupamento POSIX atual.
    pub pids: PtyPids,
    /// Informações do processo principal do shell.
    pub shell: Option<ProcessInfo>,
    /// Processo líder (ou mais recente) do grupo em foreground.
    pub foreground: Option<ProcessInfo>,
    /// Árvore hierárquica enraizada no shell.
    pub tree: Option<ProcessNode>,
    /// Todos os processos pertencentes ao cgroup do PTY (incluindo daemons reparentados).
    pub owned_processes: Vec<ProcessInfo>,
    /// Indica se algum membro não pôde ser lido por restrição de permissão.
    pub partial: bool,
    /// Total de processos descendentes/adicionais além do shell.
    pub descendant_count: u32,
    /// Timestamp da coleta em milissegundos desde a época Unix.
    pub sampled_at_ms: U64Decimal,
}

/// Cursor de posição no stream de saída do PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OutputCursor {
    /// Próximo número de sequência de frame esperado (`>= 1`).
    pub next_seq: U64Decimal,
    /// Offset acumulado de bytes já consumidos antes de `next_seq`.
    pub byte_offset: U64Decimal,
}

/// Metadados retornados ao anexar um consumidor ao stream de saída do PTY.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachInfo {
    /// Identificador exclusivo desta geração de conexão.
    pub attachment_id: AttachmentId,
    /// Cursor a partir do qual o replay se inicia.
    pub replay_from: OutputCursor,
    /// Cursor a partir do qual os dados ao vivo começam após o replay.
    pub live_from: OutputCursor,
    /// Indica se houve truncamento de bytes antigos (`Gap`).
    pub truncated: bool,
}

/// Relatório do desligamento coordenado de todos os PTYs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownReport {
    /// PTYs encerrados e limpos com sucesso (`populated == 0`).
    pub completed: Vec<PtyId>,
    /// PTYs que falharam na limpeza dentro do deadline.
    pub pending: Vec<PtyId>,
}

/// Registro interno de processo rastreado por PTY.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedProcess {
    /// Identificador do PTY proprietário.
    pub pty_id: PtyId,
    /// Identidade única do processo.
    pub identity: ProcessIdentity,
    /// Process Group ID inicial.
    pub pgid: u32,
    /// Session ID inicial.
    pub sid: u32,
}

/// Entrada tipada para escrita no PTY via IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PtyInput {
    /// Entrada textual UTF-8.
    #[serde(rename_all = "camelCase")]
    Text {
        /// Texto UTF-8 a enviar ao terminal.
        text: String,
    },
    /// Entrada de bytes brutos (`0..=255`).
    #[serde(rename_all = "camelCase")]
    Bytes {
        /// Vetor de bytes brutos.
        bytes: Vec<u8>,
    },
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn u64_decimal_serializes_as_decimal_string() {
        let val = U64Decimal::from(u64::MAX);
        let json = serde_json::to_string(&val).unwrap();
        assert_eq!(json, format!("\"{}\"", u64::MAX));
        let back: U64Decimal = serde_json::from_str(&json).unwrap();
        assert_eq!(back, val);
    }

    #[test]
    fn validates_pty_size_bounds() {
        assert!(
            PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0
            }
            .validate()
            .is_ok()
        );
        assert!(
            PtySize {
                rows: 1,
                cols: 1000,
                pixel_width: 800,
                pixel_height: 600
            }
            .validate()
            .is_ok()
        );

        let err_zero_rows = PtySize {
            rows: 0,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
        .validate()
        .unwrap_err();
        assert_eq!(err_zero_rows.code(), "pty.resize_invalid");

        let err_zero_cols = PtySize {
            rows: 24,
            cols: 0,
            pixel_width: 0,
            pixel_height: 0,
        }
        .validate()
        .unwrap_err();
        assert_eq!(err_zero_cols.code(), "pty.resize_invalid");

        let err_huge_cols = PtySize {
            rows: 24,
            cols: 1001,
            pixel_width: 0,
            pixel_height: 0,
        }
        .validate()
        .unwrap_err();
        assert_eq!(err_huge_cols.code(), "pty.resize_invalid");

        let err_huge_rows = PtySize {
            rows: 1001,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
        .validate()
        .unwrap_err();
        assert_eq!(err_huge_rows.code(), "pty.resize_invalid");
    }
}
