# Fase 12 — Integração MaxAiManager (`@` / `?`) (`instructions.md`)

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) §5.1 e contratos compartilhados de [`../README.md`](../README.md) (§2.1–§2.6).
> Especificação técnica detalhada: [`./spec.md`](./spec.md).

---

## 1. Objetivo da Fase

Conectar o MaxTerminal ao gateway local do **MaxAiManager** (header obrigatório `X-Agent-Name: MaxTerminal`) e entregar
as quatro capacidades de IA descritas em AGENTS.md §5.1:

1. **Comando por Linguagem Natural** — gatilhos `@` (popup em 3 etapas: Harness → Modelo → Nível de Raciocínio, depois
   texto em pt-BR) e `?` (usa o Harness/Modelo/Raciocínio padrão das Preferências) convertem um pedido em pt-BR em um
   **comando shell proposto**, exibido para revisão/edição e **nunca executado automaticamente**, com classificação de
   risco e confirmação extra para comandos destrutivos.
2. **Diagnóstico Rápido Inline** — quando um bloco de comando termina com erro, exibe resumo curto da causa + comando
   corretivo ao lado do botão "Inspecionar" (slot independente do `CommandBlock`), com debounce, opt-out e cache por hash.
3. **Consulta RAG** — busca na documentação indexada via `/v1/rags/{slug}/search`, com seleção de slug e resultados
   exibidos como bloco rico no terminal.
4. **API de Geração Assistida** genérica (`ai_generate_structured(schema, prompt)`) — ponto de extensão consumido pela
   Fase 14 (modal "Criar comando" Artisan) e pela Fase 15 (sugestão de Grupos de Comandos).

Tudo isso vive no crate `mt-ai` (Rust) e no domínio `ai` do frontend (`useAiStore`, `src/components/ai/*`).

---

## 2. Escopo

### 2.1. Incluído

| # | Item | Detalhe |
|---|------|---------|
| I-01 | Crate `mt-ai` — cliente HTTP | URL base configurada a partir do gateway validado (nenhuma porta presumida); header `X-Agent-Name: MaxTerminal` em **toda** requisição; timeouts por operação e retry limitado a leituras comprovadamente idempotentes. Dialeto e autenticação validados antes de habilitar a operação. |
| I-02 | Streaming condicional | SSE somente se confirmado pelo gateway/dialeto; JSON único quando streaming não for suportado. Deltas por `tauri::ipc::Channel<AiStreamEvent>`, sem repetir silenciosamente uma geração parcialmente recebida. |
| I-03 | Cancelamento | Registro `AiRequestRegistry` (`CancellationToken` por `AiRequestId`); comando `ai_cancel`; `Esc` na UI cancela; fechar a aba/painel cancela requisições pendentes dela. |
| I-04 | Catálogo Harness → Modelo → Raciocínio | Adaptador de catálogo a validar; cache em memória + SQLite (TTL 6h, `ETag` se suportado). Fallback contém apenas identidades dos 4 harnesses: Claude Code, Codex, Antigravity (`agy`), OpenCode; não inventa modelos nem níveis executáveis. |
| I-05 | Contexto da requisição | Shell, SO (distro/kernel), `cwd` do painel, stack do projeto (Fase 06), últimos N comandos **sanitizados** com exit code (Fase 05/03). Montado no Rust por trait `AiContextSource`; o usuário vê um resumo do contexto enviado. |
| I-06 | Gatilho `@` | Registrado no `TriggerRegistry` da Fase 04: `HarnessPicker.vue` em 3 etapas navegáveis por teclado, depois campo de texto pt-BR. |
| I-07 | Gatilho `?` | Registrado no `TriggerRegistry`: usa o padrão das Preferências; o popup oferece também o modo "Consultar documentação (RAG)". |
| I-08 | Bloco de sugestão | `AiSuggestionBlock.vue` no hospedeiro `AiPaneResults` da 12, junto ao terminal da 04: comando editável, explicação, `RiskBadge`, ações "Inserir no input", "Executar", "Copiar", "Regenerar", "Descartar". |
| I-09 | Classificação de risco | Classificador determinístico local (`RiskClassifier`, regras de regex/AST leve) combinado com a dica do modelo (prevalece o **maior** risco). Níveis `safe` / `caution` / `destructive`. `destructive` exige confirmação extra (digitar o 1º token do comando ou marcar caixa + confirmar). |
| I-10 | Diagnóstico Rápido Inline | Escuta término de bloco com exit conhecido diferente de zero (Fase 05); debounce 800 ms por painel; ignora `exit 130`/`SIGINT`, blocos de TUI/agentes; cache por hash contextual; fila limitada e concorrência 1; opt-out global/projeto; `HybridTerminal #block-actions` encaminha ao `CommandBlock #actions`. |
| I-11 | Consulta RAG | Caminho confirmado: `/v1/rags/{slug}/search`; método, corpo, resposta e listagem são **propostas a validar** (§4 da spec). Slug digitável/configurável quando não houver listagem; `RagResults.vue` mostra título, trecho, score e fonte quando disponíveis. |
| I-12 | Geração Assistida | Comando IPC `ai_generate_structured` + composable `useAiGenerate<T>()`; validação JSON Schema (`jsonschema`) no Rust com 1 tentativa de reparo; registro de schemas nomeados (`StructuredSchemaId`) para as Fases 14/15. |
| I-13 | Persistência | Migration própria: `ai_settings`, `ai_catalog_cache`, `ai_diagnosis_cache`, `ai_prompt_history` (opcional, desligável), todos com texto sanitizado via `mt_db::sanitizer::sanitize()`. |
| I-14 | Store e UI | `useAiStore` (`src/stores/ai.ts`), `src/ipc/ai.ts`, composables, componentes `HarnessPicker.vue`, `AiSuggestionBlock.vue`, `InlineDiagnosis.vue`, `RagResults.vue`, `RiskBadge.vue`, `AiContextPreview.vue` e seção **Preferências > IA** (`AiPreferences.vue`). |
| I-15 | Status do gateway | `ai_health` + evento `ai://status-changed`; indicador discreto (online/offline/degradado) e mensagens pt-BR acionáveis quando o MaxAiManager não está rodando. |

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| Infra do input rico, `TriggerRegistry` e slots do `CommandBlock` | 04 (consumida aqui); hospedeiro de cards IA é entregue na 12, pois não há registro genérico de blocos ricos publicado na 04. |
| Detecção de término de bloco, exit code, `TabStatus` | 05 (consumida aqui) |
| Detecção da stack do projeto | 06 (consumida aqui) |
| Abas/painéis, `cwd` do painel, sessões | 07 (consumida aqui) |
| Botão **"Inspecionar"** e abertura de conversa Plan Mode no MaxCode | 11 (o diagnóstico usa slot independente; não depende dele) |
| Abertura de abas de Agentes CLI, telemetria StatusLine, alerta de permissão, MaxSkills + memória contextual | 13 (pode reutilizar o catálogo de harnesses via `ai_catalog_get`) |
| Modal "Criar comando" Artisan e uso de `ai_generate_structured` nele | 14 |
| Sugestão de Grupos de Comandos via IA | 15 |
| Busca global FTS5 sobre `ai_prompt_history`, testes E2E com gateway real, hardening final | 17 |
| Gerenciar/instalar o MaxAiManager, indexar documentos RAG, escolher provedores no gateway | Fora do MaxTerminal (responsabilidade do MaxAiManager) |

---

## 3. Pré-requisitos

### 3.1. Fases
| Fase | Contrato consumido |
|------|--------------------|
| 01 | `mt-core` (`MtError`, `ErrorCode` por prefixo, newtypes Uuid v7, `AppPaths`), `tauri-specta`, logging. O crate `mt-ai` é criado nesta fase. |
| 03 | Pool/conexão SQLite WAL, runner de migrations, `mt_db::sanitizer::sanitize()`, repositório de comandos/blocos. |
| 04 | `triggerRegistry.register(TriggerDefinition)`, `TriggerSelectOutcome`, `useRichInput().text`, `TerminalInstanceHandle.write`, `HybridTerminal #block-actions` → `CommandBlock #actions`. |
| 05 | `shell://command-finished` (`CommandFinished`), `command_block_error_context`/`ShellService::error_context`, `ErrorContext` sanitizado e repositório de blocos. |
| 06 | Stack detectada do projeto (`ProjectStack`). |
| 07 | `TabView`/`PaneView`, sessão e PTY atuais, `WorkspaceObserver` e eventos de fechamento, substituição de PTY e arquivamento; escrita explícita segue o pipeline da 04. |

> Dependências diretas: **04, 05, 07**; 01/03/06 são transitivas. A Fase 11 não é pré-requisito.
> `spec.md` §1.2 distingue contratos publicados de adaptadores novos da 12. Divergências são resolvidas nesses
> adaptadores, sem introduzir dependência de 13/14/15 ou assumir APIs inexistentes na 04.

### 3.2. Ferramentas
| Ferramenta | Uso | Verificação |
|------------|-----|-------------|
| Rust stable ≥ 1.85 | build | `rustc --version` |
| MaxAiManager rodando localmente (opcional para testes unitários) | testes manuais/integração | Confirmar URL, autenticação, versão e operação de disponibilidade na documentação/configuração real; `/health` não é contrato confirmado. |
| `wiremock` (dev-dependency) | mock do gateway nos testes Rust | `cargo test -p mt-ai` |
| Vitest + `@tauri-apps/api/mocks` | testes frontend | `npm run test` |

---

## 4. Entregáveis

### 4.1. Backend
- `src-tauri/crates/mt-ai/Cargo.toml`
- `src-tauri/crates/mt-ai/src/lib.rs` — `AiService`, re-exports
- `src-tauri/crates/mt-ai/src/config.rs` — `AiConfig`, `AiSettings`, validação de URL
- `src-tauri/crates/mt-ai/src/error.rs` — códigos `AI_*` (constantes de `ErrorCode`)
- `src-tauri/crates/mt-ai/src/ids.rs` — `AiRequestId` (newtype local Uuid v7)
- `src-tauri/crates/mt-ai/src/client/{mod.rs,http.rs,sse.rs,dialect.rs}` — `GatewayClient`, SSE, adaptação de payload
- `src-tauri/crates/mt-ai/src/catalog/{mod.rs,fallback.rs}` + `assets/catalog.fallback.json`
- `src-tauri/crates/mt-ai/src/context.rs` — `AiContext`, trait `AiContextSource`
- `src-tauri/crates/mt-ai/src/prompts/{mod.rs,suggest.rs,diagnose.rs,structured.rs}` — templates de prompt pt-BR
- `src-tauri/crates/mt-ai/src/suggest.rs` — NL→shell + parsing da resposta
- `src-tauri/crates/mt-ai/src/risk.rs` — `RiskClassifier`
- `src-tauri/crates/mt-ai/src/diagnose.rs` — `DiagnosisService`, hash, fila
- `src-tauri/crates/mt-ai/src/rag.rs` — `RagService`
- `src-tauri/crates/mt-ai/src/structured.rs` — `StructuredGenerator`, `SchemaRegistry`
- `src-tauri/crates/mt-ai/src/registry.rs` — `AiRequestRegistry`
- `src-tauri/crates/mt-ai/src/repo.rs` — `AiRepo` (SQLite)
- `src-tauri/crates/mt-ai/tests/{gateway_mock.rs,risk_rules.rs,diagnose_cache.rs,structured.rs}`
- `src-tauri/crates/mt-db/migrations/NNNN_12_ai.sql` (NNNN = próximo número livre)
- `src-tauri/src/commands/ai.rs` — comandos IPC; `src-tauri/src/ai_context.rs` — impl `AiContextSource`
- Registro em `src-tauri/src/specta.rs` e `src-tauri/src/state.rs` (`AiService` no `tauri::State`)
- Perfil de dialeto validado e fixtures de contrato sanitizadas: URL, método, autenticação, catálogo, seleção, RAG e suporte a streaming documentados em §4 da spec antes de declarar integração real concluída.

### 4.2. Frontend
- `src/ipc/ai.ts`
- `src/stores/ai.ts` (`useAiStore`)
- `src/composables/ai/{useAiTriggers.ts,useAiSuggestion.ts,useInlineDiagnosis.ts,useRagSearch.ts,useAiGenerate.ts,adapters.ts}`
- `src/components/ai/{HarnessPicker.vue,AiPromptInput.vue,AiSuggestionBlock.vue,RiskBadge.vue,DestructiveConfirm.vue,InlineDiagnosis.vue,RagResults.vue,AiContextPreview.vue,AiStatusIndicator.vue}`
- `src/components/preferences/AiPreferences.vue` (Preferências > IA)
- `src/components/ai/AiPaneResults.vue` — hospedeiro dos cards IA por painel; composição do slot de ações sem importar a Fase 11.
- Testes: `tests/stores/ai.spec.ts`, `tests/composables/ai/*.spec.ts`, `tests/components/ai/*.spec.ts`

---

## 5. Critérios de Aceite

Executados na raiz da worktree; todos com exit code 0 salvo indicação.

### 5.1. Qualidade
- [ ] CA-01 `cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-ai --all-targets -- -D warnings` e `cargo test --manifest-path src-tauri/Cargo.toml -p mt-ai` passam.
- [ ] CA-02 `cargo test --manifest-path src-tauri/Cargo.toml --workspace` passa; geração de bindings e comparação de `src/types/bindings.ts` não produzem divergência.
- [ ] CA-03 `npm run lint`, `npm run typecheck`, `npm run typecheck:vue` e `npm run test` passam; cobertura ≥ 80% de linhas em `src/stores/ai.ts` e `src/composables/ai/`.
- [ ] CA-04 `bindings.ts` exporta `aiHealth`, `aiCatalogGet`, `aiCatalogRefresh`, `aiSettingsGet`, `aiSettingsUpdate`, `aiContextPreview`, `aiSuggestCommand`, `aiCancel`, `aiClassifyRisk`, `aiDiagnoseBlock`, `aiDiagnosisDismiss`, `aiRagListSlugs`, `aiRagSearch`, `aiGenerateStructured`, `aiHistoryList`, `aiHistoryClear`, `aiCacheClear`.

### 5.2. Cliente HTTP (testes com `wiremock`)
- [ ] CA-05 **Toda** requisição (health, catálogo, chat, RAG) contém `X-Agent-Name: MaxTerminal` — teste `every_request_has_agent_header` falha se algum endpoint omitir.
- [ ] CA-06 URL base alterada em `ai_settings_update` passa a valer na próxima requisição sem reiniciar o app.
- [ ] CA-07 URL com host não-loopback é rejeitada com `AI_URL_NOT_LOOPBACK` salvo `allow_remote = true`.
- [ ] CA-08 Dialeto com SSE confirmado: 5 deltas de origem produzem texto idêntico em 1–5 lotes ordenados + 1 `Completed`; JSON único gera 0 deltas + 1 `Completed`. EOF prematuro gera `Failed`, sem comando parcial acionável.
- [ ] CA-09 `ai_cancel` durante stream encerra a conexão em < 200 ms e emite `Cancelled`; nenhum evento chega depois de `Cancelled`.
- [ ] CA-10 Gateway sem resposta → `AI_TIMEOUT` após o timeout configurado (teste com timeout de 300 ms); gateway desligado → `AI_GATEWAY_UNAVAILABLE` com mensagem pt-BR "MaxAiManager não está acessível em …".

### 5.3. Catálogo
- [ ] CA-11 Catálogo no endpoint validado → `source = 'gateway'`; indisponível → último catálogo validado do mesmo gateway ou fallback de identidades (`claude-code`, `codex`, `agy`, `opencode`). Modelo/nível desconhecido fica indisponível, sem enviar `default/low/medium/high` inventados.
- [ ] CA-12 Segunda chamada dentro do TTL não faz HTTP (verificado por contagem no mock); `ETag` + `304` reaproveita o cache.

### 5.4. Gatilhos `@` e `?` (comportamento observável)
- [ ] CA-13 Digitar `@` no início do input abre `HarnessPicker` na etapa 1 (Harness); ↑/↓ navegam, Enter avança, Backspace/← volta etapa, Esc fecha; após a etapa 3 o foco vai ao campo de texto pt-BR e um chip mostra `Harness · Modelo · Raciocínio`.
- [ ] CA-14 Digitar `?` usa o padrão das Preferências sem mostrar as 3 etapas; sem padrão configurado, exibe aviso "Configure o padrão em Preferências > IA" com link.
- [ ] CA-15 Enviar "listar os 5 maiores arquivos desta pasta" exibe `AiSuggestionBlock` com stream visível e, ao final, comando editável. **Nenhum byte é escrito no PTY** até o clique em "Executar" ou "Inserir no input" + Enter do usuário (teste Vitest verifica que `pty write` não é chamado; teste Rust verifica que `mt-ai` não depende de `mt-pty`).
- [ ] CA-16 Sugestão `rm -rf ./build` é `destructive`; "Executar" abre `DestructiveConfirm` que só habilita o botão após digitar `rm`; `ls -la` é `safe` e executa com 1 clique.
- [ ] CA-17 O modelo dizendo `risk: safe` para `git push --force` não rebaixa o risco local (`destructive`).
- [ ] CA-18 `AiContextPreview` mostra shell, SO, cwd, stack e os comandos recentes enviados; um comando recente contendo `Bearer abc123` aparece mascarado no preview e no corpo HTTP capturado pelo mock.

### 5.5. Classificador de risco
- [ ] CA-19 `tests/risk_rules.rs` cobre ≥ 40 casos, incluindo `rm -rf /`, `rm -fr ~`, `sudo rm -r x`, `dd if=… of=/dev/sda`, `mkfs.ext4`, `git push --force`, `git push -f`, `git reset --hard`, `git clean -fdx`, `chmod -R 777 /`, `:(){ :|:& };:`, `curl … | sh`, `> /dev/sda`, `DROP DATABASE`, `php artisan migrate:fresh`, comandos encadeados (`ls && rm -rf x` → `destructive`) e falsos positivos (`echo "rm -rf"` → `caution`, `grep rm file` → `safe`).

### 5.6. Diagnóstico Inline
- [ ] CA-20 Bloco com `exit 1` dispara **1** requisição após 800 ms; 3 erros em < 800 ms no mesmo painel geram 1 requisição (o último). Exit desconhecido não dispara diagnóstico automático.
- [ ] CA-21 O mesmo erro (mesmo `error_hash`) uma segunda vez é servido do `ai_diagnosis_cache` sem HTTP e com `hits` incrementado.
- [ ] CA-22 `exit 130`, blocos marcados como TUI/agente e projetos com opt-out não disparam diagnóstico.
- [ ] CA-23 `InlineDiagnosis` aparece em `HybridTerminal #block-actions` → `CommandBlock #actions` mesmo sem o botão "Inspecionar" (Fase 11); mostra resumo ≤ 160 caracteres e comando corretivo com `RiskBadge`, ações "Inserir no input" e "Ignorar".
- [ ] CA-24 Desligar "Diagnóstico Rápido Inline" em Preferências > IA cancela diagnósticos pendentes e esconde os existentes.

### 5.7. RAG
- [ ] CA-25 Modo RAG no popup `?` funciona mesmo sem seleção padrão de geração; slug digitado/configurado → `/v1/rags/{slug}/search` pelo método/corpo validados. Fixture proposta de teste: POST `{ query, top_k }`; resultado vazio mostra "Nenhum resultado" e preserva a consulta.
- [ ] CA-26 Slug inexistente (404) → mensagem pt-BR "Base de conhecimento '{slug}' não encontrada"; slug com caracteres fora de `[a-z0-9-_]` é rejeitado antes do HTTP (`AI_INVALID_SLUG`).

### 5.8. Geração Assistida
- [ ] CA-27 `ai_generate_structured` com schema `{ name: string, signature: string }` e mock retornando JSON válido devolve o objeto; com JSON inválido, faz 1 tentativa de reparo; persistindo inválido → `AI_SCHEMA_VALIDATION` com `details.errors`.
- [ ] CA-28 `useAiGenerate<T>()` expõe `loading`, `error`, `result`, `run()`, `cancel()` e é usado em teste por um consumidor fictício (simulando Fase 14).

### 5.9. Persistência e privacidade
- [ ] CA-29 Migration aplica em banco limpo e é idempotente no runner da Fase 03; `PRAGMA foreign_key_check` vazio.
- [ ] CA-30 Com "Salvar histórico de prompts" desligado (padrão: ligado), nenhuma linha é gravada em `ai_prompt_history`; `ai_history_clear` e `ai_cache_clear` esvaziam as tabelas.
- [ ] CA-31 Nenhum texto gravado em tabelas `ai_*` contém padrões detectados pelo sanitizer (teste injeta `APP_KEY=base64:...` e verifica `[REDACTED]`).
- [ ] CA-32 Logs `tracing` de `mt-ai` não registram prompts nem respostas em nível ≥ `info` (apenas ids, tamanhos e latências).
- [ ] CA-33 Cancelar antes do aceite IPC, trocar URL, nova sessão, substituir PTY, fechar painel e receber resposta antiga não ressuscitam cards nem preenchem formulários atuais; IDs duplicados são recusados; um único estado terminal por requisição.
- [ ] CA-34 Dois painéis da mesma aba podem diagnosticar erros independentemente; deduplicação de eventos por `blockId`; cache não cruza projeto/cwd/shell/seleção/gateway/versão do prompt. Ignorar um bloco nunca ignora outro com o mesmo hash.
- [ ] CA-35 Gates de integração: nenhum endpoint proposto é anunciado como validado por um mock; registrar evidência sanitizada do gateway real para cada operação habilitada. Sem evidência, capacidade indisponível e mensagem acionável.
- [ ] CA-36 Deltas/erros/resultados são sanitizados antes da UI, logs ou disco, inclusive segredo dividido entre chunks; JSON Schema não resolve `$ref` remoto. `useAiGenerate<T>` usa decodificador de runtime e respeita revisão do formulário consumidor.

---

## 6. Riscos e Mitigação

| # | Risco | Impacto | Mitigação |
|---|-------|---------|-----------|
| R-01 | Contrato real do gateway MaxAiManager difere do assumido (rotas, payload, catálogo). | Alto | Toda tradução isolada em `client/dialect.rs` (`GatewayDialect`); suposições numeradas em `spec.md` §4.0; fallback estático do catálogo; testes de contrato com `wiremock` documentam o formato esperado. |
| R-02 | Execução acidental de comando perigoso sugerido pela IA. | Crítico | Nunca auto-executar; `mt-ai` sem dependência de `mt-pty`; risco = máx(local, modelo); `DestructiveConfirm` com digitação do token; "Inserir no input" não envia Enter. |
| R-03 | Vazamento de segredos para o gateway ou para o disco. | Alto | Contexto e prompts passam por `sanitize()` antes do HTTP e antes do SQLite; limite de 20 comandos e 4 KiB de output; recusa de host remoto por padrão; logs sem conteúdo. |
| R-04 | Rajadas de erro (loop de build) gerarem dezenas de requisições. | Médio | Debounce por painel, fila global com concorrência 1 e capacidade 3 (descarta mais antigos), cache por hash contextual, rate limit 10/min. |
| R-05 | Latência alta de modelos com raciocínio elevado travar a UX. | Médio | Streaming SSE, indicador de progresso, cancelamento por `Esc`, timeouts por operação configuráveis, diagnóstico usa nível de raciocínio baixo por padrão. |
| R-06 | Modelo retornar texto fora do formato (markdown, múltiplos comandos). | Médio | Pedir JSON quando suportado; aceitar somente JSON válido com comando não vazio; não transformar prosa ou juntar linhas com `&&`. |
| R-07 | Contratos das Fases 04–07 divergirem durante implementação. | Médio | Adaptadores únicos (`ai_context.rs`, `adapters.ts`) e tabela reconciliada em `spec.md` §1.2. |
| R-08 | Normalização excessiva do erro reutilizar correção de outro contexto. | Médio | Hash inclui gateway, projeto, cwd, shell, comando completo, saída sanitizada, seleção e versões; preservar códigos e caminhos significativos. |
| R-09 | Classificador de risco com falsos negativos. | Alto | Regras conservadoras (na dúvida `caution`), tokenização com `shell-words`, análise de cada segmento de `&&`, `;`, `|`, `$( )`; suíte de casos crescente. |
