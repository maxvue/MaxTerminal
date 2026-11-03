//! Erros tipados do subsistema de temas (`ThemeError`) e conversão para `MtError`.

use mt_core::{ErrorCode, MtError};
use mt_db::DbError;
use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

/// Erros de domínio do subsistema de temas e importador Tabby.
#[derive(Debug, Error, Serialize, Deserialize, Type, Clone)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum ThemeError {
    /// Tema não encontrado pelo ID informado.
    #[error("Tema não encontrado: {id}")]
    NotFound {
        /// Identificador buscado.
        id: String,
    },
    /// Tentativa de remover ou sobrescrever um tema nativo embutido.
    #[error("Tema nativo '{id}' é somente leitura e não pode ser removido ou sobrescrito")]
    BuiltinReadOnly {
        /// Identificador do tema nativo.
        id: String,
    },
    /// Tentativa de excluir o tema atualmente ativo sem informar `fallback_id` válido.
    #[error("Não é possível remover o tema ativo '{id}' sem informar um tema substituto válido")]
    CannotDeleteActive {
        /// Identificador do tema ativo.
        id: String,
    },
    /// Conflito de nome ou identificador quando `OverwritePolicy::Reject` está ativa.
    #[error("Já existe um tema com este identificador ou nome ({id})")]
    Conflict {
        /// Identificador conflitante.
        id: String,
    },
    /// Cor em formato inválido ou fora da especificação suportada.
    #[error("Cor inválida no campo '{field}': '{value}' ({reason})")]
    InvalidColor {
        /// Nome do campo onde a cor inválida foi encontrada.
        field: String,
        /// Valor bruto recebido.
        value: String,
        /// Motivo da falha de parsing.
        reason: String,
    },
    /// Campo obrigatório ausente ou fora dos limites permitidos.
    #[error("Campo inválido '{field}': {reason}")]
    InvalidField {
        /// Nome do campo.
        field: String,
        /// Explicação da validação violada.
        reason: String,
    },
    /// Construto YAML proibido por política de segurança (âncoras, aliases, merge keys, tags).
    #[error("Construto YAML proibido na linha {line}, coluna {column}: {construct}")]
    UnsafeYamlConstruct {
        /// Linha (1-indexed).
        line: usize,
        /// Coluna (1-indexed).
        column: usize,
        /// Nome do construto bloqueado.
        construct: String,
    },
    /// Arquivo excede o tamanho máximo permitido (`MAX_THEME_FILE_BYTES`).
    #[error("Arquivo excede o limite de {max_bytes} bytes (tamanho: {actual_bytes} bytes)")]
    FileTooLarge {
        /// Tamanho observado em bytes.
        actual_bytes: u64,
        /// Limite máximo permitido em bytes.
        max_bytes: u64,
    },
    /// Caminho rejeitado por conter symlink fora do diretório ou arquivo especial.
    #[error("Caminho inseguro rejeitado: {reason}")]
    UnsafePath {
        /// Motivo da rejeição.
        reason: String,
    },
    /// Formato de arquivo irreconhecível ou sintaxe YAML/JSON inválida.
    #[error("Formato de esquema não suportado ou inválido: {reason}")]
    UnsupportedFormat {
        /// Motivo detalhado.
        reason: String,
    },
    /// Nenhum esquema válido foi encontrado no arquivo analisado.
    #[error("Nenhum esquema de cores válido foi encontrado no arquivo")]
    NoSchemesFound,
    /// Contraste insuficiente entre texto principal e fundo (`< 3.0:1`).
    #[error("Contraste insuficiente entre texto e fundo ({ratio:.2}:1; mínimo exigido: 3.0:1)")]
    InsufficientContrast {
        /// Razão de contraste calculada.
        ratio: f32,
    },
    /// Fonte monoespaçada inválida (caracteres proibidos na pilha CSS).
    #[error("Fonte inválida: '{family}' ({reason})")]
    InvalidFont {
        /// Família informada.
        family: String,
        /// Motivo da rejeição.
        reason: String,
    },
    /// Falha de leitura/gravação de arquivo.
    #[error("Erro de E/S ao processar arquivo de tema: {message}")]
    Io {
        /// Descrição sanitizada do erro de I/O.
        message: String,
    },
    /// Falha na camada de persistência SQLite.
    #[error("Erro de banco de dados ao persistir tema: {message}")]
    Database {
        /// Descrição sanitizada do erro de banco.
        message: String,
    },
}

impl ThemeError {
    /// Código canônico estável (`theme.*`) exposto no `MtError.detail`.
    #[must_use]
    pub const fn domain_code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "theme.not_found",
            Self::BuiltinReadOnly { .. } => "theme.builtin_read_only",
            Self::CannotDeleteActive { .. } => "theme.cannot_delete_active",
            Self::Conflict { .. } => "theme.conflict",
            Self::InvalidColor { .. } => "theme.invalid_color",
            Self::InvalidField { .. } => "theme.invalid_field",
            Self::UnsafeYamlConstruct { .. } => "theme.unsafe_yaml_construct",
            Self::FileTooLarge { .. } => "theme.file_too_large",
            Self::UnsafePath { .. } => "theme.unsafe_path",
            Self::UnsupportedFormat { .. } => "theme.unsupported_format",
            Self::NoSchemesFound => "theme.no_schemes_found",
            Self::InsufficientContrast { .. } => "theme.insufficient_contrast",
            Self::InvalidFont { .. } => "theme.invalid_font",
            Self::Io { .. } => "theme.io",
            Self::Database { .. } => "theme.database",
        }
    }
}

impl From<DbError> for ThemeError {
    fn from(err: DbError) -> Self {
        match err {
            DbError::ConstraintViolation { constraint } if constraint.contains("builtin") => {
                Self::BuiltinReadOnly { id: constraint }
            }
            DbError::ConstraintViolation { constraint } => Self::Conflict { id: constraint },
            other => Self::Database {
                message: other.to_string(),
            },
        }
    }
}

impl From<std::io::Error> for ThemeError {
    fn from(err: std::io::Error) -> Self {
        Self::Io {
            message: err.to_string(),
        }
    }
}

impl From<ThemeError> for MtError {
    fn from(err: ThemeError) -> Self {
        let code = ErrorCode(err.domain_code());
        let detail = serde_json::json!({
            "themeCode": err.domain_code(),
            "error": &err,
        });

        Self::new(code, err.to_string()).with_details(detail)
    }
}
