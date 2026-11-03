//! Rastreador semântico de ciclos `PromptStart (A) -> PromptEnd (B) -> CommandExecuted (C) -> CommandFinished (D)`
//! por PTY (`CommandTracker`).
//!
//! Valida nonces de sessão, reconcilia sequências incompletas ou implícitas (`C` sem `A`/`B`,
//! novo `A` ou `C` sem `D`), alimenta [`OutputCapture`] nos segmentos de saída correspondentes
//! e classifica códigos de saída e sinais POSIX.

use mt_core::ids::CommandBlockId;
use mt_core::shell::{
    BlockEndReason, BlockStatus, CommandFinished, CommandStarted, CommandTextQuality,
    ContinuousPolicy, ExecutionRef, IntegrationMode, ObservationConfidence, ObservationSource,
    OutputKind, PersistenceState, RuntimeBinding, ShellKind, StreamPosition,
};
use mt_db::sanitizer::SanitizedText;

use crate::capture::{FinalizedOutput, OutputCapture};
use crate::osc::{OscKind, RawOscEvent, ScanOutput, ScanSegment, normalize_and_validate_cwd};
use crate::state::daemon::is_continuous_by_policy_or_command;

/// Fase atual do prompt/execução observada no shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptPhase {
    /// Ainda não observou nenhum marcador de prompt ou comando.
    #[default]
    Unknown,
    /// Dentro da renderização do prompt (`A` recebido).
    InPrompt,
    /// Prompt concluído, aguardando digitação do usuário (`B` recebido).
    AwaitingInput,
    /// Comando em execução (`C` recebido ou iniciado via fallback `/proc`).
    Executing,
}

/// Bloco de comando atualmente aberto no [`CommandTracker`].
#[derive(Debug, Clone)]
pub struct OpenCommandBlock {
    /// Identificador único do bloco (UUID v7).
    pub id: CommandBlockId,
    /// Sequência monotônica do comando na sessão (`1..=N`).
    pub command_seq: u32,
    /// Geração opcional do hook (`g=<n>`).
    pub hook_generation: Option<u64>,
    /// Texto sanitizado do comando, quando disponível.
    pub command_text: Option<SanitizedText>,
    /// Qualidade da captura do comando.
    pub command_text_quality: CommandTextQuality,
    /// Diretório de trabalho sanitizado no início do comando.
    pub cwd: Option<SanitizedText>,
    /// Timestamp UNIX em ms do início.
    pub started_at: i64,
    /// Posição no stream onde o bloco iniciou (`A` ou `C`).
    pub start_pos: StreamPosition,
    /// Posição no stream onde a saída do comando iniciou (`C`).
    pub output_start_pos: Option<StreamPosition>,
    /// Fonte de observação (`Osc133`, `Osc633`, `ProcFallback`).
    pub source: ObservationSource,
    /// Confiança semântica (`Exact` ou `Degraded`).
    pub confidence: ObservationConfidence,
    /// Indica se o comando foi classificado como contínuo (`Continuous`).
    pub is_continuous: bool,
    /// Captura incremental de saída sanitizada do comando.
    pub capture: OutputCapture,
}

/// Evento de domínio produzido pelo [`CommandTracker`] ao processar chunks ou transições.
#[derive(Debug, Clone)]
pub enum TrackerEvent {
    /// Modo de integração alterado (ex.: `Pending -> Osc133`).
    IntegrationModeChanged {
        /// Novo modo.
        mode: IntegrationMode,
        /// Motivo da transição.
        reason: String,
    },
    /// Diretório de trabalho (`cwd`) atualizado.
    CwdChanged {
        /// Caminho sanitizado.
        cwd: SanitizedText,
        /// Timestamp UNIX em ms.
        at_ms: i64,
    },
    /// Novo bloco de comando iniciado.
    BlockStarted {
        /// Payload público do evento `shell://command-started`.
        started: CommandStarted,
        /// Cópia higienizada de `command_text` para persistência no SQLite.
        sanitized_command: Option<SanitizedText>,
        /// Cópia higienizada de `cwd` para persistência no SQLite.
        sanitized_cwd: Option<SanitizedText>,
        /// Indica se nasceu como contínuo.
        is_continuous: bool,
    },
    /// Bloco de comando finalizado.
    BlockFinished {
        /// Payload público do evento `shell://command-finished`.
        finished: CommandFinished,
        /// Saída finalizada e sanitizada para persistência no SQLite.
        output: FinalizedOutput,
        /// Cópia higienizada de `command_text`.
        sanitized_command: Option<SanitizedText>,
        /// Cópia higienizada de `cwd`.
        sanitized_cwd: Option<SanitizedText>,
        /// Indica se atingiu classificação contínua.
        is_continuous: bool,
    },
}

/// Rastreador de comandos e estado de prompt para um único PTY.
#[derive(Debug, Clone)]
pub struct CommandTracker {
    binding: RuntimeBinding,
    shell_kind: ShellKind,
    expected_nonce: Option<String>,
    mode: IntegrationMode,
    continuous_policy: ContinuousPolicy,
    phase: PromptPhase,
    current_cwd: Option<SanitizedText>,
    pending_prompt_start_pos: Option<StreamPosition>,
    pending_command_line: Option<(SanitizedText, CommandTextQuality, Option<u64>)>,
    open_block: Option<OpenCommandBlock>,
    next_command_seq: u32,
    last_stream_pos: StreamPosition,
    osc_events_received: u64,
    fallback_activations: u64,
    total_commands: i64,
    total_errors: i64,
}

impl CommandTracker {
    /// Cria um novo [`CommandTracker`] para o vínculo [`RuntimeBinding`] informado.
    #[must_use]
    pub fn new(
        binding: RuntimeBinding,
        shell_kind: ShellKind,
        expected_nonce: Option<String>,
        initial_mode: IntegrationMode,
        continuous_policy: ContinuousPolicy,
        initial_cwd: Option<SanitizedText>,
    ) -> Self {
        Self {
            binding,
            shell_kind,
            expected_nonce,
            mode: initial_mode,
            continuous_policy,
            phase: PromptPhase::Unknown,
            current_cwd: initial_cwd,
            pending_prompt_start_pos: None,
            pending_command_line: None,
            open_block: None,
            next_command_seq: 1,
            last_stream_pos: StreamPosition::default(),
            osc_events_received: 0,
            fallback_activations: 0,
            total_commands: 0,
            total_errors: 0,
        }
    }

    /// Retorna o vínculo de runtime deste tracker.
    #[must_use]
    pub const fn binding(&self) -> &RuntimeBinding {
        &self.binding
    }

    /// Retorna a família do shell associada.
    #[must_use]
    pub const fn shell_kind(&self) -> &ShellKind {
        &self.shell_kind
    }

    /// Retorna o modo de integração atual (`Pending`, `Osc133`, `FallbackProc`, `Disabled`).
    #[must_use]
    pub const fn mode(&self) -> IntegrationMode {
        self.mode
    }

    /// Retorna a política de execução contínua configurada.
    #[must_use]
    pub const fn continuous_policy(&self) -> ContinuousPolicy {
        self.continuous_policy
    }

    /// Atualiza a política de execução contínua.
    pub fn set_continuous_policy(&mut self, policy: ContinuousPolicy) {
        self.continuous_policy = policy;
        if let Some(open) = &mut self.open_block {
            match policy {
                ContinuousPolicy::Always => open.is_continuous = true,
                ContinuousPolicy::Never => open.is_continuous = false,
                ContinuousPolicy::Auto => {}
            }
        }
    }

    /// Marca o bloco aberto atual como contínuo (ex.: após timer de 15 s).
    pub fn mark_active_block_continuous(&mut self, block_id: CommandBlockId) {
        if let Some(open) = &mut self.open_block
            && open.id == block_id
        {
            open.is_continuous = true;
        }
    }

    /// Retorna a fase atual do prompt.
    #[must_use]
    pub const fn phase(&self) -> PromptPhase {
        self.phase
    }

    /// Retorna o último `cwd` sanitizado conhecido.
    #[must_use]
    pub fn current_cwd(&self) -> Option<&SanitizedText> {
        self.current_cwd.as_ref()
    }

    /// Retorna a referência ao bloco atualmente aberto, se houver.
    #[must_use]
    pub const fn open_block(&self) -> Option<&OpenCommandBlock> {
        self.open_block.as_ref()
    }

    /// Total de eventos OSC 133/633 válidos processados.
    #[must_use]
    pub const fn osc_events_received(&self) -> u64 {
        self.osc_events_received
    }

    /// Total de blocos ativados via fallback `/proc`.
    #[must_use]
    pub const fn fallback_activations(&self) -> u64 {
        self.fallback_activations
    }

    /// Total de comandos finalizados na sessão.
    #[must_use]
    pub const fn total_commands(&self) -> i64 {
        self.total_commands
    }

    /// Total de comandos finalizados com falha na sessão.
    #[must_use]
    pub const fn total_errors(&self) -> i64 {
        self.total_errors
    }

    /// Processa o [`ScanOutput`] produzido pelo [`crate::osc::OscScanner`], intercalando
    /// exatamente os spans de texto e os eventos OSC na ordem em que ocorreram no stream.
    pub fn process_scan_output(&mut self, scan: &ScanOutput, now_ms: i64) -> Vec<TrackerEvent> {
        let mut out = Vec::new();

        for segment in &scan.segments {
            match segment {
                ScanSegment::Text(text_span) => {
                    self.last_stream_pos = StreamPosition {
                        pty_seq: text_span.pty_seq,
                        byte_offset: text_span.end_byte_offset,
                    };
                    if let Some(open) = &mut self.open_block {
                        open.capture.push_bytes(&text_span.bytes);
                    }
                }
                ScanSegment::Event(ev) => {
                    self.last_stream_pos = ev.end_pos;
                    self.handle_osc_event(ev, now_ms, &mut out);
                }
            }
        }

        out
    }

    /// Inicia um bloco sintético detectado pelo fallback `/proc` (quando `mode != Osc133`).
    pub fn start_proc_fallback_block(
        &mut self,
        cmdline: Option<String>,
        cwd: Option<String>,
        now_ms: i64,
    ) -> Vec<TrackerEvent> {
        if self.mode == IntegrationMode::Disabled || self.mode == IntegrationMode::Osc133 {
            return Vec::new();
        }

        let mut out = Vec::new();
        if self.mode == IntegrationMode::Pending {
            self.mode = IntegrationMode::FallbackProc;
            out.push(TrackerEvent::IntegrationModeChanged {
                mode: IntegrationMode::FallbackProc,
                reason: "proc_foreground_detected".to_string(),
            });
        }

        // Se já existe um bloco aberto, não duplica
        if self.open_block.is_some() {
            return out;
        }

        if let Some(raw_cwd) = cwd
            && let Some(valid_cwd) = normalize_and_validate_cwd(&raw_cwd)
        {
            self.update_cwd(SanitizedText::new(&valid_cwd), now_ms, &mut out);
        }

        let (cmd_sanitized, quality) = match cmdline {
            Some(c) if !c.trim().is_empty() => (
                Some(SanitizedText::new(c.trim())),
                CommandTextQuality::ArgvReconstructed,
            ),
            _ => (None, CommandTextQuality::Unavailable),
        };

        self.fallback_activations += 1;
        let pos = self.last_stream_pos;
        self.open_new_block(
            None,
            cmd_sanitized,
            quality,
            pos,
            Some(pos),
            ObservationSource::ProcFallback,
            ObservationConfidence::Degraded,
            now_ms,
            &mut out,
        );

        out
    }

    /// Finaliza um bloco iniciado via fallback `/proc` quando o processo foreground retorna ao shell.
    pub fn finish_proc_fallback_block(
        &mut self,
        exit_code: Option<i32>,
        cwd: Option<String>,
        now_ms: i64,
    ) -> Vec<TrackerEvent> {
        let mut out = Vec::new();
        if self
            .open_block
            .as_ref()
            .is_some_and(|b| b.source == ObservationSource::ProcFallback)
        {
            let pos = self.last_stream_pos;
            let (status, reason) = match exit_code {
                Some(0) => (BlockStatus::Succeeded, BlockEndReason::ExitZero),
                Some(130) => (BlockStatus::Cancelled, BlockEndReason::CtrlC),
                Some(_) => (BlockStatus::Failed, BlockEndReason::ExitNonZero),
                None => (BlockStatus::Succeeded, BlockEndReason::ImplicitNextPrompt),
            };
            self.close_open_block(
                status,
                reason,
                exit_code,
                None,
                ObservationConfidence::Degraded,
                pos,
                now_ms,
                &mut out,
            );
        }

        if let Some(raw_cwd) = cwd
            && let Some(valid_cwd) = normalize_and_validate_cwd(&raw_cwd)
        {
            self.update_cwd(SanitizedText::new(&valid_cwd), now_ms, &mut out);
        }

        out
    }

    /// Promove o modo de `Pending` para `FallbackProc` quando a janela de ativação de 1500 ms expira sem OSC 133.
    pub fn on_activation_window_expired(&mut self) -> Option<TrackerEvent> {
        if self.mode == IntegrationMode::Pending {
            self.mode = IntegrationMode::FallbackProc;
            Some(TrackerEvent::IntegrationModeChanged {
                mode: IntegrationMode::FallbackProc,
                reason: "activation_window_expired".to_string(),
            })
        } else {
            None
        }
    }

    /// Encerra qualquer bloco em andamento quando o processo do PTY termina (`PtyExited`).
    pub fn on_pty_exited(
        &mut self,
        exit_code: Option<i32>,
        signal: Option<String>,
        now_ms: i64,
    ) -> Vec<TrackerEvent> {
        let mut out = Vec::new();
        if self.open_block.is_some() {
            let pos = self.last_stream_pos;
            // Se estava em FallbackProc e o PTY saiu com erro != 0, classifica como Failed
            let status = if self.mode == IntegrationMode::FallbackProc
                && exit_code.is_some_and(|c| c != 0)
            {
                BlockStatus::Failed
            } else {
                BlockStatus::PtyExited
            };
            self.close_open_block(
                status,
                BlockEndReason::PtyExited,
                exit_code,
                signal,
                ObservationConfidence::Degraded,
                pos,
                now_ms,
                &mut out,
            );
        }
        self.phase = PromptPhase::Unknown;
        out
    }

    /// Encerra qualquer bloco em andamento durante o desligamento gracioso do aplicativo (`AppShutdown`).
    pub fn on_app_shutdown(&mut self, now_ms: i64) -> Vec<TrackerEvent> {
        let mut out = Vec::new();
        if self.open_block.is_some() {
            let pos = self.last_stream_pos;
            self.close_open_block(
                BlockStatus::PtyExited,
                BlockEndReason::AppShutdown,
                None,
                None,
                ObservationConfidence::Degraded,
                pos,
                now_ms,
                &mut out,
            );
        }
        out
    }

    #[allow(clippy::too_many_lines)]
    fn handle_osc_event(&mut self, ev: &RawOscEvent, now_ms: i64, out: &mut Vec<TrackerEvent>) {
        if self.mode == IntegrationMode::Disabled {
            return;
        }

        match &ev.kind {
            OscKind::Osc7Cwd { cwd } => {
                // OSC 7 atualiza o diretório de trabalho quando válido
                self.update_cwd(SanitizedText::new(cwd), now_ms, out);
            }

            OscKind::Property {
                cwd,
                handshake_mt,
                nonce,
                ..
            } => {
                if nonce.is_some() && !self.is_nonce_valid(nonce.as_deref()) {
                    return;
                }
                if (*handshake_mt || nonce.is_some()) && self.mode != IntegrationMode::Osc133 {
                    self.promote_to_osc133("first_osc133_received", now_ms, out);
                }
                self.osc_events_received += 1;
                if let Some(c) = cwd {
                    self.update_cwd(SanitizedText::new(c), now_ms, out);
                }
            }

            OscKind::CommandLine {
                command,
                quality,
                nonce,
                generation,
                ..
            } => {
                if !self.is_nonce_valid(nonce.as_deref()) {
                    return;
                }
                if self.mode != IntegrationMode::Osc133 {
                    self.promote_to_osc133("first_osc133_received", now_ms, out);
                }
                self.osc_events_received += 1;
                if let Some(cmd) = command {
                    self.pending_command_line =
                        Some((SanitizedText::new(cmd), *quality, *generation));
                } else {
                    self.pending_command_line = None;
                }
            }

            OscKind::PromptStart { nonce, .. } => {
                if !self.is_nonce_valid(nonce.as_deref()) {
                    return;
                }
                if self.mode != IntegrationMode::Osc133 {
                    self.promote_to_osc133("first_osc133_received", now_ms, out);
                }
                self.osc_events_received += 1;

                // Se havia um bloco em execução sem `D`, fecha implicitamente no retorno ao prompt `A`
                if self.open_block.is_some() {
                    self.close_open_block(
                        BlockStatus::Succeeded,
                        BlockEndReason::ImplicitNextPrompt,
                        None,
                        None,
                        ObservationConfidence::Degraded,
                        ev.start_pos,
                        now_ms,
                        out,
                    );
                }

                self.phase = PromptPhase::InPrompt;
                self.pending_prompt_start_pos = Some(ev.start_pos);
                self.pending_command_line = None;
            }

            OscKind::PromptEnd { nonce, .. } => {
                if !self.is_nonce_valid(nonce.as_deref()) {
                    return;
                }
                if self.mode != IntegrationMode::Osc133 {
                    self.promote_to_osc133("first_osc133_received", now_ms, out);
                }
                self.osc_events_received += 1;
                self.phase = PromptPhase::AwaitingInput;
                if self.pending_prompt_start_pos.is_none() {
                    self.pending_prompt_start_pos = Some(ev.start_pos);
                }
            }

            OscKind::CommandExecuted {
                nonce,
                generation,
                command,
                cwd,
                source,
            } => {
                if !self.is_nonce_valid(nonce.as_deref()) {
                    return;
                }
                if self.mode != IntegrationMode::Osc133 {
                    self.promote_to_osc133("first_osc133_received", now_ms, out);
                }
                self.osc_events_received += 1;

                // Se for `C` duplicado da mesma geração já aberta, ignora de forma idempotente
                if let Some(open) = &self.open_block
                    && generation.is_some()
                    && open.hook_generation == *generation
                {
                    return;
                }

                // Se havia outro bloco aberto sem `D`, fecha implicitamente com `ImplicitNextCommand`
                if self.open_block.is_some() {
                    self.close_open_block(
                        BlockStatus::Succeeded,
                        BlockEndReason::ImplicitNextCommand,
                        None,
                        None,
                        ObservationConfidence::Degraded,
                        ev.start_pos,
                        now_ms,
                        out,
                    );
                }

                if let Some(c) = cwd {
                    self.update_cwd(SanitizedText::new(c), now_ms, out);
                }

                let (cmd_text, quality) = if let Some(cmd) = command {
                    (Some(SanitizedText::new(cmd)), CommandTextQuality::Exact)
                } else if let Some((pending_cmd, pending_q, pending_gen)) =
                    self.pending_command_line.take()
                {
                    if pending_gen.is_none() || pending_gen == *generation {
                        (Some(pending_cmd), pending_q)
                    } else {
                        (None, CommandTextQuality::Unavailable)
                    }
                } else {
                    (None, CommandTextQuality::Unavailable)
                };

                let had_prompt = matches!(
                    self.phase,
                    PromptPhase::InPrompt | PromptPhase::AwaitingInput
                );
                let confidence = if had_prompt {
                    ObservationConfidence::Exact
                } else {
                    ObservationConfidence::Degraded
                };

                let start_pos = self.pending_prompt_start_pos.take().unwrap_or(ev.start_pos);
                let output_start_pos = Some(ev.end_pos);

                self.open_new_block(
                    *generation,
                    cmd_text,
                    quality,
                    start_pos,
                    output_start_pos,
                    *source,
                    confidence,
                    now_ms,
                    out,
                );
            }

            OscKind::CommandFinished {
                exit_code,
                nonce,
                generation,
                cwd,
                ..
            } => {
                if !self.is_nonce_valid(nonce.as_deref()) {
                    return;
                }
                if self.mode != IntegrationMode::Osc133 {
                    self.promote_to_osc133("first_osc133_received", now_ms, out);
                }
                self.osc_events_received += 1;

                // Só fecha se houver um bloco aberto (e se `generation` for compatível quando informado)
                if let Some(open) = &self.open_block {
                    if let (Some(open_gen), Some(ev_gen)) = (open.hook_generation, *generation)
                        && open_gen != ev_gen
                    {
                        return;
                    }

                    let (status, reason, signal, conf) =
                        classify_osc_d_exit(*exit_code, open.confidence);

                    self.close_open_block(
                        status,
                        reason,
                        *exit_code,
                        signal,
                        conf,
                        ev.start_pos,
                        now_ms,
                        out,
                    );
                }

                // O `cwd` reportado em `D` reflete o diretório após o término do comando (ex.: `cd /novo`)
                if let Some(c) = cwd {
                    self.update_cwd(SanitizedText::new(c), now_ms, out);
                }
            }
        }
    }

    fn is_nonce_valid(&self, received: Option<&str>) -> bool {
        match &self.expected_nonce {
            None => true,
            Some(expected) => received.is_some_and(|r| r == expected),
        }
    }

    fn promote_to_osc133(&mut self, reason: &str, now_ms: i64, out: &mut Vec<TrackerEvent>) {
        // Se havia um bloco inferido por `/proc` aberto no momento em que o hook OSC 133 ativou,
        // fecha-o de forma limpa antes de mudar para Osc133.
        if self
            .open_block
            .as_ref()
            .is_some_and(|b| b.source == ObservationSource::ProcFallback)
        {
            let pos = self.last_stream_pos;
            self.close_open_block(
                BlockStatus::Succeeded,
                BlockEndReason::ImplicitNextPrompt,
                None,
                None,
                ObservationConfidence::Degraded,
                pos,
                now_ms,
                out,
            );
        }

        self.mode = IntegrationMode::Osc133;
        out.push(TrackerEvent::IntegrationModeChanged {
            mode: IntegrationMode::Osc133,
            reason: reason.to_string(),
        });
    }

    fn update_cwd(&mut self, new_cwd: SanitizedText, now_ms: i64, out: &mut Vec<TrackerEvent>) {
        if self.current_cwd.as_ref() != Some(&new_cwd) {
            self.current_cwd = Some(new_cwd.clone());
            out.push(TrackerEvent::CwdChanged {
                cwd: new_cwd,
                at_ms: now_ms,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn open_new_block(
        &mut self,
        hook_generation: Option<u64>,
        command_text: Option<SanitizedText>,
        command_text_quality: CommandTextQuality,
        start_pos: StreamPosition,
        output_start_pos: Option<StreamPosition>,
        source: ObservationSource,
        confidence: ObservationConfidence,
        now_ms: i64,
        out: &mut Vec<TrackerEvent>,
    ) {
        let block_id = CommandBlockId::new();
        let command_seq = self.next_command_seq;
        self.next_command_seq = self.next_command_seq.saturating_add(1);

        let command_string = command_text.as_ref().map(|s| s.as_str().to_string());
        let preview = command_string.as_deref().map(make_command_preview);
        let is_continuous =
            is_continuous_by_policy_or_command(self.continuous_policy, command_string.as_deref());

        let cwd_cloned = self.current_cwd.clone();
        let workdir_string = cwd_cloned.as_ref().map(|s| s.as_str().to_string());

        let execution = ExecutionRef {
            block_id,
            command_seq,
            started_at: now_ms,
            command_preview: preview,
            source,
        };

        let started_event = CommandStarted {
            binding: self.binding.clone(),
            execution,
            command_text: command_string,
            command_text_quality,
            cwd: workdir_string,
            start_pos,
            output_start_pos,
            confidence,
            persistence: PersistenceState::Queued,
        };

        self.open_block = Some(OpenCommandBlock {
            id: block_id,
            command_seq,
            hook_generation,
            command_text: command_text.clone(),
            command_text_quality,
            cwd: cwd_cloned.clone(),
            started_at: now_ms,
            start_pos,
            output_start_pos,
            source,
            confidence,
            is_continuous,
            capture: OutputCapture::new(),
        });
        self.phase = PromptPhase::Executing;

        out.push(TrackerEvent::BlockStarted {
            started: started_event,
            sanitized_command: command_text,
            sanitized_cwd: cwd_cloned,
            is_continuous,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn close_open_block(
        &mut self,
        status: BlockStatus,
        end_reason: BlockEndReason,
        exit_code: Option<i32>,
        signal: Option<String>,
        confidence: ObservationConfidence,
        end_pos: StreamPosition,
        now_ms: i64,
        out: &mut Vec<TrackerEvent>,
    ) {
        let Some(open) = self.open_block.take() else {
            return;
        };

        let ended_at = now_ms.max(open.started_at);
        let duration_ms = ended_at.saturating_sub(open.started_at);

        let inspectable = is_block_inspectable(status, end_reason, exit_code);
        let include_error_excerpt = inspectable
            || matches!(
                status,
                BlockStatus::Failed | BlockStatus::TerminatedBySignal | BlockStatus::PtyExited
            );

        let finalized_output = open.capture.finish(include_error_excerpt);

        self.total_commands += 1;
        if inspectable || status == BlockStatus::Failed {
            self.total_errors += 1;
        }

        let finished_event = CommandFinished {
            binding: self.binding.clone(),
            block_id: open.id,
            command_seq: open.command_seq,
            command_text: open.command_text.as_ref().map(|s| s.as_str().to_string()),
            command_text_quality: open.command_text_quality,
            cwd: open.cwd.as_ref().map(|s| s.as_str().to_string()),
            started_at: open.started_at,
            ended_at,
            duration_ms,
            status,
            end_reason,
            exit_code,
            signal,
            start_pos: open.start_pos,
            output_start_pos: open.output_start_pos,
            end_pos,
            output_bytes: finalized_output.output_bytes,
            output_lines: finalized_output.output_lines,
            output_preview: finalized_output.output_preview.clone(),
            error_excerpt: finalized_output
                .error_excerpt
                .as_ref()
                .map(|s| s.as_str().to_string()),
            inspectable,
            source: open.source,
            confidence,
            persistence: PersistenceState::Queued,
        };

        self.phase = PromptPhase::Unknown;

        out.push(TrackerEvent::BlockFinished {
            finished: finished_event,
            output: finalized_output,
            sanitized_command: open.command_text,
            sanitized_cwd: open.cwd,
            is_continuous: open.is_continuous,
        });
    }
}

fn classify_osc_d_exit(
    exit_code: Option<i32>,
    initial_confidence: ObservationConfidence,
) -> (
    BlockStatus,
    BlockEndReason,
    Option<String>,
    ObservationConfidence,
) {
    match exit_code {
        Some(0) => (
            BlockStatus::Succeeded,
            BlockEndReason::ExitZero,
            None,
            initial_confidence,
        ),
        Some(130) => (
            BlockStatus::Cancelled,
            BlockEndReason::CtrlC,
            Some("SIGINT".to_string()),
            initial_confidence,
        ),
        Some(code) if (129..=192).contains(&code) => {
            let sig_num = code - 128;
            let sig_name = posix_signal_name(sig_num);
            (
                BlockStatus::TerminatedBySignal,
                BlockEndReason::KilledBySignal,
                Some(sig_name),
                initial_confidence,
            )
        }
        Some(_) => (
            BlockStatus::Failed,
            BlockEndReason::ExitNonZero,
            None,
            initial_confidence,
        ),
        None => (
            BlockStatus::Succeeded,
            BlockEndReason::ImplicitNextPrompt,
            None,
            ObservationConfidence::Degraded,
        ),
    }
}

fn is_block_inspectable(
    status: BlockStatus,
    end_reason: BlockEndReason,
    exit_code: Option<i32>,
) -> bool {
    if status == BlockStatus::Cancelled
        || end_reason == BlockEndReason::CtrlC
        || exit_code == Some(130)
    {
        return false;
    }
    matches!(
        status,
        BlockStatus::Failed | BlockStatus::TerminatedBySignal
    ) || (status == BlockStatus::PtyExited && exit_code.is_some_and(|c| c != 0))
}

fn posix_signal_name(sig_num: i32) -> String {
    match sig_num {
        1 => "SIGHUP".to_string(),
        2 => "SIGINT".to_string(),
        3 => "SIGQUIT".to_string(),
        6 => "SIGABRT".to_string(),
        9 => "SIGKILL".to_string(),
        11 => "SIGSEGV".to_string(),
        13 => "SIGPIPE".to_string(),
        14 => "SIGALRM".to_string(),
        15 => "SIGTERM".to_string(),
        _ => format!("SIG{sig_num}"),
    }
}

fn make_command_preview(cmd: &str) -> String {
    let first_line = cmd.lines().next().unwrap_or(cmd).trim();
    if first_line.len() <= 256 {
        first_line.to_string()
    } else {
        let mut idx = 256;
        while idx > 0 && !first_line.is_char_boundary(idx) {
            idx -= 1;
        }
        first_line[..idx].to_string()
    }
}

/// Constrói um DTO de bloco em andamento (`Running`) para consultas de cache antes do término.
#[must_use]
pub fn open_block_to_dto(
    binding: &RuntimeBinding,
    open: &OpenCommandBlock,
) -> mt_core::shell::CommandBlockDto {
    mt_core::shell::CommandBlockDto {
        id: open.id,
        session_id: binding.session_id,
        tab_id: binding.tab_id,
        pane_id: binding.pane_id,
        pty_id: binding.pty_id,
        command_seq: open.command_seq,
        command_text: open.command_text.as_ref().map(|s| s.as_str().to_string()),
        command_text_quality: open.command_text_quality,
        cwd: open.cwd.as_ref().map(|s| s.as_str().to_string()),
        started_at: open.started_at,
        ended_at: None,
        duration_ms: None,
        exit_code: None,
        signal: None,
        status: BlockStatus::Running,
        end_reason: None,
        is_continuous: open.is_continuous,
        source: open.source,
        confidence: open.confidence,
        output_text: None,
        output_kind: OutputKind::None,
        output_bytes: i64::try_from(open.capture.raw_bytes_total()).unwrap_or(i64::MAX),
        output_lines: i64::try_from(open.capture.raw_lines_total()).unwrap_or(i64::MAX),
        error_excerpt: None,
        inspectable: false,
        start_pty_seq: open.start_pos.pty_seq,
        start_byte_offset: open.start_pos.byte_offset,
        output_start_pty_seq: open.output_start_pos.map(|p| p.pty_seq),
        output_start_byte_offset: open.output_start_pos.map(|p| p.byte_offset),
        end_pty_seq: None,
        end_byte_offset: None,
        persistence: PersistenceState::Queued,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use mt_core::ids::{PaneId, PtyId, SessionId, TabId};

    use crate::osc::OscScanner;

    use super::*;

    fn make_tracker(nonce: Option<&str>) -> CommandTracker {
        CommandTracker::new(
            RuntimeBinding {
                pty_id: PtyId::new(),
                session_id: SessionId::new(),
                tab_id: Some(TabId::new()),
                pane_id: Some(PaneId::new()),
            },
            ShellKind::Bash,
            nonce.map(ToString::to_string),
            IntegrationMode::Pending,
            ContinuousPolicy::Auto,
            Some(SanitizedText::new("/home/user")),
        )
    }

    #[test]
    fn correlates_full_cycle_a_b_c_d_and_rejects_forged_nonce() {
        let mut scanner = OscScanner::new();
        let mut tracker = make_tracker(Some("secret123"));

        let cmd_b64 = STANDARD.encode("echo hello");
        let workdir_b64 = STANDARD.encode("/home/user/app");

        // 1. Evento forjado com nonce inválido deve ser ignorado
        let forged = format!(
            "\x1b]133;C;nonce=forged;cmd_b64={cmd_b64};cwd_b64={workdir_b64}\x07saida falsa\x1b]133;D;0;nonce=forged\x07"
        );
        let scan1 = scanner.scan_chunk(1, forged.as_bytes());
        let evs1 = tracker.process_scan_output(&scan1, 1000);
        assert!(evs1.is_empty());
        assert_eq!(tracker.mode(), IntegrationMode::Pending);

        // 2. Ciclo real com nonce válido
        let valid = format!(
            "\x1b]133;A;nonce=secret123\x07\x1b]133;B;nonce=secret123\x07\x1b]133;C;nonce=secret123;cmd_b64={cmd_b64};cwd_b64={workdir_b64}\x07hello\r\n\x1b]133;D;0;nonce=secret123;cwd_b64={workdir_b64}\x07"
        );
        let scan2 = scanner.scan_chunk(2, valid.as_bytes());
        let evs2 = tracker.process_scan_output(&scan2, 1100);

        assert_eq!(tracker.mode(), IntegrationMode::Osc133);
        let started = evs2
            .iter()
            .find_map(|e| match e {
                TrackerEvent::BlockStarted { started, .. } => Some(started),
                _ => None,
            })
            .unwrap();
        assert_eq!(started.command_text.as_deref(), Some("echo hello"));
        assert_eq!(started.confidence, ObservationConfidence::Exact);

        let finished = evs2
            .iter()
            .find_map(|e| match e {
                TrackerEvent::BlockFinished { finished, .. } => Some(finished),
                _ => None,
            })
            .unwrap();
        assert_eq!(finished.status, BlockStatus::Succeeded);
        assert_eq!(finished.exit_code, Some(0));
        assert!(!finished.inspectable);
    }
}
