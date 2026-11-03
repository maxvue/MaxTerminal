//! Tipos de domínio e DTOs públicos do detector de portas ativas (`mt-ports` — Fase 08).

use mt_core::ids::{PaneId, PtyId, TabId};
use serde::{Deserialize, Serialize};

/// Família de endereço IP do socket em escuta.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum IpFamily {
    /// IPv4 (`/proc/net/tcp`).
    Ipv4,
    /// IPv6 (`/proc/net/tcp6`).
    Ipv6,
}

/// Protocolo de transporte da porta monitorada.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum PortProtocol {
    /// TCP em estado `LISTEN` (`0x0A`).
    Tcp,
}

/// Identificador estável de uma porta lógica (`<pane_id>:tcp:<port>`).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
#[specta(transparent)]
pub struct PortId(pub String);

impl PortId {
    /// Constrói o [`PortId`] canônico para um par `(pane_id, port)`.
    #[must_use]
    pub fn for_pane_tcp(pane_id: PaneId, port: u16) -> Self {
        Self(format!("{pane_id}:tcp:{port}"))
    }

    /// Retorna a representação textual do identificador.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Endereço de bind individual associado a uma porta lógica.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortAddress {
    /// Família IP (`ipv4` ou `ipv6`).
    pub family: IpFamily,
    /// Endereço textual formatado (`"127.0.0.1"`, `"0.0.0.0"`, `"::1"`, `"::"`).
    pub bind_addr: String,
    /// `true` quando o socket escuta em todas as interfaces (`0.0.0.0` ou `::`).
    pub is_wildcard: bool,
    /// `true` quando o socket está restrito ao loopback (`127.0.0.0/8`, `::1`, `::ffff:127.0.0.1`).
    pub is_loopback: bool,
}

/// Porta TCP em estado `LISTEN` atribuída à árvore de processos de um painel do `MaxTerminal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ListeningPort {
    /// Identificador estável `<pane_id>:tcp:<port>`.
    pub id: PortId,
    /// Aba proprietária do painel.
    pub tab_id: TabId,
    /// Painel proprietário do PTY.
    pub pane_id: PaneId,
    /// PTY em cuja árvore de processos o socket foi aberto.
    pub pty_id: PtyId,
    /// Número da porta TCP (`1..=65535`).
    pub port: u16,
    /// Protocolo (`tcp`).
    pub protocol: PortProtocol,
    /// Endereços de bind consolidados (IPv4 e/ou IPv6 dual-stack).
    pub addresses: Vec<PortAddress>,
    /// URL segura de abertura no navegador (`http://127.0.0.1:<port>/`).
    pub browser_url: String,
    /// PID do processo proprietário direto do socket.
    pub pid: u32,
    /// `starttime` em clock ticks de `/proc/<pid>/stat` (campo 22) para prevenção de PID reuse.
    pub start_time_ticks: u64,
    /// Nome curto do executável (`/proc/<pid>/comm`, até 64 chars).
    pub process_name: String,
    /// Prévia sanitizada da linha de comando (`/proc/<pid>/cmdline`, até 160 chars).
    pub cmdline_preview: Option<String>,
    /// Timestamp UNIX em milissegundos em que a porta foi confirmada pela primeira vez.
    pub detected_at_ms: i64,
}

/// Desfecho do encerramento seguro do processo proprietário de uma porta (`ports_kill`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum KillOutcome {
    /// Processo encerrou graciosamente após `SIGTERM`.
    GracefulTerm,
    /// Processo precisou de `SIGKILL` após expirar a janela de `SIGTERM`.
    ForcedKill,
    /// Processo já havia encerrado antes ou durante a operação.
    AlreadyExited,
}

/// Resultado retornado por `ports_kill`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortKillResult {
    /// Painel onde a porta estava aberta.
    pub pane_id: PaneId,
    /// Porta TCP encerrada.
    pub port: u16,
    /// PID do processo encerrado.
    pub pid: u32,
    /// Desfecho do encerramento.
    pub outcome: KillOutcome,
    /// Duração total em milissegundos até a confirmação da morte do processo.
    pub duration_ms: u64,
}

/// Cadência adaptativa de polling de `/proc/net/tcp{,6}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PollMode {
    /// Janela visível (`1500 ms`).
    #[default]
    Active,
    /// Janela oculta ou minimizada para a bandeja (`5000 ms`).
    Background,
    /// Polling periódico pausado (apenas consultas manuais).
    Paused,
}

/// Fotografia consolidada das portas ativas de uma aba (`ports_list_for_tab`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortSnapshot {
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Lista de portas ordenadas por `(port ASC, pane_id ASC)`.
    pub ports: Vec<ListeningPort>,
    /// Revisão monotônica global do detector de portas.
    pub revision: u64,
}

/// Evento emitido quando uma nova porta passa pela histerese de abertura (`ports://opened`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortOpened {
    /// Aba proprietária.
    pub tab_id: TabId,
    /// Painel proprietário.
    pub pane_id: PaneId,
    /// Dados completos da porta aberta.
    pub port: ListeningPort,
    /// Revisão monotônica.
    pub revision: u64,
}

/// Evento emitido quando uma porta aberta muda de PID ou de endereços de bind (`ports://updated`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortUpdated {
    /// Aba proprietária.
    pub tab_id: TabId,
    /// Painel proprietário.
    pub pane_id: PaneId,
    /// Dados atualizados da porta.
    pub port: ListeningPort,
    /// Revisão monotônica.
    pub revision: u64,
}

/// Evento emitido quando uma porta deixa de estar em `LISTEN` (`ports://closed`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortClosed {
    /// Aba proprietária.
    pub tab_id: TabId,
    /// Painel proprietário.
    pub pane_id: PaneId,
    /// Identificador estável `<pane_id>:tcp:<port>`.
    pub id: PortId,
    /// Número da porta encerrada.
    pub port: u16,
    /// Revisão monotônica.
    pub revision: u64,
}
