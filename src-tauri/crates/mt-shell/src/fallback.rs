//! Detector de fallback por grupo de processos em foreground (`/proc`) quando o shell
//! não emite marcadores `OSC 133` (ex.: `sh`/`dash`, `bash --norc`, container minimalista).
//!
//! Respeita a janela de ativação inicial de `1_500 ms` desde o spawn e o debounce de `150 ms`
//! antes de abrir um bloco sintético `ObservationSource::ProcFallback`.

use std::sync::Arc;

use mt_pty::proc::ProcSource;

/// Janela de ativação inicial em milissegundos aguardando o primeiro `OSC 133` antes de habilitar `/proc` (1500 ms).
pub const OSC_ACTIVATION_WINDOW_MS: i64 = 1_500;

/// Tempo mínimo em milissegundos em que `foreground_pgid != shell_pgid` deve persistir para abrir bloco `/proc` (150 ms).
pub const FOREGROUND_DEBOUNCE_MS: i64 = 150;

/// Transição produzida pelo [`FallbackTracker`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackAction {
    /// Janela de ativação de 1500 ms expirou sem OSC 133.
    ActivationWindowExpired,
    /// Novo comando em foreground detectado via `/proc`.
    StartCommand {
        /// PGID do grupo em foreground.
        fg_pgid: u32,
        /// Linha de comando reconstruída de `/proc/<pid>/cmdline`.
        cmdline: Option<String>,
        /// Diretório de trabalho lido de `/proc/<pid>/cwd`.
        cwd: Option<String>,
    },
    /// O controle do terminal retornou ao grupo do shell líder (`foreground_pgid == shell_pgid`).
    FinishCommand {
        /// Diretório de trabalho atual do shell líder lido de `/proc/<shell_pid>/cwd`.
        shell_cwd: Option<String>,
    },
}

/// Estado por PTY para supervisão de fallback `/proc`.
#[derive(Debug, Clone)]
pub struct FallbackTracker {
    shell_pid: u32,
    shell_pgid: u32,
    spawned_at_ms: i64,
    immediate_fallback: bool,
    osc_active: bool,
    activation_expired_emitted: bool,
    candidate_fg_pgid: Option<(u32, i64)>,
    active_fg_pgid: Option<u32>,
}

impl FallbackTracker {
    /// Cria um novo [`FallbackTracker`] para um PTY recém-iniciado.
    #[must_use]
    #[allow(clippy::similar_names)]
    pub const fn new(
        shell_pid: u32,
        shell_pgid: u32,
        spawned_at_ms: i64,
        immediate_fallback: bool,
    ) -> Self {
        Self {
            shell_pid,
            shell_pgid,
            spawned_at_ms,
            immediate_fallback,
            osc_active: false,
            activation_expired_emitted: immediate_fallback,
            candidate_fg_pgid: None,
            active_fg_pgid: None,
        }
    }

    /// Desativa imediatamente o fallback `/proc` assim que o primeiro marcador `OSC 133` válido é recebido.
    pub fn notify_osc_received(&mut self) {
        self.osc_active = true;
        self.candidate_fg_pgid = None;
        self.active_fg_pgid = None;
    }

    /// Indica se o fallback `/proc` já está autorizado a operar (seja por `immediate_fallback` ou por expiração dos 1500 ms).
    #[must_use]
    pub const fn is_fallback_allowed(&self, now_ms: i64) -> bool {
        if self.osc_active {
            return false;
        }
        self.immediate_fallback || (now_ms - self.spawned_at_ms) >= OSC_ACTIVATION_WINDOW_MS
    }

    /// Registra uma mudança de `foreground_pgid` reportada pelo supervisor de PTY (`PtyEvent::ForegroundChanged`).
    pub fn on_foreground_changed(
        &mut self,
        fg_pgid: Option<u32>,
        proc_source: &Arc<dyn ProcSource>,
        now_ms: i64,
    ) -> Vec<FallbackAction> {
        if self.osc_active {
            return Vec::new();
        }

        let mut actions = Vec::new();
        if !self.activation_expired_emitted
            && (now_ms - self.spawned_at_ms) >= OSC_ACTIVATION_WINDOW_MS
        {
            self.activation_expired_emitted = true;
            actions.push(FallbackAction::ActivationWindowExpired);
        }

        let Some(pgid) = fg_pgid else {
            return actions;
        };

        if pgid == self.shell_pgid || pgid == 0 {
            self.candidate_fg_pgid = None;
            if self.active_fg_pgid.take().is_some() {
                let shell_cwd = read_pid_cwd(proc_source, self.shell_pid);
                actions.push(FallbackAction::FinishCommand { shell_cwd });
            }
            return actions;
        }

        // Foreground pertence a um grupo filho diferente do shell líder
        if self.active_fg_pgid == Some(pgid) {
            return actions;
        }

        match self.candidate_fg_pgid {
            Some((cand_pgid, since_ms)) if cand_pgid == pgid => {
                if self.is_fallback_allowed(now_ms) && (now_ms - since_ms) >= FOREGROUND_DEBOUNCE_MS
                {
                    self.promote_candidate_to_active(pgid, proc_source, &mut actions);
                }
            }
            _ => {
                self.candidate_fg_pgid = Some((pgid, now_ms));
            }
        }

        actions
    }

    /// Avalia periodicamente (tick de 100..250 ms) se a janela de 1500 ms expirou ou se o candidato
    /// em foreground atingiu o debounce de 150 ms.
    pub fn poll_tick(
        &mut self,
        current_fg_pgid: Option<u32>,
        proc_source: &Arc<dyn ProcSource>,
        now_ms: i64,
    ) -> Vec<FallbackAction> {
        if self.osc_active {
            return Vec::new();
        }

        let mut actions = Vec::new();
        if !self.activation_expired_emitted
            && (now_ms - self.spawned_at_ms) >= OSC_ACTIVATION_WINDOW_MS
        {
            self.activation_expired_emitted = true;
            actions.push(FallbackAction::ActivationWindowExpired);
        }

        if let Some(fg) = current_fg_pgid {
            let mut fg_actions = self.on_foreground_changed(Some(fg), proc_source, now_ms);
            actions.append(&mut fg_actions);
        } else if let Some((cand_pgid, since_ms)) = self.candidate_fg_pgid
            && self.is_fallback_allowed(now_ms)
            && (now_ms - since_ms) >= FOREGROUND_DEBOUNCE_MS
        {
            self.promote_candidate_to_active(cand_pgid, proc_source, &mut actions);
        }

        actions
    }

    fn promote_candidate_to_active(
        &mut self,
        pgid: u32,
        proc_source: &Arc<dyn ProcSource>,
        actions: &mut Vec<FallbackAction>,
    ) {
        if self.active_fg_pgid.take().is_some() {
            let shell_cwd = read_pid_cwd(proc_source, self.shell_pid);
            actions.push(FallbackAction::FinishCommand { shell_cwd });
        }

        self.candidate_fg_pgid = None;
        self.active_fg_pgid = Some(pgid);

        let (cmdline, cwd) = inspect_foreground_group(proc_source, pgid, self.shell_pid);
        actions.push(FallbackAction::StartCommand {
            fg_pgid: pgid,
            cmdline,
            cwd,
        });
    }
}

fn inspect_foreground_group(
    proc_source: &Arc<dyn ProcSource>,
    fg_pgid: u32,
    shell_pid: u32,
) -> (Option<String>, Option<String>) {
    if fg_pgid == 0 {
        return (None, None);
    }
    let leader_pid = fg_pgid;
    if let Ok(Some(info)) = proc_source.process(leader_pid) {
        let cmd = format_cmdline(&info.cmdline, &info.comm);
        let cwd = info
            .cwd
            .clone()
            .or_else(|| read_pid_cwd(proc_source, shell_pid));
        return (cmd, cwd);
    }

    // Se o líder do PGID já encerrou ou não é diretamente o PID, procura um membro vivo do grupo
    if let Ok(all_pids) = proc_source.all_pids() {
        for pid in all_pids {
            if let Ok(Some(info)) = proc_source.process(pid)
                && info.pgid == fg_pgid
            {
                let cmd = format_cmdline(&info.cmdline, &info.comm);
                let cwd = info
                    .cwd
                    .clone()
                    .or_else(|| read_pid_cwd(proc_source, shell_pid));
                return (cmd, cwd);
            }
        }
    }

    (None, read_pid_cwd(proc_source, shell_pid))
}

fn read_pid_cwd(proc_source: &Arc<dyn ProcSource>, pid: u32) -> Option<String> {
    if let Ok(Some(info)) = proc_source.process(pid)
        && let Some(cwd) = info.cwd
    {
        return Some(cwd);
    }
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

fn format_cmdline(cmdline: &[String], fallback_name: &str) -> Option<String> {
    if !cmdline.is_empty() {
        let joined = cmdline
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !joined.is_empty() {
            return Some(joined);
        }
    }
    let trimmed = fallback_name.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use mt_pty::ProcState;
    use mt_pty::proc::FakeProcSource;
    use mt_pty::types::{ProcessInfo, U64Decimal};

    use super::*;

    #[test]
    fn respects_activation_window_and_debounce_before_starting_fallback_block() {
        let fake = Arc::new(FakeProcSource::new("boot-1"));
        fake.insert(ProcessInfo {
            pid: 100,
            ppid: 1,
            pgid: 100,
            sid: 100,
            tpgid: 200,
            comm: "sh".to_string(),
            cmdline: vec!["/bin/sh".to_string()],
            exe: Some("/bin/sh".to_string()),
            cwd: Some("/home/user".to_string()),
            state: ProcState::Sleeping,
            start_time: U64Decimal(10),
            rss_bytes: Some(U64Decimal(1024)),
            threads: 1,
        });
        fake.insert(ProcessInfo {
            pid: 200,
            ppid: 100,
            pgid: 200,
            sid: 100,
            tpgid: 200,
            comm: "sleep".to_string(),
            cmdline: vec!["sleep".to_string(), "5".to_string()],
            exe: Some("/bin/sleep".to_string()),
            cwd: Some("/home/user/sub".to_string()),
            state: ProcState::Running,
            start_time: U64Decimal(20),
            rss_bytes: Some(U64Decimal(1024)),
            threads: 1,
        });

        let proc_src: Arc<dyn ProcSource> = fake;
        let mut fb = FallbackTracker::new(100, 100, 1_000, true);

        // Primeiro evento registra candidato no instante t=1000
        let a1 = fb.on_foreground_changed(Some(200), &proc_src, 1_000);
        assert_eq!(a1, Vec::<FallbackAction>::new());

        // Após 160 ms (>= 150 ms), promove e extrai cmdline + cwd
        let a2 = fb.poll_tick(Some(200), &proc_src, 1_160);
        assert_eq!(
            a2,
            vec![FallbackAction::StartCommand {
                fg_pgid: 200,
                cmdline: Some("sleep 5".to_string()),
                cwd: Some("/home/user/sub".to_string()),
            }]
        );

        // Retorno do foreground ao shell (pgid 100) encerra o comando
        let a3 = fb.on_foreground_changed(Some(100), &proc_src, 1_500);
        assert_eq!(
            a3,
            vec![FallbackAction::FinishCommand {
                shell_cwd: Some("/home/user".to_string()),
            }]
        );
    }
}
