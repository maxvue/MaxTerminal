//! Códigos de erro canônicos e construtores de [`MtError`] para `mt-commands`.

use mt_core::{ErrorCode, MtError};

/// Códigos de erro do domínio `artisan.*` e `shortcut.*`.
pub mod codes {
    use super::ErrorCode;

    /// A pasta informada não é uma aplicação Laravel válida.
    pub const NOT_LARAVEL: ErrorCode = ErrorCode("artisan.not_laravel");
    /// O caminho informado está fora de qualquer projeto cadastrado.
    pub const ROOT_FORBIDDEN: ErrorCode = ErrorCode("artisan.root_forbidden");
    /// Binário `php` não encontrado no PATH.
    pub const PHP_NOT_FOUND: ErrorCode = ErrorCode("artisan.php_not_found");
    /// Descoberta de comandos Artisan excedeu o tempo limite.
    pub const TIMEOUT: ErrorCode = ErrorCode("artisan.timeout");
    /// Subprocesso `php artisan list` encerrou com código de saída diferente de zero.
    pub const EXIT_NONZERO: ErrorCode = ErrorCode("artisan.exit_nonzero");
    /// Saída JSON do Symfony Console inválida ou corrompida.
    pub const INVALID_JSON: ErrorCode = ErrorCode("artisan.invalid_json");
    /// Saída JSON excedeu o limite máximo de 8 MiB.
    pub const OUTPUT_TOO_LARGE: ErrorCode = ErrorCode("artisan.output_too_large");
    /// Formulário de criação de comando Artisan inválido.
    pub const INVALID_FORM: ErrorCode = ErrorCode("artisan.invalid_form");
    /// Arquivo de destino já existe no disco.
    pub const FILE_EXISTS: ErrorCode = ErrorCode("artisan.file_exists");
    /// Caminho inseguro (symlink, travessia ou fora da raiz autorizada).
    pub const UNSAFE_PATH: ErrorCode = ErrorCode("artisan.unsafe_path");
    /// Falha de leitura ou gravação ao publicar o comando Artisan.
    pub const WRITE_FAILED: ErrorCode = ErrorCode("artisan.write_failed");
    /// Falha ao atualizar `routes/console.php`.
    pub const CONSOLE_ROUTES_FAILED: ErrorCode = ErrorCode("artisan.console_routes_failed");
    /// Revisão do preview divergente do estado atual do projeto.
    pub const PREVIEW_STALE: ErrorCode = ErrorCode("artisan.preview_stale");
    /// Escrita parcial detectada após falha em etapa subsequente.
    pub const PARTIAL_WRITE: ErrorCode = ErrorCode("artisan.partial_write");
    /// Runner configurado não suportado ou sem supervisão verificável.
    pub const RUNNER_UNSUPPORTED: ErrorCode = ErrorCode("artisan.runner_unsupported");

    /// Diretório de trabalho inválido ou fora de projeto autorizado para atalhos.
    pub const SHORTCUT_INVALID_CWD: ErrorCode = ErrorCode("shortcut.invalid_cwd");
    /// Atalho indisponível para os manifestos atuais do projeto.
    pub const SHORTCUT_UNRESOLVED: ErrorCode = ErrorCode("shortcut.unresolved");
    /// Conflito entre múltiplos lockfiles de gerenciadores JS sem `packageManager` declarado.
    pub const MANAGER_CONFLICT: ErrorCode = ErrorCode("shortcut.manager_conflict");
    /// Capacidade não suportada para a variante/versão do gerenciador detectado.
    pub const UNSUPPORTED_CAPABILITY: ErrorCode = ErrorCode("shortcut.unsupported_capability");
}

/// Constrói erro `artisan.not_laravel` em `pt-BR`.
#[must_use]
pub fn not_laravel(detail: impl Into<String>) -> MtError {
    MtError::new(codes::NOT_LARAVEL, detail.into())
}

/// Constrói erro `artisan.root_forbidden` em `pt-BR`.
#[must_use]
pub fn root_forbidden(detail: impl Into<String>) -> MtError {
    MtError::new(codes::ROOT_FORBIDDEN, detail.into())
}

/// Constrói erro `artisan.php_not_found` em `pt-BR`.
#[must_use]
pub fn php_not_found() -> MtError {
    MtError::new(
        codes::PHP_NOT_FOUND,
        "PHP não encontrado no PATH. Instale o PHP ou inicie o Sail (`./vendor/bin/sail up -d`).",
    )
}

/// Constrói erro `artisan.timeout` em `pt-BR`.
#[must_use]
pub fn timeout_error(secs: u64) -> MtError {
    MtError::new(
        codes::TIMEOUT,
        format!("A descoberta de comandos Artisan excedeu {secs} s e foi interrompida."),
    )
}

/// Constrói erro `artisan.exit_nonzero` com cauda sanitizada do `stderr`.
#[must_use]
pub fn exit_nonzero(code: Option<i32>, stderr_tail: String) -> MtError {
    let status_label = code.map_or_else(|| "sinal".to_string(), |c| c.to_string());
    MtError::new(
        codes::EXIT_NONZERO,
        format!("Falha ao executar `php artisan list` (saída {status_label})."),
    )
    .with_details(serde_json::json!({
        "exitCode": code,
        "stderrTail": stderr_tail,
    }))
}

/// Constrói erro `artisan.invalid_json` em `pt-BR`.
#[must_use]
pub fn invalid_json(detail: impl Into<String>) -> MtError {
    MtError::new(
        codes::INVALID_JSON,
        format!(
            "A saída de `php artisan list --format=json` não pôde ser interpretada: {}",
            detail.into()
        ),
    )
}

/// Constrói erro `artisan.output_too_large` em `pt-BR`.
#[must_use]
pub fn output_too_large(max_bytes: usize) -> MtError {
    MtError::new(
        codes::OUTPUT_TOO_LARGE,
        format!("A saída da descoberta Artisan excedeu o limite máximo de {max_bytes} bytes."),
    )
}

/// Constrói erro `artisan.unsafe_path` em `pt-BR`.
#[must_use]
pub fn unsafe_path(detail: impl Into<String>) -> MtError {
    MtError::new(codes::UNSAFE_PATH, detail.into())
}

/// Constrói erro `artisan.file_exists` em `pt-BR`.
#[must_use]
pub fn file_exists(rel_path: &str) -> MtError {
    MtError::new(
        codes::FILE_EXISTS,
        format!("O arquivo {rel_path} já existe; nada foi alterado."),
    )
}

/// Constrói erro `artisan.write_failed` em `pt-BR`.
#[must_use]
pub fn write_failed(detail: impl Into<String>) -> MtError {
    MtError::new(codes::WRITE_FAILED, detail.into())
}

/// Constrói erro `artisan.console_routes_failed` em `pt-BR`.
#[must_use]
pub fn console_routes_failed(detail: impl Into<String>) -> MtError {
    MtError::new(codes::CONSOLE_ROUTES_FAILED, detail.into())
}

/// Constrói erro `artisan.preview_stale` em `pt-BR`.
#[must_use]
pub fn preview_stale() -> MtError {
    MtError::new(
        codes::PREVIEW_STALE,
        "O projeto ou o formulário mudou desde a geração do preview. Revise o novo preview antes de criar.",
    )
}

/// Constrói erro `artisan.runner_unsupported` em `pt-BR`.
#[must_use]
pub fn runner_unsupported(detail: impl Into<String>) -> MtError {
    MtError::new(codes::RUNNER_UNSUPPORTED, detail.into())
}
