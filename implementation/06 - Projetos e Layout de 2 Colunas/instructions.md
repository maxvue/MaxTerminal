# Fase 06 — Projetos e Layout de 2 Colunas

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) §4 e §4.1. Contratos compartilhados: [`../README.md`](../README.md) §2.
> Especificação técnica detalhada: [`spec.md`](./spec.md).

---

## 1. Objetivo da Fase

Entregar a **espinha dorsal de navegação** do MaxTerminal: o cadastro persistente de projetos (individuais e
importados de uma "Pasta de Projetos"), a auto-descoberta de novas subpastas via `notify`, a auto-detecção de stack
com atribuição de ícone Iconify e cor, os contadores de status por projeto (badges 1–5) e o layout principal de
**2 colunas redimensionáveis** com `splitpanes`, cujo tamanho é persistido.

Ao final da fase, o usuário consegue abrir o app, adicionar `~/GitHub` como Pasta de Projetos, ver cada subpasta
listada com ícone/cor coerentes com a stack, editar/remover/reordenar projetos, receber sugestões (ou importação
automática) quando uma nova pasta surgir em `~/GitHub`, selecionar um projeto e ver a coluna direita (placeholder
`ProjectWorkspace.vue`) reagir à seleção. Os badges já refletem os estados emitidos pela Fase 05 para as abas
associadas ao projeto.

---

## 2. Escopo

### 2.1. Incluído nesta fase

**Backend (crate `mt-projects` + migration em `mt-db` + comandos no crate `maxterminal`)**
- Tabelas `projects`, `project_roots`, `project_root_entries` e `project_discovery_dismissed` (DDL em `spec.md` §5).
- CRUD de projetos: adicionar (pasta individual), editar (título, ícone, cor), remover (somente do cadastro —
  nunca apaga arquivos do disco), reordenar, re-detectar identidade.
- CRUD de "Pastas de Projetos" (`project_roots`) com modo de descoberta por raiz: `suggest` (padrão) ou
  `auto_import`; habilitar/desabilitar monitoramento; re-varredura manual.
- Importação das subpastas de nível 1 de uma raiz (filtrando ocultas, arquivos, links quebrados e pastas já
  cadastradas ou dispensadas).
- Auto-descoberta com `notify` + `notify-debouncer-full`: debounce de 500 ms, `RecursiveMode::NonRecursive`
  (somente nível 1), ignorar entradas ocultas (prefixo `.`), janela de "assentamento" de 1,5 s antes de detectar a
  stack (cobre `git clone` em andamento).
- Eventos `project://discovered`, `project://removed`, `project://changed`, `project://root-changed`.
- Auto-detecção de stack (Laravel, Vue, Tauri, Extensão VS Code, Python, Rust) com **prioridade determinística**
  quando múltiplas stacks coexistem, mapa stack → ícone Iconify + cor (inspirado no `max-big-icon-pack`) e
  **fallback aleatório determinístico** (hash FNV-1a do caminho canônico).
- Expansão de `~`, canonicalização de caminhos, validação de título/ícone/cor.
- Ponto de extensão `ProjectRemovalHook` (Fase 07 encerra abas/PTYs antes da remoção).
- `ProjectInsertContributor` transacional interno a `mt-db` e lease `acquire_activity` definidos nesta fase, vazios/sem
  consumidores na entrega isolada; a Fase 07 os ativa para template padrão e abertura de abas.
- Reconciliação inicial, periódica (30 s), no foco e manual; recuperação de overflow, raiz indisponível e
  renames; distinção entre ausência confirmada e falha de leitura. Aliases de symlink e raízes sobrepostas
  compartilham um único projeto por caminho canônico, preservando as origens separadamente.
- Snapshot consistente com `epoch/revision`; eventos pós-commit invalidam o cache e não substituem uma
  reconciliação. Registro das settings de seleção/layout por `mt_projects::register_db` na extensão da Fase 03.

**Frontend**
- `AppLayout.vue` com `splitpanes` (coluna esquerda/direita), tamanho persistido em settings com debounce.
- `ProjectList.vue`, `ProjectItem.vue`, `ProjectStatusBadges.vue`, `ProjectDiscoveryPanel.vue`,
  `ProjectAddModal.vue`, `ProjectRootModal.vue`, `ProjectEditModal.vue`, `ProjectEmptyState.vue`,
  `ProjectWorkspace.vue` (placeholder).
- Store `useProjectsStore` (projetos, raízes, sugestões, seleção, vínculo aba→projeto, contadores agregados).
- Composables `useProjectEvents`, `useProjectTabFocus`, `useLayoutPersistence`, `useProjectBadgeRegistry`.
  `useProjectTabFocus` adapta seleção/apresentação ao `useTabSeenTracker` **já pertencente à Fase 05**.
- Wrappers IPC tipados `src/ipc/projects.ts` sobre `src/types/bindings.ts` (gerado por `tauri-specta`).
- Badges 1–5 com tokens `--max-success-500`, `--max-info-500`, `--max-warning-500`, `--max-danger-500` e cinza
  neutro, agregados a partir de `useTabStatusStore` (Fase 05).
- Regra de "visualizado": delegada à Fase 05, com janela visível/focada, painel efetivamente apresentado e
  ACK da execução/revisão renderizada. Foco sozinho não confirma conclusões. Badges consomem `badgeBucket`
  agregado por aba; não recontam painéis nem alteram `seen` diretamente.
- **Slot de extensão** de badges (registry ordenado) para o badge 🟣 Issues.

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| Badge 🟣 **Issues Abertas** e aba fixa de Issues (somente o slot de extensão nasce aqui) | 16 |
| Conteúdo real da coluna direita: barra de abas, "Adicionar nova aba", "Iniciar Aba Template", Templates, Split Panes, Sessões | 07 |
| Criação/encerramento de abas e chamada de `attachTab`/`setActiveTab` deste store | 07 |
| Indicadores Git/Worktree e Portas Ativas | 08 |
| Notificação nativa / tray ao surgir sugestão ou erro de aba; destaque de projeto com agente aguardando input | 09 e 13 |
| Sincronização de projeto ativo com o MaxCode (`maxterminal://`) | 11 |
| Temas e importador Tabby (as cores de projeto apenas usam tokens/hex) | 10 |
| Busca global FTS5 envolvendo projetos | 17 |
| Máquina de estados `TabStatus` e campo `seen` (apenas consumidos aqui) | 05 |

**Invariante por estágio:** nesta fase, cadastro válido não exige template e a coluna direita é um placeholder.
A obrigação "todo projeto tem um Template Padrão" será ativada pela Fase 07: migration/backfill idempotente
dos projetos preexistentes e integração de criação para os posteriores, antes de habilitar abertura de abas.
Não criar tabela de templates, FK para tabela futura, stub IPC ou chamada a serviço da 07 nesta fase.

---

## 3. Pré-requisitos

### 3.1. Fases
- **Fase 01**: workspace Cargo, crate `mt-core` (`ProjectId`, `MtError`, Uuid v7), `tauri-specta` gerando
  `src/types/bindings.ts`, frontend Vue 3.6 + UnoCSS + `@maxvue/max-components-ui` (`MaxIcon`, `MaxModal`,
  `MaxButton`), Pinia (`@maxvue/max-pinia`), Vitest, `tsgo`, ESLint, dependência `splitpanes` `4.1.2` (já instalada na Fase 01; não instalar de novo nem adicionar `@types/splitpanes`, pois o pacote já é tipado).
- **Fase 03**: repositórios tipados sobre `Db`; `Db::read`/`Db::write` são internos a `mt-db`. `DbSetup`,
  `SettingDescriptor`, `SettingsSchema`, `useSetting` e wrappers `getSetting/setSetting` em `src/ipc/db.ts`.
  Esta fase adiciona `mt-db/src/repos/projects.rs`, sua migration `0003_06_projects.sql` e registra suas
  settings antes de `Db::open`; não torna público o executor SQL genérico.
- **Fase 05**: `useTabStatusStore.byTab`, `countsFor`, `setFocus`, snapshots com `revision` e `badgeBucket`,
  `SeenRequest` com ExecutionRef/revisão e `useTabSeenTracker` como proprietário único do ACK de visualização.

> Não inventar `Db::with_conn`, `tabStatusStore.entries()` ou outro rastreador de `seen`. A implementação deve
> consumir os contratos acima; incompatibilidades observadas nas bases devem ser reportadas, não remendadas
> com dependências de fases futuras. A Fase 06 não preenche `sessions.project_id`; o vínculo persistente de
> sessões/abas é responsabilidade da ativação da Fase 07.

### 3.2. Ferramentas
- Rust estável ≥ 1.85, `cargo clippy`, `cargo nextest` (opcional).
- Node ≥ 22, gerenciador definido na Fase 01, `tsgo`.
- Linux com inotify; disponibilidade de watches deve ser tratada em runtime, sem presumir limite do host.
- Novas dependências Rust em `mt-projects`: `notify = { workspace = true }` e `notify-debouncer-full = { workspace = true }` (versões 8 / 0.5 centralizadas em `[workspace.dependencies]` pela Fase 01), `edition.workspace = true`, `serde_json`,
  `dirs`, `async-trait`, `tempfile` (dev), `tokio` (já no workspace). SQL usa `rusqlite` já presente em `mt-db`.
- Novas dependências frontend: `@tauri-apps/plugin-dialog` (seletor de pasta) caso a Fase 01 não o tenha incluído;
  coleções offline `@iconify-json/logos` e `@iconify-json/mdi` para os ícones do mapa de stacks.

---

## 4. Entregáveis

### 4.1. Backend
| Caminho | Conteúdo |
|---------|----------|
| `src-tauri/crates/mt-projects/Cargo.toml` | Manifesto do crate |
| `src-tauri/crates/mt-projects/src/lib.rs` | Reexports públicos |
| `src-tauri/crates/mt-projects/src/model.rs` | `Project`, `ProjectRoot`, `ProjectRootId`, `StackKind`, `ProjectIdentity`, `DiscoveryMode`, DTOs |
| `src-tauri/crates/mt-projects/src/paths.rs` | Expansão de `~`, canonicalização, filtro de ocultas |
| `src-tauri/crates/mt-projects/src/detect.rs` | `StackDetector`, regras e prioridade |
| `src-tauri/crates/mt-projects/src/identity.rs` | Mapa stack → ícone/cor, fallback FNV-1a, validações |
| `src-tauri/crates/mt-projects/src/store.rs` | `ProjectStore`, mutações de domínio e `SqliteProjectStore` (adaptador do repo tipado em `mt-db`) |
| `src-tauri/crates/mt-db/src/repos/projects.rs` | Repo tipado, rows de persistência e contributor transacional definidos pela 06 dentro de mt-db |
| `src-tauri/crates/mt-projects/src/scanner.rs` | Varredura de nível 1 e diff |
| `src-tauri/crates/mt-projects/src/watcher.rs` | `RootWatcher` (notify + debounce + assentamento) |
| `src-tauri/crates/mt-projects/src/service.rs` | `ProjectService` (orquestração, hooks, eventos) |
| `src-tauri/crates/mt-projects/src/events.rs` | Payloads e trait `ProjectEventSink` |
| `src-tauri/crates/mt-projects/src/settings.rs` | `register_db`, defaults/validadores das duas settings da fase |
| `src-tauri/crates/mt-projects/tests/*.rs` | Testes de integração (detecção, store, watcher) |
| `src-tauri/crates/mt-db/migrations/0003_06_projects.sql` | DDL (número canônico `0003` da sequência contígua D-01) |
| `src-tauri/src/commands/projects.rs` | Comandos Tauri `project_*` e `project_root_*` |
| `src-tauri/src/events/projects.rs` | `TauriProjectEventSink` (implementa `ProjectEventSink` via `AppHandle::emit`) |

### 4.2. Frontend
| Caminho | Conteúdo |
|---------|----------|
| `src/ipc/projects.ts` | Wrappers tipados de comandos e `listen` dos eventos |
| `src/stores/projects.ts` | `useProjectsStore` |
| `src/composables/useProjectEvents.ts` | Assinatura dos eventos `project://*` |
| `src/composables/useProjectTabFocus.ts` | Liga a seleção ao tracker existente da Fase 05, sem duplicá-lo |
| `src/composables/useLayoutPersistence.ts` | Persistência do tamanho das colunas |
| `src/composables/useProjectBadgeRegistry.ts` | Slot de extensão de badges (Fase 16) |
| `src/components/layout/AppLayout.vue` | 2 colunas `splitpanes` |
| `src/components/projects/ProjectList.vue` | Lista, ordenação, cabeçalho com ações |
| `src/components/projects/ProjectItem.vue` | Item com `MaxIcon`, título, badges, menu de contexto |
| `src/components/projects/ProjectStatusBadges.vue` | Badges 1–5 + slots registrados |
| `src/components/projects/ProjectDiscoveryPanel.vue` | Sugestões de novas pastas (aceitar/dispensar) |
| `src/components/projects/ProjectAddModal.vue` | Adicionar projeto individual |
| `src/components/projects/ProjectRootModal.vue` | Adicionar/editar Pasta de Projetos |
| `src/components/projects/ProjectEditModal.vue` | Editar título/ícone/cor, re-detectar |
| `src/components/projects/ProjectEmptyState.vue` | Estado vazio |
| `src/components/projects/ProjectWorkspace.vue` | Placeholder da coluna direita (Fase 07) |
| `src/constants/projectIdentity.ts` | Espelho somente-leitura das sugestões de ícone por stack (para o seletor) |
| `src/types/projectSettings.ts` | Extensão de `SettingsSchema` com seleção e proporção das colunas |
| `tests/frontend/projects/*.spec.ts` | Testes Vitest |

---

## 5. Critérios de Aceite

### 5.1. Automatizados (devem passar no CI local)
1. `cargo test -p mt-projects` passa com a matriz de casos de `spec.md` §11, cobrindo: cada stack isolada; as combinações
   Tauri+Vue+Rust → `tauri`, Laravel+Vue → `laravel`, VS Code+Vue → `vscode_extension`, Rust+Python → `rust`;
   pasta vazia → `unknown`; `package.json` inválido ou > 1 MiB não causa pânico e é tratado como ausente.
2. Fallback tem vetores de hash fixos e preserva ícone/cor após persistência, restart e atualização do mapa de
   ícones; edição manual não é sobrescrita. Testar preview que não transforma identidade automática em manual.
3. Watcher (tempdir): criação isolada em ≤ 3 s no ambiente local nominal; rajada de 20 pastas produz exatamente
   20 candidatos únicos, independentemente do número de callbacks do SO. Ocultas/arquivos/subníveis não geram
   candidatos extras. Teto de agendamento de 5 s sob rajadas; overflow e evento perdido convergem na reconciliação
   de 30 s + tempo de scan. Ausência real marca `missing`, erro de permissão não marca.
4. Teste do store: `path` duplicado retorna `MtError::new(ErrorCode("project.duplicate_path"), "…")` (código `dominio.motivo` minúsculo, D-07); `project_reorder`
   é transacional (falha no meio não altera nenhuma ordem); remover raiz mantém projetos com `root_id = NULL`.
5. `cargo clippy -p mt-projects -p maxterminal -- -D warnings` sem avisos.
6. Bindings regenerados contêm DTOs, snapshot e envelopes de eventos; não há duplicação manual de tipos de
   domínio fora de `src/types/bindings.ts`. Fixture de serialização confirma camelCase e enums snake_case.
7. Typecheck `tsgo --noEmit` e lint (ESLint + Stylelint) sem erros.
8. Vitest (`tests/frontend/projects`) passa cobrindo: agregação de contadores (incluindo `finished`/`error` com
   `seen = true` contando conforme `badgeBucket` da 05); ACK da execução/revisão apresentada, janela oculta
   não marca, perda de foco cancela, ACK antigo não confirma nova execução;
   persistência de layout com debounce (1 gravação para 10 eventos `resized`); badge com valor 0 não renderiza;
   provider registrado no registry aparece após os 5 badges nativos na ordem informada.
9. Casos obrigatórios de integridade: dois aliases e duas raízes para o mesmo destino → um projeto;
   remover alias não marca destino existente como ausente; symlink redirecionado não muda `Project.id/path`;
   rename de projeto mantém cadastro anterior ausente e oferece o novo caminho; scan parcial não exclui origens.
10. Concorrência: aceitar sugestão × auto-import × cadastro manual não duplica caminho; falha no lote não emite
    sucesso parcial; remoção + dispensa são uma transação; rescan atrasado não ressuscita raiz removida/desabilitada.
11. Hidratação: evento durante snapshot, resposta antiga após exclusão e listener concluído após unmount não
    deixam estado obsoleto nem listeners vivos. Soma dos 5 buckets = abas vinculadas com snapshot conhecido;
    múltiplos painéis e badge de Issues não alteram essa soma.
12. Migration em banco contendo somente 03+05 funciona sem 07/16; `foreign_key_check` vazio, rollback em falha,
    reopen idempotente, settings válidas aceitas e inválidas recusadas. Cadastro/remoção/seleção funcionam sem
    chamar templates, tabs ou GitHub.

### 5.2. Comportamento observável (validação manual no app)
13. Adicionar uma pasta de fixtures com "Importar subpastas existentes agora" importa cada subpasta elegível;
    fixture Tauri aparece com ícone `logos:tauri`, Laravel com `logos:laravel`, etc.
14. Criar uma subpasta de teste na raiz de fixtures com o app aberto: em modo `suggest` aparece no painel em ≤ 3 s;
    em modo `auto_import` aparece diretamente na lista. "Dispensar" impede que a pasta volte a ser sugerida após
    reiniciar o app.
15. Mover a subpasta de teste para fora da raiz marca seu cadastro antigo como "pasta ausente" sem removê-lo;
    mover de volta restaura o mesmo id. Realizar o roteiro somente em fixtures descartáveis.
16. Redimensionar o divisor das colunas, fechar e reabrir o app: o tamanho é restaurado (tolerância ±1 %); limites
    de 15 %–45 % para a coluna esquerda são respeitados.
17. Com uma aba simulada via store da 05 em erro não visto, o badge vermelho mostra `1`; selecionar apenas o
    projeto não basta. Tornar a aba ativa e apresentar sua conclusão com janela visível/focada aciona ACK da
    execução/revisão pelo tracker da 05 e atualiza o badge somente após confirmação.
18. Textos/aria em pt-BR; seleção, reordenação, divisor e modais operáveis por teclado, foco restaurado e sem
    atalhos disparando dentro de inputs. Raiz indisponível oferece "Tentar novamente" e status compreensível.
19. Remover projeto ou raiz nunca apaga conteúdo no disco; todos os testes verificam isso.

---

## 6. Riscos e Mitigação

| Risco | Impacto | Mitigação |
|-------|---------|-----------|
| Rajadas de eventos inotify (`git clone`, `npm install` em pasta irmã) | CPU/IPC alto, sugestões duplicadas | `NonRecursive`, debounce 500 ms, diff por varredura do nível 1 em vez de interpretar cada evento, coalescência por raiz |
| Pasta criada vazia e populada depois (clone) | Stack detectada como `unknown` | Janela de assentamento de 1,5 s + re-detecção automática na próxima varredura enquanto `identity_source = 'auto'` e `stack = 'unknown'`; comando `project_redetect` |
| Limite de inotify esgotado / raiz em FS sem inotify (NFS, FUSE) | Watcher falha | Erro `project_root.watch_failed` não fatal, `watch_status = 'error'` exibido na UI, fallback para re-varredura manual e no foco da janela |
| Raiz removida/desmontada | Eventos órfãos | Watcher observa remoção da própria raiz → `watch_status = 'missing'`, re-tentativa com backoff (5 s → 60 s) |
| Detecção ambígua de stack | Ícone incoerente | Tabela de prioridade fixa e testada; edição manual muda `identity_source` para `manual` e nunca é sobrescrita |
| Contratos de Fase 03/05 | Retrabalho/contagem divergente | Repo tipado em mt-db (SQL genérico privado), settings da 03 e `badgeBucket`/ACK da 05 (§3.1) |
| `package.json` gigante ou malicioso | Lentidão/DoS | Limite de 1 MiB, leitura somente de arquivos regulares, nenhuma execução de processo na pasta |
| Re-render da lista a cada mudança de status | Jank com muitas abas | Contadores em um único `computed`; compartilhar objetos apenas quando todos os cinco valores forem iguais; sem `v-memo` que congele providers externos |
| Ícones Iconify buscados online | Latência/privacidade | Coleções offline (`@iconify-json/*`); ícone inválido cai para `mdi:folder` |
| Remoção de projeto com abas vivas | Processos órfãos (Diretriz §4) | `ProjectRemovalHook` executado antes do `DELETE`; Fase 07 registra o encerramento de PTYs |
