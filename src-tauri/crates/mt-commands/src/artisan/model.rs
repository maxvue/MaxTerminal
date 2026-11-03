//! Modelos de domínio, DTOs serializáveis e payloads de eventos para Comandos Artisan.

use mt_core::{MtError, ProjectId};
use serde::{Deserialize, Serialize};

/// Elemento individual do catálogo de comandos Artisan (compatível com `.max-code/commands/artisan.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanCommand {
    /// Nome canônico do comando (ex.: `"migrate:fresh"`).
    pub name: String,
    /// Descrição curta exibida no console.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Lista de opções (`--flags`) do comando.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ArtisanOption>,
    /// Lista ordenada de argumentos posicionais do comando.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<ArtisanArgument>,
    /// Apelidos do comando (ignorados pelo leitor legado do `MaxCode`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

/// Metadados de uma opção (`--flag`) de um comando Artisan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanOption {
    /// Nome da opção sempre precedido por `--` (ex.: `"--database"`).
    pub name: String,
    /// Descrição da opção.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Atalho curto (ex.: `"-d"` ou `"-v|-vv|-vvv"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
    /// Indica se a opção aceita valor (`--opt=valor`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept_value: Option<bool>,
    /// Indica se o valor é obrigatório quando a opção é informada.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_value_required: Option<bool>,
    /// Indica se a opção aceita múltiplos valores (array).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_multiple: Option<bool>,
    /// Valor padrão serializado em JSON (omitido quando `null`, `false` ou `[]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    /// Indica se a opção pertence ao conjunto global do Symfony Console.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_global: Option<bool>,
}

/// Metadados de um argumento posicional de um comando Artisan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanArgument {
    /// Nome do argumento posicional.
    pub name: String,
    /// Descrição do argumento.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Indica se o argumento é obrigatório.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Indica se o argumento recebe múltiplos valores (`*`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_array: Option<bool>,
    /// Valor padrão serializado em JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

/// Tipo de executor usado para descoberta e invocação do Artisan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    /// Binário `php` local no `PATH`.
    Local,
    /// Wrapper `./vendor/bin/sail` com contêiner ativo.
    Sail,
}

/// Estrutura de inicialização detectada no projeto Laravel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LaravelLayout {
    /// Laravel >= 11 (`Application::configure` em `bootstrap/app.php` com `routes/console.php`).
    Modern,
    /// Laravel <= 10 (`app/Console/Kernel.php`).
    Legacy,
    /// Layout customizado ou não reconhecido.
    Unknown,
}

/// Origem do catálogo Artisan retornado ao frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename = "ArtisanCatalogSource", rename_all = "snake_case")]
pub enum CatalogSource {
    /// Catálogo recém-descoberto via `php artisan list --format=json`.
    Fresh,
    /// Catálogo lido do cache validado por sidecar e fingerprint.
    Cache,
    /// Catálogo lido de `.max-code/commands/artisan.json` sem sidecar válido (ex.: gravado pelo `MaxCode`).
    ForeignCache,
}

/// Metadados de uma raiz Laravel resolvida para uma aba/painel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanRoot {
    /// Caminho absoluto canônico da raiz onde está o arquivo `artisan`.
    pub root: String,
    /// Identificador do projeto proprietário no `MaxTerminal`.
    pub project_id: Option<ProjectId>,
    /// Indica se a raiz resolvida é uma Git worktree isolada.
    pub is_worktree: bool,
    /// Prefixo textual pronto para inserção no input relativo ao `cwd`.
    pub invoker: String,
    /// Tokens do invocador para comparação estruturada pelo parser de linha.
    pub invoker_tokens: Vec<String>,
    /// Runner selecionado para esta raiz.
    pub runner: RunnerKind,
    /// Layout Laravel detectado (`Modern`, `Legacy` ou `Unknown`).
    pub layout: LaravelLayout,
    /// Versão do `laravel/framework` extraída de `composer.lock` (sem prefixo `v`).
    pub laravel_version: Option<String>,
}

/// Resumo de erro da última tentativa de descoberta quando um cache antigo é servido.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogErrorInfo {
    /// Código estável do erro (`artisan.*`).
    pub code: String,
    /// Mensagem amigável em `pt-BR`.
    pub message: String,
}

/// Catálogo completo de comandos Artisan para uma raiz Laravel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanCatalog {
    /// Caminho canônico da raiz Laravel.
    pub root: String,
    /// Comandos visíveis ordenados alfabeticamente por `name`.
    pub commands: Vec<ArtisanCommand>,
    /// Origem dos dados (`fresh`, `cache` ou `foreign_cache`).
    pub source: CatalogSource,
    /// Indica se há revalidação em segundo plano em andamento ou se o cache está desatualizado.
    pub stale: bool,
    /// Timestamp Unix em milissegundos da geração do catálogo.
    pub generated_at_ms: Option<i64>,
    /// Erro da última revalidação quando o cache anterior foi preservado.
    pub last_error: Option<CatalogErrorInfo>,
}

impl ArtisanCatalog {
    /// Retorna uma cópia marcada como `stale` anexando informações do erro ocorrido.
    #[must_use]
    pub fn stale_with_error(&self, err: &MtError) -> Self {
        let mut copy = self.clone();
        copy.stale = true;
        copy.last_error = Some(CatalogErrorInfo {
            code: err.code.0.to_string(),
            message: err.message.to_string(),
        });
        copy
    }
}

/// Payload do evento `artisan://catalog-updated`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogUpdatedPayload {
    /// Caminho canônico da raiz Laravel.
    pub root: String,
    /// Projeto proprietário, quando conhecido.
    pub project_id: Option<ProjectId>,
    /// Quantidade de comandos visíveis no catálogo atualizado.
    pub command_count: u32,
    /// Origem do catálogo (`fresh`, `cache` ou `foreign_cache`).
    pub source: CatalogSource,
    /// Timestamp Unix em milissegundos.
    pub generated_at_ms: i64,
    /// Revisão monotônica por raiz durante o tempo de vida do processo.
    pub revision: u64,
    /// Identificador de correlação da operação.
    pub request_id: String,
}

/// Payload do evento `artisan://discovery-failed`.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryFailedPayload {
    /// Caminho canônico da raiz Laravel.
    pub root: String,
    /// Projeto proprietário, quando conhecido.
    pub project_id: Option<ProjectId>,
    /// Erro estruturado sanitizado.
    pub error: MtError,
    /// Revisão monotônica por raiz durante o tempo de vida do processo.
    pub revision: u64,
    /// Identificador de correlação da operação.
    pub request_id: String,
}

/// Payload do evento `artisan://command-created`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandCreatedPayload {
    /// Caminho canônico da raiz Laravel.
    pub root: String,
    /// Projeto proprietário, quando conhecido.
    pub project_id: Option<ProjectId>,
    /// Caminho relativo do arquivo PHP criado (`app/Console/Commands/...`).
    pub path: String,
    /// Nome da classe PHP criada.
    pub class_name: String,
    /// Nome do comando na signature (`reports:send-weekly`).
    pub signature_name: String,
    /// Revisão monotônica por raiz durante o tempo de vida do processo.
    pub revision: u64,
    /// Identificador de correlação da operação.
    pub request_id: String,
}
