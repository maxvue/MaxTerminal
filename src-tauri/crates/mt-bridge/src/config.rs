//! Configurações persistidas da ponte `MaxTerminal` ⇄ `MaxCode` (`BridgeSettings`).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::BridgeError;

/// Política de fallback quando o loopback do `MaxCode` não está acessível ao clicar em "Inspecionar".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum InspectFallbackMode {
    /// Dispara `vscode://johnattas.maxcode/inspect?...` (e arquivo temporário `0600` se > 6 KiB).
    #[default]
    UriScheme,
    /// Copia o prompt formatado para a área de transferência e avisa o usuário.
    ClipboardOnly,
    /// Não executa fallback; retorna erro explícito.
    Disabled,
}

/// Configurações da ponte `MaxCode` persistidas na tabela `settings` (`namespace = "bridge"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct BridgeSettings {
    /// Habilita o servidor loopback e o cliente de conexão com o `MaxCode`.
    pub enabled: bool,
    /// Sincroniza automaticamente o projeto ativo em ambas as direções (`project.opened`).
    pub auto_sync_active_project: bool,
    /// Cria e seleciona automaticamente o projeto no `MaxTerminal` se a pasta aberta no `MaxCode` ainda não estiver cadastrada.
    pub auto_register_unknown_project: bool,
    /// Traz a janela do `MaxTerminal` para frente ao receber `open_terminal_for_issue`.
    pub focus_window_on_incoming_command: bool,
    /// Modo de fallback quando o `MaxCode` está fechado ao acionar "Inspecionar".
    pub inspect_fallback_mode: InspectFallbackMode,
    /// Porta TCP fixa opcional (`1024..=65535`); `None` usa porta efêmera alocada pelo kernel (`127.0.0.1:0`).
    pub preferred_port: Option<u16>,
}

impl Default for BridgeSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_sync_active_project: true,
            auto_register_unknown_project: true,
            focus_window_on_incoming_command: true,
            inspect_fallback_mode: InspectFallbackMode::UriScheme,
            preferred_port: None,
        }
    }
}

/// Atualização parcial das configurações da ponte (`bridge_update_settings`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSettingsPatch {
    /// Habilita ou desabilita a ponte.
    pub enabled: Option<bool>,
    /// Altera a sincronização automática de projeto ativo.
    pub auto_sync_active_project: Option<bool>,
    /// Altera o auto-cadastro de projetos vindos do `MaxCode`.
    pub auto_register_unknown_project: Option<bool>,
    /// Altera o foco automático de janela em comandos recebidos.
    pub focus_window_on_incoming_command: Option<bool>,
    /// Altera o modo de fallback do botão "Inspecionar".
    pub inspect_fallback_mode: Option<InspectFallbackMode>,
    /// Define (`Some(Some(port))`) ou limpa (`Some(None)`) a porta preferencial.
    pub preferred_port: Option<Option<u16>>,
}

impl BridgeSettings {
    /// Aplica um patch validando que `preferred_port`, se presente, está em `1024..=65535`.
    #[allow(clippy::needless_pass_by_value)]
    pub fn apply_patch(&mut self, patch: BridgeSettingsPatch) -> Result<(), BridgeError> {
        if let Some(Some(port)) = patch.preferred_port
            && port < 1024
        {
            return Err(BridgeError::new(
                crate::error::BridgeErrorCode::InvalidEnvelope,
                "A porta preferencial do loopback deve estar entre 1024 e 65535.",
            ));
        }
        if let Some(v) = patch.enabled {
            self.enabled = v;
        }
        if let Some(v) = patch.auto_sync_active_project {
            self.auto_sync_active_project = v;
        }
        if let Some(v) = patch.auto_register_unknown_project {
            self.auto_register_unknown_project = v;
        }
        if let Some(v) = patch.focus_window_on_incoming_command {
            self.focus_window_on_incoming_command = v;
        }
        if let Some(v) = patch.inspect_fallback_mode {
            self.inspect_fallback_mode = v;
        }
        if let Some(v) = patch.preferred_port {
            self.preferred_port = v;
        }
        Ok(())
    }
}

/// Repositório abstrato para leitura e gravação de `BridgeSettings`.
#[async_trait]
pub trait BridgeSettingsStore: Send + Sync + 'static {
    /// Carrega as configurações persistidas (ou `BridgeSettings::default()` se ausentes).
    async fn load(&self) -> Result<BridgeSettings, BridgeError>;

    /// Persiste as configurações atualizadas.
    async fn save(&self, settings: &BridgeSettings) -> Result<(), BridgeError>;
}
