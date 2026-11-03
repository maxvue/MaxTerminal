//! Catálogo tipado de configurações (`SettingsRegistry`) e validação de chaves/valores JSON.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

static SETTING_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z][a-zA-Z0-9_]*(\.[a-zA-Z0-9_]+)+$")
        .unwrap_or_else(|e| panic!("Regex de chave de setting inválida: {e}"))
});

/// Descritor de uma chave de configuração registrada no `SettingsRegistry`.
#[derive(Clone)]
pub struct SettingDescriptor {
    /// Chave canônica em notação de ponto (ex.: `"db.retention.backup_daily_keep"`).
    pub key: &'static str,
    /// Função geradora do valor padrão JSON.
    pub default_value: fn() -> Value,
    /// Função de validação de domínio; retorna `Err(mensagem_pt_br)` quando inválido.
    pub validate: fn(&Value) -> Result<(), String>,
    /// Quando `true`, proíbe persistência na tabela `settings` (`db.setting_secret_rejected`).
    pub sensitive: bool,
    /// Quando `true`, rejeita alterações vindas da UI (`SettingChangeOrigin::Ui`).
    pub internal_only: bool,
    /// Descrição humana da configuração em `pt-BR`.
    pub description: &'static str,
}

impl std::fmt::Debug for SettingDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingDescriptor")
            .field("key", &self.key)
            .field("sensitive", &self.sensitive)
            .field("internal_only", &self.internal_only)
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

/// Resultado da resolução de uma chave de configuração no `SettingsRegistry`.
#[derive(Debug, Clone, Copy)]
pub enum SettingLookup<'a> {
    /// Chave explicitamente registrada com descritor e validador.
    Registered(&'a SettingDescriptor),
    /// Chave livre pertencente ao namespace `ui.*` (validada sintaticamente e contra segredos).
    FreeUi,
    /// Chave desconhecida ou malformada.
    Unknown,
}

/// Registro de configurações conhecidas da aplicação.
#[derive(Clone, Debug)]
pub struct SettingsRegistry {
    descriptors: BTreeMap<&'static str, SettingDescriptor>,
}

impl Default for SettingsRegistry {
    fn default() -> Self {
        Self::with_phase03_defaults()
    }
}

impl SettingsRegistry {
    /// Cria um registro vazio.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            descriptors: BTreeMap::new(),
        }
    }

    /// Cria o `SettingsRegistry` pré-populado com as configurações canônicas da Fase 03.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn with_phase03_defaults() -> Self {
        let mut reg = Self::empty();

        reg.register(SettingDescriptor {
            key: "db.retention.backup_daily_keep",
            default_value: || json!(7),
            validate: |v| {
                if let Some(n) = v.as_i64()
                    && (1..=60).contains(&n)
                {
                    return Ok(());
                }
                Err(String::from(
                    "O número de backups diários deve ser um inteiro entre 1 e 60.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Quantidade de backups diários retidos na rotação automática.",
        });

        reg.register(SettingDescriptor {
            key: "db.retention.command_blocks_days",
            default_value: || json!(90),
            validate: |v| {
                if let Some(n) = v.as_i64()
                    && (1..=3650).contains(&n)
                {
                    return Ok(());
                }
                Err(String::from(
                    "A retenção de blocos de comando deve ser um inteiro entre 1 e 3650 dias.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Quantidade de dias para retenção de blocos de comando no histórico SQLite.",
        });

        reg.register(SettingDescriptor {
            key: "db.retention.max_db_size_mb",
            default_value: || json!(2048),
            validate: |v| {
                if let Some(n) = v.as_i64()
                    && (64..=65_536).contains(&n)
                {
                    return Ok(());
                }
                Err(String::from(
                    "O limite de tamanho do banco deve estar entre 64 MiB e 65536 MiB.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Limite de alerta para o tamanho total do banco SQLite em MiB.",
        });

        reg.register(SettingDescriptor {
            key: "db.maintenance.interval_hours",
            default_value: || json!(24),
            validate: |v| {
                if let Some(n) = v.as_i64()
                    && (1..=168).contains(&n)
                {
                    return Ok(());
                }
                Err(String::from(
                    "O intervalo de manutenção deve estar entre 1 e 168 horas.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Intervalo em horas entre execuções automáticas de manutenção.",
        });

        reg.register(SettingDescriptor {
            key: "db.maintenance.last_run_at",
            default_value: || Value::Null,
            validate: |v| {
                if v.is_null() || v.as_i64().is_some_and(|n| n >= 0) {
                    return Ok(());
                }
                Err(String::from(
                    "O timestamp da última manutenção deve ser nulo ou um inteiro não negativo.",
                ))
            },
            sensitive: false,
            internal_only: true,
            description: "Timestamp UNIX em ms da última execução do ciclo de manutenção.",
        });

        reg.register(SettingDescriptor {
            key: "ui.theme",
            default_value: || json!("max-navy-teal"),
            validate: |v| {
                if v.as_str().is_some_and(|s| !s.trim().is_empty()) {
                    Ok(())
                } else {
                    Err(String::from("O tema visual deve ser uma string não vazia."))
                }
            },
            sensitive: false,
            internal_only: false,
            description: "Tema visual ativo da interface.",
        });

        reg.register(SettingDescriptor {
            key: "ui.locale",
            default_value: || json!("pt-BR"),
            validate: |v| {
                if v.as_str().is_some_and(|s| !s.trim().is_empty()) {
                    Ok(())
                } else {
                    Err(String::from("O idioma deve ser uma string não vazia."))
                }
            },
            sensitive: false,
            internal_only: false,
            description: "Idioma ativo da interface.",
        });

        reg.register(SettingDescriptor {
            key: "terminal.scrollbackLines",
            default_value: || json!(10_000),
            validate: |v| {
                if let Some(n) = v.as_i64()
                    && (100..=500_000).contains(&n)
                {
                    return Ok(());
                }
                Err(String::from(
                    "O limite de linhas de scrollback deve estar entre 100 e 500000.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Quantidade máxima de linhas no buffer de scrollback do terminal.",
        });

        reg.register(SettingDescriptor {
            key: "terminal.fontFamily",
            default_value: || json!("JetBrains Mono"),
            validate: |v| {
                if v.as_str().is_some_and(|s| !s.trim().is_empty()) {
                    Ok(())
                } else {
                    Err(String::from(
                        "A família tipográfica do terminal deve ser uma string não vazia.",
                    ))
                }
            },
            sensitive: false,
            internal_only: false,
            description: "Família tipográfica utilizada no terminal.",
        });

        reg.register(SettingDescriptor {
            key: "terminal.fontSize",
            default_value: || json!(13),
            validate: |v| {
                if let Some(n) = v.as_i64()
                    && (8..=48).contains(&n)
                {
                    return Ok(());
                }
                Err(String::from(
                    "O tamanho da fonte do terminal deve estar entre 8 e 48.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Tamanho da fonte do terminal em pixels.",
        });

        reg.register(SettingDescriptor {
            key: "privacy.redactCustomPatterns",
            default_value: || json!([]),
            validate: |v| {
                if v.as_array()
                    .is_some_and(|arr| arr.iter().all(Value::is_string))
                {
                    Ok(())
                } else {
                    Err(String::from(
                        "Os padrões customizados de redação devem ser uma lista de strings.",
                    ))
                }
            },
            sensitive: false,
            internal_only: false,
            description: "Lista opcional de padrões extras de redação de segredos.",
        });

        reg.register_phase06_settings();
        reg.register_phase07_settings();
        reg.register_phase11_settings();
        reg.register_phase13_settings();

        reg
    }

    /// Registra os descritores de configuração da Fase 13 (`agents.settings` e `skills.settings`).
    pub fn register_phase13_settings(&mut self) {
        self.register(SettingDescriptor {
            key: "agents.settings",
            default_value: || {
                json!({
                    "shortcutsEnabled": true,
                    "shortcuts": {},
                    "isolation": {
                        "defaultLocation": "project",
                        "branchPrefix": "agent",
                        "openIsolatedByDefault": false
                    },
                    "attention": {
                        "enabled": true,
                        "onTurnComplete": "ignore",
                        "silenceMs": 300,
                        "notify": true
                    },
                    "telemetry": {
                        "enabled": true,
                        "staleAfterS": 60,
                        "readSessionFiles": true
                    },
                    "inject": {
                        "claude-code": true,
                        "codex": true,
                        "agy": false,
                        "opencode": false
                    }
                })
            },
            validate: |v| {
                if v.is_object() {
                    Ok(())
                } else {
                    Err(String::from(
                        "O campo agents.settings deve ser um objeto JSON válido.",
                    ))
                }
            },
            sensitive: false,
            internal_only: false,
            description: "Configurações globais de agentes CLI, atalhos, isolamento, telemetria e atenção.",
        });

        self.register(SettingDescriptor {
            key: "skills.settings",
            default_value: || {
                json!({
                    "sourceDir": "~/GitHub/MaxSkills",
                    "watchEnabled": true
                })
            },
            validate: |v| {
                if v.is_object() {
                    Ok(())
                } else {
                    Err(String::from(
                        "O campo skills.settings deve ser um objeto JSON válido.",
                    ))
                }
            },
            sensitive: false,
            internal_only: false,
            description: "Configurações de sincronização e monitoramento do repositório MaxSkills.",
        });
    }

    /// Registra os descritores de configuração da Fase 11 (`bridge.*`).
    #[allow(clippy::too_many_lines)]
    pub fn register_phase11_settings(&mut self) {
        self.register(SettingDescriptor {
            key: "bridge.enabled",
            default_value: || json!(true),
            validate: |v| {
                if v.is_boolean() {
                    Ok(())
                } else {
                    Err(String::from("O campo bridge.enabled deve ser um booleano."))
                }
            },
            sensitive: false,
            internal_only: false,
            description: "Habilita a ponte bidirecional loopback e URI com a extensão MaxCode.",
        });

        self.register(SettingDescriptor {
            key: "bridge.auto_sync_active_project",
            default_value: || json!(true),
            validate: |v| {
                if v.is_boolean() {
                    Ok(())
                } else {
                    Err(String::from(
                        "O campo bridge.auto_sync_active_project deve ser um booleano.",
                    ))
                }
            },
            sensitive: false,
            internal_only: false,
            description: "Sincroniza automaticamente o projeto ativo entre MaxTerminal e MaxCode.",
        });

        self.register(SettingDescriptor {
            key: "bridge.auto_register_unknown_project",
            default_value: || json!(true),
            validate: |v| {
                if v.is_boolean() {
                    Ok(())
                } else {
                    Err(String::from(
                        "O campo bridge.auto_register_unknown_project deve ser um booleano.",
                    ))
                }
            },
            sensitive: false,
            internal_only: false,
            description: "Cadastra automaticamente pastas de projetos recebidas do MaxCode.",
        });

        self.register(SettingDescriptor {
            key: "bridge.focus_window_on_incoming_command",
            default_value: || json!(true),
            validate: |v| {
                if v.is_boolean() {
                    Ok(())
                } else {
                    Err(String::from(
                        "O campo bridge.focus_window_on_incoming_command deve ser um booleano.",
                    ))
                }
            },
            sensitive: false,
            internal_only: false,
            description: "Traz a janela do MaxTerminal para frente ao receber comandos de abertura de aba.",
        });

        self.register(SettingDescriptor {
            key: "bridge.inspect_fallback_mode",
            default_value: || json!("uri_scheme"),
            validate: |v| {
                if matches!(
                    v.as_str(),
                    Some("uri_scheme" | "clipboard_only" | "disabled")
                ) {
                    Ok(())
                } else {
                    Err(String::from(
                        "Modo de fallback inválido; esperado 'uri_scheme', 'clipboard_only' ou 'disabled'.",
                    ))
                }
            },
            sensitive: false,
            internal_only: false,
            description: "Política de fallback ao inspecionar erro quando o MaxCode está desconectado.",
        });

        self.register(SettingDescriptor {
            key: "bridge.preferred_port",
            default_value: || Value::Null,
            validate: |v| {
                if v.is_null() {
                    return Ok(());
                }
                if let Some(port) = v.as_u64()
                    && (1024..=65535).contains(&port)
                {
                    return Ok(());
                }
                Err(String::from(
                    "A porta preferencial da ponte deve ser nula ou um inteiro entre 1024 e 65535.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Porta TCP fixa opcional para o servidor loopback (null usa porta efêmera).",
        });
    }

    /// Registra os descritores de configuração da Fase 07 (`workspace.sessions.*` e `workspace.scrollback.*`).
    pub fn register_phase07_settings(&mut self) {
        self.register(SettingDescriptor {
            key: "workspace.sessions.keep_per_tab",
            default_value: || json!(50),
            validate: |v| {
                if let Some(n) = v.as_i64()
                    && (1..=1000).contains(&n)
                {
                    return Ok(());
                }
                Err(String::from(
                    "A quantidade de sessões retidas por aba deve ser um inteiro entre 1 e 1000.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Quantidade máxima de sessões históricas retidas por aba.",
        });

        self.register(SettingDescriptor {
            key: "workspace.sessions.max_age_days",
            default_value: || json!(90),
            validate: |v| {
                if let Some(n) = v.as_i64()
                    && (1..=3650).contains(&n)
                {
                    return Ok(());
                }
                Err(String::from(
                    "O tempo máximo de retenção de sessões deve ser um inteiro entre 1 e 3650 dias.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Idade máxima em dias das sessões arquivadas antes da remoção automática.",
        });

        self.register(SettingDescriptor {
            key: "workspace.scrollback.limit_mib",
            default_value: || json!(8),
            validate: |v| {
                if let Some(n) = v.as_i64()
                    && (1..=256).contains(&n)
                {
                    return Ok(());
                }
                Err(String::from(
                    "O limite de scrollback por sessão deve ser um inteiro entre 1 e 256 MiB.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Limite máximo em MiB de scrollback persistido por sessão de aba.",
        });
    }

    /// Registra os descritores de configuração da Fase 06 (`projects.selected_id` e `layout.columns.left_size`).
    pub fn register_phase06_settings(&mut self) {
        self.register(SettingDescriptor {
            key: "projects.selected_id",
            default_value: || Value::Null,
            validate: |v| {
                if v.is_null() {
                    return Ok(());
                }
                if let Some(s) = v.as_str()
                    && uuid::Uuid::parse_str(s).is_ok()
                {
                    return Ok(());
                }
                Err(String::from(
                    "O identificador do projeto selecionado deve ser nulo ou um UUID válido.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Identificador do projeto atualmente selecionado na barra lateral.",
        });

        self.register(SettingDescriptor {
            key: "layout.columns.left_size",
            default_value: || json!(22),
            validate: |v| {
                if let Some(n) = v.as_f64()
                    && n.is_finite()
                    && (15.0..=45.0).contains(&n)
                {
                    return Ok(());
                }
                Err(String::from(
                    "A largura percentual da coluna esquerda deve ser um número finito entre 15 e 45.",
                ))
            },
            sensitive: false,
            internal_only: false,
            description: "Largura percentual da coluna esquerda (lista de projetos) no layout principal.",
        });
    }

    /// Registra um novo `SettingDescriptor`.
    ///
    /// # Panics
    ///
    /// Dispara `panic!` se `descriptor.key` não obedecer ao formato canônico (`is_valid_setting_key`).
    pub fn register(&mut self, descriptor: SettingDescriptor) {
        assert!(
            is_valid_setting_key(descriptor.key),
            "Chave de configuração inválida ao registrar no SettingsRegistry: '{}'",
            descriptor.key
        );
        self.descriptors.insert(descriptor.key, descriptor);
    }

    /// Resolve uma chave no catálogo (retornando `Registered`, `FreeUi` ou `Unknown`).
    #[must_use]
    pub fn resolve(&self, key: &str) -> SettingLookup<'_> {
        if !is_valid_setting_key(key) {
            return SettingLookup::Unknown;
        }
        if let Some(desc) = self.descriptors.get(key) {
            return SettingLookup::Registered(desc);
        }
        if key.starts_with("ui.") {
            return SettingLookup::FreeUi;
        }
        SettingLookup::Unknown
    }

    /// Retorna um iterador sobre todos os descritores registrados.
    pub fn descriptors(&self) -> impl Iterator<Item = &SettingDescriptor> {
        self.descriptors.values()
    }
}

/// Valida que a chave tem entre 3 e 128 caracteres e segue `^[a-z][a-z0-9_]*(\.[a-z0-9_]+)+$`.
#[must_use]
pub fn is_valid_setting_key(key: &str) -> bool {
    (3..=128).contains(&key.len()) && SETTING_KEY_RE.is_match(key)
}
