# Fase 17 — Especificação Técnica: Busca Global, Qualidade, Performance e Empacotamento

> Complementa [`instructions.md`](./instructions.md). Consome exclusivamente as Fases 01–16 e os
> contratos de [`../README.md`](../README.md). Os exemplos são contratos de implementação, não
> evidência de benchmarks já executados. Baselines e resultados permanecem pendentes até medição.

## 1. Arquitetura e decisões

```text
GlobalSearchPalette → useSearchStore → ipc/search.ts → commands/search.rs
    → SearchEngine → Db::read / lease exclusivo → snapshot SQLite → fontes FTS
    → snapshot limitado em memória → página de SearchHit → segmentos de texto Vue
    → useSearchNavigation → stores/visualizadores das Fases 06/07/15/16

PTY bytes → Channel da 02 → consumidor xterm da 04 → ACK
         → taps 05/07 → StreamSanitizer da 03 → writer único → tabelas/FTS
```

- `mt-db` permanece sem dependência de Tauri e sem dependências reversas de `mt-github`/`mt-commands`.
  Composição de provedores e adaptadores fica no binário. Nenhum novo crate é necessário.
- Busca global é local e somente leitura. Não consulta GitHub/IA nem executa comandos ao encontrar texto.
  Fontes remotas entram apenas pelo cache sanitizado da fase responsável, com data da última sincronização.
- Seções têm ordem fixa; relevância é calculada **dentro** da fonte. Scores `bm25` de corpora diferentes não
  são comparáveis. A revisão substitui a fusão RRF inicialmente proposta por ordenação explícita por seção.
- Paginação usa snapshot materializado limitado e cursor ordinal. Keyset apenas por score/rowid em consultas
  independentes não mantém estabilidade: updates, deletes e novas linhas alteram inclusive o `bm25` antigo.
- O namespace `search.*` inclui erros e settings. Busca no buffer xterm continua pertencendo à Fase 04.

## 2. Contratos consumidos e dependências de integração

| Fase | Contrato usado | Verificação antes da implementação |
|------|---------------|-----------------------------------|
| 01 | `MtResult`, `MtError`, ids, `AppPaths`, `tauri-specta`, `ipc/core.ts`, scripts de checks e libs locais | Gerar bindings, conferir nomes e casing; nenhuma cópia manual de DTO |
| 02 | `PtyManager`, `OutputSink/OutputTap`, ACK, `Gap`, `(pid, start_time)`, `EnvPolicy` | Kill assíncrono não significa saída confirmada; testes aguardam `kill_and_wait`/evento final |
| 03 | Infraestrutura `Db::{read,write}`, `DbSetup`, `CodeMigration`, `FtsSpec`, `FtsRegistry`, sanitizador, backup/manutenção | Reusar writer, PRAGMAs, migrations e registro FTS; não atribuir à 03 criação de tabelas de sessões/comandos/saída |
| 04 | `HybridTerminal`, busca local, registro de atalhos/instâncias, camada de blocos | Navegação precisa localizar bloco já descartado do buffer usando histórico |
| 05 | Cria `sessions`, `command_blocks`/`command_blocks_fts`; expõe `command_block_*` | Spec §7: `command,cwd,output_text` nullable, `command_quality`, `rowid INTEGER PRIMARY KEY`, filtros por `session_id/tab_id/pane_id`, exit nullable |
| 06 | `projects`, `useProjectsStore`, seleção, remoção de projeto | Mapear título/pasta e seleção ao contrato efetivamente integrado |
| 07 | Estende `sessions` da 05; cria `tabs`, `tab_templates`, `tab_sessions`, `tab_session_panes`, `session_scrollback`/FTS | `session_get`/`session_scrollback_get` e viewer paginado; não criar `sessions` novamente nem presumir tabelas `commands`/`scrollback_chunks` da 03 |
| 08–10 | Git/portas, tray/Quake/notificações, temas | Smoke instalado, processos de portas, contraste e fallback gráfico |
| 11–12 | Bridge, URIs, diagnóstico e gateway Rust | Testar sanitização na saída e autenticação com protocolos das fases donas |
| 13–14 | Agentes/skills/telemetria, comandos rápidos | Testar degradação sem ferramentas externas, hooks e recursos empacotados |
| 15 | `command_groups`, itens/execuções, `useCommandGroupsStore` | Provedor deve informar chave da execução/item, timestamps, saída sanitizada, truncamento e vínculo ao projeto/sessão |
| 16 | `github_issues`, `github_issue_comments`, `useGithubIssuesStore` | Provedor deve informar chave `(repo,number,comment?)`, texto sanitizado, projeto, updated/fetched e política de evicção |

Os `spec.md` de 15/16 estavam ausentes na leitura desta revisão; seus `instructions.md` reservam esses
domínios, mas não estabelecem todas as colunas. Os adaptadores abaixo definem o contrato exigido sem inventar
DDL para essas fases. Confirmar os schemas finais na implementação e anexar o resultado ao teste
`schema_contract`. Fonte obrigatória ainda não integrada é pendência de entrega, não resultado vazio aprovado.

Também há divergências entre descrições 02/07 sobre assinatura de `kill` e `OutputTap`: prevalece a API
implementada pela fase dona, com teste de conclusão real. A Fase 17 não deve esconder isso em um mock.

## 3. Módulos e modelos Rust

```text
mt-db/src/search/
  mod.rs          SearchEngine, composição das fontes
  model.rs        DTOs exportados e tipos internos
  query.rs        validação e build_match_query da 03
  rank.rs         ordenação determinística por seção
  cursor.rs       tokens opacos, validação de proprietário/expiração
  snapshot.rs     cache limitado por bytes, TTL e invalidação
  cancel.rs       registry, lease e descarte coordenado
  projection.rs   projeções sanitizadas de metadados
  sources/        command_blocks, scrollback, sessions, projects, templates, groups, issues
  error.rs        códigos search.* → MtError
maxterminal/src/commands/search.rs     fronteira Tauri, validação de janela
maxterminal/src/search/adapters.rs     integração com provedores 06/07/15/16
maxterminal/src/events/search.rs       progresso sem texto pesquisado
```

DTOs derivam `Serialize/Deserialize` quando aplicável e `specta::Type`; structs usam `camelCase` no wire,
enums usam tags `kind` e valores `snake_case`. Novos ids de busca são opacos locais ao domínio, não substituem
`SessionId`, `CommandBlockId` etc. Datas são Unix ms validados no intervalo seguro de inteiros JavaScript;
contadores potencialmente maiores e `seq` de PTY continuam strings decimais/bigint conforme a Fase 02.

```rust
pub struct SearchRequest {
    pub request_id: String,
    pub generation: u32,
    pub query: String,
    pub filters: SearchFilters,
    pub sort: SearchSort,
    pub page_size: u16,
    pub cursor: Option<String>,
}
pub struct SearchFilters {
    pub project_id: Option<ProjectId>,
    pub tab_id: Option<TabId>,
    pub session_id: Option<SessionId>,
    pub pane_id: Option<PaneId>,
    pub from_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub exit: ExitFilter,
    pub sources: Vec<SearchSourceKind>,
}
pub enum ExitFilter { Any, Zero, Nonzero, Unknown }
pub enum SearchSort { Relevance, Recent }
pub enum SearchSourceKind {
    Commands, Output, Sessions, Projects, Templates, Groups, GroupRuns, Issues, Comments,
}
pub struct HighlightSegment { pub text: String, pub matched: bool }
pub struct SearchHit {
    pub key: String,
    pub source: SearchSourceKind,
    pub target: SearchTarget,
    pub title: Vec<HighlightSegment>,
    pub excerpt: Vec<HighlightSegment>,
    pub occurred_at_ms: Option<i64>,
    pub source_truncated: bool,
    pub fetched_at_ms: Option<i64>,
}
pub enum SearchTarget {
    Command { project_id: Option<ProjectId>, tab_id: TabId,
        session_id: SessionId, pane_id: Option<PaneId>, block_id: CommandBlockId },
    Output { project_id: ProjectId, tab_id: TabId, session_id: SessionId,
        pane_id: PaneId, chunk_seq: i64 },
    Session { project_id: Option<ProjectId>, tab_id: TabId, session_id: SessionId },
    Project { project_id: ProjectId },
    Template { project_id: ProjectId, template_id: TemplateId },
    Group { group_id: CommandGroupId, project_id: Option<ProjectId> },
    GroupRun { run_key: String, item_key: Option<String>, project_id: Option<ProjectId> },
    Issue { repo_key: String, number: u32, comment_key: Option<String>, project_id: ProjectId },
}
pub struct SearchSection {
    pub source: SearchSourceKind,
    pub items: Vec<SearchHit>,
    pub next_cursor: Option<String>,
    pub truncated: bool,
}
pub struct SearchResponse {
    pub request_id: String,
    pub generation: u32,
    pub snapshot_id: String,
    pub expires_at_ms: i64,
    pub sections: Vec<SearchSection>,
    pub indexed_through_ms: Option<i64>,
}
```

`run_key/repo_key/comment_key` são identificadores serializados pelos provedores 15/16, sem SQL/caminho/URL
executável. Substituí-los por newtypes exportados dessas fases quando confirmados. `indexed_through_ms` só é
preenchido se existir watermark real de commits/projeções; não usar horário da consulta como falsa atualização.

Contratos internos (assinaturas de projeto, implementadas sobre as APIs fixadas no lockfile):

```rust
pub trait SearchSource: Send + Sync {
    fn kind(&self) -> SearchSourceKind;
    fn collect(&self, conn: &rusqlite::Connection, query: &ParsedQuery,
        context: &SearchContext) -> Result<CollectedSection, SearchError>;
}
pub struct SearchContext {
    pub cancellation: Arc<AtomicBool>,
    pub deadline: std::time::Instant,
    pub max_hits: usize,
    pub max_snapshot_bytes: usize,
}
pub struct SearchEngine { /* Db, fontes, CancelRegistry, SnapshotCache, limites */ }
impl SearchEngine {
    pub async fn search(&self, owner: SearchOwner, request: SearchRequest) -> MtResult<SearchResponse>;
    pub fn cancel(&self, owner: SearchOwner, request_id: &str) -> CancelOutcome;
    pub fn release(&self, owner: SearchOwner, snapshot_id: &str);
}
```

`SearchOwner` vem da janela chamadora autenticada, nunca de um campo enviado pelo frontend. `CollectedSection`
contém hits ordenados, informação de limite e bytes; erros de fonte obrigatória abortam a consulta com erro
tipado, sem falso “nenhum resultado”. Não criar um SQL executor genérico no IPC.

## 4. SQLite, índices e projeções

### 4.1. Schema canônico e colunas pesquisáveis

**Ownership obrigatório:** a Fase 03 fornece WAL, writer/leitores, runner de migrations, sanitização e
registro/manutenção FTS. A Fase 05 cria os domínios `sessions`, `command_blocks` e `command_blocks_fts`
na migration `NNNN_05_shell_sessions_command_blocks.sql` (spec §7). A Fase 07 estende essas sessões e
acrescenta vínculos de workspace e scrollback. A Fase 17 consome o schema resultante; não pressupõe
`sessions`, `commands` ou `scrollback_chunks` criadas pela 03.

| Fonte | Origem e índice | Campos e identidade |
|-------|-----------------|---------------------|
| Comando/saída por bloco | `command_blocks` + `command_blocks_fts` (05) | `command,cwd,output_text` nullable; identidade pública `id`; `rowid INTEGER PRIMARY KEY` estável só no JOIN FTS |
| Saída por sessão/painel | `session_scrollback` + `session_scrollback_fts` (07) | `text`; identidade `(session_id,pane_id,seq)` |
| Sessão | `sessions` criada na 05, estendida pela 07 com `tab_sessions`/`tab_session_panes` | identidade `sessions.id` preservada; `cwd` nullable, contexto sanitizado e `archived_at` da sessão/agrupamento 07 |
| Projeto/template/grupo | Tabelas de domínio 06/07/15 | nomes/descrições sanitizados; conteúdo executável/env excluído por padrão |
| Execução de grupo | Tabelas de execução 15 | comando/saída sanitizados e limites da fase dona |
| Issue/comentário | Cache 16 | título/corpo sanitizados; nunca credenciais, respostas HTTP brutas ou headers |

Sessões também são encontradas por seus comandos/saídas, com `session_id` em cada alvo. A seção “Sessões”
pesquisa metadados, sem copiar toda a saída para outra tabela. Comando e scrollback podem ter texto sobreposto;
isso é explicado pelas seções e não deduplicado por igualdade textual (que perderia contexto).

`command_quality = partial|unavailable` não autoriza inventar comando completo a partir da saída;
comando nulo recebe rótulo “Comando indisponível”, e o bloco continua encontrável pelo texto de saída.
`dropped_bytes`/`output_truncated` alimentam aviso de incompletude. Histórico legado sem vínculo 07 é
preservado e pesquisável por `sessions.id`; consultas globais não usam INNER JOIN obrigatório com
`tab_sessions`, que o eliminaria. Para scrollback, usar `(session_id,pane_id)` de
`tab_session_panes` e sua aba/projeto. A 07 estende `sessions.status` com `archived` após fechamento e
flush confirmado, preservando códigos de saída; consultar `sessions.archived_at` e, quando houver vínculo,
`tab_sessions.archived_at`. `SessionId` identifica a sessão base; resolver `TabSessionId` por
`tab_session_panes.tab_session_id` antes de chamar `session_get(tab_session_id)` ou
`session_scrollback_get(tab_session_id, session_id, ...)`. Sem vínculo, usar o viewer de busca para
metadados/blocos legados, sem fabricar agrupamento nem passar SessionId no lugar de TabSessionId.

`schema_contract` aplica 03 isolada e confirma ausência de `sessions/command_blocks`; aplica 05 e verifica
o DDL/FTS canônico; aplica 07 e verifica preservação de ids, vínculos e scrollback; aplica 17 e confirma
ausência de tabelas alternativas `commands/scrollback_chunks`. Divergência textual entre fases deve ser
resolvida pelo ownership acima antes de integrar, nunca por criar aliases/tabelas paralelas silenciosamente.

### 4.2. Projeção auxiliar única, derivada e reconstruível

Não recriar `sessions`, `command_blocks`, `session_scrollback`, settings, backups ou seus FTS. Para fontes
sem FTS, adotar **uma** projeção auxiliar sanitizada, em vez dos quatro índices de conteúdo externo direto
inicialmente propostos sobre configurações possivelmente sensíveis:

```sql
-- NNNN_17_search.sql; número global atribuído pelo runner da 03.
CREATE TABLE search_metadata (
    rowid INTEGER PRIMARY KEY,
    source TEXT NOT NULL CHECK (source IN
        ('sessions','projects','templates','groups','group_runs','issues','comments')),
    entity_key TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    occurred_at INTEGER,
    source_revision TEXT NOT NULL,
    UNIQUE (source, entity_key)
) STRICT;
CREATE INDEX ix_search_metadata_project_source
    ON search_metadata(project_id, source, occurred_at, entity_key);
CREATE INDEX ix_search_metadata_source_time
    ON search_metadata(source, occurred_at, entity_key);
```

`search_metadata` não é novo armazenamento de domínio: é projeção descartável de campos permitidos, sem
comando executável original, env, credenciais ou dados ausentes no cache de origem. `source_revision` é uma
versão/digest do conteúdo **sanitizado**, não hash de segredo. Chaves heterogêneas não têm FK genérica:
deleção/atualização de origem deve atualizar a projeção na mesma transação do writer; `project_id` nulo é
permitido para grupos globais. Evicção do cache 16 remove projeções correspondentes.

Registrar `FtsSpec { content_table: "search_metadata", fts_table: "search_metadata_fts",
columns: &["title", "body"], tokenizer: FtsTokenizer::Text, prefix: &[2, 3] }` no `DbSetup` da 03.
Usar DDL/triggers `ai/ad/au` gerados por `FtsSpec`; backfill e `rebuild` são necessários para dados antigos:
criar triggers não indexa linhas já presentes. Sem trigger SQL que copie texto cru para essa projeção.

O adaptador da fase dona chama `upsert_projection(tx, SanitizedProjection)` na transação de CRUD/cache;
`SanitizedProjection` só se constrói por `SanitizedText`. Os módulos dependentes recebem trait/função do
`mt-db`, evitando dependência reversa. Se esse hook não existir, é dependência de integração bloqueante.
Eventos de frontend não garantem atomicidade e não são mecanismo de indexação.

Índices B-tree adicionais candidatos (somente se não houver equivalente e o plano/benchmark justificar):
`sessions(project_id,started_at,id)`, `command_blocks(session_id,started_at,id)` e
`command_blocks(exit_code,started_at,id)`. A migration inspeciona os índices existentes; não adiciona outro
`session_scrollback(session_id,pane_id,seq)`, já presente na 07. Documentar `EXPLAIN QUERY PLAN`, tamanho,
custo de escrita e resultado do benchmark antes/depois; não prometer que FTS+JOIN usará todo índice.

### 4.3. Filtros e consulta

- Valores sempre bindados; tabela/coluna/ordem vêm de enum/SQL estático. `build_match_query` da 03 recebe
  entrada validada (até 256 caracteres/1024 bytes, sem NUL); operadores digitados são literais, não DSL.
  Entrada vazia/só pontuação retorna orientação de UI sem consulta MATCH. Prefixo segue a regra da 03.
- Datas usam intervalo `[from_ms,until_ms)`, com validação `from < until`; nunca converter tempo em faixa de
  `rowid`. Projeto do comando vem de JOIN com sessão, e não de pressuposição sobre UUID.
- Filtros de sessão/aba/painel excluem fontes sem esses vínculos. Filtro de exit aplica-se a comandos e
  itens de execução cujo exit seja conhecido; saída de sessão e metadados não herdam exit de outro bloco.
  `nonzero` é `IS NOT NULL AND != 0`; `unknown` é `IS NULL`. UI informa as fontes compatíveis.
- `bm25` menor é melhor; ordenar ascendente, depois timestamp descendente e chave de domínio crescente.
  No modo recente, ordenar timestamp descendente e chave crescente. Pesos começam iguais e mudanças
  exigem fixture de relevância e baseline, sem boost oculto de recência.

Exemplo no schema da 05 (demais predicados são variantes de SQL estáticas):

```sql
SELECT c.id, c.session_id, c.tab_id, c.pane_id, s.project_id,
       c.started_at, c.output_truncated,
       bm25(command_blocks_fts) AS score,
       snippet(command_blocks_fts, -1, ?1, ?2, '…', 24) AS excerpt
FROM command_blocks_fts
JOIN command_blocks c ON c.rowid = command_blocks_fts.rowid
JOIN sessions s ON s.id = c.session_id
WHERE command_blocks_fts MATCH ?3 AND s.project_id = ?4
ORDER BY score ASC, c.started_at DESC, c.id ASC
LIMIT ?5;
```

`?5` = limite de materialização + 1 para detectar truncamento, não limite oculto de candidatos antes do
ranking. Não usar `LIKE` como oráculo de relevância/tokenização; fixtures definem explicitamente os ids.

### 4.4. Manutenção e migrations

Migrations são imutáveis e registradas pelo runner da 03, com backup pré-migration, checksum, FK e rollback.
Rebuild usa allowlist de `FtsRegistry`, nunca nome arbitrário do IPC; exclusão/retention verifica MATCH,
conteúdo externo e `integrity-check` FTS com comparação ao conteúdo (`rank=1` quando suportado).
Um `SELECT count(*)` sem MATCH pode contar a origem e não comprova integridade FTS.

Rebuild é job explícito serializado com o writer; invalidar snapshots, exibir estado, medir tempo de bloqueio
e permitir cancelamento entre unidades transacionais. `rebuild` SQLite é operação monolítica: não simular
progresso percentual por linhas nem prometê-la em lotes. Se bloquear persistência além do orçamento,
agendar para período sem produtores ou implementar reconstrução por geração com troca atômica medida.
Na 05 concluída, `command_blocks.rowid` é `INTEGER PRIMARY KEY`, assim como o scrollback da 07 e a projeção
17; não presumir rowid implícito instável nesses domínios. Qualquer índice legado de outra fonte com rowid
implícito exige estratégia explícita de preservação/rebuild em manutenção e invalidação de snapshots.

## 5. Snapshot, paginação, cancelamento e destaques

### 5.1. Consistência limitada e explícita

1. Validar owner/request/generation e adquirir semáforo de busca antes de executar SQL bloqueante.
2. Reservar conexão da infraestrutura 03 exclusivamente para a operação. Abrir transação de leitura
   `DEFERRED`, estabelecer snapshot e consultar fontes sequencialmente nessa conexão. Consultas em pools
   independentes não compõem um snapshot SQLite único. Writer continua usando WAL.
3. Materializar somente alvos e trechos sanitizados ordenados. Defaults de proteção: 10 000 hits totais,
   16 MiB por janela, página 25 (máximo 100), TTL 120 s, no máximo dois snapshots por janela. São limites
   de recursos configuráveis a calibrar, não promessas de performance; retornar `truncated=true` ao atingir
   limite, com “Refine os filtros”. Limite de bytes inclui strings, overhead estimado e cursors.
4. Fechar a transação antes de devolver primeira página. Não prender snapshot WAL durante interação humana.
   Cache armazena a lista congelada; novos commits só aparecem em nova pesquisa.
5. Cursor aleatório opaco referencia `{owner,snapshot,source,nextOrdinal,queryFingerprint,expires}` no
   servidor; não precisa expor scores/SQL. Valida consulta/filtros/sort e proprietário; páginas são idempotentes.
   Entrada adulterada → `search.invalid_cursor`; expirado/evicto → `search.cursor_expired`.
6. Cursor por seção permite “Carregar mais” sem misturar fontes. Em resposta paginada, somente a seção pedida
   é anexada. Mesma chave não é anexada duas vezes na store após retry.

Inserções/updates/deletes comuns não reordenam o snapshot; navegação revalida a origem. Remoção de projeto,
restauração de backup, rebuild e mudanças de política de sanitização invalidam snapshots e limpam UI para
não manter texto que deveria ter sido eliminado. Expiração é erro recuperável com “Atualizar busca”.

### 5.2. Cancelamento cooperativo sem atingir outra consulta

`CancelRegistry` usa chave `(window_label,request_id)` e generation monotônica por janela. Estado:
`Queued → Running(lease,interrupt) → Completed`, com `Cancelled` terminal. `search_cancel` é idempotente;
pedido antes do registro cria tombstone limitado por TTL/capacidade. Reutilização de request id é rejeitada.

Durante lease, handle `Connection::get_interrupt_handle()` só é associado ao registro ativo. Cancelar
marca `AtomicBool` e chama `interrupt()` sob a mesma exclusão que retira o handle ao finalizar. O cancelador
não mantém clone utilizável depois de liberar o lock. Cleanup espera cancelamento em andamento, remove
handler/handle, encerra transação e só então devolve conexão ao pool. Testar corrida A→release→B→cancel(A).

Cheque flag antes de cada fonte, durante materialização e antes de publicar snapshot. `progress_handler`
com feature `hooks` do `rusqlite` permite deadline/cancelamento cooperativo durante SQL, se essa feature
for adotada no lockfile; remover callback antes de reutilizar conexão. Deadline inclui espera em fila e
materialização, sem prometer interrupção instantânea de I/O. SQLite interrompido vira `search.cancelled`
ou `search.timeout` conforme a causa registrada. AbortSignal do JavaScript sozinho não cancela `invoke`.

Se uma fonte obrigatória falhar/expirar, liberar recursos e retornar erro; não publicar resultados parciais
como completos. Limitar concorrência global e retornar `search.busy` ao esgotar a fila em vez de criar
threads/snapshots ilimitados. Fechar janela libera requests e snapshots do owner.

### 5.3. Destaques seguros

SQLite `snippet/highlight` produz texto com sentinelas escolhidas pelo backend, nunca HTML confiável.
Sentinelas fixas `U+E000/U+E001` podem ocorrer no conteúdo; gerar par aleatório, verificar ausência na coluna
original sanitizada e repetir se colidir. Converter em `HighlightSegment[]` antes do IPC; parse incompleto
ou colisão não resolvida resulta em texto sem destaque. Testar strings contendo as próprias sentinelas.

`HighlightedText.vue` renderiza `<mark>{{ segment.text }}</mark>` ou interpolação normal, sem `v-html`,
Markdown, atributos/URLs derivados do resultado ou regex construída a partir do usuário. Texto truncado
respeita UTF-8; offsets, caso necessários à navegação, são convertidos explicitamente e não misturam bytes
Rust com UTF-16 JS. Emoji, acentos, texto RTL, escapes e `<img onerror>` integram fixtures.

## 6. IPC, eventos e manutenção de dados

| Comando | Entrada | Saída / regra |
|---------|---------|---------------|
| `search_global` | `request: SearchRequest` | `SearchResponse`; operação bloqueante fora da thread da UI |
| `search_cancel` | `request_id` | `CancelOutcome { cancelled: bool }`; escopo da própria janela |
| `search_release` | `snapshot_id` | `()`; idempotente, libera cache e cursors |
| `search_index_stats` | — | lista `{source,ready,rowCount,updatedAt?,jobId?}` sem query/conteúdo |
| `search_index_rebuild` | `sources: Vec<SearchSourceKind>` | `{jobId}`; só fontes conhecidas, um job por vez |
| `db_backup_list` | — | metadados/id opaco de backups gerenciados |
| `db_backup_create` | — | id/metadados; reutiliza Online Backup API/validação/permissões da 03 |
| `db_backup_restore` | `backup_id` | `{pending:true}`; restauração somente no próximo boot |

Todos retornam `MtResult<T>`. `search://index-progress` é evento de baixo volume com
`{jobId,source,state:'queued'|'running'|'done'|'failed',completedUnits,totalUnits?,errorCode?}`;
`totalUnits` ausente em rebuild monolítico. `search://invalidated {snapshotId,reason}` é enviado apenas à
janela proprietária, sem texto da consulta. Não enviar output de PTY por eventos globais.

Erros: `search.invalid_query`, `invalid_filters`, `invalid_cursor`, `cursor_expired`, `cancelled`, `timeout`,
`busy`, `index_unavailable`, `source_unavailable`, `target_missing` e `internal`, todos com prefixo `search.`.
Mensagens em pt-BR; details só campos/ids públicos, sem SQL, consulta, env ou conteúdo de resultado.

Backup manual exige estender `BackupKind` da 03 com `Manual` (não reclassificar como Daily). Caminhos aceitos
somente por ids do catálogo, confinados ao diretório de backups, sem symlinks. Restore valida quick/integrity,
FK, versão do schema e checksums de migrations; banco mais novo é recusado. Gravar intenção atomicamente.
No boot, com lock de instância e antes de conexões: backup do estado atual consistente, estágio do backup
validado, remoção/quarentena coordenada dos sidecars antigos, troca atômica do banco e fsync de diretório.
Registrar etapas para recuperação após crash; em falha restaurar conjunto anterior, sem associar WAL antigo
ao banco restaurado. Rodar migrations só após validar versão. Testar falha em cada etapa.

## 7. Frontend Vue/Pinia e navegação

### 7.1. Store e wrappers

`src/ipc/search.ts` encapsula bindings gerados com helper da 01; expõe `searchGlobal`, `cancelSearch`,
`releaseSearch` e listeners que retornam `UnlistenFn`. `src/ipc/backup.ts` reutiliza o mesmo padrão.
`useSearchStore` mantém somente estado em memória (sem localStorage/cache persistido do plugin MaxPinia):

```ts
interface SearchUiState {
    open: boolean;
    query: string;
    filters: SearchFilters;
    generation: number;
    activeRequestId: string | null;
    snapshotId: string | null;
    sections: SearchSection[];
    selectedKey: string | null;
    status: 'idle' | 'debouncing' | 'loading' | 'ready' | 'empty' | 'error';
    error: MtError | null;
}
```

`SearchFilters/SearchSection/MtError` são imports de `bindings.ts`, não novas interfaces TS equivalentes.
Ações: `show(initialQuery?)`, `updateQuery`, `updateFilters`, `run`, `loadMore(source)`, `select`, `hide`,
`invalidate` e `dispose`. Timers, promises, handles de foco e requisições por seção ficam fora do estado
serializável. Não disparar busca diretamente de componentes concorrentes.

### 7.2. Fluxo assíncrono

1. Cada alteração de query/filtro incrementa generation **imediatamente**, cancela timer e request anterior,
   libera snapshot anterior e invalida páginas em voo. Debounce inicial 120 ms, configurável.
2. `run` captura generation/id/fingerprint e define loading. Ao resolver, compara todos e `open` antes
   de mutar store. Resposta velha nunca muda resultados, error ou loading do pedido atual; liberar snapshot
   que chegou atrasado. O mesmo gate se aplica ao `catch/finally` e a cada página.
3. `loadMore` deduplica por `(snapshotId,source,cursor)`, valida generation e anexa apenas chaves inéditas.
   Paginação não roda debounce nem substitui resultados de outras seções.
4. `hide/dispose` invalida generation, cancela timer/request, libera snapshots, remove listeners e limpa
   trechos. Composição IME suspende busca até `compositionend`. Foco anterior é restaurado se ainda existir.

### 7.3. Composables/componentes

| Elemento | Contrato |
|----------|----------|
| `useGlobalSearch` | Coordena timer/IME/dispose com a store; não persiste query |
| `useSearchNavigation` | `openTarget(target, action)` assíncrono; revalida ids e aciona domínios responsáveis |
| `useSearchShortcut` | Registra uma vez no gerenciador 04; respeita modal/foco/passthrough |
| `useHighlightSegments` | Valida limites/concatenação dos segmentos; não interpreta HTML |
| `GlobalSearchPalette.vue` | Dialog com input combobox, listbox agrupada, estado carregando/erro/expirado |
| `SearchFilterBar.vue` | Projeto, período, sessão/aba, exit, fontes; explica filtros incompatíveis |
| `SearchResultSection.vue` | `section`, `loadingMore`; emite `load-more(source)` |
| `SearchResultItem.vue` | `hit`, `selected`; emite `select(key)` e `open(target)` |
| `HighlightedText.vue` | `segments: HighlightSegment[]`; somente texto e mark |

`aria-expanded`, `aria-controls`, `aria-activedescendant` e labels pt-BR; setas percorrem resultados,
Enter abre, Escape fecha; “Carregar mais” acessível por teclado. Virtualização só se medida necessária,
preservando item ativo montado e sem anunciar cada tecla em `aria-live`. Testar zoom, contraste nos temas
10, foco visível e leitor de tela; axe automatizado não substitui revisão manual.

### 7.4. Atalhos e origem

`Ctrl+K` abre fora do terminal ou no input rico vazio; não interceptar kill-line do shell. `Ctrl+Shift+F`
escalona a busca local da 04 já aberta, levando seu texto; alt-screen/passthrough preserva a tecla da TUI.
Botão “Buscar em tudo…” no slot `actions` da 01 garante acesso sem conflito de atalho.

Comando: selecionar projeto → conferir aba e sessão atuais → focar painel/bloco se ainda retido. Caso
arquivado ou bloco removido do buffer, `loadDetail(sessionId)` → `SessionReadonlyViewer` sem stdin/PTY →
posição do bloco. Saída: carregar página por `(session,pane,chunk_seq)` e posicionar o chunk; não prometer
linha exata inferida de snippet ou offset bruto que mudou na sanitização. Retenção removeu alvo → aviso
“Este resultado não está mais disponível” e ação para atualizar busca.

Sessão/comando legado sem projeto ou aba atualmente vinculável abre histórico somente leitura por
`session_id`, com contexto “Histórico não vinculado”; não exigir projeto fictício nem criar aba/PTY.

Projeto seleciona; template abre detalhes, e `Ctrl+Enter` solicita abertura via fluxo 07; grupo abre editor/
histórico, sem executar; issue abre aba fixa/detalhe e comentário, sem enviar BUG FIX automaticamente.
Navegação nunca concatena conteúdo encontrado em comandos ou URIs. Realce visual temporário de 1,5 s
respeita preferência de movimento reduzido. Helpers de abertura do viewer são adaptadores da 17, não APIs
preexistentes fictícias como `useSessionsStore.openArchived()`.

## 8. Matriz de testes de integração e segurança operacional

### 8.1. Níveis e evidências distintas

| Nível | Ambiente | O que comprova |
|-------|----------|----------------|
| Unit/contrato | Vitest, timers falsos, IPC mockado | Corridas, argumentos, geração, semântica DOM; não comprova Rust/PTY/SQLite |
| Integração Rust | SQLite em arquivo/WAL e PTY Linux reais | Índices, migrations, bytes, sanitização e processos |
| E2E frontend | Vite + navegador, IPC simulado declarado | Fluxos Vue, teclado, erro/empty/loading, regressões visuais |
| E2E Tauri real | Binário + WebKitGTK + WebDriver externo | IPC nativo, DB, PTYs, busca após reiniciar, capabilities e recursos |
| Smoke instalado | VM CachyOS/sessão gráfica real | Pacman/AppImage, tray, hotkey, URI, permissões, Wayland/X11 |

Cada execução tem home/XDG/DB/runtime próprios, manifesto de fixture e teardown mesmo em falha. Backend
real não usa `mockIPC`, interceptação de comandos nem servidor Vite. Endpoints externos podem ser dublês
HTTP controlados e identificados (não prova de serviço real); smoke opt-in com serviço real é relatório
separado. Instrumentação de teste fica atrás de feature sem permissão no artefato release.

### 8.2. PTY, bytes e ausência de órfãos

- Produtor determinístico com linhas de tamanho fixo, sequência, seed e hash, além de payloads UTF-8/ANSI
  partidos em cada fronteira. Configurar termios sem echo/OPOST para comparação binária; no teste shell
  normal registrar transformações CRLF esperadas. Contar produzido→lido→enviado→consumido→ACK.
- ACK só depois de consumo pelo xterm. Anexado/saudável: hashes e sequência coincidem após drenagem, zero
  byte perdido/duplicado. Renderização visual de terminal não é oráculo de integridade do stream binário.
- Consumidor lento: medir filas máximas/marcas d'água e bloqueio do produtor, sem crescimento ilimitado.
  Suspender ACK, detach, reattach e expirar replay: `Gap` informa exatamente bytes perdidos e UI avisa.
  Persistência tem contabilidade distinta: texto sanitizado, TUI omitida, limite 07 e backpressure do writer
  geram truncamento explícito. Não comparar hash bruto com texto sanitizado nem chamar perda no DB de sucesso.
- Fixture gera shell, filho, neto, background, `nohup`, `setsid`, double-fork e processo que ignora HUP/TERM.
  Rodar fechamento de painel/aba, sessão nova, cancelamento de grupo, saída explícita, SIGTERM e panic em
  processo de teste separado; esperar orçamento de shutdown configurado e inspecionar `/proc`/reaping.
- Validar posse por `(pid,start_time)`, UID e registro do supervisor; PGID isolado não detecta `setsid`.
  Caso de PID reutilizado nunca sinaliza processo alheio. Zumbi conta como falha de reaping, não como sucesso.
- Fechar janela enquanto há tarefa preserva PTYs no tray. `SIGKILL` não executa cleanup: testar recuperação
  no próximo boot e documentar janela de sobrevivência, sem garantir zero órfãos imediato nesse caso.

### 8.3. Segredos, retenção e migrations

Segredos sintéticos de todos os tipos da 03 entram por input/output real, comandos, ANSI fragmentado,
URLs, hooks OSC e payloads bridge/IA. Após flush e reinício, verificar comandos, scrollback, projeções,
vocabulário FTS, backups, DB/WAL e logs, inclusive bytes em páginas livres. Não basta buscar o token pelo
próprio FTS, pois tokenização pode mudar caracteres. Logs de teste/screenshots não podem vazar fixtures
de credenciais reais. Testar idempotência, final de sessão sem newline e linha maior que carry do sanitizador.

Templates da 07 documentam exceção de configuração literal: Fase 17 não indexa `env_json`/comando inicial
cru nem declara proteção universal desses campos. Se a versão integrada permitir segredo literal persistido,
registrar incompatibilidade com a regra canônica e bloquear aprovação de “sem segredos em disco” até a fase
dona mover valores para armazenamento adequado/referências. Sanitizar apenas na leitura não apaga passado.

Fixtures de cada versão suportada: banco populado com ao menos 10 000 blocos, sessões arquivadas/ativas,
projetos removidos, templates, grupos e cache 16; backup consistente inclui WAL pendente via API da 03.
Testar upgrade, reabertura idempotente, checksum divergente, banco mais novo, disco cheio e falha entre
migrations. FK/integrity/FTS íntegros; ids/contagens preservados, exceto retenção explicitamente esperada.
Nenhuma migration pode descartar sessões “não reconhecidas” para passar no teste. Restore/crash testado por etapa.

## 9. Performance: protocolo, baseline e limites

### 9.1. Ambiente registrado

`bench/environment.json` registra CPU/modelo/núcleos/governor, RAM, armazenamento/filesystem/espaço,
kernel/CachyOS/glibc, GPU/driver/compositor/X11-Wayland/escala/resolução/Hz, WebKitGTK, SQLite/compile-options,
Rust/Node/npm/Tauri, locks, commit e features, perfil release, renderer WebGL/fallback, DB/FTS/WAL em bytes.
Registrar tamanho/seed do corpus, distribuição de linhas por fonte, tamanho de linha e taxa de produção.
Máquina dedicada, energia/temperatura controladas; registrar interferência externa e não ocultar outliers.

### 9.2. Cenários e métricas

| Cenário | Método e métricas |
|---------|-------------------|
| Busca | 1M linhas, 200 consultas versionadas, termos raros/comuns/prefixos/filtros; p50/p95/p99 de fila/SQL/IPC/pintura e timeouts |
| Rajada | produtor finito 1 GiB e arquivo 500 MiB; bytes, checksum, ACK, filas/replay, tempo de drenagem e latência de clique |
| 20 abas | 200 linhas/s por aba, tamanho fixo e duração registrada, visíveis/ocultas, busca concorrente e parada |
| TUI | vim fixture 10 000 linhas/htop; teclado, resize, troca de aba, frame intervals WebGL/fallback |
| Ocioso | 5 abas por 60 s após warmup; CPU normalizada por núcleo, wakeups, PSS/RSS e timers |
| Persistência | writer lag, rejeições, flush, crescimento/checkpoint WAL sob busca/rebuild/retention |
| Bundle/startup | bytes crus/gzip por entry/chunk/CSS/recursos; processo→janela→primeiro prompt no pacote instalado |

Warmup e duração/número de amostras são fixados no manifesto antes de comparar. Primeira calibração usa pelo
menos cinco execuções independentes; percentis de cauda exigem amostras suficientes, senão declarar insuficiência.
Cache frio não é “reiniciar a aplicação”: documentar se apenas cache SQLite/processo foi limpo ou se page cache
do SO também foi controlado em VM dedicada. Não alterar caches globais de máquina compartilhada.

CPU e memória distinguem processo Rust, árvore WebKit e processos de workloads; somar PSS para evitar dupla
contagem de páginas compartilhadas, reportar RSS também. Frontend mede input→ACK/consumo e input→pintura
separados. Não subtrair `Instant` Rust de `performance.now()` JS sem sincronização e erro estimado.
Callback `xterm.write` não prova pintura; usar marcador/correlação, frame seguinte e validação visual quando
possível. Se Long Tasks API não existir no WebKitGTK fixado, usar heartbeat/rAF e declarar a limitação.

### 9.3. Gates calibrados

```json
{
    "status": "pending_measurement",
    "environmentId": null,
    "referenceCommit": null,
    "corpusSha256": null,
    "metrics": {
        "search.hot.p95.ms": {
            "baseline": null,
            "absoluteLimit": null,
            "allowedRegression": null,
            "sampleCount": null
        }
    }
}
```

Campos nulos bloqueiam aceite de desempenho. A primeira medição produz amostras brutas e orçamento por
métrica aprovado pelo mantenedor; tolerância deriva da dispersão e necessidade de UX, não de percentual
universal. Nova versão precisa cumprir limite absoluto **e** regressão permitida no mesmo ambiente/corpus.
Alteração de ambiente invalida comparação e exige nova calibração, não atualização automática do baseline.
Não melhorar resultado reduzindo corpus, omitindo output, silenciando truncamento ou retirando sanitização.

CI compartilhada valida execução do benchmark/smoke, não certifica números de hardware. `bench.yml` em
runner dedicado compara baseline, publica raw JSON/percentis e aponta falhas. `PerfOverlay` e spans dev
(`search.queue/sql/serialize`, `pty.read/send/ack`, `term.consume`, `db.flush`) não incluem consultas/saída.
`tokio-console` é opt-in; medir overhead com e sem profiling. Analisador de bundle precisa funcionar no
bundler efetivo do Vite 8; relatório estático de tamanho já é suficiente para o gate.

## 10. Segurança integrada

### 10.1. Conteúdo e rede

Consultas só em memória; proibir query/resultado em tracing, telemetry, panic details e localStorage.
Sanitização precede persistência/indexação, com testes das entradas 05/07/15/16. Verificar segredos antes de
qualquer envio externo 11/12. Não conceder ao WebView SQL, filesystem ou shell arbitrários para implementar busca.
Testes de bridge usam Host/Origin/token/rotas das fases 09/11, incluindo replay, payload grande, traversal e
listener exclusivamente loopback. Não inventar rota `maxterminal://focus` sem confirmar o contrato integrado.

### 10.2. CSP e capabilities efetivas

- `script-src` sem unsafe-eval/CDN; recursos empacotados locais. Tauri pode injetar hashes/nonces: validar a
  política efetiva, não igualdade literal da string de configuração. Estilo inline necessário ao terminal
  deve ter justificativa e teste; sem relaxar scripts para contornar problema de CSS.
- `connect-src` permite somente origem local/transportes IPC exigidos pela versão fixada. Gateway/GitHub
  chamados em Rust não justificam abrir rede no frontend. Assets remotos não são dependência de boot.
- Inventário `janela → comando/plugin → motivo → teste negativo`. Janela principal possui busca; manutenção/
  backup apenas onde há ação correspondente. Quake reutiliza janela conforme 09; não criar capability fictícia.
- Comandos próprios precisam integrar o manifesto/ACL do aplicativo e/ou validar chamador no backend,
  conforme Tauri fixado. `invoke_handler` por si só não representa uma allowlist por janela. Teste em janela
  sem autorização e origem remota deve recusar busca, backup, rebuild e comandos de processo.
- Plugins com escopo mínimo; retirar permissões default sobrando, WebDriver e comandos de perf de produção.
  Auditorias Cargo/npm incluem dependências locais empacotadas; exceções têm justificativa, dono e prazo.

## 11. CachyOS/Arch, AppImage e release reproduzível

### 11.1. Separação de build e runtime

Arch/CachyOS: verificar pacotes WebKitGTK **4.1**, GTK3, libsoup3, OpenSSL, librsvg, AppIndicator e dependências
nativas realmente ligadas. `pkg-config --modversion webkit2gtk-4.1` e inventário de bibliotecas ELF compõem
preflight. `base-devel`, Rust/Cargo, Node/npm, ferramentas de packaging são `makedepends`; bibliotecas
compartilhadas requeridas são `depends`; CLIs opcionais (`gh`, agentes, shells extras) são `optdepends`,
com erro/degradação em pt-BR quando ausentes. Não exigir instalação de Node apenas para abrir o terminal.

AppImage é construída na base mais antiga declarada e testada que ofereça WebKitGTK 4.1; glibc continua
limite de compatibilidade. Não construir no Arch mais novo e afirmar suporte a qualquer Linux. Fixar imagem
base por digest, sysroot/pacotes e versões/checksums de linuxdeploy/appimagetool. Testar FUSE e modo extraído,
WebKit subprocessos, GPU e fallback no CachyOS; não desativar sandbox globalmente para “fazer funcionar”.
O `.deb` só é validado na matriz Debian/Ubuntu declarada; gerar arquivo não prova compatibilidade.

### 11.2. Recursos e bibliotecas Max desacopladas

`sync:local-libs` da 01 é conveniência de desenvolvimento. Release resolve cada `@maxvue/*` para tarball/fonte
imutável por commit/checksum em `release-manifest.json`, prepara peers/exports e incorpora o código no bundle.
Sem symlink absoluto para `~/GitHub`, checkout flutuante de branch, fetch `latest` ou credencial embutida.
Na CI pode haver token somente leitura durante obtenção, mas ele não entra em cache/artefato/log.

Inventário inclui ícones Iconify usados, fontes, temas padrão, hooks bash/zsh/fish, scripts auxiliares,
licenças e arquivos de configuração necessários. UI e hooks funcionam offline; recursos nativos são
resolvidos pelo diretório de resources do Tauri, não cwd do checkout. Testar runtime sem pasta de fontes,
sem Node e com rede indisponível. Se licença/fontes privadas impedirem redistribuição, PKGBUILD fica para
uso interno e publicação AUR é bloqueada explicitamente, sem baixar dependências privadas no pós-install.

`EnvPolicy` da 02 recebe adaptador AppImage: restaurar ambiente original dos subprocessos ou remover apenas
entradas injetadas por AppDir (`LD_LIBRARY_PATH`, `GIO_MODULE_DIR`, caminhos GTK e outros confirmados pelo
runtime). Não remover indiscriminadamente variáveis legítimas do usuário. Testar git, shell e CLI externa
fora do AppDir; terminal não pode carregar bibliotecas empacotadas incompatíveis por herança acidental.

### 11.3. PKGBUILD e integração desktop

`packaging/arch/PKGBUILD` define versão, arch, licença, fontes com checksums, depends/makedepends/optdepends;
`prepare()` obtém/prepara deps imutáveis; `build()` usa lockfiles congelados e toolchain declarada;
`check()` executa checks aplicáveis no chroot; `package()` instala binário, resources, desktop e ícones
em `$pkgdir/usr/...` com permissões corretas. Compilação executa como usuário sem root; hooks de pacote
não alteram HOME, associação MIME, autostart ou banco de usuário.

`PKGBUILD-bin` baixa release por versão/checksum verificado, `provides/conflicts` com pacote source;
`.SRCINFO` é gerado do PKGBUILD correspondente (diretórios separados por variante se necessário).
`namcap`, validação `.desktop`, conteúdo/ELF do pacote e build em chroot limpo são obrigatórios.

Desktop: `Exec=maxterminal %u`, `Terminal=false`, `Icon=maxterminal`,
`MimeType=x-scheme-handler/maxterminal;`, nome/comentário pt-BR, ícones hicolor e escalável. URI passa pelo
parser 09/11, sem interpolação shell. Associação MIME é preferência do usuário; instalador só registra
capacidade. Autostart é opt-in via mecanismo 09/plugin escolhido, sem duplicar entradas. Testar login,
`--minimized`, reabertura de workspace, tray e ausência de monitor quando aplicável.

### 11.4. Pipeline de release

1. Tag/commit autorizado, árvore limpa e checks aprovados; coerência de versão Cargo/npm/Tauri/pacote.
2. Obter dependências por manifests/locks/checksums, incluindo libs locais e ferramentas de bundle.
3. Build limpo em imagem por digest, rede desabilitada após preparação quando possível; `npm ci` com lock
   e cache preparado, Cargo `--locked --offline`. Fixar locale/timezone, timestamps (`SOURCE_DATE_EPOCH`),
   ordenação de arquivos e remap de caminhos de build. Registrar qualquer ferramenta não determinística.
4. Duas builds independentes em diretórios diferentes: comparar hashes dos payloads não assinados;
   diferenças analisadas, sem chamar de bit a bit um pacote apenas funcionalmente equivalente.
5. Produzir SBOM/licenças, hashes, manifesto de toolchains/locks/recursos/ambiente e evidências de testes;
   assinar artefatos e atestações após comparação. Assinaturas/timestamps externos são comparados à parte.
6. Instalação limpa e upgrade de versão anterior em VM, boot offline, shell, busca/histórico, tray, URI,
   remoção/reinstalação mantendo dados XDG. Só publicar quando todos os gates obrigatórios estiverem verdes.

AppImage opcionalmente habilita `updater`; Arch nunca instala atualização sobre pacote gerenciado pelo
pacman. Chave privada só no job protegido de assinatura; verificar assinatura e versão antes da instalação,
com ação explícita do usuário. Download falho/interrompido não modifica aplicativo/banco atual. Não prometer
rollback binário seguro se schema já migrou: versões antigas recusam schema novo e restauração usa backup
validado. AUR/publicação e atualização são ações separadas do simples build.

## 12. Suítes, execução e CI

| Arquivo/suíte | Casos bloqueantes |
|--------------|-------------------|
| `mt-db/tests/search_query.rs` | Propriedades de escaping, NUL/limites, prefixos, acentos, operadores literais |
| `search_sources.rs` | Oráculo por ids, exit NULL/nonzero, filtros combinados, exclusão de fontes incompatíveis |
| `search_snapshot.rs` | 10 000 hits paginados, inserts/updates/deletes concorrentes, limites/TTL/tampering/owner |
| `search_cancel.rs` | queued/running/antes do registro, A cancelada e B na mesma conexão, fechamento de janela |
| `search_index.rs` | CRUD/backfill/cascade/evicção/rebuild, FTS integrity com conteúdo externo, schema_contract |
| `upgrade_from_fixtures.rs` | Todas as versões suportadas, rollback, backup, corrupção e restauração por etapa |
| `tests/no_orphans.rs` | Matriz §8.2 em subprocesso isolado, incluindo grupos background |
| `tests/sanitizer_e2e.rs` | Pipeline real até DB/WAL/backups/FTS/logs/bridge, fragmentação e finalização |
| `tests/unit/search/*` | Debounce/IME, races incluindo finally, cursor, dispose, highlights e foco |
| `e2e/frontend/*` | UI com mocks identificados, falhas/retention, teclado e apresentação |
| `e2e/specs/*` | Busca após reiniciar Tauri real, sessão histórica read-only, IPC/PTY, CSP e capabilities |

Comandos de desenvolvimento partem da Fase 01: `npm run check`, `npm run typecheck:vue`,
`cargo test --manifest-path src-tauri/Cargo.toml --workspace`; nextest pode substituir o runner Rust com
filtros verificados. `cargo bench -p mt-db --bench search` executado em `src-tauri`; `npm run e2e` aponta
explicitamente à suíte **nativa** e `npm run e2e:frontend` à simulada. Registrar a lista de testes encontrados:
filtro que seleciona zero testes falha, não conta como aceite. Documentação desta fase não executa essas suítes.

CI: jobs separados `frontend`, `rust`, `e2e-frontend`, `e2e-native`, `audit`, `bundle-budget`, `traceability`,
`package`; `bench.yml` dedicado e `release.yml` protegido. Actions por SHA e permissões mínimas, sem segredos
em PR externo; dependências privadas indisponíveis bloqueiam o job necessário, não o convertem em sucesso.
WebDriver nativo usa binário real e versão fixada de driver/WebKit compatível, D-Bus e Xvfb na CI. Desktop
manual/VM cobre tray/atalho/Wayland não comprováveis no Xvfb. Retry registra falha original; flakiness não
pode mascarar falha de integridade/segredo/órfão. Branch protection é configurada no repositório, não por
comentário YAML. Evidências incluem logs sanitizados, versão/ambiente, exit codes, testes e artefatos.

## 13. Erros e recuperação operacional

- Query inválida: mensagem de campo, sem retry automático. Busy/timeout: manter entrada e botão “Tentar
  novamente”; não renderizar resultado de geração anterior como atual. Cancelamento esperado não gera toast.
- Índice indisponível: expor estado e ação de manutenção; não fazer scan ilimitado silencioso como fallback.
- Falta de espaço/backpressure: informar truncamento de histórico e preservar responsividade; nunca
  descartar erro de gravação e afirmar sessão completa. Backup/rebuild falho mantém DB anterior utilizável.
- Falta de ferramenta externa: indicar integração indisponível, mantendo terminal/busca local operacionais.
- Recurso de pacote ausente: erro com id do recurso e diagnóstico sanitizado; teste de smoke bloqueia release.

## 14. Rastreabilidade e dependências entregues

| Requisito canônico | Fases donas | Evidência de fechamento nesta fase |
|--------------------|-------------|-----------------------------------|
| Idioma, SFC, tipos, worktree e stack | 01 + todas | Checks estáticos, revisão pt-BR, bindings sem drift |
| PTYs, supervisão e output responsivo | 02/04/05/07 | Matriz bytes/ACK/perdas e no_orphans + baseline |
| SQLite WAL, histórico, FTS5 e segredos | 03/05/07/17 | Busca real, sanitizer_e2e, migrations, backups |
| Projetos, badges, templates/splits/sessões | 05/06/07/16 | E2E navegação/seen/tray, sessão arquivada sem stdin |
| Git/worktree/portas e sistema operacional | 08/09/11 | Smoke instalado, URI, porta/processo, hotkey/tray |
| Temas, cores e acessibilidade | 04/06/10 | Contraste/teclado/axe em WebView real e fallback |
| MaxCode e Inspecionar | 11 | Contrato bridge/deeplink, rejeições e payload sanitizado |
| MaxAiManager, IA, RAG e geração | 12 | Fluxos com dublê de protocolo + smoke real identificado |
| Agentes CLI, telemetria, MaxSkills/memória | 13 | Recursos, degradação e processos de agentes |
| Artisan, grupos e atalhos universais | 14/15 | Smoke de gatilhos, grupos e cancelamento real |
| Issues, labels e ciclo MaxCode | 16 | Cache/navegação, contrato externo, FTS comentários |
| Busca global, qualidade e distribuição | 17 | CA-01–CA-42, baseline aprovado, pacote e manifesto reproduzível |

Esta é a matriz agregada. Manifesto de rastreabilidade de implementação detalha cada requisito com
`requirementId`, referência canônica, fase, testId, runId, ambiente, artifact e status. Todos os itens
obrigatórios precisam de evidência; existência de `instructions.md/spec.md` só valida ligação estrutural.

**Contratos novos entregues:** `SearchRequest/Response/Hit/Target`, `search_*`, progresso/invalidação,
`useSearchStore`, navegação segura, projeção sanitizada reconstruível, `db_backup_*`, formatos de benchmark/
release e gates. **Dependências bloqueantes:** hooks transacionais de projeção 06/07/15/16; DDL/chaves 15/16;
resolver sanitização de configuração literal 07; confirmar APIs 02/07 e ACL próprio Tauri; libs Max
reproduzíveis/redistribuíveis; runner/VM e baseline medido. Nenhum desses contratos exige duplicar os domínios
criados pela 05 e estendidos pela 07; a 03 permanece responsável pela infraestrutura de persistência.

## 15. Referências verificadas nesta revisão

Consulta Context7 foi tentada, mas o serviço retornou cota mensal excedida. Consultadas fontes oficiais
diretamente em 29/10/2026; versões de implementação continuam determinadas pelo lockfile, não por `latest`:

- [SQLite FTS5](https://sqlite.org/fts5.html): conteúdo externo, triggers/backfill, `bm25`, snippet e integridade.
- [rusqlite InterruptHandle](https://docs.rs/rusqlite/latest/rusqlite/struct.InterruptHandle.html): interrupção
  da consulta em execução na conexão; não é token de cancelamento por request.
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/): dependências nativas Linux/WebKitGTK 4.1.
- [Tauri AppImage](https://v2.tauri.app/distribute/appimage/): base glibc mais antiga suportada e recursos.
- [Tauri WebDriver](https://v2.tauri.app/develop/tests/webdriver/): rota externa `tauri-driver` em Linux e
  distinção de browser mode simulado. Esta fase mantém driver externo para o gate nativo; qualquer serviço
  com plugin embedded exige configuração de teste isolada e não pode substituir evidência real por mocking.
