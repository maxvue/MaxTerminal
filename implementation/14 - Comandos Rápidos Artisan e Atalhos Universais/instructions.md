# Fase 14 — Comandos Rápidos: Artisan (`>`) e Atalhos Universais (`/`) (`instructions.md`)

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) §8.1 e §8.3, e contratos compartilhados de [`../README.md`](../README.md) (§2.1–§2.6).
> Especificação técnica detalhada: [`./spec.md`](./spec.md).
> Referência de compatibilidade: código-fonte da extensão MaxCode (`~/GitHub/MaxCode/src/core/artisanDiscovery.ts`,
> `slashComandos.ts`, `terminalChatExecutor.ts`). Cache validado no código local, revisão
> `1054eb8a886de412de8912ad56f64b1367da8b09`; atalhos preservam a intenção, mas respeitam a versão/capacidades da CLI (§8 da spec).

---

## 1. Objetivo da Fase

Entregar, no input rico do terminal (Fase 04), dois provedores de gatilho que transformam o MaxTerminal num lançador
de comandos de projeto:

1. **Artisan (`>`)** — exclusivo de projetos Laravel: descobre os comandos com `php artisan list --format=json`
   (fora do PTY, com timeout), mantém um cache **compatível com o MaxCode** em `.max-code/commands/artisan.json`,
   oferece filtro fuzzy navegável por setas, autocompletar de flags/opções e argumentos posicionais e insere no input o
   comando montado (`php artisan <comando> <flags>`) **sem executar** até o usuário pressionar Enter. A opção final
   **"Criar comando"** abre o modal `ArtisanCommandModal.vue`, que gera o arquivo PHP (`Criar`) ou envia o pedido ao
   MaxCode em Plan Mode (`Criar com MaxCode`), com Geração Assistida por IA.
2. **Atalhos Universais (`/`)** — `/install`, `/install-force`, `/update`, `/update-force`, `/build`, `/deploy`,
   `/test`: detecta os gerenciadores do projeto (composer, npm, pnpm, yarn, bun, cargo) e resolve cada atalho para uma
   linha de shell concreta (inclusive projetos com múltiplos gerenciadores, ex.: Laravel + Vue → ambos), lendo os
   scripts `build`/`deploy`/`test` de `package.json`/`composer.json` quando existirem.

Toda a lógica de domínio vive no crate `mt-commands` (módulos `artisan` e `shortcuts`); a Fase 15 adicionará o módulo
`groups` no mesmo crate.

---

## 2. Escopo

### 2.1. Incluído

| # | Item | Detalhe |
|---|------|---------|
| I-01 | Crate `mt-commands` — módulo `artisan` | Resolução da raiz Laravel (worktree-aware), runners (`LocalPhpRunner`, `SailRunner` opcional), parser do JSON do Symfony Console, cache + sidecar de metadados, fingerprint de invalidação, single-flight por raiz. |
| I-02 | Cache compatível com MaxCode | `<raiz>/.max-code/commands/artisan.json` = **array JSON** de comandos no schema do MaxCode (`name`, `description?`, `options?[{name, description?, shortcut?}]`, `arguments?[{name, description?, required?}]`) acrescido de campos opcionais estendidos (`acceptValue`, `isValueRequired`, `isMultiple`, `default`, `isArray`, `isGlobal`). Metadados de invalidação no sidecar `artisan.meta.json`. Garante `.max-code/.gitignore` com `*` (mesmo comportamento do MaxCode). |
| I-03 | Invalidação | Hash/stamp por arquivo de comandos, manifesto, lockfile, bootstrap e routes + hash do `artisan.json`; cache de outro gerador é servido como *stale* e revalidado, mesmo com memória quente. |
| I-04 | Execução segura da descoberta | `tokio::process::Command` sem shell, `cwd` = raiz Laravel, `stdin` nulo, timeout padrão 15 s com kill do processo, limite de 8 MiB de stdout, `stderr` sanitizado. Sail/Docker **opcional** (auto-detectado; fallback para `php` local). |
| I-05 | Gatilho `>` | Provedor registrado no `TriggerRegistry` (Fase 04); exige raiz Laravel validada (`artisan` e `laravel/framework` no manifesto), usando a stack apenas como dica; contexto do painel e worktree, sem conflito com `>>` (Fase 15). |
| I-06 | Filtro fuzzy | Ranqueamento por subsequência com bônus de prefixo/fronteira de namespace (`:`, `-`), navegação ↑/↓, Enter/Tab seleciona, Esc fecha. |
| I-07 | Autocompletar de flags e argumentos | Após o comando ser inserido (`php artisan migrate `), digitar `-` lista as opções (descrição, se aceita valor, se é obrigatório o valor, default, atalho, se é global); com token vazio exibe dica do próximo argumento posicional. Funciona também quando o usuário digita `php artisan …` manualmente. |
| I-08 | Inserção sem execução | Seleções apenas alteram o texto do input; nada é enviado ao PTY sem Enter explícito. |
| I-09 | Opção "Criar comando" | Sempre o último item da lista `>` (e único quando não há correspondência), abre `ArtisanCommandModal.vue`. |
| I-10 | Modal `ArtisanCommandModal.vue` | Campos: Nome da Classe, Signature, Descrição, construtor visual de Argumentos e Flags (nome, opcional, array, default, descrição, atalho, aceita valor), checkboxes Isolatable, PromptsForMissingInput, Agendamento (frequência + parâmetros), Objetivo/Regras; validação por campo; preview do PHP e do patch de `routes/console.php`. |
| I-11 | Ação **Criar** | Gera `app/Console/Commands/<Classe>.php` via template e PSR-4 validado, interfaces conforme versão; schedule em arquivo ausente e referenciado pelo bootstrap pode ser criado exclusivamente. Arquivo de routes existente ou layout customizado recebe patch/passos manuais. Preview vinculado à criação, sem sobrescrita; invalida catálogo após sucesso. |
| I-12 | Ação **Criar com MaxCode** | Chama `maxcode_open_plan` (Fase 11) com o texto exato `"Criar comando artisan:\n${dados_formatados_do_formulario}"` no formato definido em spec §7.3. |
| I-13 | Geração Assistida | Botão "Preencher com IA" no modal, usando `ai_generate_structured` (Fase 12) com JSON Schema exportado pelo Rust; resultado validado pelo mesmo validador e reversível ("Desfazer sugestão"). |
| I-14 | Crate `mt-commands` — módulo `shortcuts` | Evidências de manifesto, `packageManager`, lockfiles e versão/capacidades (incluindo Cargo em `src-tauri`); conflito sem evidência conclusiva fica desabilitado, sem escolha arbitrária. Plano tipado com cwd/argv e origem, scripts de build/deploy/test e configuração MaxCode validada; composição Composer → JS → Cargo com `&&`. Deploy somente com configuração explícita. |
| I-15 | Gatilho `/` | Provedor no `TriggerRegistry`: lista os 7 atalhos com a linha resolvida como descrição; itens não resolvíveis aparecem desabilitados com o motivo; `/install-force`, `/update-force` e `/deploy` exigem confirmação antes de inserir (mesma regra do MaxCode). |
| I-16 | Comandos IPC e eventos | `artisan_*` e `shortcut_*` (spec §5) + eventos `artisan://catalog-updated`, `artisan://discovery-failed`, `artisan://command-created` (spec §6). |
| I-17 | Store `useQuickCommandsStore` | `src/stores/quickCommands.ts` — catálogo Artisan por raiz, atalhos por cwd, estado do modal, ações de criação/preview/IA. |
| I-18 | Testes | Unitários Rust (parser, cache, fingerprint, validação, gerador com snapshot, patch de `routes/console.php`, resolução de atalhos), Vitest (parser de linha, fuzzy, provedores, store, modal). |

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| Grupos de Comandos `>>` (local/global, Sequencial/Paralelo/Multi-Abas, criação, sugestão IA de grupos) e o módulo `mt-commands::groups` | 15 |
| Infraestrutura do input rico, popup genérico e `TriggerRegistry` (apenas consumidos aqui) | 04 |
| Detecção de stack do projeto (`Laravel`, `Vue`, `Tauri`, `Rust`…) | 06 (consumida) |
| Execução do comando no PTY ao pressionar Enter, blocos de comando, exit codes | 04/05 (consumidos) |
| Abas, cwd da aba, templates | 07 (consumidos) |
| Transporte loopback/URI até o MaxCode (`maxcode_open_plan`) | 11 (consumido) |
| Gateway MaxAiManager e `ai_generate_structured`; comandos `@`/`?` | 12 (consumido) |
| Botão "Inspecionar" quando um comando Artisan/atalho falha | 11 (já cobre qualquer bloco com `exit != 0`) |
| Busca global FTS5, testes E2E com app empacotado, benchmarks globais | 17 |
| Watcher contínuo (`fs::watch`) do diretório de comandos — a invalidação aqui é **preguiçosa** (verificada a cada abertura do popup); um watcher pode ser plugado na Fase 17 via `ArtisanService::invalidate` | 17 (opcional) |
| Edição/execução de configurações de build/deploy pela UI do MaxTerminal (apenas leitura de `package.json`, `composer.json` e da config do MaxCode) | — (não previsto no AGENTS.md) |

---

## 3. Pré-requisitos

### 3.1. Fases
| Fase | Contrato consumido |
|------|--------------------|
| 01 | `mt-core` (`MtError`, `MtResult`, ids, `AppPaths`), `tauri-specta`, logging, convenções de lint. |
| 03 | `mt_db::sanitizer::sanitize()` — stderr, cache persistido e prompt MaxCode. Nenhuma tabela, índice ou migration nova; execução usa histórico das Fases 03/05/07. |
| 04 | `TriggerDefinition`, `TriggerQuery`, `CompletionItem`, `CompletionResult`, `TriggerSelectOutcome`, `TerminalContext` e `triggerRegistry.register()`. |
| 06 | `Project.stack: StackKind`, `useProjectsStore().projectById(id)`; root validada prevalece sobre a stack primária. |
| 07 | Contexto do painel (`PaneView.cwd`, `paneId`, `projectId`) integrado ao `TerminalContext` da Fase 04. |
| 11 | Comando IPC `maxcode_open_plan`. |
| 12 | Comando IPC `ai_generate_structured`. |

> Se o nome exato de algum símbolo divergir na spec da fase dona, **a fase dona prevalece**; o acoplamento desta fase
> fica concentrado em `src/composables/quick-commands/adapters.ts` (frontend) e `mt-commands/src/ports.rs` (Rust)
> para que o ajuste seja local.

### 3.2. Ferramentas
- Rust stable (toolchain da Fase 01), `cargo-insta` para snapshots.
- Node/npm da Fase 01, Vitest.
- Para testes de integração opcionais: `php` ≥ 8.2 e `composer` no `PATH`, e um esqueleto Laravel 11/12 apontado por
  `MT_LARAVEL_FIXTURE` (testes marcados `#[ignore]`). Docker + Sail apenas para o teste opcional do `SailRunner`.

---

## 4. Entregáveis

### 4.1. Rust (`src-tauri/crates/mt-commands/`)
- `Cargo.toml` (deps: `mt-core`, `mt-db` (sanitizer), `serde`, `serde_json`, `schemars`, `specta`, `tokio`, `thiserror`, `tracing`, `sha2`, `walkdir`, `async-trait`, `dashmap`; dev: `tempfile`, `insta`, `pretty_assertions`).
- `src/lib.rs`, `src/error.rs`, `src/ports.rs`, `src/fs_util.rs`.
- `src/artisan/{mod.rs, model.rs, root.rs, runner.rs, parser.rs, cache.rs, fingerprint.rs, service.rs}`.
- `src/artisan/generator/{mod.rs, form.rs, validate.rs, signature.rs, psr4.rs, template.rs, schedule.rs, console_routes.rs, maxcode_prompt.rs}`.
- `src/shortcuts/{mod.rs, model.rs, detect.rs, manifests.rs, maxcode_config.rs, resolve.rs, service.rs}`.
- `tests/fixtures/` (`artisan_list_laravel11.json`, `artisan_list_laravel10.json`, `maxcode_cache_legacy.json`, projetos-fixture de atalhos), `tests/*.rs`, `src/**/snapshots/*.snap`.

### 4.2. Binário (`src-tauri/src/`)
- `commands/quick_commands.rs` (comandos `artisan_*`, `shortcut_*`), registro no builder do `specta.rs` e no `invoke_handler`.
- `state.rs`: campo `quick_commands: Arc<QuickCommandsState>`.

### 4.3. Frontend (`src/`)
- `ipc/quickCommands.ts`.
- `stores/quickCommands.ts` (`useQuickCommandsStore`).
- `composables/quick-commands/{adapters.ts, fuzzy.ts, artisanLineParser.ts, useArtisanTrigger.ts, useShortcutTrigger.ts, useArtisanForm.ts, registerQuickCommandTriggers.ts}`.
- `components/quick-commands/{QuickCommandsHost.vue, ArtisanCommandModal.vue, ArtisanParamsBuilder.vue, ArtisanParamRow.vue, ArtisanSchedulePicker.vue, ArtisanPhpPreview.vue, QuickCommandItem.vue}`.
- Chamada de `registerQuickCommandTriggers()` no bootstrap (`src/app/main.ts`).
- Testes em `src/composables/quick-commands/__tests__/`, `src/stores/__tests__/quickCommands.spec.ts`, `src/components/quick-commands/__tests__/`.
- `src/types/bindings.ts` regenerado.

---

## 5. Critérios de Aceite

### 5.1. Build e qualidade
- [ ] CA-01 `cargo clippy --workspace --all-targets -- -D warnings` e `cargo fmt --check` retornam 0.
- [ ] CA-02 `cargo test -p mt-commands` passa; `cargo insta test -p mt-commands --check` sem snapshots pendentes.
- [ ] CA-03 `npm run typecheck`, `npm run typecheck:vue`, `npm run lint`, `npm run lint:style` e `npm run test` retornam 0.
- [ ] CA-04 `npm run bindings && git diff --exit-code src/types/bindings.ts` retorna 0 e o arquivo expõe `commands.artisanResolveRoot`, `artisanCatalogGet`, `artisanCatalogRefresh`, `artisanFormSchema`, `artisanValidateForm`, `artisanPreviewCommand`, `artisanCreateCommand`, `artisanFormatMaxcodePrompt`, `shortcutList`, `shortcutResolve`.

### 5.2. Descoberta e cache Artisan
- [ ] CA-05 Num projeto Laravel sem cache, abrir o popup `>` executa **uma única vez** `php artisan list --format=json` (verificado por teste com `FakeRunner` e 10 chamadas concorrentes → 1 execução) e cria `.max-code/commands/artisan.json`, `.max-code/commands/artisan.meta.json` e `.max-code/.gitignore` (`*\n`, sem sobrescrever se existir).
- [ ] CA-06 `artisan.json` é um array JSON indentado com 2 espaços, lido sem erro pelo port de `lerCacheArtisan()` do MaxCode; o port de `normalizarArtisanJson` recebe separadamente o JSON Symfony, não o array de cache. Comandos `hidden: true` são excluídos; opções mantêm `--`; `shortcut` vazio é omitido.
- [ ] CA-07 Um `artisan.json` legado do MaxCode (fixture sem campos estendidos e sem sidecar) é servido imediatamente com `stale: true` e revalidado em segundo plano; ao concluir, `artisan://catalog-updated` é emitido e o arquivo passa a conter campos estendidos.
- [ ] CA-08 Tocar (`touch`) um arquivo em `app/Console/Commands/`, `composer.lock` ou `routes/console.php` faz a próxima `artisan_catalog_get` redescobrir; sem alterações, a segunda chamada não executa o runner (teste).
- [ ] CA-09 Runner que excede o timeout é morto (nenhum processo `php` remanescente — teste com script `sleep 30` como binário fake via `ARTISAN_PHP_BIN` de teste) e retorna `artisan.timeout`; `php` ausente retorna `artisan.php_not_found`; saída não-JSON retorna `artisan.invalid_json`; exit ≠ 0 retorna `artisan.exit_nonzero` com cauda do `stderr` sanitizada (≤ 4 KiB).
- [ ] CA-10 Sem raiz Laravel validada, `>` não abre popup e o texto segue o shell. Laravel+Vue funciona mesmo com stack primária Vue; cwd de worktree nunca recua à raiz principal atravessando a fronteira Git.
- [ ] CA-11 Em uma aba cujo `cwd` é uma worktree (`.max-code-worktrees/x` ou `.worktrees/x`) contendo `artisan`, a raiz resolvida é a worktree, e o cache é gravado nela.

### 5.3. Popup `>`, flags e inserção
- [ ] CA-12 Digitar `>mig` lista `migrate`, `migrate:fresh`, `migrate:rollback`… com `migrate` primeiro; `>mfr` ranqueia `migrate:fresh` no topo (teste do fuzzy); ↑/↓ navegam; Esc fecha.
- [ ] CA-13 Selecionar `migrate` substitui o input por `php artisan migrate ` (ou `./vendor/bin/sail artisan migrate ` quando o runner Sail está ativo) com cursor no fim e **nenhuma** escrita no PTY (teste com espião de `pty_write`).
- [ ] CA-14 Com `php artisan migrate --` no input, o popup lista as opções de `migrate` com descrição, indicação "aceita valor", default e atalho; opções já usadas (não múltiplas) não reaparecem; selecionar `--database` insere `--database=` mantendo o cursor após `=`; selecionar `--force` insere `--force `.
- [ ] CA-15 Com `php artisan make:model ` (token vazio), o popup exibe a dica não selecionável do argumento `name` (obrigatório) com descrição.
- [ ] CA-16 "Criar comando" é sempre o último item e é o único quando nada corresponde (`>zzzzz`); selecioná-lo abre `ArtisanCommandModal` com `Signature` pré-preenchida a partir da consulta quando ela for um nome de signature válido.
- [ ] CA-17 Digitar `>>` não ativa o provedor `>` (reservado à Fase 15).

### 5.4. Modal, geração e MaxCode
- [ ] CA-18 Validações bloqueiam "Criar" e exibem mensagem pt-BR por campo: classe não PascalCase/palavra reservada PHP, signature inválida ou já existente no catálogo, nomes duplicados entre argumentos/opções, argumento obrigatório após opcional, array que não seja o último argumento, atalho inválido/duplicado/reservado (`h q V n v`), opção reservada (`help`, `quiet`, `verbose`, `version`, `ansi`, `no-ansi`, `no-interaction`, `env`, e `isolated` quando Isolatable), cron inválido, `{`/`}` em descrições.
- [ ] CA-19 O preview do PHP atualiza em ≤ 300 ms após a última edição (debounce 250 ms) e é idêntico byte a byte ao arquivo gravado por "Criar" (teste).
- [ ] CA-20 "Criar" num layout moderno suportado gera `app/Console/Commands/SendWeeklyReport.php` com namespace PSR-4 e interfaces selecionadas; schedule inclui argumentos obrigatórios (ex.: `Schedule::command(SendWeeklyReport::class, ['42'])->weeklyOn(1, '08:00');`). Arquivo de routes existente fica intacto com patch manual idempotente; ausente e referenciado pode ser criado exclusivamente. `php -l`, descoberta e `schedule:list` validam a fixture com o patch aplicado.
- [ ] CA-21 Destino existente retorna `artisan.file_exists` sem alterar arquivos. Preview obsoleto retorna `artisan.preview_stale` antes de gravar. Falha de schedule reverte somente arquivos próprios ainda idênticos aos bytes gravados; edição externa é preservada e retorna estado parcial com caminhos e passos de recuperação.
- [ ] CA-22 Em Laravel ≤ 10 (sem `withSchedule` em `bootstrap/app.php` e com `app/Console/Kernel.php`), o schedule **não** é escrito; o resultado traz `manualSteps` com o trecho para `Kernel::schedule()` exibido no modal.
- [ ] CA-23 Após "Criar", o cache é invalidado, `artisan://command-created` é emitido e o novo comando aparece no popup `>` sem reiniciar o app.
- [ ] CA-24 "Criar com MaxCode" invoca `maxcode_open_plan` com `prompt` exatamente igual ao snapshot `maxcode_prompt__full_form.snap` (começa com `Criar comando artisan:\n`); não grava arquivos.
- [ ] CA-25 "Preencher com IA" envia o objetivo e o JSON Schema de `artisan_form_schema` a `ai_generate_structured`; resposta inválida é rejeitada com toast pt-BR sem alterar o formulário; resposta válida preenche os campos e "Desfazer sugestão" restaura o estado anterior.

### 5.5. Atalhos Universais `/`
- [ ] CA-26 Fixture Laravel + Vue (npm): `/install` → `composer install && npm install`; `/update-force` → `composer update --no-cache && npm update --force`; `/build` → `npm run build`; `/test` → `php artisan test` (e `&& npm run test` se houver script `test` não padrão).
- [ ] CA-27 Fixture Tauri com `pnpm-lock.yaml` + `src-tauri/Cargo.toml`: `/install` → `pnpm install && cargo build --manifest-path src-tauri/Cargo.toml`; `/test` → `pnpm run test && cargo test --manifest-path src-tauri/Cargo.toml` quando houver script `test`.
- [ ] CA-28 Somente `package.json` com o script padrão do `npm init` (`echo "Error: no test specified" && exit 1`) → `/test` aparece desabilitado com motivo "Nenhum comando de teste encontrado".
- [ ] CA-29 `packageManager` válido seleciona o gerenciador e evidencia lockfiles divergentes; sem declaração, um lockfile conclusivo seleciona, múltiplos gerenciadores conflitantes desabilitam a resolução dependente de JS. `npm-shrinkwrap.json`, Bun textual/binário e Yarn Classic/Modern possuem fixtures. Somente `package.json`, sem outra evidência, usa npm com origem explícita.
- [ ] CA-30 `/build` e `/deploy` usam `comandoBuild`/`comandoDeploy` da configuração do MaxCode para o projeto quando o arquivo existir e o valor não for vazio (fixture `config-trabalho.json`); ausência do arquivo não gera erro.
- [ ] CA-31 Selecionar `/install-force`, `/update-force` ou `/deploy` exibe confirmação (`MaxPopoverConfirm`) antes de inserir; nenhum atalho escreve no PTY sem Enter.
- [ ] CA-32 Um input como `/usr/bin/env` não exibe popup (nenhum atalho corresponde) e Enter executa normalmente.

### 5.6. Convenções
- [ ] CA-33 Textos de UI, mensagens de `MtError` e comentários (inclusive os do PHP gerado) em pt-BR; identificadores em inglês; SFCs na ordem template → script → style com tags de abertura em linha única.
- [ ] CA-34 Nenhuma migration SQLite é adicionada por esta fase.

### 5.7. Concorrência, capacidades e recuperação
- [ ] CA-35 `/deploy` sem script/configuração permanece desabilitado, inclusive em Laravel, Tauri e Cargo; nunca gera `publish`, SSH, upload ou migração por inferência. Yarn Modern não recebe `upgrade --force`; flags `force` desconhecidas deixam o atalho indisponível com motivo.
- [ ] CA-36 Mudança de painel/cwd, fechamento do modal ou edição durante preview/IA descarta resultados antigos por revisão. Enter/Tab que seleciona não executa; `Escape` restaura foco e não perde o rascunho. "Criar comando" permanece ao fim durante loading, catálogo vazio e erro.
- [ ] CA-37 Fingerprint detecta edição de qualquer PHP, remoção, renomeação, manifesto/bootstrap e cache regravado pelo MaxCode mesmo com memória quente; erro de discovery não causa loop de refresh nem retry por tecla.
- [ ] CA-38 Runner drena stdout/stderr simultaneamente, mata o grupo em timeout/cancelamento/overflow e aguarda o filho; stderr abundante não causa deadlock. Sail opcional só fica disponível com encerramento verificável da tarefa no contêiner.
- [ ] CA-39 Symlink de pai/cache, nome hostil, comentário `*/`, variável PHP reservada, aliases de import e alteração concorrente de `routes/console.php` não escrevem fora da raiz nem sobrescrevem trabalho externo. Falha de revalidação após criação não converte criação bem-sucedida em nova tentativa de gravação.
- [ ] CA-40 Resolver revalida evidências ao selecionar; cwd aninhado, caminhos com espaços/apóstrofos e contexto de shell bash/zsh/fish produzem comandos corretos. Conflito em parte necessária não executa composição parcial silenciosamente.

---

## 6. Riscos e Mitigação

| # | Risco | Impacto | Mitigação |
|---|-------|---------|-----------|
| R-01 | `php artisan list` lento (boot do Laravel, providers pesados, conexão a banco) ou travado. | Alto | Execução fora do PTY com timeout (15 s, configurável), kill do processo, single-flight por raiz, cache em disco, *stale-while-revalidate*; popup mostra "Carregando comandos Artisan…" sem bloquear o input. |
| R-02 | Divergência de schema com o MaxCode, que só lê arrays e ignora metadados. | Alto | Arquivo principal permanece array no schema do MaxCode; campos estendidos só opcionais; metadados em sidecar; teste de compatibilidade com port do leitor do MaxCode. |
| R-03 | MaxCode sobrescreve o cache sem campos estendidos/sidecar. | Médio | Sidecar ausente ou hash divergente → *stale* e revalidação; ausência de extensões sozinha não identifica produtor. |
| R-04 | Projeto depende de Docker/Sail (sem `php` no host). | Médio | `SailRunner` opcional auto-detectado (`vendor/bin/sail` + serviço `laravel.test` em execução); erro `artisan.php_not_found` sugere Sail na mensagem. |
| R-05 | Gatilhos `/` e `>` colidirem com uso legítimo do shell (`/usr/bin/x`, `> arquivo`). | Médio | `/` só abre popup se algum atalho corresponder ao token; `>` só em projetos Laravel e com Esc desativando o gatilho até o input ser limpo; Enter sem item destacado segue o fluxo normal. |
| R-06 | Gerar PHP inválido ou sobrescrever código do usuário. | Alto | Validador único (UI, IA e criação), escape de strings PHP, `create_new` (O_EXCL), rollback, snapshot tests e `php -l` no teste de integração. |
| R-07 | Patch de `routes/console.php` corromper arquivo customizado. | Médio | Preview conservador com compreensão de tokens/aliases; arquivo existente recebe patch manual, novo usa publicação exclusiva; hash+rename não é tratado como CAS. |
| R-08 | Atalhos executarem ações destrutivas sem intenção (`--force`, deploy). | Alto | Nada executa sem Enter; confirmação para `*-force` e `/deploy`; a linha resolvida fica visível no input. |
| R-09 | Saída da IA maliciosa/inconsistente. | Médio | Tratada como dado não confiável: desserialização estrita + mesmo validador; nunca grava sem clique em "Criar". |
| R-10 | Contratos das Fases 04/06/11/12 com nomes diferentes dos assumidos. | Médio | Acoplamento isolado em `adapters.ts` / `ports.rs`; fase dona prevalece. |
| R-11 | Divergências intencionais em relação ao MaxCode (`/test` real, multi-gerenciador, versões de CLIs). | Baixo | Documentadas em spec §8.4; manter fixture do legado e matriz de capacidades sem copiar flags inválidas. |
| R-12 | Segredos vazarem pelo stderr, cache ou prompt MaxCode. | Médio | Sanitização antes de persistir/enviar/truncar; diagnóstico até 4 KiB; nada desta fase cria histórico SQLite paralelo. |
