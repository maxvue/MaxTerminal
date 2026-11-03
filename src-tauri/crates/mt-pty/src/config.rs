//! Configuração operacional do gerenciador de PTYs e supervisão de processos.

use std::path::PathBuf;
use std::time::Duration;

/// Configuração operacional do gerenciador de PTYs e supervisão de processos.
#[derive(Debug, Clone)]
pub struct PtyConfig {
    /// Limite máximo de instâncias de PTY simultâneas.
    pub max_ptys: usize,
    /// Tamanho do buffer de leitura por syscall `read(2)` (padrão: 64 KiB).
    pub read_buffer_size: usize,
    /// Janela temporal de coalescing de saída (padrão: 4 ms).
    pub coalesce_window: Duration,
    /// Tamanho máximo de payload de um frame de dados/replay (padrão: 256 KiB).
    pub max_frame_bytes: usize,
    /// Marca d'água alta de bytes não confirmados para pausar leitura (padrão: 1 MiB).
    pub high_watermark: usize,
    /// Marca d'água baixa de bytes não confirmados para retomar leitura (padrão: 256 KiB).
    pub low_watermark: usize,
    /// Tempo máximo sem progresso de ACK quando há bytes pendentes (padrão: 5 s).
    pub ack_timeout: Duration,
    /// Capacidade em bytes do ring buffer de replay por PTY (padrão: 1 MiB).
    pub replay_capacity: usize,
    /// Capacidade máxima de mensagens pendentes na fila de escrita (padrão: 1024).
    pub write_queue_capacity: usize,
    /// Orçamento máximo de bytes na fila de escrita por PTY (padrão: 4 MiB).
    pub write_queue_bytes: usize,
    /// Tamanho máximo de um único pedido de escrita (padrão: 1 MiB).
    pub max_write_bytes: usize,
    /// Orçamento global de buffers entre todos os PTYs (padrão: 256 MiB).
    pub global_buffer_budget: usize,
    /// Tamanho do bloco fatiado de escrita no master FD (padrão: 4 KiB).
    pub write_chunk_bytes: usize,
    /// Tempo de espera após `SIGHUP` antes de escalar para `SIGTERM` (padrão: 1 s).
    pub hup_grace: Duration,
    /// Tempo de espera após `SIGTERM` antes de escalar para `SIGKILL`/`cgroup.kill` (padrão: 2 s).
    pub term_grace: Duration,
    /// Tempo de espera após `SIGHUP` durante shutdown do aplicativo (padrão: 300 ms).
    pub shutdown_hup_grace: Duration,
    /// Tempo de espera após `SIGTERM` durante shutdown do aplicativo (padrão: 700 ms).
    pub shutdown_term_grace: Duration,
    /// Intervalo do loop periódico do supervisor (padrão: 1 s).
    pub supervisor_interval: Duration,
    /// Tempo de retenção de tombstones de PTYs encerrados (padrão: 60 s).
    pub tombstone_ttl: Duration,
    /// Tamanho da stack das threads trabalhadoras de PTY (padrão: 256 KiB).
    pub thread_stack_size: usize,
    /// Diretório de runtime da instância (`$XDG_RUNTIME_DIR/maxterminal/<instance_id>`).
    pub registry_dir: PathBuf,
    /// Caminho opcional para sobrescrever a raiz do subtree cgroup v2 (usado em testes).
    pub cgroup_root_override: Option<PathBuf>,
    /// Caminho opcional para sobrescrever o binário auxiliar `mt-pty-helper` (usado em testes).
    pub helper_bin_override: Option<PathBuf>,
    /// Define se deve varrer e limpar instâncias órfãs mortas na inicialização.
    pub orphan_sweep_on_startup: bool,
}

impl Default for PtyConfig {
    fn default() -> Self {
        let runtime_base = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(
            || {
                let uid = nix::unistd::getuid().as_raw();
                PathBuf::from(format!("/run/user/{uid}"))
            },
            PathBuf::from,
        );
        let instance_id = uuid::Uuid::now_v7().to_string();
        Self {
            max_ptys: 256,
            read_buffer_size: 64 * 1024,
            coalesce_window: Duration::from_millis(4),
            max_frame_bytes: 256 * 1024,
            high_watermark: 1024 * 1024,
            low_watermark: 256 * 1024,
            ack_timeout: Duration::from_secs(5),
            replay_capacity: 1024 * 1024,
            write_queue_capacity: 1024,
            write_queue_bytes: 4 * 1024 * 1024,
            max_write_bytes: 1024 * 1024,
            global_buffer_budget: 256 * 1024 * 1024,
            write_chunk_bytes: 4 * 1024,
            hup_grace: Duration::from_secs(1),
            term_grace: Duration::from_secs(2),
            shutdown_hup_grace: Duration::from_millis(300),
            shutdown_term_grace: Duration::from_millis(700),
            supervisor_interval: Duration::from_secs(1),
            tombstone_ttl: Duration::from_secs(60),
            thread_stack_size: 256 * 1024,
            registry_dir: runtime_base.join("maxterminal").join(instance_id),
            cgroup_root_override: None,
            helper_bin_override: None,
            orphan_sweep_on_startup: true,
        }
    }
}
