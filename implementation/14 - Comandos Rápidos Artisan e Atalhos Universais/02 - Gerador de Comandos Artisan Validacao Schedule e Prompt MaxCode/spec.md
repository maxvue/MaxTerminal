# Implementação 02 — Gerador de Comandos Artisan, Validação, Schedule e Prompt MaxCode (`spec.md`)

> **Fase:** 14 — Comandos Rápidos: Artisan (`>`) e Atalhos Universais (`/`)
> **Unidade:** `02 - Gerador de Comandos Artisan Validacao Schedule e Prompt MaxCode`
> **Referências Canônicas:** [`../../AGENTS.md`](../../../AGENTS.md) §8.1, [`../instructions.md`](../instructions.md) (I-10 a I-13, CA-18 a CA-24, CA-33, CA-39) e [`../spec.md`](../spec.md) (§7, §14, §15.1).

---

## 1. Contexto, Objetivo e Escopo

Esta segunda implementação constrói o submódulo `mt-commands::artisan::generator`, responsável por validar o formulário do modal "Criar comando", exportar o JSON Schema Draft 2020-12 para a Geração Assistida por IA (Fase 12), montar a signature do Symfony Console, resolver o namespace PSR-4 a partir do `composer.json`, renderizar de forma determinística o arquivo PHP `app/Console/Commands/<Classe>.php`, planejar/aplicar o agendamento (`Schedule`) em `routes/console.php` novo ou existente (instruções manuais apenas em layouts legados/ambíguos) e formatar o prompt exato para o Plan Mode do MaxCode (`"Criar comando artisan:\n${dados_formatados_do_formulario}"`).

### 1.1. Objetivos Específicos
1. Definir o modelo de formulário `ArtisanCommandForm` e tipos associados (`ArtisanParam`, `ParamKind`, `ScheduleSpec`, `ScheduleOption`, `ScheduleFrequency`, `ValidationReport`, `FieldIssue`, `ArtisanPreview`, `ConsoleRoutesPatch`, `ArtisanCreateResult`) com `specta::Type`, `schemars::JsonSchema` e `#[serde(deny_unknown_fields)]`.
2. Implementar a validação única (`validate.rs`) utilizada tanto pela UI quanto pela Geração Assistida por IA e pela gravação final em disco, cobrindo 100% das regras de negócio da tabela §7.2 da spec.
3. Implementar o construtor de `$signature` (`signature.rs`), o resolvedor PSR-4 (`psr4.rs`) e o gerador de código PHP (`template.rs`) com suporte a `Isolatable`, `PromptsForMissingInput`, `promptForMissingArgumentsUsing()`, escapes seguros de strings e PHPDoc (`*/` → `*\/`) e prevenção de colisão de variáveis (`$this`, prefixo numérico, colisão camelCase e imports).
4. Implementar o renderizador (`schedule.rs`) e o planejador (`console_routes.rs`): criar exclusivamente `routes/console.php` ausente em layout moderno validado, ou inserir bloco delimitado idempotente em arquivo existente após preview/confirmação. Layout Legacy (`app/Console/Kernel.php`) ou ambíguo recebe passos manuais (T-18).
5. Implementar o formatador `format_maxcode_prompt` (`maxcode_prompt.rs`) gerando a mensagem exata do snapshot `maxcode_prompt__full_form.snap`, sanitizada via `mt_db::sanitizer::sanitize`.
6. Implementar o orquestrador `CommandGenerator` (`generator/mod.rs`) garantindo igualdade byte a byte entre `preview` e `create` (CA-19), verificação de `expected_revision` (`artisan.preview_stale`), escrita exclusiva sem sobrescrita (`artisan.file_exists`), proteção contra path traversal e symlinks (`artisan.unsafe_path`), rollback seguro que preserva edições externas (`artisan.partial_write`), invalidação do `ArtisanService` e emissão de `artisan://command-created`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Modificados em `src-tauri/crates/mt-commands/`
- `src-tauri/crates/mt-commands/src/artisan/mod.rs` — exposição de `pub mod generator;`.
- `src-tauri/crates/mt-commands/src/error.rs` — adição dos construtores de erro `invalid_form`, `file_exists`, `write_failed`, `console_routes_failed`, `preview_stale` e `partial_write`.

### 2.2. Novos Arquivos em `src-tauri/crates/mt-commands/src/artisan/generator/`
- `src-tauri/crates/mt-commands/src/artisan/generator/mod.rs`
- `src-tauri/crates/mt-commands/src/artisan/generator/form.rs`
- `src-tauri/crates/mt-commands/src/artisan/generator/validate.rs`
- `src-tauri/crates/mt-commands/src/artisan/generator/signature.rs`
- `src-tauri/crates/mt-commands/src/artisan/generator/psr4.rs`
- `src-tauri/crates/mt-commands/src/artisan/generator/template.rs`
- `src-tauri/crates/mt-commands/src/artisan/generator/schedule.rs`
- `src-tauri/crates/mt-commands/src/artisan/generator/console_routes.rs`
- `src-tauri/crates/mt-commands/src/artisan/generator/maxcode_prompt.rs`

### 2.3. Testes e Snapshots a Criar
- `src-tauri/crates/mt-commands/tests/generator.rs`
- `src-tauri/crates/mt-commands/tests/laravel_integration.rs`
- `src-tauri/crates/mt-commands/src/artisan/generator/snapshots/mt_commands__artisan__generator__tests__template__full_form.snap` (ou caminho de snapshot `insta` correspondente)
- `src-tauri/crates/mt-commands/src/artisan/generator/snapshots/mt_commands__artisan__generator__tests__template__minimal.snap`
- `src-tauri/crates/mt-commands/src/artisan/generator/snapshots/mt_commands__artisan__generator__tests__template__arrays.snap`
- `src-tauri/crates/mt-commands/src/artisan/generator/snapshots/mt_commands__artisan__generator__tests__maxcode_prompt__full_form.snap`
- `src-tauri/crates/mt-commands/src/artisan/generator/snapshots/mt_commands__artisan__generator__tests__console_routes__fresh_laravel11.snap`
- `src-tauri/crates/mt-commands/src/artisan/generator/snapshots/mt_commands__artisan__generator__tests__console_routes__idempotent.snap`

---

## 3. Especificação Técnica e Assinaturas Exatas

### 3.1. Modelos do Formulário e Resultado (`src/artisan/generator/form.rs`)

```rust
use serde::{Deserialize, Serialize};

/// Formulário do modal "Criar comando" — também exportado como JSON Schema Draft 2020-12 (`artisan.command.v1`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtisanCommandForm {
    /// Nome da classe PHP em PascalCase, ex.: SendWeeklyReport.
    pub class_name: String,
    /// Nome do comando na signature, ex.: reports:send-weekly.
    pub signature_name: String,
    /// Descrição curta exibida em `php artisan list`.
    pub description: String,
    #[serde(default)]
    pub params: Vec<ArtisanParam>,
    #[serde(default)]
    pub isolatable: bool,
    #[serde(default)]
    pub prompts_for_missing_input: bool,
    #[serde(default)]
    pub schedule: Option<ScheduleSpec>,
    /// Objetivo e regras de negócio do comando (vai para o PHPDoc e para o MaxCode).
    #[serde(default)]
    pub objective: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    Argument,
    Option,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtisanParam {
    pub kind: ParamKind,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub is_array: bool,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub accepts_value: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleSpec {
    pub frequency: ScheduleFrequency,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub options: Vec<ScheduleOption>,
    #[serde(default)]
    pub without_overlapping: bool,
    #[serde(default)]
    pub on_one_server: bool,
    #[serde(default)]
    pub run_in_background: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleOption {
    pub name: String,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScheduleFrequency {
    EveryMinute,
    EveryFiveMinutes,
    EveryFifteenMinutes,
    EveryThirtyMinutes,
    Hourly,
    HourlyAt { minute: u8 },
    Daily,
    DailyAt { time: String },
    TwiceDaily { first: u8, second: u8 },
    Weekly,
    WeeklyOn { day: u8, time: String },
    Monthly,
    MonthlyOn { day: u8, time: String },
    Cron { expression: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FieldIssue {
    pub path: String,
    pub severity: IssueSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub valid: bool,
    pub issues: Vec<FieldIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleRoutesPatch {
    pub path: String,
    pub will_create: bool,
    pub added_lines: Vec<String>,
    pub manual_only: bool,
    pub base_sha256: Option<String>,
    pub unified_diff: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanPreview {
    pub revision: String,
    pub report: ValidationReport,
    pub path: String,
    pub signature: String,
    pub php: Option<String>,
    pub console_patch: Option<ConsoleRoutesPatch>,
    pub manual_steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanCreateResult {
    pub path: String,
    pub console_routes_patched: bool,
    pub manual_steps: Vec<String>,
    pub catalog_refresh_pending: bool,
}

pub fn artisan_form_json_schema() -> serde_json::Value;
```

### 3.2. Validador Único (`src/artisan/generator/validate.rs`)

```rust
pub struct ValidationContext<'a> {
    pub root: &'a Path,
    pub catalog_commands: &'a [ArtisanCommand],
    pub laravel_version: Option<&'a str>,
    pub psr4_namespace: Option<&'a str>,
}

pub fn validate_form(form: &ArtisanCommandForm, ctx: &ValidationContext<'_>) -> ValidationReport;
pub fn validate_for_maxcode_prompt(form: &ArtisanCommandForm) -> ValidationReport;
pub fn is_php_reserved_word(ident: &str) -> bool;
pub fn validate_cron_expression(expr: &str) -> Result<(), (usize, &'static str)>;
```

**Regras Exatas de Validação (CA-18, CA-39):**
1. **Limites Globais:** Tamanho serializado do formulário ≤ 64 KiB e `form.params.len() <= 100`.
2. **`className` (`path = "className"`):**
   - Regex `^[A-Z][A-Za-z0-9]{0,63}$` → código `"invalid_class_name"`, mensagem `"Use PascalCase (ex.: SendWeeklyReport)."`.
   - Lista completa de palavras reservadas e tipos internos do PHP 8.3 (comparação ASCII case-insensitive: `__halt_compiler`, `abstract`, `and`, `array`, `as`, `break`, `callable`, `case`, `catch`, `class`, `clone`, `const`, `continue`, `declare`, `default`, `die`, `do`, `echo`, `else`, `elseif`, `empty`, `enddeclare`, `endfor`, `endforeach`, `endif`, `endswitch`, `endwhile`, `enum`, `eval`, `exit`, `extends`, `false`, `final`, `finally`, `fn`, `for`, `foreach`, `function`, `global`, `goto`, `if`, `implements`, `include`, `include_once`, `instanceof`, `insteadof`, `interface`, `isset`, `list`, `match`, ` mixed`, `namespace`, `never`, `new`, `null`, `numeric`, `object`, `or`, `parent`, `print`, `private`, `protected`, `public`, `readonly`, `require`, `require_once`, `resource`, `return`, `self`, `static`, `string`, `switch`, `throw`, `trait`, `true`, `try`, `unset`, `use`, `var`, `void`, `while`, `xor`, `yield`, `int`, `float`, `bool`) → código `"php_reserved_word"`, mensagem `"\"{className}\" é palavra reservada do PHP."`.
   - Arquivo `app/Console/Commands/<className>.php` já existe em `ctx.root` → código `"file_exists"`, mensagem `"O arquivo app/Console/Commands/{className}.php já existe."`.
3. **`signatureName` (`path = "signatureName"`):**
   - Regex `^[a-z][a-z0-9-]*(:[a-z0-9][a-z0-9-]*)*$` e comprimento `1..=100` → código `"invalid_signature_name"`, mensagem `"Signature inválida: use minúsculas, `-` e `:`."`.
   - Colisão com `cmd.name` ou `cmd.aliases` de `ctx.catalog_commands` → código `"duplicate_signature"`, mensagem `"Já existe o comando \"{signatureName}\"."`.
4. **`description` (`path = "description"`) e `params[i].description` (`path = "params[i].description"`):**
   - `description` do comando: 1..=255 caracteres (após `trim`); `params[i].description`: 0..=255 caracteres.
   - Proibido conter `{`, `}`, `\r`, `\n` ou `\0` → código `"invalid_description_chars"`, mensagem `"Descrições não podem conter { ou }."`.
5. **`params[i]`:**
   - `name` (`path = "params[i].name"`): regex `^[a-z][a-z0-9_-]*$`, 1..=64 caracteres.
   - Unicidade entre **todos** os argumentos e opções → código `"duplicate_param_name"`, mensagem `"Nome duplicado: \"{name}\"."`.
   - Regras de `ParamKind::Argument`:
     - `shortcut` deve ser `None` (ou vazio) e `accepts_value` deve ser `false` → código `"incompatible_argument_fields"`.
     - Um argumento obrigatório (`!optional && default.is_none()`) não pode aparecer após um argumento opcional (`optional || default.is_some()`) → código `"required_after_optional"`, mensagem `"Argumento obrigatório não pode vir após um opcional."`.
     - `is_array == true` só é permitido no **último** argumento da lista de argumentos → código `"array_not_last_argument"`, mensagem `"Apenas o último argumento pode ser array."`.
     - Argumento obrigatório (`!optional`) não pode ter `default` preenchido → código `"default_on_required_argument"`, mensagem `"Argumento obrigatório não pode ter valor padrão."`.
   - Regras de `ParamKind::Option`:
     - `optional` não se aplica (deve ser `false`); se `is_array == true`, então `accepts_value` deve ser `true` (array de flag sem valor é inválido → `"invalid_option_array"`).
     - Nome não pode pertencer às opções globais reservadas `{ "help", "quiet", "verbose", "version", "ansi", "no-ansi", "no-interaction", "env" }`, nem ser `"isolated"` quando `form.isolatable == true` → código `"reserved_option_name"`, mensagem `"\"--{name}\" é uma opção global reservada."`.
     - `shortcut` (`path = "params[i].shortcut"`): quando presente, deve casar `^[A-Za-z]$`, ser único entre todas as opções e não pertencer a `{'h', 'q', 'V', 'n', 'v'}` → código `"reserved_or_invalid_shortcut"`, mensagem `"Atalho reservado pelo Symfony Console."` ou `"Atalho inválido: use uma única letra."`.
   - `default` (`path = "params[i].default"`): quando presente e não vazio, proibido em flag sem valor (`Option` com `!accepts_value`) e proibido quando `is_array == true`; deve casar `^[^{}\s][^{}\r\n\0]*$` e não conter `" : "` → código `"invalid_default"`, mensagem `"Valor padrão inválido."`.
6. **Interfaces e Capacidades (`isolatable`, `prompts_for_missing_input`):**
   - `prompts_for_missing_input` exige Laravel ≥ 9.40 (quando `ctx.laravel_version` for conhecida e `< 9.40.0`, emite erro em `path = "promptsForMissingInput"` informando a versão instalada).
   - Se `prompts_for_missing_input == true` e não houver nenhum argumento obrigatório (`Argument` com `!optional && default.is_none()`), adiciona `FieldIssue` com `severity: IssueSeverity::Warning`, `path: "promptsForMissingInput"`, `code: "prompts_without_required_arguments"`, mensagem `"Sem argumentos obrigatórios: PromptsForMissingInput não terá efeito."`.
7. **`schedule` (`path = "schedule..."`):**
   - Valida `HourlyAt.minute <= 59`.
   - Valida `DailyAt.time`, `WeeklyOn.time`, `MonthlyOn.time` no formato estrito `HH:MM` (`00:00` a `23:59`).
   - Valida `TwiceDaily { first, second }` com `first <= 23`, `second <= 23` e `first != second`.
   - Valida `WeeklyOn.day <= 6` e `1 <= MonthlyOn.day <= 31`.
   - Valida `Cron { expression }` com exatos 5 campos separados por espaço (minuto `0..=59`, hora `0..=23`, dia do mês `1..=31`, mês `1..=12`, dia da semana `0..=7`), aceitando `*`, inteiros `n`, faixas `a-b` (`a <= b`), listas `a,b` e passos `/step` (`step >= 1`) → código `"invalid_cron"`, mensagem `"Expressão cron inválida no campo \"{campo}\"."`.
   - Valida que `schedule.arguments` fornece valor não vazio (e sem `\r`, `\n`, `\0`) para cada argumento obrigatório da signature na ordem posicional.
   - Valida que cada item habilitado em `schedule.options` existe como `ParamKind::Option` no formulário, respeita se aceita valor/array e não contém `\r`, `\n`, `\0`.
8. **`objective` (`path = "objective"`):** `objective.chars().count() <= 8_000` → código `"objective_too_long"`, mensagem `"Objetivo muito longo (máx. 8 000)."`.
9. **PSR-4 (`path = "className"`):** Se `ctx.psr4_namespace` for `None`, emite erro bloqueante `"psr4_unresolved"`, mensagem `"Não foi possível resolver o namespace PSR-4 para app/Console/Commands/ no composer.json."`.

### 3.3. Signature, PSR-4 e Template PHP (`signature.rs`, `psr4.rs`, `template.rs`)

```rust
// signature.rs
pub fn param_token(param: &ArtisanParam) -> String;
pub fn build_signature(form: &ArtisanCommandForm) -> String;

// psr4.rs
pub fn resolve_commands_namespace(root: &Path) -> MtResult<Option<String>>;

// template.rs
pub fn render_php_command(form: &ArtisanCommandForm, namespace: &str) -> String;
```

**Regras de Geração PHP (`template.rs`, CA-20, CA-33, CA-39):**
- `build_signature`:
  - Ordena os parâmetros mantendo primeiro todos os `ParamKind::Argument` (na ordem do formulário) e depois todos os `ParamKind::Option` (na ordem do formulário).
  - Se o total de parâmetros for `<= 1`, gera em linha única: `reports:send-weekly {user : ID do usuário}` (ou apenas `reports:send-weekly` se 0 parâmetros).
  - Se houver `>= 2` parâmetros, gera em múltiplas linhas com recuo de 28 espaços a partir da segunda linha (padrão `make:command` do Laravel, conforme §7.6 da spec).
- Prevenção de colisão de identificadores PHP em `template.rs`:
  - Se `form.class_name` colidir com `Command`, `Isolatable` ou `PromptsForMissingInput`, o `use` correspondente usa alias estável (`use Illuminate\Console\Command as BaseCommand;`).
  - Na geração de variáveis locais dentro de `handle()`, converte o nome do parâmetro para `camelCase`. Se o identificador resultante for `"this"`, começar com dígito, ou colidir com outra variável já emitida em `handle()` (ex.: `user-id` e `user_id` ambos gerando `$userId`), utiliza nomes desambiguados ou indexação direta sem sobrescrever variáveis.
- Escapes obrigatórios:
  - Literais entre aspas simples PHP: `\` → `\\` e `'` → `\'`.
  - Comentários PHPDoc (`description` e `objective`): toda ocorrência de `*/` é escapada para `*\/`, e linhas em branco de `objective` produzem ` *` sem trailing whitespace.

### 3.4. Agendamento e `routes/console.php` (`schedule.rs`, `console_routes.rs`)

```rust
// schedule.rs
pub fn render_schedule_chain(spec: &ScheduleSpec) -> String;
pub fn render_modern_schedule_statement(class_ref: &str, spec: &ScheduleSpec) -> String;
pub fn render_legacy_kernel_statement(fqcn: &str, spec: &ScheduleSpec) -> String;

// console_routes.rs
#[derive(Debug, Clone)]
pub struct ConsoleRoutesPlan {
    pub patch: Option<ConsoleRoutesPatch>,
    pub file_bytes_to_create: Option<Vec<u8>>,
    pub file_bytes_to_update: Option<Vec<u8>>,
    pub expected_source_sha256: Option<String>,
    pub manual_steps: Vec<String>,
}

pub fn plan_console_routes(
    root: &Path,
    layout: LaravelLayout,
    console_routes_loaded: bool,
    namespace: &str,
    form: &ArtisanCommandForm,
) -> MtResult<ConsoleRoutesPlan>;
```

**Regras Críticas de `routes/console.php` (CA-20, CA-21, CA-22, CA-39):**
- Se `form.schedule` for `None`: retorna `ConsoleRoutesPlan` vazio.
- Se `layout != LaravelLayout::Modern` ou `!console_routes_loaded`:
  - Nada é escrito em disco; retorna `manual_steps` com a instrução em `pt-BR` contendo `$schedule->command(\Namespace\Class::class, ...)->...;` para `app/Console/Kernel.php` (se `Legacy`) ou `bootstrap/app.php` / `routes/console.php` (CA-22).
- Se `layout == LaravelLayout::Modern` e `console_routes_loaded == true`:
  - Se `routes/console.php` **não existir**: prepara `file_bytes_to_create` com `<?php\n\nuse Illuminate\\Support\\Facades\\Schedule;\nuse {fqcn};\n\n{statement}\n`, marca `patch.will_create = true, patch.manual_only = false` e gera o `unified_diff`. Na gravação (`create`), publica `routes/console.php` usando criação estritamente exclusiva (`create_new_file_under_root`).
   - Se `routes/console.php` **já existir**: lê com `read_capped(256 KiB)` e reconhece tokens PHP de topo (`namespace`, `declare`, imports/aliases, strings, comentários, heredoc e `?>`). Em estrutura suportada, prepara `file_bytes_to_update` e `expected_source_sha256`; insere antes de `?>` ou no fim do segmento PHP o bloco `// <maxterminal:schedule:Classe>` … `// </maxterminal:schedule:Classe>` com FQCNs, preservando todos os bytes externos. `patch.manual_only = false`; exibe `unified_diff` e `added_lines` para confirmação explícita via Criar com `expected_revision`.
   - Registro equivalente já presente gera patch vazio; nunca duplicar. Delimitadores conflitantes/duplicados, registro customizado divergente ou estrutura não reconhecida geram `manual_only = true` e `manual_steps`. Arquivo novo também usa bloco delimitado. Preview nunca escreve; o hash da base integra sua revisão.

### 3.5. Formatador do Prompt MaxCode (`src/artisan/generator/maxcode_prompt.rs`)

```rust
pub fn format_maxcode_prompt(
    form: &ArtisanCommandForm,
    layout: LaravelLayout,
    laravel_version: Option<&str>,
) -> MtResult<String>;
```

- Valida que `class_name` e `signature_name` não estão vazios (senão retorna `artisan.invalid_form`).
- Produz exatamente o formato textual definido em `spec.md` §7.3 (começando por `"Criar comando artisan:\n"`), aplicando `mt_db::sanitizer::sanitize` ao final.

### 3.6. Orquestrador `CommandGenerator` (`src/artisan/generator/mod.rs`)

```rust
pub struct CommandGenerator {
    artisan: Arc<ArtisanService>,
    roots: Arc<RootResolver>,
    sink: Arc<dyn CommandsEventSink>,
    locks: DashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>,
}

impl CommandGenerator {
    pub fn new(
        artisan: Arc<ArtisanService>,
        roots: Arc<RootResolver>,
        sink: Arc<dyn CommandsEventSink>,
    ) -> Self;

    pub fn form_schema(&self) -> serde_json::Value;

    pub async fn validate(&self, root: &Path, form: &ArtisanCommandForm) -> MtResult<ValidationReport>;

    pub async fn preview(&self, root: &Path, form: &ArtisanCommandForm) -> MtResult<ArtisanPreview>;

    pub async fn create(
        &self,
        root: &Path,
        form: &ArtisanCommandForm,
        expected_revision: &str,
    ) -> MtResult<ArtisanCreateResult>;

    pub fn format_maxcode_prompt(&self, root: &Path, form: &ArtisanCommandForm) -> MtResult<String>;
}
```

**Fluxo Transacional e Rollback de `CommandGenerator::create` (CA-19, CA-20, CA-21, CA-23, CA-39):**
1. Adquire o `Mutex` de geração para `root`.
2. Executa o cálculo interno de `preview_locked(root, form)`.
3. Se `!preview.report.valid`:
   - Se a única/primeira issue for `"file_exists"`, retorna `Err(error::file_exists(&preview.path))`.
   - Caso contrário, retorna `Err(error::invalid_form(preview.report))`.
4. Compara `preview.revision == expected_revision`. Se divergir, retorna `Err(error::preview_stale(&preview.revision))` sem tocar no disco (CA-21).
5. Grava `app/Console/Commands/<class_name>.php` via `create_new_file_under_root` com os bytes exatos de `preview.php.as_ref().unwrap()` e registra `(inode, size, sha256)` do arquivo recém-criado.
6. Se `console_plan.file_bytes_to_create` for `Some(routes_bytes)`:
   - Tenta criar `routes/console.php` via `create_new_file_under_root`.
   - Se falhar: verifica se `app/Console/Commands/<class_name>.php` ainda possui o mesmo `(inode, size, sha256)`. Se estiver intacto, remove-o (`std::fs::remove_file`) e retorna `artisan.console_routes_failed` (ou `artisan.preview_stale` se `routes/console.php` surgiu concorrentemente). Se o comando PHP já tiver sido modificado externamente ou a remoção falhar, preserva-o e retorna `artisan.partial_write` com `createdPaths`, `preservedPaths` e `manualSteps` (CA-21, CA-39).
7. Se `file_bytes_to_update` for `Some`, revalida identidade/hash da base sob o mesmo lock imediatamente antes da publicação; mudança detectada retorna `artisan.preview_stale` e aplica recuperação do passo 6 à classe recém-criada. Preserva backup original/permissões e publica temporário próprio sincronizado no mesmo diretório. Em falha, só restaura backup/remove classe quando identidade/bytes ainda correspondem à operação; caso contrário retorna `artisan.partial_write`, preservando alterações externas. Hash+rename não é CAS contra editor não cooperativo: há janela residual, conforme §7.7 da spec da fase; não prometer transação multi-arquivo.
8. Emite `artisan://command-created` via `sink.command_created(...)`, chama `self.artisan.invalidate(root)` e dispara revalidação assíncrona em background. Se a revalidação não terminar imediatamente, retorna `ArtisanCreateResult` com `catalog_refresh_pending: true` (uma falha posterior de discovery nunca desfaz nem reexecuta a criação do arquivo — CA-23, CA-39).

---

## 4. Tratamento de Erros e Casos de Borda

1. **Injeção de fechamento de comentário PHPDoc (`*/`) em `description` ou `objective`:** Escapado para `*\/` antes de renderizar o PHPDoc; testado com `php -l` e snapshot (CA-39).
2. **Parâmetros com nomes conflitantes em camelCase (`foo-bar` e `foo_bar`) ou nome `this`:** `handle()` gera `$this_arg` / `$fooBar_2` (ou acessa pelo nome original) sem gerar `$this = ...` (que é erro fatal de compilação no PHP).
3. **Duplo clique ou chamadas concorrentes a `create`:** Serializadas pelo `Mutex` de geração por raiz e protegidas no kernel por `O_EXCL` (`create_new`).
4. **Argumento obrigatório ausente no `ScheduleSpec`:** Bloqueado pelo `validate_form` com erro em `schedule.arguments`, pois tarefas agendadas rodam com `--no-interaction`.
