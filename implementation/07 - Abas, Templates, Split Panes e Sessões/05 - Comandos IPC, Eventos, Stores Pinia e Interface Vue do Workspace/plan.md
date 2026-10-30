# Plano de Execução — Implementação 05: Comandos IPC, Eventos, Stores Pinia e Interface Vue do Workspace

## 1. Checklist Sequencial de Implementação

### Passo 1: Eventos, Comandos Tauri e Geração de Bindings TypeScript
1. Criar `src-tauri/src/events/workspace.rs` com `WorkspaceEvent<T>` e os payloads tipados (`TabClosedPayload`, `TabReorderedPayload`, `PanePtyReplacedPayload`, `PaneExitedPayload`, `TemplateChangedPayload`) e conectá-los ao `WorkspaceManager`.
2. Criar os módulos de comandos em `src-tauri/src/commands/`:
   - `templates.rs` (`template_list`, `template_get`, `template_create`, `template_update`, `template_delete`, `template_set_default`)
   - `tabs.rs` (`tab_list`, `tab_open`, `tab_close`, `tab_reorder`, `tab_focus`)
   - `panes.rs` (`pane_split`, `pane_close`, `pane_restart`, `pane_layout_update`)
   - `sessions.rs` (`session_new`, `session_list`, `session_get`, `session_scrollback_get`, `session_search`)
   - `workspace.rs` (`workspace_snapshot`, `workspace_initial_command_run`)
3. Registrar todos os 22 comandos e 10 eventos no builder `tauri_specta` em `src-tauri/src/lib.rs` e executar o teste/binário gerador para atualizar `src/types/bindings.ts` sem edições manuais.

### Passo 2: Wrappers IPC e Composables do Workspace
1. Criar `src/ipc/templates.ts`, `src/ipc/tabs.ts`, `src/ipc/sessions.ts` e `src/ipc/workspace.ts` encapsulando as chamadas `invoke` e assinaturas `listen` tipadas.
2. Criar `src/composables/workspace/layoutTree.ts` com funções puras `leaves(node)`, `findPane(node, paneId)` e `findNode(node, nodeId)`.
3. Criar `src/composables/workspace/usePaneLayout.ts` com o normalizador do evento `resized` do `splitpanes` (suportando formatos v3 e v4), debounce de 300 ms, coalescência por `nodeId` e tratamento de `layout.conflict`.
4. Criar `src/composables/workspace/useWorkspaceEvents.ts` (assinatura pré-snapshot, buffer ordenado por `eventSeq`, detecção de gap/`bootId` e cleanup seguro no `onScopeDispose`) e `src/composables/workspace/useActiveTabSeen.ts`.

### Passo 3: Stores Pinia (`templates.ts`, `tabs.ts`, `sessions.ts`)
1. Criar `src/stores/templates.ts` (`useTemplatesStore`) com cache por `ProjectId`, controle de `revision` e reação ao evento `template://changed`.
2. Criar `src/stores/tabs.ts` (`useTabsStore`) com `tabsById`, `orderByProject`, `tombstones` por revisão, integração com `useProjectsStore` (`attachTab`, `detachTab`, `setActiveTab`), `useTabStatusStore` e descarte de instâncias no `TerminalInstanceRegistry`.
3. Criar `src/stores/sessions.ts` (`useSessionsStore`) com cache paginado por aba, LRU de 3 sessões detalhadas com teto de bytes de scrollback e busca FTS5 com cancelamento lógico por token de requisição.

### Passo 4: Componentes Vue 3.6 de Abas, Templates, Split Panes e Histórico
1. Criar `src/components/workspace/TabBarItem.vue` e `src/components/workspace/TabBar.vue` com `role="tablist"`, roving tabindex, navegação/reordenação por teclado, slots `#pinned`, `#indicators` e `#actions`, e botões `"Adicionar nova aba"`, `"Iniciar Aba Template"`, `"Iniciar nova Sessão"` e `"Histórico de Sessões"`.
2. Criar `src/components/workspace/TemplateLaunchPopover.vue` e `src/components/workspace/TemplateEditorModal.vue` com formulário completo validado, pares de variáveis de ambiente (`Literal` / `Inherited`), aviso de resolvedor de agente e eleição de padrão.
3. Criar `src/components/workspace/PaneView.vue`, `src/components/workspace/PaneLayout.vue` (recursivo com `Splitpanes` `:horizontal="node.direction === 'horizontal'"`, `:min-size="5"` e keys estáveis) e `src/components/workspace/TabWorkspaceView.vue`.
4. Criar `src/components/workspace/SessionReadonlyViewer.vue` (somente leitura, sem `v-html` e sem PTY) e `src/components/workspace/SessionHistoryDrawer.vue`.
5. Atualizar `src/components/projects/ProjectWorkspace.vue` mantendo a prop `project: Project | null` da Fase 06 e conectar `useWorkspaceEvents()` e `useActiveTabSeen()` em `src/components/layout/AppLayout.vue`.

### Passo 5: Testes Frontend (Vitest) e Validação End-to-End da Fase 07
1. Escrever testes unitários e de componentes com Vitest cobrindo:
   - `useTabsStore`: chegada de evento antes/depois da resposta IPC, proteção por tombstone contra ressurreição de aba fechada, buffer de `useWorkspaceEvents` durante o snapshot inicial, ressincronização em gap de `eventSeq`, unmount antes de `listen()` resolver, e integração com `useProjectsStore` e badges da Fase 05/06.
   - `usePaneLayout`: normalização de payload `resized` nos formatos v3 (array) e v4 (`{ panes }`), coalescência de múltiplos eventos em 300 ms e recuperação em `layout.conflict`.
   - `TabBar`, `PaneLayout`, `PaneView`: navegação por teclado (`role="tablist"`), troca de aba preservando instância xterm (`visible=false`), substituição de xterm e limpeza de `CommandBlockFeed` ao receber nova `generation`/`PtyId` em `session_new`.
   - `SessionHistoryDrawer` e `SessionReadonlyViewer`: paginação de sessões/scrollback, filtro por painel, busca sanitizada e garantia de que nenhum escape ativo ou HTML é interpretado.

---

## 2. Comandos de Verificação

```bash
# 1. Verificação completa do Backend Rust e geração de bindings
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --workspace

# 2. Verificação de tipos TypeScript (tsgo), lint e testes Frontend (Vitest)
npm run check-types
npm run lint
npx --no-install vitest run src/stores/__tests__/templates.spec.ts src/stores/__tests__/tabs.spec.ts src/stores/__tests__/sessions.spec.ts src/composables/workspace/__tests__/ src/components/workspace/__tests__/
npm run build
```

---

## 3. Critérios de Aceite desta Implementação

- [ ] **CA-05.1 (Sincronização IPC, Eventos e Tombstones):** Respostas IPC atrasadas e eventos duplicados/fora de ordem não ressuscitam abas fechadas; o protocolo `listen` → buffer → `workspace_snapshot` → replay por `eventSeq` não perde eventos durante o boot; foco e seleção permanecem isolados por projeto.
- [ ] **CA-05.2 (Ciclo de Vida de Instâncias xterm e Stream):** Alternar entre abas apenas oculta/reanexa a view (`visible`) mantendo xterm e PTYs ativos; iniciar nova sessão (`session_new`) ou reiniciar painel descarta a instância xterm antiga e limpa o `CommandBlockFeed` antes de anexar o novo `PtyId`, mantendo uma única cadeia de ACK por PTY.
- [ ] **CA-05.3 (Splitpanes, Acessibilidade e Teclado):** `PaneLayout` normaliza o payload real de `resized` do `splitpanes`, envia `expectedLayoutRevision` e aciona `fit` da Fase 04; conflito de revisão recarrega o snapshot sem corromper outros nós; `tablist`, abas e divisores têm navegação completa por teclado.
- [ ] **CA-05.4 (Histórico Somente Leitura e Políticas 05/06):** `SessionHistoryDrawer` e `SessionReadonlyViewer` exibem sessões, blocos e scrollback paginados e filtráveis por painel sem interpretar escapes/HTML e sem enviar input a PTY; badges e `seen` usam exclusivamente a política integrada das Fases 05 e 06.
- [ ] **CA-05.5 (Qualidade Estática e Bindings sem Drift):** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, geração de `src/types/bindings.ts` sem drift, `npm run check-types` (`tsgo`), `npm run lint`, testes Vitest e `npm run build` passam sem erros.
