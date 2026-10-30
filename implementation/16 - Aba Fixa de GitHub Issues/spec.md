# Fase 16 — Especificação técnica da aba fixa de GitHub Issues

> Escopo e aceite: [`instructions.md`](./instructions.md). Contratos comuns: [`../README.md`](../README.md).
> Dependências diretas: 06, 07, 08, 11, 13 e 15; infraestrutura transitiva: 01/03/09.
> A aresta 15→16 é de persistência: `0009_15_command_groups.sql` deve preceder `0010_16_github.sql` (D-01), sem consumo obrigatório de serviços de grupos.
> Este documento especifica implementação futura. Nomes da integração adicional MaxCode em §8 são
> **propostas de contrato**, não handlers comprovadamente disponíveis na extensão.

## 1. Arquitetura, invariantes e fronteiras

```text
Project.path → mt-git/GitRunner → GithubRepoRef → GithubService
                                               ├─ GithubClient → GhCliClient / RestClient
                                               ├─ IssueSyncService → mt-db writer → eventos
                                               ├─ LabelCatalog (export MaxCode validado)
                                               ├─ IssueWorktreeResolver → fases 08/07/13
                                               └─ MaxCodePort → transporte fase 11 → MaxCode
eventos + IPC → useGithubIssuesStore → TabBar #pinned / ProjectBadgeProvider
```

- O GitHub é autoritativo para issues, comentários e labels; SQLite é cache sanitizado. MaxCode é autoritativo
  para a fila de bugs, deduplicação semântica, planejamento, execução, verificação e resultado do ciclo.
- Projeto sem GitHub não ganha aba nem contador. Falha de rede em repo identificado não remove a aba.
- A aba virtual de issues não cria PTY, sessão, TabId ou estado de execução, nem altera os cinco badges nativos.
- Chave remota: `(host, owner_normalizado, repo_normalizado)`; chave de cache: `(repo, auth_scope)`;
  chave de issue: `(repo, auth_scope, number)`. O número sozinho não identifica uma issue.
- `auth_scope` é UUID opaco associado à seleção/geração de credencial, sem token/hash de token. Conta trocada,
  credencial revogada ou origem de autenticação alterada gera nova partição e cancela trabalhos da anterior.
- Nunca executar texto de issue/comentário como shell ou instrução operacional. Comandos Git/gh são argv,
  sem `sh -c`. Nenhum loop de correção é implementado no MaxTerminal.

### 1.1. Contratos consumidos e adaptações

| Base | Contrato observado | Adaptação nesta fase |
|------|--------------------|---------------------|
| 06 | `useProjectsStore.selectedId`, `Project.path`, `ProjectBadgeProvider`, `register(provider)`, `project://removed` | Badge `id: 'github-issues'`, `order: 60`; reatividade do contador; desligar assinatura ao remover projeto |
| 07 | `TabBar #pinned`; `TabOpenRequest { project_id, template_id, overrides, focus }`; `TabOpenOverrides.cwd` | Composição virtual; `cwd_fallback_to_project: false`; nunca fingir API `registerPinnedTab` |
| 08 | `GitRunner`, descoberta de `common_dir`/repo, `WorktreeManager::{list,create}` e `WorktreeCreateRequest` | Remote por argv `git remote get-url origin`; validar pertencimento e branches; não criar segundo gerenciador |
| 11/06 | `MaxCodeClient::{send_issue_command, bugs_cycle_preview, bugs_cycle_start, bugs_cycle_status}`, capabilities `issues_protocol_v1`/`bugs_cycle_v1`, evento `bugs.cycle.progress` e builder de URI | Consumir via `MaxCodePort`; contrato D-03 ainda sujeito à confirmação bilateral com MaxCode, sem alterar a bridge nesta fase |
| 13 | `AppAgentState::open_agent_tab(AgentTabOpenRequest) -> MtResult<AgentTabOpened>`, `WorktreeIsolation::Existing { path }`, `AgentKind` | Adaptador de `AgentPort` chama o serviço D-08; IDs `claude-code`, `codex`, `agy`, `opencode`; usar worktree já resolvida |
| 03/09 | Writer, migrations WAL, sanitizer, retenção e notificação nativa | Persistir antes de emitir; notificar término somente confirmado pelo MaxCode |
| 15 | Migration `0009_15_command_groups.sql` | Pré-requisito da versão 10 de GitHub; nenhuma porta/serviço de grupos é injetado por causa desta aresta. |

`ports.rs` define portas assíncronas testáveis `ProjectPort`, `GitPort`, `WorkspacePort`, `AgentPort`,
`MaxCodePort` e `NotificationPort`. Implementações no composition root de `maxterminal`; `mt-github` não
depende de `AppHandle` nem chama IPC como mecanismo interno. Interfaces novas não são atribuídas às fases-base.

## 2. Catálogo de labels: autoridade e gate obrigatório

### 2.1. Procedência

Fonte conferida somente para leitura: `~/GitHub/MaxCode/src/core/labelsDeBugs.ts`, checkout
`1054eb8a886de412de8912ad56f64b1367da8b09`. A fonte exporta `LABELS_DE_ESTADO`,
`LABELS_DE_ENCERRAMENTO`, `LABELS_DE_CATEGORIA`, `TRANSICOES` e helpers de comparação.
`cicloDeBugs.ts::nomeDaWorktreeDeIssue` retorna `wt-implement-issue-${numero}` no checkout consultado.
Constantes executáveis determinam a grafia; D-02 estabiliza o catálogo literal abaixo, preservado byte a byte:

| Tipo | Quantidade | Nomes exatos |
|------|------------|-------------|
| `state` | 15 | `Waiting Plan`, `Waiting More Info`, `Planned`, `Waiting Execute`, `Executed`, `Waiting Check`, `Checked Approved`, `Checked Rejected`, `Waiting Re-plan`, `Fix Merged`, `Waiting Deploy`, `Deployed`, `Waiting user test`, `User Rejected`, `User Approved` |
| `closure` | 3 | `Duplicated`, `Evidence not Found`, `Dismissed` |
| `category` | 6 | `Regra de Negocio`, `Testes`, `Desempenho`, `Seguranca`, `Audit`, `Documentacao` |

O catálogo acima é suficiente para conferir os 24 nomes sem checkout externo. A fixture versionada mantém
procedência, transições e vetores de comparação; teste de paridade consulta a fonte MaxCode quando disponível,
registrando ausência como teste opcional não executado. Build/CI normal valida a fixture embutida e esta lista.

Entregáveis futuros em `mt-github/fixtures/maxcode/`:

- `labels.v1.json`: `schemaVersion`, `sourceRepository`, `sourceCommit`, `sourcePath`,
  `sourceSha256`, `exportSha256` (hash canônico excluindo este campo), `labels[]` e `transitions[]`.
- Cada label: `name` original, `kind: state | closure | category`, `color?: string`,
  `description?: string`. Cores e descrições ausentes não são inventadas como canônicas.
- `labels-cases.v1.json`: entradas e saídas produzidas pelos helpers reais do MaxCode, incluindo caixa,
  espaços, vazio, labels externas e combinações de estado válidas. Gerar no ambiente do MaxCode, sem executar
  TypeScript externo arbitrário no build do MaxTerminal.
- `bridge-cases.v1.json` e `worktree-cases.v1.json`: export de handshake/payloads aceitos e resolução de nomes.
  Ausência de implementação bilateral impede afirmar compatibilidade mesmo que exista fixture local.

### 2.2. Validação

1. Validar schema, repositório/commit de origem, hash do arquivo fonte e export, referências das transições.
2. Exigir **24 entradas distintas**, **15 state**, **3 closure**, **6 category**. Contar entradas reais,
   não `enum` com placeholders. Unicidade tanto byte a byte quanto pela chave de comparação do MaxCode.
3. Conferir igualdade byte a byte com export fixado e vetores de comparação. Build/CI falha se fixture
   estiver ausente, alterada sem procedência, ou cardinalidade divergir. CI normal usa artefatos versionados;
   atualização do catálogo requer nova exportação e revisão, não acesso ao checkout pessoal.
4. Runtime retorna `CatalogStatus::Compatible | Missing | Invalid | PeerMismatch` com motivo. Mismatch entre
   versão/hash local e peer desabilita automação dependente do workflow; labels do próprio GitHub continuam
   legíveis/editáveis e issues podem ser criadas sem inventar categorias.
5. Transições exportadas só alimentam preview/avisos. Não assumir exclusividade de uma label de estado: o
   MaxCode pode legitimamente manter, por exemplo, `Planned` junto de `Waiting Execute`.

`ensure_labels`: lista **todas** as labels do repo; compara pela normalização exportada; apresenta faltantes.
Criação é explícita. Se export não trouxer cor, solicitar cor válida de seis dígitos hex; descrição pode ser
omitida. Essas escolhas são metadados do repositório, não alterações ao catálogo canônico. Não atualizar cor,
descrição ou caixa de label existente. Em corrida/422 de duplicidade, reler e confirmar identidade.

## 3. Backend Rust: módulos e contratos

Arquivos previstos em `src-tauri/crates/mt-github/src/`:

| Módulo | Responsabilidade |
|--------|------------------|
| `lib.rs`, `model.rs`, `error.rs` | Fachada `GithubService`, DTOs Specta e erros `github.*` |
| `remote.rs`, `auth.rs` | Descoberta GitHub; `TokenProvider` e `SecretStore`/keyring |
| `client/{mod,gh_cli,rest,fallback}.rs` | Transporte paginado equivalente e política de fallback |
| `rate_limit.rs`, `sync.rs`, `cache.rs` | Orçamento compartilhado, coalescência, cache e revisões |
| `labels.rs`, `plan.rs` | Catálogo/preview e extração estrutural de Markdown |
| `actions.rs`, `bulk.rs`, `worktree.rs` | Entrega MaxCode, espelho de ciclo e resolução Git |
| `ports.rs`, `events.rs` | Dependências injetadas e `GithubEventSink` |

### 3.1. Modelo de domínio

DTOs usam `Serialize`, `Deserialize` quando forem entrada e `specta::Type`, com campos `camelCase`.
Enums de estado usam `snake_case`. Datas IPC são epoch ms (`i64` seguro em JS); números de issue positivos
cabem em inteiro seguro; IDs opacos de GitHub, repo e ciclo são strings. `RepoId`, `AuthScopeId` e `RequestId`
são newtypes locais UUID, sem redefinir `ProjectId`, `TabId`, `TemplateId` ou `AgentKind`.

```rust
pub struct GithubRepoRef {
    pub id: RepoId,
    pub host: String,
    pub owner: String,
    pub name: String,
    pub html_url: String,
}
pub struct RepoContext {
    pub project_id: ProjectId,
    pub repo: GithubRepoRef,
    pub auth_scope: AuthScopeId,
    pub generation: u32,
}
pub enum IssueState { Open, Closed }
pub enum LabelKind { State, Closure, Category }
pub struct LabelDefinition {
    pub name: String,
    pub kind: LabelKind,
    pub color: Option<String>,
    pub description: Option<String>,
}
pub struct LabelInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
    pub kind: Option<LabelKind>,
}
pub struct IssueSummary {
    pub number: u32,
    pub node_id: String,
    pub title: String,
    pub state: IssueState,
    pub author_login: Option<String>,
    pub labels: Vec<LabelInfo>,
    pub comment_count: u32,
    pub updated_at_ms: i64,
    pub html_url: String,
}
pub struct IssueComment {
    pub id: String,
    pub author_login: Option<String>,
    pub body: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub html_url: String,
}
pub struct IssueDetail {
    pub summary: IssueSummary,
    pub body: String,
    pub comments: Vec<IssueComment>,
    pub comments_complete: bool,
    pub comments_cursor: Option<String>,
    pub plan: Option<IssuePlan>,
}
pub struct IssuePlan {
    pub source_comment_id: Option<String>,
    pub source_updated_at_ms: i64,
    pub markdown: String,
    pub start_line: u32,
    pub end_line: u32,
    pub provisional: bool,
}
pub struct CacheMeta {
    pub revision: u32,
    pub checked_at_ms: Option<i64>,
    pub stale: bool,
    pub complete: bool,
}
pub struct Cached<T> { pub value: T, pub meta: CacheMeta }
pub struct OpenIssueCount {
    pub count: Option<u32>,
    pub checked_at_ms: Option<i64>,
    pub stale: bool,
}
```

`IssueListQuery { state: Option<IssueState>, labels: Vec<String>, text: String, cursor: Option<String>,
limit: u16 }` tem limite 1..200. Cursor é opaco, vinculado a repo/scope/filtros/revisão, não uma URL livre.
`IssuePage { items, nextCursor, count: OpenIssueCount }`; total do badge não é tamanho desta página.
`IssueCreateRequest { projectId, requestId, title, body, labels }`; título não vazio até 256 caracteres,
corpo até 64 KiB, no máximo 100 labels; validar limites reais e erros remotos sem truncar submissão silenciosamente.
`LabelPatchRequest { projectId, number, requestId, expectedUpdatedAtMs, add, remove }` rejeita a mesma label
em ambos os conjuntos e retorna `LabelPatchResult { issue, applied, failed, outcome }`.

### 3.2. Serviços

`GithubClient` expõe `repo`, `list_issues_page`, `issue`, `comments_page`, `labels_page`, `create_issue`,
`add_issue_labels`, `remove_issue_label` e `create_label`, todos assíncronos com contexto de credencial explícito.
Retorno de leitura: `FetchResult<T>::Modified { value, etag, next, rate } | NotModified { rate }`.
`GithubCacheRepo` materializa dados já sanitizados; `IssueSyncService` controla transações de sincronização;
`GithubService` valida o projeto, scope e geração antes de toda operação, inclusive após awaits.

`GithubError` mapeia `not_a_github_repo`, `auth_required`, `forbidden`, `not_found`, `issues_disabled`,
`rate_limited`, `transport`, `timeout`, `invalid_response`, `validation`, `catalog_invalid`,
`bridge_unsupported`, `delivery_unknown`, `mutation_unknown`, `conflict`, `stale_request`,
`worktree_ambiguous`, `worktree_foreign` e `keyring_unavailable` para `MtError` com prefixo `github.`.
Detalhes são limitados e sanitizados; 404 remoto não implica automaticamente remoção local.

## 4. GitHub: detecção, transporte, cache HTTP e sincronização

### 4.1. Detecção e autenticação

- Resolver projeto pela Fase 06 e descoberta Git da 08, inclusive `.git` como arquivo em worktree.
  Remote `origin` SSH scp-like, `ssh://` ou HTTPS com `github.com` normalizado; remover somente sufixo `.git`
  e barra final. Rejeitar host parecido, query, fragmento, segmentos extras, controle, path traversal e porta
  incompatível. Usuário no remote não vira credencial HTTP nem é persistido. Sem origin GitHub: `None`.
- Reconsultar ao mudar projeto/path/remote; uma geração invalida respostas antigas. Projetos diferentes do
  mesmo repo/scope compartilham rede/cache; remoção de um vínculo não apaga cache usado por outro projeto.
- `TokenProvider`: gh autenticado primeiro, keyring como alternativa selecionada; não mudar conta no meio
  de uma operação. `gh auth token --hostname github.com` é capturado somente em memória, quando necessário
  ao transporte REST. Modo público anônimo é somente leitura. Identidade desconhecida não reutiliza cache privado.
- Token salvo via IPC vai ao keyring, campo de formulário é apagado após submit. Settings guardam só referência
  opaca/fonte. Token não entra em argv, URL, eventos, tracing ou SQLite. Se passado ao gh, usar `GH_TOKEN` apenas
  no ambiente filho e desativar prompts; falha no keyring não cai para arquivo texto.

### 4.2. APIs e equivalência dos transportes

Base REST fixa `https://api.github.com`; `Accept: application/vnd.github+json`, `User-Agent: MaxTerminal` e
`X-GitHub-Api-Version` fixado em configuração de build com fixtures da versão suportada. A documentação
consultada apresenta `2026-03-10`; confirmar a versão escolhida nos testes de contrato, sem atualizar headers
automaticamente em produção. Nunca anexar Authorization a host vindo de conteúdo de issue.

| Operação | Endpoint REST |
|----------|---------------|
| Identificar repo/permissões | `GET /repos/{owner}/{repo}` |
| Lista completa/incremental | `GET /repos/{owner}/{repo}/issues?state=all&sort=updated&direction=asc&per_page=100[&since=…]` |
| Uma issue | `GET /repos/{owner}/{repo}/issues/{number}` |
| Comentários | `GET /repos/{owner}/{repo}/issues/{number}/comments?per_page=100` |
| Labels do repo | `GET /repos/{owner}/{repo}/labels?per_page=100` |
| Criar issue | `POST /repos/{owner}/{repo}/issues` com `{title, body, labels}` |
| Adicionar labels | `POST /repos/{owner}/{repo}/issues/{number}/labels` com `{labels}` |
| Remover uma label | `DELETE /repos/{owner}/{repo}/issues/{number}/labels/{name_encoded}` |
| Criar label faltante | `POST /repos/{owner}/{repo}/labels` com metadados validados |

`GhCliClient` invoca `gh api --hostname github.com --method GET --include <endpoint>` por página; para escrita,
`--method POST --input -` recebe JSON por stdin. Args incluem Accept/versão, nunca token. Usar corpo em stdin
evita interpretação de `@arquivo` por `--field`. Não assumir `--json` em `gh issue create/edit`.
`--include` permite ler status, `Link`, ETag e rate limit; parser tolera CRLF e ausência de corpo no 304.
Não combinar paginação interna de gh com a do serviço; não usar `--cache` (cache de terceiro não sanitizado).
Timeout de 30 s por chamada, saída limitada por página (8 MiB), `kill_on_drop` mais kill/wait explícitos no
cancelamento, stderr sanitizado até 2 KiB. Testar fixture de gh compatível, inclusive códigos de erro.

Fallback de leitura: executável ausente, indisponibilidade de transporte ou resposta impossível de parsear
permitem REST na mesma identidade. 401/403 por credencial requer autenticação/fonte válida, 404/410/422 são
semânticos, 403 rate limit/429 pausam ambos os transportes. POST enviado cujo resultado se perdeu não é
repetido automaticamente; REST e gh atingem o mesmo servidor e não contornam limites.

### 4.3. Paginação, ETag e rate limit

- Seguir `Link rel=next` até sua ausência, `per_page ≤ 100`, inclusive comentários e labels. Validar HTTPS,
  host `api.github.com` e caminho do recurso esperado antes de seguir links; detectar ciclo de páginas.
  Recurso com `pull_request` é descartado tanto na lista quanto no detalhe; nunca somar PR ao badge.
- Cache HTTP por `(auth_scope, método, URL com query normalizada, Accept, versão da API, versão do sanitizer)`.
  ETag pertence a **uma página**, não à coleção. Persistir só JSON projetado e sanitizado, mais ETag e Link
  validado; não gravar response bruto/headers de autenticação. 304 sem payload local força GET incondicional.
- Revalidar cada página relevante. 304 na página 1 não prova que páginas seguintes não mudaram; usar Link
  guardado para continuá-las e validar cada resposta. Atualizar `checked_at` sem incrementar revisão se os
  dados não mudaram. Nova versão do sanitizer invalida corpos/ETags vinculados à versão anterior.
- `RateLimitState { resource, limit, remaining, resetAtMs, retryAtMs, secondary }` usa headers, não cotas fixas.
  `Retry-After` prevalece; sem ele, reset primário ou backoff exponencial com jitter para secundário (início
  de 60 s, teto 15 min). Não interpretar todo 403 como rate limit. Pausa compartilhada por conta/host/recurso,
  inclusive gh, REST, refresh manual e projetos em segundo plano; manter leitura local.
- Duas sincronizações globais simultâneas; uma por recurso/repo/scope, projeto ativo prioritário. Lista TTL
  120 s, detalhe 300 s, projetos inativos no mínimo 10 min conforme orçamento. Refresh manual não ignora backoff.
  Coalescência compartilha uma operação paginada; cancelar um consumidor não mata trabalho ainda necessário.

### 4.4. Reconciliação e contagem correta

1. Primeira sincronização percorre `state=all` sem filtros da UI; reúne páginas em área de staging vinculada
   à geração. Deduplicar por `node_id`/número e aceitar a versão de maior `updated_at`, nunca retroceder.
2. Publicar upserts, joins, `count(open)`, revisão e watermark em uma transação **somente após todas as páginas**.
   Em erro intermediário, manter snapshot completo anterior; UI exibe atraso, não um total parcial exato.
3. Incremental usa `since = último watermark - janela de sobreposição (60 s)`, `state=all` para capturar
   fechamentos/reaberturas. Watermark avança só após sucesso completo, usando timestamp de início de sync
   referenciado ao relógio do servidor quando disponível. Manter reconciliação completa periódica (24 h) e
   acionável, pois remoções/transferências não aparecem necessariamente no incremental.
4. Uma lista paginada GitHub não é snapshot atômico. Depois de varredura completa, executar catch-up desde
   seu início com sobreposição; ausências geram candidatos a verificar individualmente. Nunca apagar por
   ausência em página, filtro ou resposta 404 de repo (pode ser perda de permissão). Transferência/410 confirmada
   marca indisponível e reconcilia o vínculo; 404 ambíguo preserva cache privado restrito e sinaliza acesso.
5. Excluir PRs **antes** de contar. `open_issues_count` do repo não serve (pode incluir PRs), nem lista limitada
   a 1000 ou Search incompleto. Contador usa snapshot reconciliado, sem filtros locais; `None` = desconhecido,
   `Some(0)` = zero confirmado. Resultado é consistente com a última sincronização, não promessa de tempo real.
6. Troca de conta cancela geração, limpa estado da store e torna partição antiga inacessível. Perda de acesso
   impede servir cache privado como autenticado; purge por política de retenção, com ação explícita de limpeza.

## 5. Comandos IPC

Todos assíncronos, registrados em `src-tauri/src/commands/github.rs`, janela `main`, retorno `Result<T, MtError>`.
Tipos são exportados via Specta para `src/types/bindings.ts`; wrappers ficam em `src/ipc/github.ts`.
Projeto é entrada de autorização/resolução; frontend não fornece URL, executável ou caminho arbitrário de rede.

| Comando | Entrada | Saída / efeito |
|---------|---------|----------------|
| `github_repo_resolve` | `projectId` | `RepoContext?`; não GitHub é `null`, erro de descoberta é erro |
| `github_auth_status` | — | `AuthStatus { source, accountLogin?, available, scope }`, sem segredo |
| `github_auth_set_token` | `token` | `AuthStatus`; valida, salva keyring e rotaciona scope |
| `github_auth_clear_token` | — | `AuthStatus`; remove segredo local, não revoga token no GitHub |
| `github_issues_list` | `projectId, query: IssueListQuery` | `Cached<IssuePage>` local; inicia sync se vencido |
| `github_issue_detail` | `projectId, number, commentsCursor?` | `Cached<IssueDetail>`; comentários paginados |
| `github_issue_counts` | `projectIds` (máx. 200) | contagens cacheadas por projeto, sem GET por item |
| `github_refresh` | `projectId, target: list\|detail\|labels, number?` | `SyncTicket { requestId, coalesced }` |
| `github_labels_catalog` | — | `LabelCatalog { version, hash, status, labels, reason? }` |
| `github_repo_labels` | `projectId, cursor?` | labels remotas paginadas e completude |
| `github_labels_ensure` | `projectId, requestId, missingMetadata, confirmed` | `LabelEnsureResult { created, existing, failed }` |
| `github_issue_create` | `IssueCreateRequest` | `MutationReceipt<IssueDetail>` |
| `github_issue_labels_update` | `LabelPatchRequest` | `MutationReceipt<LabelPatchResult>` |
| `github_issue_send_maxcode` | `projectId, number, action: open\|plan\|fix, requestId` | `DeliveryReceipt` (§8) |
| `github_issue_worktree_resolve` | `projectId, number` | `IssueWorktreeResolution` (§9), sem criação |
| `github_issue_worktree_create` | `proposalId, confirmed` | `ResolvedIssueWorkspace`, proposta revalidada |
| `github_issue_open_tab` | `projectId, number, workspaceId, target: shell\|agent, templateId?, agent?` | `TabView` ou resultado agente normalizado com `tabId` |
| `github_bug_cycle_preview` | `projectId` | `BugCyclePreview` autoritativo MaxCode, sem iniciar |
| `github_bug_cycle_start` | `projectId, previewId, requestId, confirmed` | `BugCycleProgress`, após ACK ou estado `unknown` |
| `github_bug_cycle_status` | `projectId, cycleId?` | espelho reconciliado por `requestId`/`cycleId`, sem iniciar |

`MutationReceipt<T> { requestId, outcome: confirmed|partial|unknown, value?: T }` é distinto de erro antes do
envio. Estado desconhecido persiste para reconciliação, e não apresenta botão de retry cego. Mesmo requestId
com payload diferente retorna conflito. `confirmed` não substitui validação backend de proposta/permissão.

## 6. Eventos, revisões e ciclo de vida

`TauriGithubEventSink` em `src-tauri/src/events/github.rs` implementa `GithubEventSink`. Emitir depois do commit
local. Envelope de dados: `{ projectId, repoId, authScope, generation, revision, requestId?, payload }`.
Emitir por projeto vinculado, com a geração específica daquele vínculo, mesmo quando o cache é compartilhado.
Revisão monotônica pertence à partição; geração pertence ao vínculo ativo do projeto. Contadores `u32` não
reiniciam dentro de uma partição ativa. Eventos de autenticação globais usam scope anterior/novo explícitos.

| Evento | Payload | Regra |
|--------|---------|-------|
| `github://repo-changed` | `projectId, context?` | mudança de vínculo/remote; incrementa geração |
| `github://issues-updated` | `changedNumbers, removedNumbers, count, complete` | até 200 números; acima disso `invalidate: true` |
| `github://issue-updated` | `number` | invalida detalhe/comentários daquele número |
| `github://labels-updated` | `catalogStatus, repoLabelsChanged` | invalidar editor/filtros |
| `github://sync-state` | `status: syncing\|idle\|stale\|error, target, error?` | inclui ticket, erro sanitizado |
| `github://rate-limit` | `RateLimitState` | UI mostra próxima tentativa, sem loop de refresh |
| `github://bug-cycle-progress` | `BugCycleProgress` | revisão MaxCode separada da revisão do cache |
| `github://auth-changed` | `oldScope, newScope, status` | esvaziar seleções/detalhes privados |

Assinar uma vez no composition root. Store descarta scope/geração divergente e revisão antiga. Gap de revisão
invalida consultas e busca snapshot IPC; eventos são notificações, não fila durável. Registrar listeners antes
da carga inicial e reconciliar com revisão do snapshot; remover listeners no dispose/HMR. `project://removed`
retira vínculo/provider, cancela consumidores e não fecha ciclo remoto. Fechar a aba issues não existe; mudar
para terminal pausa apenas detalhamento prioritário, mantendo atualização orçamentada do badge.

## 7. SQLite, índices, transações e retenção

Migration futura `mt-db/migrations/0010_16_github.sql` usa a versão 10 fixa da sequência D-01. DDL abaixo assume
`projects(id TEXT PRIMARY KEY)` da Fase 06. WAL/FK/busy_timeout vêm da conexão da Fase 03; nada de conexão
concorrente de escrita criada por `mt-github`. `auth_scope` nunca contém segredo.

```sql
CREATE TABLE github_repos (
    id TEXT PRIMARY KEY,
    host TEXT NOT NULL CHECK (host = 'github.com'),
    owner TEXT NOT NULL COLLATE NOCASE,
    name TEXT NOT NULL COLLATE NOCASE,
    html_url TEXT NOT NULL,
    UNIQUE (host, owner, name)
);
CREATE TABLE github_project_repos (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    repo_id TEXT NOT NULL REFERENCES github_repos(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL DEFAULT 1 CHECK (generation > 0)
);
CREATE INDEX idx_github_project_repo ON github_project_repos(repo_id);
CREATE TABLE github_sync_state (
    repo_id TEXT NOT NULL REFERENCES github_repos(id) ON DELETE CASCADE,
    auth_scope TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    checked_at_ms INTEGER,
    full_sync_at_ms INTEGER,
    watermark_ms INTEGER,
    open_count INTEGER CHECK (open_count IS NULL OR open_count >= 0),
    complete INTEGER NOT NULL DEFAULT 0 CHECK (complete IN (0, 1)),
    PRIMARY KEY (repo_id, auth_scope)
);
CREATE TABLE github_issues (
    repo_id TEXT NOT NULL,
    auth_scope TEXT NOT NULL,
    number INTEGER NOT NULL CHECK (number > 0),
    node_id TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT,
    state TEXT NOT NULL CHECK (state IN ('open', 'closed')),
    author_login TEXT,
    comment_count INTEGER NOT NULL DEFAULT 0,
    html_url TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    detail_checked_at_ms INTEGER,
    unavailable INTEGER NOT NULL DEFAULT 0 CHECK (unavailable IN (0, 1)),
    PRIMARY KEY (repo_id, auth_scope, number),
    UNIQUE (repo_id, auth_scope, node_id),
    FOREIGN KEY (repo_id, auth_scope) REFERENCES github_sync_state(repo_id, auth_scope) ON DELETE CASCADE
);
CREATE INDEX idx_github_issues_state_updated ON github_issues(repo_id, auth_scope, state, updated_at_ms DESC, number DESC);
CREATE INDEX idx_github_issues_updated ON github_issues(repo_id, auth_scope, updated_at_ms DESC, number DESC);
CREATE TABLE github_issue_comments (
    repo_id TEXT NOT NULL,
    auth_scope TEXT NOT NULL,
    issue_number INTEGER NOT NULL,
    id TEXT NOT NULL,
    body TEXT NOT NULL,
    author_login TEXT,
    html_url TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (repo_id, auth_scope, issue_number, id),
    FOREIGN KEY (repo_id, auth_scope, issue_number) REFERENCES github_issues(repo_id, auth_scope, number) ON DELETE CASCADE
);
CREATE INDEX idx_github_comments_order ON github_issue_comments(repo_id, auth_scope, issue_number, created_at_ms, id);
CREATE TABLE github_labels (
    repo_id TEXT NOT NULL,
    auth_scope TEXT NOT NULL,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL,
    color TEXT NOT NULL,
    description TEXT,
    PRIMARY KEY (repo_id, auth_scope, id),
    UNIQUE (repo_id, auth_scope, normalized_name),
    FOREIGN KEY (repo_id, auth_scope) REFERENCES github_sync_state(repo_id, auth_scope) ON DELETE CASCADE
);
CREATE TABLE github_issue_labels (
    repo_id TEXT NOT NULL,
    auth_scope TEXT NOT NULL,
    issue_number INTEGER NOT NULL,
    label_id TEXT NOT NULL,
    PRIMARY KEY (repo_id, auth_scope, issue_number, label_id),
    FOREIGN KEY (repo_id, auth_scope, issue_number) REFERENCES github_issues(repo_id, auth_scope, number) ON DELETE CASCADE,
    FOREIGN KEY (repo_id, auth_scope, label_id) REFERENCES github_labels(repo_id, auth_scope, id) ON DELETE CASCADE
);
CREATE INDEX idx_github_issue_labels_filter ON github_issue_labels(repo_id, auth_scope, label_id, issue_number);
CREATE TABLE github_http_cache (
    repo_id TEXT NOT NULL,
    auth_scope TEXT NOT NULL,
    request_key TEXT NOT NULL,
    etag TEXT,
    next_url TEXT,
    sanitized_json TEXT NOT NULL,
    sanitizer_version INTEGER NOT NULL,
    checked_at_ms INTEGER NOT NULL,
    PRIMARY KEY (repo_id, auth_scope, request_key),
    FOREIGN KEY (repo_id, auth_scope) REFERENCES github_sync_state(repo_id, auth_scope) ON DELETE CASCADE
);
CREATE INDEX idx_github_http_expiry ON github_http_cache(checked_at_ms);
CREATE TABLE github_bug_cycles (
    request_id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL REFERENCES github_repos(id) ON DELETE CASCADE,
    origin_project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    peer_id TEXT NOT NULL,
    cycle_id TEXT,
    catalog_hash TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'accepted', 'running', 'unknown', 'completed', 'failed', 'cancelled')),
    peer_revision INTEGER NOT NULL DEFAULT 0,
    sanitized_progress_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE (repo_id, peer_id, cycle_id)
);
CREATE UNIQUE INDEX idx_github_one_active_cycle ON github_bug_cycles(repo_id)
    WHERE status IN ('pending', 'accepted', 'running', 'unknown');
CREATE TABLE github_mutations (
    request_id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL REFERENCES github_repos(id) ON DELETE CASCADE,
    auth_scope TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload_digest TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'confirmed', 'partial', 'unknown', 'failed')),
    result_json TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
CREATE INDEX idx_github_mutations_pending ON github_mutations(repo_id, auth_scope, status);
```

- Digest de mutação é HMAC do payload original com chave efêmera da sessão; não armazenar texto de envio
  nem digest simples de segredo. Após reinício, não usar digest irrecuperável para reenviar: registros incertos
  continuam reconciliáveis por requestId/resultado, e repetição de conteúdo requer decisão explícita.
- Sanitizar título, corpo, comentários, descrições, resultados/progresso e JSON antes de persistir. IDs/nomes
  usados como identidade são validados estritamente; se sanitizer alteraria identidade, rejeitar esse dado
  em vez de gravar chaves divergentes. Conteúdo original da criação permanece só em memória até o envio.
- Upsert de lista não apaga corpo detalhado com `NULL` nem sobrepõe versão mais recente do detalhe. Atualizar
  issue e joins de labels na mesma transação; substituir relação local somente por resposta remota completa.
- Busca por texto usa filtros parametrizados, escape de LIKE e paginação estável `(updated_at, number)`;
  sem construir SQL com nomes de labels. Fase 17 recebe repos/índices e DTOs para adicionar FTS, sem dependência reversa.
- `RetentionRegistry`: expirar HTTP cache após 24 h sem uso, detalhes de fechadas após 30 dias, partições
  antigas e órfãos conforme preferência; preservar issues abertas e ciclos/mutações incertos. Cache removido
  invalida completude/contagem, exigindo nova sincronização antes de afirmar total exato.
- Remover projeto só apaga seu vínculo. Ciclo remoto segue; `origin_project_id` vira NULL. Não remover registro
  de repo com ciclo ativo nem inferir que apagar cache cancela trabalho do MaxCode.

## 8. MaxCode: ações e ciclo em lote delegado

### 8.1. Gate de protocolo

A sub-implementação **11/06** é proprietária do protocolo D-03: `issue.command`, `bugs.cycle.preview`,
`bugs.cycle.start`, `bugs.cycle.status`, evento `bugs.cycle.progress`, capabilities `issues_protocol_v1` e
`bugs_cycle_v1` e fallback URI `vscode://johnattas.maxcode/issue?action=<open|plan|fix>&repo=…&number=…`.
A Fase 16 somente consome `MaxCodeClient::{send_issue_command, bugs_cycle_preview, bugs_cycle_start,
bugs_cycle_status}` por um adaptador de `MaxCodePort`, sem editar `mt-bridge` nem duplicar decoder/dispatch.
O contrato continua proposta a confirmar com a extensão; fixtures locais não comprovam suporte remoto.

`MaxCodePort::issue_capabilities` traduz o handshake: `issues_protocol_v1` habilita ações individuais;
`bugs_cycle_v1` habilita ciclo. `Unknown`/ausência desabilita a ação com motivo em pt-BR, nunca simula suporte
ou degrada `fix` para conversa Plan. Catálogo compatível e deduplicação durável continuam gates adicionais.
O adaptador converte o DTO de domínio abaixo para o DTO de transporte da 11/06: `command → action`,
`request_id → correlationId` estável, `repo → owner/name`, `issue_number → issueNumber`,
`project_path → projectPath` e worktree validada opcional. Não serializar diretamente o DTO de domínio como wire.

```rust
pub enum MaxCodeIssueCommand { Open, Plan, Fix }
pub struct IssueCommandPayload {
    pub schema_version: u16,
    pub request_id: RequestId,
    pub project_path: String,
    pub worktree: Option<String>,
    pub repo: GithubRepoRef,
    pub issue_number: u32,
    pub command: MaxCodeIssueCommand,
    pub catalog_hash: String,
}
pub enum DeliveryState { Pending, Accepted, Rejected, Unknown }
pub struct DeliveryReceipt {
    pub request_id: RequestId,
    pub state: DeliveryState,
    pub peer_operation_id: Option<String>,
}
pub struct BugCycleProgress {
    pub request_id: RequestId,
    pub cycle_id: Option<String>,
    pub status: BugCycleStatus,
    pub peer_revision: u32,
    pub total: Option<u32>,
    pub stages: Vec<BugStageCount>,
    pub updated_at_ms: i64,
    pub observation_stale: bool,
}
```

`BugStageCount { stage: String, count: u32 }` usa identificadores de etapas fornecidos pelo MaxCode, separados
dos nomes de labels. Não reconstruir enum de etapas pela ordem das 24 labels. `BugCycleStatus` corresponde
aos estados de acompanhamento no SQL, sem confundir `accepted` com `completed`.

### 8.2. Ações individuais

| UI | Comando do contrato novo | Responsabilidade MaxCode |
|----|--------------------------|--------------------------|
| Abrir | `open` | Abrir issue/contexto correspondente na extensão |
| Analisar (`BUG PLAN`) | `plan` | Acionar handler de planejamento de bugs da issue |
| Corrigir (`BUG FIX`) | `fix` | Acionar handler de correção da issue, com workflow próprio |

Usar `requestId` estável entre WS, reconexão e URI. Repo/path vêm do backend validado; enviar identidade
da issue, não corpo integral nem prompt construído com conteúdo não confiável. ACK deve correlacionar envelope
e requestId e informar accepted/rejected; abertura de janela pelo SO não é ACK de ação.
URI de issues proposta (`vscode://johnattas.maxcode/issue?...`) só é habilitada após handler bilateral validado,
com percent-encoding, payload mínimo sem token e mesma identidade de entrega. Quando URI apenas acordar,
aguardar handshake e então transmitir pela bridge. Se a primeira entrega pode ter sido aceita, consultar
requestId; nunca disparar automaticamente uma segunda ação com outro ID.

### 8.3. Corrigir todos os Bugs

1. `preview(project)` consulta MaxCode, que seleciona candidatos, deduplica e retorna `previewId`, versão,
   validade, repo, contagens por etapa e critério de seleção. Não presumir uma label genérica chamada `bug`.
   UI mostra quantidade e destino; preview expirado ou repo divergente exige nova prévia.
2. Confirmação → inserir `pending` com requestId único → transmitir `bugs.cycle.start` uma vez com previewId.
   MaxCode faz dedup durável, cria ou recupera um ciclo e responde cycleId/estado. Índice parcial local evita
   dois cliques/projetos iniciarem ciclos concorrentes, mas autoridade de exclusão entre clientes é o MaxCode.
3. MaxCode executa **a única fila**: deduplicação → planejamento → execução → verificação. MaxTerminal não
   chama `BUG PLAN`/`BUG FIX` por item, não troca labels para avançar e não cria agentes paralelos do lote.
4. `bugs.cycle.status` aceita cycleId ou requestId, retorna snapshot/revisão; evento de progresso acelera UI.
   Queda da bridge marca `unknown/stale`, preserva ciclo ativo e tenta reconexão com backoff, sem novo start.
5. Reinício carrega `pending/accepted/running/unknown` e consulta o mesmo peer/requestId. Se peer não reconhece
   o pedido, exige reconciliação explícita; não há garantia de exactly-once sem dedup durável do receptor.
6. Labels da lista são exibição auxiliar. Nenhuma sequência de labels ou ausência de candidatos comprova
   sucesso do ciclo. Somente snapshot terminal confirmado permite `completed/failed/cancelled` e notificação.
7. Fechar app, remover projeto ou trocar conta GitHub não cancela o ciclo do MaxCode. Cancelamento remoto
   não faz parte desta fase; não apresentar botão que apenas deixa de observar como se cancelasse execução.

## 9. Worktree/branch da issue e abertura de aba

`IssueWorktreeResolution` é união `existing { workspace } | branch_only { proposal } |
missing { proposal } | ambiguous { candidates } | incompatible { reason }`.
`ResolvedIssueWorkspace { workspaceId, repoId, number, path, branch, commonDir, source }` fica registrado
no backend; IDs de resolução/proposta expiram em 5 min e são revalidados antes de usar.

Ordem: mapeamento explícito retornado pelo MaxCode compatível → worktree registrada pelo Git vinculada à
branch da issue → convenção exportada `wt-implement-issue-<n>`. A convenção é pista, não prova de propriedade;
validar `common_dir`, branch e destino canônico. Homônimo em outro repo, symlink externo ou diretório sem
registro Git não é adotado. Múltiplos candidatos exigem escolha; nunca checkout forçado na working tree atual.

- Worktree existente: validar, abrir aba direto. Branch já em uso numa worktree registrada usa essa worktree.
- Apenas branch local: proposta confirmada chama `WorktreeCreateRequest` com `new_branch: None`,
  `base: Some(branch)` e destino livre; não criar outra branch com mesmo nome.
- Sem branch: proposta com nome/base/destino explícitos, `location: MaxCode`, `new_branch: Some(branch)`,
  `base: Some(base_ref)`. Confirmar base válida, usar Fase 08, lidar com corrida `worktree_exists` relendo Git.
- Antes da criação, verificar diretório ignorado e informar ajuste de `.gitignore` no plano quando necessário;
  criação autorizada usa a política `ensure_gitignore` da Fase 08. Nenhuma remoção/merge automático nesta fase.
- Terminal: `tab_open` com template escolhido, `overrides.cwd = path`, `cwd_fallback_to_project = false`,
  título `Issue #<n>`. Comando inicial é o do template aprovado, nunca texto da issue.
- Agente: `AgentPort` delega a `AppAgentState::open_agent_tab` com `isolation: Existing { path }`; telemetria, PTY e teardown ficam na Fase 13. O handler IPC `agent_tab_open` não é chamado internamente.
  A abertura não inicia `BUG FIX` implicitamente. No erro de spawn, informar worktree criada e preservá-la;
  não apagar diretório que MaxCode pode ter começado a usar. Guardar vínculo issue↔aba em memória recuperável.

## 10. Frontend Vue/Pinia, componentes e fluxos

### 10.1. Store e composição

`useGithubIssuesStore` em `src/stores/githubIssues.ts` é única entrada frontend para consultas de issues.
Sem fetch direto nos componentes, token persistido na Pinia ou poller paralelo ao backend. Estado tipado:

```ts
import type { Cached, IssueDetail, IssueSummary, OpenIssueCount, RepoContext } from '@/types/bindings';

type IssueKey = `${string}:${string}:${number}`;
type WorkspaceSelection = { kind: 'issues' } | { kind: 'terminal'; tabId: string };

// Estrutura interna ilustrativa; DTOs concretos Cached_* vêm do gerador Specta.
interface GithubIssuesState {
    contexts: Record<string, RepoContext>;
    summaries: Record<IssueKey, IssueSummary>;
    details: Record<IssueKey, Cached<IssueDetail>>;
    counts: Record<string, OpenIssueCount>;
    selectedIssueByProject: Record<string, number | null>;
    workspaceSelectionByProject: Record<string, WorkspaceSelection>;
    loadingByKey: Record<string, boolean>;
    errorByKey: Record<string, string | null>;
}
```

Na implementação, usar IDs importados `ProjectId`/`TabId` e aliases reais gerados para genéricos, sem declarar
`IssueSummary`/`LabelKind` manualmente. Complementar estado com páginas/cursors, catálogo, filtros por projeto,
receipt de mutações, ciclo ativo e revisões por scope. Actions: `resolveProject`, `loadList`, `loadDetail`,
`refresh`, `createIssue`, `patchLabels`, `sendToMaxCode`, `openIssueTab`, `previewCycle`, `startCycle`,
`reconcileCycle`, `applyEvent` e `disposeProject`.

Não aplicar resposta tardia de seleção A ao detalhe B. Scope/geração/request token são checados antes de
mutar store; coalescer Promises por chave e limpar `finally`. Erro conserva conteúdo anterior com indicador
de idade. Criar/editar só confirma após resultado remoto; invalidar cache HTTP relacionado e refrescar resumo,
detalhe e badge. Novo item aparece conforme ordenação/filtros (ou ação “Ver issue criada”), não quebra ordenação.

### 10.2. Aba virtual e badge

Compor botão `Issues` no `TabBar #pinned` antes das abas normais; renderizar painel conforme
`workspaceSelectionByProject`. Selecionar issues preserva `activeTabId` do terminal em segundo plano; ao voltar,
restaurar foco/painel. Nenhum fake TabView, `attachTab`, registro de sessão ou contador running/idle.
Badge usa `count(projectId)` de F06: null para não GitHub/desconhecido, zero oculto, positivo renderizado.
Tooltip pt-BR inclui estado de atraso pelo componente envolvente; erro é sinalizado sem exibir zero falso.
Roxo deve vir de token semântico realmente disponível; adapter de tema escolhe token verificado.

### 10.3. Composables e componentes

| Unidade | Responsabilidade / contrato |
|---------|----------------------------|
| `useGithubIssueEvents` | Uma assinatura global, revisão/geração, cleanup e resync em gaps |
| `useIssueFilters` | Estado open/closed, labels de workflow/categoria e texto; AND entre dimensões; comparação pelo catálogo |
| `useIssuePlan` | Consumir `IssuePlan` extraído em Rust e criar âncora; não manter segunda regra divergente |
| `useIssueBadgeProvider` | Registro/desregistro do provider F06; total independente do filtro |
| `useMarkdownRenderer` | `markdown-it` html=false + DOMPurify; URLs seguras, sem execução |
| `IssuesTab.vue` | `projectId`; coordena lista/detalhe e loading, vazio, offline, autenticação, rate limit |
| `IssueList.vue`, `IssueListItem.vue` | `items`, `selectedNumber`; emit `select(number)`, `load-more`; virtualizar listas extensas |
| `IssueFilters.vue` | `modelValue`; emit `update:modelValue`; opções do catálogo/labels remotas |
| `IssueDetail.vue`, `IssuePlanSection.vue` | `detail`; descrição completa, plano e origem; âncora “Ir para o plano” |
| `IssueComments.vue` | comentários com autor/data; carregar mais; informar plano provisório se incompleto |
| `IssueLabelEditor.vue`, `IssueLabelChip.vue` | Labels canônicas e externas; produzir diff add/remove, nunca substituir tudo |
| `IssueCreateModal.vue` | título/corpo/labels; requestId estável; submit único e tratamento de resultado incerto |
| `IssueActionsMenu.vue` | três ações MaxCode e abrir aba; disabled por capacidade/permissão com motivo |
| `IssueOpenTabModal.vue` | seleção shell/agente/template e resolução de worktree; confirmação se criar |
| `BugCycleConfirmModal.vue`, `BugCycleProgress.vue` | prévia MaxCode, confirmação, progresso confirmado e observações atrasadas |
| `GithubAuthPanel.vue` | fonte/conta, token temporário, limpeza; não mostra segredo retornado pelo backend |
| `LabelSyncBanner.vue` | faltantes/gate inválido; criação explícita e feedback parcial |

SFC template/script setup TS/style SCSS scoped, 4 espaços, tags em linha única, textos/aria-label pt-BR,
MaxComponentsUi e UnoCSS conforme AGENTS.md. Lista/detalhe navegáveis por teclado, roles tab/tablist/tabpanel,
aria-selected/controls, foco restaurado ao fechar modal, região de status sem anunciar cada refresh.

### 10.4. Markdown e plano

Parser Rust de Markdown por AST/eventos (dependência a escolher e fixar, por exemplo `pulldown-cmark`) identifica
heading H2 cujo texto seja exatamente `PLANO DE IMPLEMENTAÇÃO`; ignora código cercado/indentado, HTML e
citações. Seção termina no próximo heading de nível ≤ 2 ou fim do documento. Em múltiplas ocorrências no
mesmo corpo, usar a última; entre corpo e comentários, escolher maior `(updated_at, created_at, source_id)`
com regra estável documentada. Carregar todas as páginas antes de afirmar “último plano”; enquanto incompleto,
mostrar `provisional: true`. Comentários editados/removidos invalidam extração; ausência não gera plano fictício.
Reconciliar remoções de comentários somente após percorrer completamente a coleção com acesso válido;
falha em página intermediária preserva comentários e plano anteriores como atrasados.

Frontend sanitiza mesmo Markdown vindo do cache. Bloquear script, event handlers, `javascript:`, `data:`,
`file:`, links de comando/URI de execução e HTML embutido; links http/https validados abrem via serviço do SO,
com `noopener`. Imagens remotas bloqueadas por padrão ou carregadas por ação explícita, evitando tracking;
nenhum token enviado em assets. API/body não fornece CSS ou HTML confiável ao `v-html`.

## 11. Escritas GitHub e concorrência

Criar issue: validar → persistir intenção sem conteúdo secreto → uma requisição → confirmar `201`/número →
sanitizar/upsert → invalidar lista/contagem → emitir evento. GitHub não oferece idempotência geral para este POST;
perda de resposta vira `mutation_unknown`. Consultar issues recentes pode sugerir candidatos, mas título/corpo
iguais não provam identidade: usuário reconcilia antes de reenviar. Não adicionar marcador secreto ao texto.
Verificar labels na resposta, pois permissões podem fazer o GitHub omiti-las; issue criada com labels ausentes
é sucesso parcial, nunca repetir criação para corrigir labels.

Editar labels: ler estado atual, comparar `expectedUpdatedAtMs` e campos tocados; se divergiu, apresentar
diff/recarregar antes de escrever. Operações incrementais de adicionar/remover preservam labels externas e
mudanças do MaxCode em labels não tocadas. Sem garantia de CAS remoto: outro cliente pode alterar durante
a escrita; reler no fim, mostrar resultado real/conflitos e não restaurar snapshot antigo por rollback cego.
Falha na terceira operação de cinco retorna parcial com lista aplicada/falha e refetch. Durante ciclo ativo,
UI avisa sobre mudanças manuais de workflow; não suspende, acelera ou substitui fila do MaxCode.

## 12. Validação, entrega e fontes

### Matriz mínima de verificação futura

| Área | Casos obrigatórios |
|------|--------------------|
| Catálogo | 24/15/3/6, fixture incompleta, caixa irregular preservada, hash inválido, peer incompatível, metadados ausentes |
| Rede | 3+ páginas/mais de 1000 issues, PR misturado, comentário editado, 304 em página 1 com mudança na 2, Link cíclico/externo |
| Falhas | 401, 403 permissão versus rate limit, 429, Retry-After, timeout gh kill/wait, JSON inválido, POST sem resposta sem replay |
| Cache | WAL, FK/cascatas, migração vazia/existente, índices por EXPLAIN, falha no meio sem alterar watermark, conta trocada, sanitizer |
| Contagem | Filtros não afetam badge, open→closed e reopen, desconhecido versus zero, transferência sem apagar por 404 ambíguo |
| Plano | Múltiplos comentários/páginas, heading no código/citação ignorado, edição/remoção, limite H2 e origem/provisório |
| MaxCode | Gate sem capability, open/BUG PLAN/BUG FIX distintos, URI sem ACK, ACK perdido, requestId duplicado, reinício sem novo ciclo |
| Worktree | Existente, branch sem worktree, falta de branch, nome homônimo externo, branch em uso, corrida de criação, spawn falho preserva diretório |
| Frontend | Slot fixo sem PTY/fechar/reordenar, store sem resposta obsoleta, cleanup listeners, XSS/URLs, acessibilidade, falha parcial de labels |

Executar no futuro `cargo test -p mt-github`, testes de migração da Fase 03, `cargo clippy -p mt-github -p maxterminal -- -D warnings`,
geração/diff de bindings, `tsgo --noEmit`, lint/Stylelint e Vitest de `tests/frontend/github`. Fixtures gh/REST
e peer falso permitem validação sem criar issues reais. Integração com MaxCode real e keyring usa repositório
de teste autorizado. Fase 17 recebe E2E, benchmark de 1000+ issues e hooks de busca; não condiciona a lógica desta fase.

### Pendências explícitas de integração

- Produzir/exportar e versionar catálogo e fixtures autoritativos; nomes não presentes na fonte não podem ser inferidos.
- Implementar e validar bilateralmente protocolo de issue/ciclo, dedup durável, consulta de status e URI;
  transporte F11 existente não é evidência de que esses handlers já existem.
- Conferir token roxo e variantes geradas Specta no ambiente real; manter APIs-base sem duplicação.

### Referências consultadas

- `AGENTS.md`, `implementation/README.md` e specs 06/07/08/11/13 desta worktree.
- Fonte MaxCode indicada em §2; somente leitura, exportação ainda é entregável futuro.
- [GitHub CLI: gh api](https://cli.github.com/manual/gh_api): métodos, stdin, headers e paginação.
- [GitHub REST: issues](https://docs.github.com/en/rest/issues/issues): lista mistura PRs, state/since,
  paginação e criação/labels sujeitas a permissões; consulta em 29/10/2026.
- Context7 foi tentado, mas retornou limite mensal excedido; referências oficiais acima foram usadas como fallback.
