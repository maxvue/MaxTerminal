# Fase 16 — Aba Fixa de GitHub Issues

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) §7 e §4.1 (badge 🟣 Issues Abertas).
> Contratos compartilhados: [`../README.md`](../README.md) §2. Especificação técnica: [`spec.md`](./spec.md).
> Referência externa (somente leitura): vocabulário de labels do MaxCode em
> `~/GitHub/MaxCode/src/core/labelsDeBugs.ts` (fonte única das 24 labels, grafia byte a byte do GitHub) e
> convenção de worktree em `~/GitHub/MaxCode/src/core/cicloDeBugs.ts` (`nomeDaWorktreeDeIssue`).
> Leitura de referência em 29/10/2026: checkout MaxCode `1054eb8a886de412de8912ad56f64b1367da8b09`.
> O checkout local serve à conferência; o build deve consumir export/fixtures versionados, sem depender dessa pasta.

---

## 1. Objetivo da Fase

Transformar todo projeto cujo diretório seja um repositório GitHub em um **painel operacional de Issues** dentro do
MaxTerminal, totalmente compatível com o workflow de bugs do MaxCode:

1. Detectar automaticamente o repositório GitHub (remote `origin` em `github.com`, SSH ou HTTPS).
2. Buscar, cachear (SQLite WAL) e manter atualizadas as issues e comentários, via `gh` CLI com fallback para a
   REST API (token no keyring do sistema), respeitando paginação, rate limit e ETag.
3. Exibir o contador 🟣 **Issues Abertas** na lista de projetos (slot de extensão da Fase 06).
4. Exibir uma **aba fixa** "Issues" (primeira posição, não fechável) no TabBar da Fase 07, com lista filtrável,
   detalhe em Markdown, destaque do `## PLANO DE IMPLEMENTAÇÃO`, edição manual de labels e criação de issue.
5. Oferecer as ações por issue — **Abrir**, **Analisar (`BUG PLAN`)**, **Corrigir (`BUG FIX`)** no MaxCode com
   1 clique e **Abrir Aba no MaxTerminal** na worktree/branch da issue.
6. Oferecer a ação global **"Corrigir todos os Bugs"**, que dispara o ciclo automatizado do MaxCode (deduplicação →
   planejamento → execução → verificação) e acompanha o progresso confirmado pelo MaxCode; snapshots de labels
   são apenas observações auxiliares, nunca evidência de término — **sem reimplementar o MaxCode**.

---

## 2. Escopo

### 2.1. Incluído nesta fase

**Backend — crate `mt-github` (+ migration em `mt-db` + comandos no crate `maxterminal`)**
- `remote`: parser de URLs de remote (`git@github.com:o/r.git`, `ssh://git@github.com/o/r`,
  `https://github.com/o/r(.git)`, `https://user@github.com/o/r`), suporte a worktrees (`.git` arquivo) e
  descoberta do remote via API da Fase 08 (`mt-git`).
- `client`: trait `GithubClient` com dois transportes da mesma REST — `GhCliClient` (`gh api --include`, JSON
  por stdin nas escritas) e `RestClient` (`reqwest`, ETag/`If-None-Match`, paginação por `Link`, rate limit por
  `X-RateLimit-*`/`Retry-After`). `FallbackClient` usa gh primeiro; fallback automático é permitido em leituras
  ou falha comprovadamente anterior ao envio. Escrita com resultado incerto exige reconciliação, nunca replay.
- `auth`: `TokenProvider` (ordem: `gh auth token` → keyring), `SecretStore` sobre o crate `keyring` (Secret
  Service/KWallet no CachyOS). **Token nunca é gravado no SQLite nem em logs**; settings guardam apenas a
  preferência de fonte do token.
- `labels`: importação do catálogo canônico das **24 labels do MaxCode** (15 de estado, 3 de encerramento,
  6 de categoria), sem completar nomes por inferência. Gate de cardinalidade, unicidade, procedência e fixtures
  byte a byte; cor/descrição são opcionais quando ausentes na fonte. Transições exportadas servem somente à
  validação/visualização, não à execução local do workflow. `ensure_labels` cria apenas faltantes por ação
  explícita; metadados ausentes exigem definição explícita antes da criação, sem sobrescrever existentes.
- `cache`: repositórios SQLite para `github_repos`, `github_issues`, `github_issue_comments`, `github_labels`,
  `github_http_cache`, vínculos projeto/repositório, joins de labels e `github_bug_cycles`
  (migration `0010_16_github.sql`, D-01), com TTL, partição de autenticação e revisão monotônica.
- `sync`: `IssueSyncService` com refresh em background (TTL padrão 120 s para lista, 300 s para detalhe), refresh
  incremental por `since`/`updatedAt`, coalescência de pedidos concorrentes, backoff em rate limit e contagem de
  issues abertas por projeto.
- `actions`: payloads de envio ao MaxCode (`abrir` / `analisar` / `corrigir`) pelo transporte da Fase 11.
  Contrato adicional versionado de issues/ciclos e URI devem ser implementados e validados nos dois lados;
  `maxcode_open_plan` existente não executa `BUG FIX` nem substitui o ciclo em lote (spec §8).
- `worktree`: resolução da worktree/branch da issue seguindo a convenção do MaxCode
  (`.max-code-worktrees/wt-implement-issue-<n>`, branch `wt-implement-issue-<n>`), consulta pela API de worktrees da
  Fase 08 e criação somente após confirmação do usuário.
- `bulk`: `BugCycleTracker` — único disparo idempotente ao MaxCode, consulta por `requestId`/`cycleId`,
  reconexão e acompanhamento; a fila e a deduplicação semântica pertencem exclusivamente ao MaxCode.
- Comandos Tauri `github_*` e eventos `github://*` (lista completa no `spec.md` §5 e §6).
- Ponto de extensão da Fase 06: implementação de `ProjectBadgeProvider` alimentando o badge 🟣.

**Frontend**
- `src/ipc/github.ts`, `src/stores/githubIssues.ts` (`useGithubIssuesStore`).
- Composables `useGithubIssueEvents`, `useIssueFilters`, `useIssuePlan`, `useIssueBadgeProvider`,
  `useMarkdownRenderer` (render seguro com sanitização de HTML).
- Componentes em `src/components/github/`: `IssuesTab.vue`, `IssueList.vue`, `IssueListItem.vue`,
  `IssueFilters.vue`, `IssueDetail.vue`, `IssuePlanSection.vue`, `IssueComments.vue`, `IssueLabelEditor.vue`,
  `IssueLabelChip.vue`, `IssueCreateModal.vue`, `IssueActionsMenu.vue`, `IssueOpenTabModal.vue`,
  `BugCycleConfirmModal.vue`, `BugCycleProgress.vue`, `GithubAuthPanel.vue`, `LabelSyncBanner.vue`.
- Composição pelo slot `TabBar #pinned` da Fase 07, antes das abas de terminal, apenas quando o projeto tiver
  `GithubRepoRef`. Aba virtual sem PTY/SessionId/TabId fictício, sem fechar ou reordenar.
- Registro do badge 🟣 no `useProjectBadgeRegistry` da Fase 06 (`order: 60`); usar token roxo verificado no tema
  instalado, sem presumir a existência de `--max-help-500`.

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| Busca global FTS5 que inclua issues/comentários (esta fase só cria índices B-tree; a tabela FTS das issues é registrada na 17) | 17 |
| Testes E2E (WebDriver/tauri-driver) do fluxo completo, benchmarks de sincronização com milhares de issues, empacotamento | 17 |
| Execução do ciclo de bugs (dedup, plano, execução, verificação, merge) — **é do MaxCode**; aqui só disparo e acompanhamento | — (MaxCode) |
| Handler MaxCode de comandos de issue/ciclo e URI (proposta versionada em spec §8; gate de integração, não API já existente) | — (MaxCode) |
| Comentar, fechar/reabrir, atribuir responsáveis ou milestones (não citados no AGENTS.md §7) | Não planejado |
| Issues de repositórios fora do GitHub (GitLab, Gitea) e GitHub Enterprise com host próprio (o parser aceita `host`, mas só `github.com` é ativado) | Não planejado |
| Telemetria de agente na aba aberta a partir da issue (StatusLine, alerta de permissão) | 13 (consumida, não alterada) |
| Notificação nativa ao concluir o ciclo em lote (usa a infra de notificação da Fase 09; nenhuma nova API de SO) | 09 (consumida) |

---

## 3. Pré-requisitos

### 3.1. Fases (somente contratos de número menor)
| Fase | Contrato consumido |
|------|--------------------|
| 01 | `mt-core` (`ProjectId`, `TabId`, `TemplateId`, `MtError`/`ErrorCode` com prefixo de domínio, Uuid v7), `tauri-specta`, Vitest, `tsgo`, ESLint |
| 03 | `mt-db` (writer thread, leituras em `spawn_blocking`, runner de migrations, `SettingsRegistry`, `RetentionRegistry`, `sanitizer::sanitize`) |
| 06 | `useProjectsStore.selectedId`, `Project.path`, `ProjectBadgeProvider`/`useProjectBadgeRegistry().register`, evento `project://removed` |
| 07 | `TabBar #pinned`, `useTabsStore`, `tab_open(TabOpenRequest { overrides: TabOpenOverrides { cwd, cwd_fallback_to_project: false, … }, … })` |
| 08 | `mt-git`: `GitRunner` para remote `origin`, descoberta de `common_dir`, `git_worktree_list`, `git_worktree_create(WorktreeCreateRequest)`, validação de branch |
| 09 | Notificação nativa (`os_notify`) para término do ciclo em lote |
| 11/06 | `MaxCodeClient::{send_issue_command, bugs_cycle_preview, bugs_cycle_start, bugs_cycle_status}`, capabilities `issues_protocol_v1`/`bugs_cycle_v1`, evento `bugs.cycle.progress` e URI; consumidor via `MaxCodePort` (D-03) |
| 13 | `AgentKind` (`claude-code`, `codex`, `agy`, `opencode`), `AppAgentState::open_agent_tab(AgentTabOpenRequest) -> MtResult<AgentTabOpened>` com `WorktreeIsolation::Existing { path }` (D-08) |
| 15 | Pré-requisito de persistência: `0009_15_command_groups.sql` deve existir antes de `0010_16_github.sql` (D-01); sem acoplamento aos serviços de grupos. |

> Portas são definidas em `mt-github/src/ports.rs`; adaptadores nativos ficam em `src-tauri/src/github/adapters.rs`
> e wrappers frontend em `src/ipc/github.ts`. A 11/06 fornece a extensão de protocolo D-03; a 16 somente consome.
> O suporte remoto ainda requer confirmação bilateral com MaxCode, nunca inferida da existência do adaptador.
> O catálogo exportado e o handshake de issues/ciclo são gates de entrega. Ausentes, leitura/criação/edição
> genérica continuam disponíveis; ações de workflow ficam indisponíveis com motivo explícito.

### 3.2. Ferramentas
- `gh` CLI ≥ 2.40 (opcional; sem ele o app usa REST). `git` ≥ 2.40.
- Rust estável ≥ 1.85. Novas dependências em `mt-github`: `reqwest` (`rustls-tls`, `json`, `gzip`),
  `keyring = "3"` (feature `linux-native-sync-persistent` / Secret Service), `url`, `httpdate`, `serde_json`,
  `tokio`, `wiremock` (dev), `tempfile` (dev), `insta` (dev, snapshots de payloads).
- Frontend: `markdown-it` + `dompurify` (render seguro de Markdown), sem dependências de rede em runtime.

---

## 4. Entregáveis

### 4.1. Backend
| Caminho | Conteúdo |
|---------|----------|
| `src-tauri/crates/mt-github/Cargo.toml` | Manifesto |
| `src-tauri/crates/mt-github/src/lib.rs` | Reexports, `GithubService` (fachada) |
| `src-tauri/crates/mt-github/src/model.rs` | `GithubRepoRef`, `Issue`, `IssueSummary`, `IssueComment`, `IssueState`, `LabelInfo`, DTOs |
| `src-tauri/crates/mt-github/src/error.rs` | Códigos `github.*` e mapeamento para `MtError` |
| `src-tauri/crates/mt-github/src/remote.rs` | Parser de remote + detecção |
| `src-tauri/crates/mt-github/src/auth.rs` | `TokenProvider`, `SecretStore`, `KeyringSecretStore` |
| `src-tauri/crates/mt-github/src/client/mod.rs` | Trait `GithubClient`, `ClientKind` |
| `src-tauri/crates/mt-github/src/client/gh_cli.rs` | `GhCliClient` (processos `gh` com timeout e kill garantido) |
| `src-tauri/crates/mt-github/src/client/rest.rs` | `RestClient` (ETag, paginação, rate limit) |
| `src-tauri/crates/mt-github/src/client/fallback.rs` | `FallbackClient` |
| `src-tauri/crates/mt-github/src/rate_limit.rs` | `RateLimitState`, política de backoff |
| `src-tauri/crates/mt-github/src/labels.rs` | Importação/validação do catálogo, `LabelKind`, preview puro de transições exportadas, `ensure_labels` |
| `src-tauri/crates/mt-github/fixtures/maxcode/*` | Export versionado do catálogo, procedência/hash, vetores de comparação/transição e protocolo/worktree |
| `src-tauri/crates/mt-github/src/plan.rs` | Extração do último `## PLANO DE IMPLEMENTAÇÃO` |
| `src-tauri/crates/mt-github/src/cache.rs` | `GithubCacheRepo` (SQLite) |
| `src-tauri/crates/mt-github/src/sync.rs` | `IssueSyncService` (TTL, background, coalescência) |
| `src-tauri/crates/mt-github/src/actions.rs` | `MaxCodeIssueCommand`, payloads bridge/URI |
| `src-tauri/crates/mt-github/src/worktree.rs` | `IssueWorktreeResolver` |
| `src-tauri/crates/mt-github/src/bulk.rs` | `BugCycleTracker` |
| `src-tauri/crates/mt-github/src/ports.rs` | Adaptadores para `mt-git`, `mt-bridge`, `mt-agents`, notificações |
| `src-tauri/crates/mt-github/src/events.rs` | Payloads e trait `GithubEventSink` |
| `src-tauri/crates/mt-github/tests/*.rs` | `remote.rs`, `labels.rs`, `plan.rs`, `rest_client.rs` (wiremock), `gh_cli.rs` (gh falso em `PATH`), `cache.rs`, `sync.rs`, `bulk.rs` |
| `src-tauri/crates/mt-db/migrations/0010_16_github.sql` | DDL (versão 10 fixa; D-01) |
| `src-tauri/src/commands/github.rs` | Comandos `github_*` |
| `src-tauri/src/events/github.rs` | `TauriGithubEventSink` |

### 4.2. Frontend
| Caminho | Conteúdo |
|---------|----------|
| `src/ipc/github.ts` | Wrappers tipados sobre `bindings.ts` + `listen` dos eventos |
| `src/constants/githubLabels.ts` | Metadados de UI derivados do catálogo exposto por `github_labels_catalog` (sem duplicar tipos) |
| `src/stores/githubIssues.ts` | `useGithubIssuesStore` |
| `src/composables/useGithubIssueEvents.ts` | Assinatura de `github://*` |
| `src/composables/useIssueFilters.ts` | Filtros, busca e ordenação puros |
| `src/composables/useIssuePlan.ts` | Localização/realce do plano |
| `src/composables/useIssueBadgeProvider.ts` | Registro do badge 🟣 |
| `src/composables/useMarkdownRenderer.ts` | `markdown-it` + `DOMPurify` |
| `src/components/github/*.vue` | 16 componentes listados em §2.1 |
| `tests/frontend/github/*.spec.ts` | Testes Vitest |

---

## 5. Critérios de Aceite

### 5.1. Automatizados
1. `cargo test -p mt-github` passa com cobertura dos seguintes cenários (sem meta artificial de quantidade):
   - `remote`: ≥ 12 formatos de URL (SSH scp-like, `ssh://`, HTTPS com/sem `.git`, com usuário, com porta,
     barra final, maiúsculas no host) → `GithubRepoRef` correto; GitLab/Bitbucket/caminho local → `None`.
   - `labels`: catálogo tem exatamente 24 entradas (15/3/6), nomes byte a byte iguais aos de
      export versionado de `labelsDeBugs.ts`; falta de fonte/hash/fixtures, colisão normalizada ou contagem
      divergente bloqueia compatibilidade. Preview puro é idempotente e preserva labels alheias/categoria;
      nenhuma mudança automática de etapa é escrita no GitHub pelo MaxTerminal.
   - `plan`: com 3 comentários de plano, retorna o **último**; cabeçalho dentro de bloco de código é ignorado;
     sem plano ⇒ `None`.
   - `rest_client` (wiremock): `304 Not Modified` com ETag reaproveita cache e não conta como alteração;
     paginação por `Link` coleta 3 páginas; `403` com `X-RateLimit-Remaining: 0` ⇒ `github.rate_limited` com
     `reset_at`; `429`/`Retry-After` respeitado; pull requests (`pull_request` presente) são descartados.
    - `gh_cli`: gh falso fornece status/headers/JSON de `gh api`; ausência ⇒ REST; timeout mata e recolhe o
      processo. POST enviado seguido de timeout não é repetido por REST; erro 422 não ativa fallback.
   - `cache`: TTL vencido ⇒ `is_stale = true`; upsert de lista remove do cache issues que deixaram de existir
      apenas após reconciliação completa/autorizada; corpo e comentários passam por `sanitize()`.
      Falha na página intermediária preserva contagem/cursor anterior; troca de conta isola cache privado.
    - `sync`: 10 chamadas concorrentes do mesmo recurso ⇒ **1** operação paginada (não uma chamada por consumidor).
   - `bulk`: sequência de snapshots simulada (`Waiting Plan` → `Planned`/`Waiting Execute` → `Executed`/
      `Waiting Check` → `Checked Approved`) atualiza observações; só ACK/status MaxCode confirma início/fim.
      Perda de ACK e reinício recuperam o mesmo `requestId`, sem novo ciclo; ordenação usa revisão do emissor.
2. Teste de segurança: após `github_auth_set_token`, `SELECT` em **todas** as tabelas e em `settings` não contém o
   token; logs de teste (`tracing` capturado) não contêm o token.
3. `cargo clippy -p mt-github -p maxterminal -- -D warnings` sem avisos.
4. Na raiz da worktree, `cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings` e depois `git diff --exit-code src/types/bindings.ts` ⇒ 0; `bindings.ts` exporta
   `GithubRepoRef`, `IssueSummary`, `IssueDetail`, `LabelDefinition`, `LabelKind`, `BugCycleProgress`,
   `MaxCodeIssueCommand` e nenhum deles é declarado manualmente em `src/` (`grep` vazio).
5. `tsgo --noEmit`, ESLint e Stylelint sem erros.
6. Vitest (`tests/frontend/github`) cobre: filtros combinados (label + estado + categoria + busca) puros; store
   aplica `github://issues-updated` de forma incremental; badge 🟣 não renderiza com `0` nem para projeto sem
   GitHub; `IssuesTab` não exibe botão de fechar e é sempre índice 0; `useMarkdownRenderer` remove `<script>`,
   `on*=` e `javascript:`; `IssuePlanSection` destaca o último plano; confirmação obrigatória antes de
   `github_bug_cycle_start` e de `github_issue_worktree_create`.

### 5.2. Comportamento observável
7. Selecionar o projeto MaxTerminal (remote GitHub) exibe a aba fixa "Issues" na 1ª posição, sem botão de fechar;
   selecionar um projeto sem remote GitHub não exibe a aba.
8. O badge 🟣 conta todas as issues abertas, excluindo PRs e filtros da UI; validar com fixture paginada acima
   de 1000 issues. Repositório selecionado atualiza no TTL de 120 s quando há rede/cota; cache atrasado é marcado,
   erro/autenticação não vira zero e projetos em segundo plano compartilham orçamento de atualização.
9. Com `gh` desinstalado (ou `PATH` sem `gh`) e token salvo no painel de autenticação, a lista continua
   funcionando via REST; `secret-tool search service maxterminal` mostra o item; `sqlite3 maxterminal.db .dump |
   grep -c <token>` ⇒ `0`.
10. Filtrar por estado `Waiting Check` + categoria `Seguranca` + busca textual reduz a lista em < 50 ms com 1000
    issues em cache (medido no DevTools).
11. O detalhe renderiza Markdown (títulos, listas, código, tabelas, links externos abertos no navegador do
    sistema) e a seção `## PLANO DE IMPLEMENTAÇÃO` aparece em destaque com âncora "Ir para o plano".
12. Editar labels manualmente: adicionar `Waiting Execute` a uma issue com `Planned` reflete no GitHub
    (`gh issue view <n> --json labels`) e no cache; se a combinação violar uma transição conhecida, a UI avisa
    (aviso não bloqueante, pois a edição manual é soberana).
13. "Criar nova issue" cria a issue no GitHub com título, corpo e labels escolhidos; ela aparece conforme a
    ordenação/filtros sem esperar o TTL. Labels omitidas pelo servidor são informadas como resultado parcial.
14. "Abrir", "Analisar (`BUG PLAN`)" e "Corrigir (`BUG FIX`)" recebem ACK correlacionado do MaxCode com
    contrato compatível; URI apenas acorda/transporta conforme suporte anunciado. Abrir o VS Code não significa
    ação executada. Testar falta de capability, ACK perdido e entrega duplicada com um único efeito.
15. "Abrir Aba no MaxTerminal" de uma issue sem worktree pede confirmação, cria
    `.max-code-worktrees/wt-implement-issue-<n>` (branch `wt-implement-issue-<n>`) e abre a aba com `cwd` nela; se
    a worktree já existir, abre direto, sem diálogo.
16. "Corrigir todos os Bugs" exige confirmação listando a quantidade de issues por etapa; após confirmar, o painel
    `BugCycleProgress` atualiza contadores por etapa conforme as labels mudam no GitHub; fechar e reabrir o app
     retoma o acompanhamento do ciclo ativo por consulta ao MaxCode, sem reiniciar fila. Labels não provam término.
17. Repositório sem as labels canônicas exibe `LabelSyncBanner` ("Faltam N labels do MaxCode") e o botão "Criar
    labels" cria **somente** as faltantes (labels existentes com outra cor permanecem intactas).
18. Todos os textos visíveis, tooltips, `aria-label` e mensagens `MtError.message` estão em pt-BR.

---

## 6. Riscos e Mitigação

| Risco | Impacto | Mitigação |
|-------|---------|-----------|
| Rate limits primário/secundário variam com credencial | Lista atrasada | Ler headers, compartilhar orçamento gh/REST, ETag por página, TTL e pausa com jitter. Não assumir cota fixa, isenção universal de 304 ou exatidão de Search; preservar último total completo com idade |
| `gh` com saída/flags diferentes entre versões | Parse quebrado | Fixar campos `--json` explícitos, testes com fixtures por versão, fallback REST em erro de parse |
| Vazamento de token (logs, SQLite, argumentos de processo) | Segurança | Token só em memória/keyring; `gh` recebe token via ambiente `GH_TOKEN` apenas quando a fonte for keyring; `tracing` com `SanitizerLogRedactor`; teste explícito (CA-2) |
| Keyring indisponível (sessão sem Secret Service) | Sem fallback REST autenticado | Erro `github.keyring_unavailable` com instrução pt-BR; REST não autenticada continua (somente leitura) |
| Grafia divergente de labels | Labels duplicadas no repo | Export versionado byte a byte das constantes reais (não comentários), comparação conforme fixtures MaxCode; catálogo incompleto bloqueia workflow, sem nomes inventados |
| Disputa com o MaxCode ao editar labels durante o ciclo | Estado inconsistente | Durante ciclo ativo, edição manual de labels de estado exige confirmação extra; edições usam `add`/`remove` (nunca substituição total) |
| MaxCode sem handler para as novas mensagens/URI | Ações sem efeito | Detecção de capacidade via handshake da Fase 11 (`capabilities`); sem suporte ⇒ botão desabilitado com tooltip explicativo |
| Markdown malicioso (XSS no WebView) | Execução de script | `markdown-it` com `html: false` + `DOMPurify`; links com `rel="noopener"` abertos fora do WebView; CSP da Fase 01 |
| Muitos projetos GitHub (dezenas) | Consumo de cota/CPU | Scheduler com janela por projeto, prioridade ao selecionado, limite de 2 sincronizações simultâneas |
| Criação acidental de worktree | Sujeira no repo | Confirmação obrigatória, caminho apenas em `.max-code-worktrees/`, verificação de `.gitignore` antes de criar |

## 7. Sequência de implementação e gates

1. Validar export MaxCode e contratos pendentes da bridge; fixar fixtures e versões em spec §2/§8.
2. Implementar transporte/cache/sync e testar paginação, reconciliação, autenticação e falhas parciais.
3. Integrar store, badge e aba virtual; depois detalhe/plano, criação e labels com confirmação de resultado remoto.
4. Integrar abertura de worktree/aba pelo ciclo de vida das fases 07/08/13.
5. Habilitar ações MaxCode/ciclo somente com contrato validado e testes de idempotência/reconexão.
6. Verificar bindings, Rust, frontend e cenários observáveis. Esta documentação não constitui implementação
   dos handlers externos nem execução desses testes; a Fase 17 recebe os cenários E2E e índices de leitura.
