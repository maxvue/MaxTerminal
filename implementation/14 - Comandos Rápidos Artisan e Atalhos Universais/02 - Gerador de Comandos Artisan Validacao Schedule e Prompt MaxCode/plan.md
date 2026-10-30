# Implementação 02 — Gerador de Comandos Artisan, Validação, Schedule e Prompt MaxCode (`plan.md`)

> **Fase:** 14 — Comandos Rápidos: Artisan (`>`) e Atalhos Universais (`/`)
> **Unidade:** `02 - Gerador de Comandos Artisan Validacao Schedule e Prompt MaxCode`
> **Especificação associada:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Execução

### Passo 1 — Modelos do Formulário e Schema JSON (`form.rs`)
- [ ] **1.1.** Implementar `src-tauri/crates/mt-commands/src/artisan/generator/form.rs` com `ArtisanCommandForm`, `ParamKind`, `ArtisanParam`, `ScheduleSpec`, `ScheduleOption`, `ScheduleFrequency`, `IssueSeverity`, `FieldIssue`, `ValidationReport`, `ConsoleRoutesPatch`, `ArtisanPreview`, `ArtisanCreateResult` e `artisan_form_json_schema()`.
- [ ] **1.2.** Adicionar teste unitário em `form.rs` garantindo que `artisan_form_json_schema()` gera um JSON Schema Draft 2020-12 válido com tamanho `<= 32 KiB` e que `serde_json::from_value::<ArtisanCommandForm>` rejeita campos desconhecidos (`deny_unknown_fields`).

### Passo 2 — Validador Único (`validate.rs`) e Resolvedor PSR-4 (`psr4.rs`)
- [ ] **2.1.** Implementar `resolve_commands_namespace` em `src-tauri/crates/mt-commands/src/artisan/generator/psr4.rs`, suportando `autoload.psr-4` com valores `string` ou `string[]` e escolhendo o prefixo de diretório mais longo para `app/Console/Commands/`.
- [ ] **2.2.** Implementar `validate_form`, `validate_for_maxcode_prompt`, `is_php_reserved_word` e `validate_cron_expression` em `src-tauri/crates/mt-commands/src/artisan/generator/validate.rs` cobrindo todas as regras da tabela §7.2 da spec e §3.2 de `spec.md`.
- [ ] **2.3.** Criar testes unitários exaustivos em `validate.rs` e `psr4.rs` com pelo menos uma asserção para cada regra de validação (CA-18):
  - `className` fora de PascalCase e palavras reservadas PHP (`List`, `Class`, `Match`, `Enum`, `Readonly`).
  - `signatureName` inválida e colisão com `name` ou `aliases` do catálogo.
  - Descrições contendo `{`, `}` ou quebra de linha.
  - Nomes duplicados entre argumento e opção.
  - Argumento obrigatório após argumento opcional; `isArray` fora do último argumento; `default` em argumento obrigatório.
  - Atalho inválido, duplicado ou reservado (`h`, `q`, `V`, `n`, `v`).
  - Opção reservada (`help`, `quiet`, `verbose`, `version`, `ansi`, `no-ansi`, `no-interaction`, `env`, e `isolated` quando `isolatable: true`).
  - Expressões cron de 5 campos válidas e inválidas (minuto 60, hora 24, dia 0, mês 13, dia da semana 8, passo `/0`).
  - Schedule sem valor para argumento obrigatório ou com opção inexistente.
  - Warning não-bloqueante para `promptsForMissingInput` sem argumento obrigatório.

### Passo 3 — Signature (`signature.rs`), Template PHP (`template.rs`) e Prompt MaxCode (`maxcode_prompt.rs`)
- [ ] **3.1.** Implementar `param_token` e `build_signature` em `src-tauri/crates/mt-commands/src/artisan/generator/signature.rs` (linha única para `<= 1` parâmetro; múltiplas linhas indentadas com 28 espaços para `>= 2` parâmetros; argumentos antes das opções).
- [ ] **3.2.** Implementar `render_php_command` em `src-tauri/crates/mt-commands/src/artisan/generator/template.rs` com suporte a `Isolatable`, `PromptsForMissingInput`, `promptForMissingArgumentsUsing()`, escapes de strings PHP, escape de `*/` no PHPDoc e proteção contra variáveis `$this` ou colisões em camelCase e imports.
- [ ] **3.3.** Implementar `format_maxcode_prompt` em `src-tauri/crates/mt-commands/src/artisan/generator/maxcode_prompt.rs` reproduzindo exatamente o layout de `spec.md` §7.3 e sanitizando a saída com `mt_db::sanitizer::sanitize`.

### Passo 4 — Agendamento (`schedule.rs`), `routes/console.php` (`console_routes.rs`) e `CommandGenerator` (`mod.rs`)
- [ ] **4.1.** Implementar `render_schedule_chain`, `render_modern_schedule_statement` e `render_legacy_kernel_statement` em `src-tauri/crates/mt-commands/src/artisan/generator/schedule.rs`.
- [ ] **4.2.** Implementar `plan_console_routes` com lexer PHP, detecção idempotente e bloco delimitado: criação exclusiva se ausente; inserção em `routes/console.php` existente suportado após preview/confirmação da revisão e revalidação de hash. Preservar bytes externos/permissões/backup; `manual_steps` para estrutura ambígua/conflitante ou layout Legacy (`app/Console/Kernel.php`).
- [ ] **4.3.** Implementar `CommandGenerator` em `src-tauri/crates/mt-commands/src/artisan/generator/mod.rs` unificando `form_schema`, `validate`, `preview`, `create` (com checagem de `expected_revision`, `create_new_file_under_root`, rollback seguro com verificação de inode/hash e emissão de `artisan://command-created` + `artisan.invalidate`) e `format_maxcode_prompt`.

### Passo 5 — Testes de Snapshot (`insta`), Rollback e Integração Laravel (`tests/generator.rs`, `tests/laravel_integration.rs`)
- [ ] **5.1.** Criar `src-tauri/crates/mt-commands/tests/generator.rs` e consolidar os snapshots `insta`:
  - `template__full_form` (formulário completo da spec §7.3/§7.6).
  - `template__minimal` (comando sem parâmetros, sem interfaces e sem schedule).
  - `template__arrays` (argumento array e opção array).
  - `maxcode_prompt__full_form` (CA-24).
  - `console_routes__fresh_laravel11` e `console_routes__idempotent` (CA-20).
- [ ] **5.2.** Adicionar em `tests/generator.rs` os testes comportamentais:
  - Igualdade byte a byte entre `preview.php` e o arquivo gravado por `create` (CA-19).
  - Layout Legacy (Laravel 10 com `app/Console/Kernel.php`) populando `manual_steps` sem escrever em `routes/console.php` (CA-22).
  - `routes/console.php` existente recebe uma única inserção delimitada confirmada, preservando bytes externos; segunda aplicação é no-op. Testar preview sem escrita, alteração concorrente → `preview_stale`, aliases/`?>`, estrutura ambígua → `manual_steps` e recuperação preservando edição externa (CA-20, CA-39).
  - Destino existente retornando `artisan.file_exists`; `expected_revision` divergente retornando `artisan.preview_stale`; rollback em falha preservando edição externa com `artisan.partial_write` (CA-21, CA-39).
- [ ] **5.3.** Criar `src-tauri/crates/mt-commands/tests/laravel_integration.rs` (com teste `#[ignore]` ativado quando `MT_LARAVEL_FIXTURE` estiver definido, mais teste de sintaxe `php -l` quando `php` estiver no `PATH`) validando que o PHP gerado passa em `php -l` sem erros (CA-20).

---

## 2. Comandos de Verificação e Validação

Execute a partir de a raiz da worktree ativa:

```bash
cd "$(git rev-parse --show-toplevel)/src-tauri"
cargo fmt --package mt-commands -- --check
cargo clippy -p mt-commands --all-targets -- -D warnings
cargo test -p mt-commands --test generator
cargo insta test -p mt-commands --check
```

---

## 3. Mapeamento de Critérios de Aceite

| Critério | Cobertura nesta Implementação |
|----------|-------------------------------|
| **CA-02** | `cargo test -p mt-commands` e `cargo insta test -p mt-commands --check` passando sem snapshots pendentes. |
| **CA-18** | Validador único em `validate.rs` cobrindo todas as regras da tabela §7.2 com mensagens em `pt-BR` por campo (`path`). |
| **CA-19** | `CommandGenerator::preview` e `CommandGenerator::create` usam exatamente a mesma função de renderização e produzem bytes idênticos. |
| **CA-20** | Geração PHP com namespace PSR-4/interfaces/agendamento; `routes/console.php` existente suportado recebe inserção delimitada idempotente após preview/confirmação, preservando código externo; ausente é criado exclusivamente. |
| **CA-21** | Retorno de `artisan.file_exists`, `artisan.preview_stale` e rollback seguro (`artisan.partial_write` se houve edição externa). |
| **CA-22** | Em Laravel ≤ 10 (`LaravelLayout::Legacy`), nenhum schedule é gravado automaticamente e `manual_steps` contém o snippet para `Kernel::schedule()`. |
| **CA-23** | `CommandGenerator::create` invalida o `ArtisanService` e emite `artisan://command-created`. |
| **CA-24** | `format_maxcode_prompt` gera texto idêntico ao snapshot `maxcode_prompt__full_form.snap` começando com `Criar comando artisan:\n`. |
| **CA-39** | Proteção contra symlinks, nomes hostis, `*/` no PHPDoc, variável `$this`, colisão de imports e preservação de `routes/console.php` existente. |
