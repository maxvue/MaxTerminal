# Implementação 04 — Plano de Execução Passo a Passo (`plan.md`)

> Parte 04 de 05 da **Fase 06: Projetos e Layout de 2 Colunas**.
> Especificação técnica correspondente: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Implementação

### Passo 1 — Configurar Dependências, Tipagem de Settings e Constantes de Identidade
1. Verificar em `package.json` a presença de `@tauri-apps/plugin-dialog`, `@iconify-json/logos` e `@iconify-json/mdi` (adicionando-as caso a Fase 01 não as tenha incluído).
2. Criar `src/types/projectSettings.ts` estendendo `SettingsSchema` da Fase 03 com `'projects.selected_id': ProjectId | null` e `'layout.columns.left_size': number`.
3. Criar `src/constants/projectIdentity.ts`:
   - Exportar `STACK_IDENTITY_SUGGESTIONS`, `FALLBACK_PROJECT_ICONS`, `FALLBACK_PROJECT_COLORS`, `DEFAULT_PROJECT_ICON`, `ICONIFY_NAME_REGEX` e `HEX_COLOR_REGEX`.
   - Implementar `ensureProjectIconsRegistered()` registrando offline os ícones necessários de `@iconify-json/logos` e `@iconify-json/mdi`, e `resolveSafeProjectIcon(icon)` caindo para `'mdi:folder'` se o ícone for inválido ou não registrado.

### Passo 2 — Implementar Wrappers IPC (`src/ipc/projects.ts`) e Registry de Badges (`src/composables/useProjectBadgeRegistry.ts`)
1. Criar `src/ipc/projects.ts` encapsulando todos os 17 comandos de `commands.*` (gerados em `src/types/bindings.ts`) com `unwrap` de `src/ipc/core.ts`, os 4 listeners (`onProjectDiscovered`, `onProjectRemoved`, `onProjectChanged`, `onProjectRootChanged`) e `pickFolder(title)`.
2. Criar `src/composables/useProjectBadgeRegistry.ts` implementando `createProjectBadgeRegistry`, `provideProjectBadgeRegistry` e `useProjectBadgeRegistry`:
   - Validar `id` não vazio e único, `order >= 60` finito, ordenação por `(order, id)` e disposer idempotente integrado a `onScopeDispose`.

### Passo 3 — Implementar Store `useProjectsStore` (`src/stores/projects.ts`)
1. Implementar o state reativo (`projects`, `roots`, `discovered`, `selectedId`, `tabOwner`, `activeTabByProject`, `loading`, `error`, `epoch`, `revision`).
2. Implementar os getters `selectedProject`, `projectById`, `discoveredByRoot`, `countersByProject` (com reutilização de referência quando os 5 campos `running`, `continuous`, `finished`, `error`, `idle` não mudarem) e `countersFor` (retornando `ZERO_PROJECT_COUNTERS` congelado quando ausente).
3. Implementar `applySnapshot`, `invalidate`, `resync` (single-flight + repetição automática se `revision` mais nova foi sinalizada durante o voo) e `init`.
4. Implementar `select(id)` persistindo `'projects.selected_id'` com debounce de 200 ms via `setSetting` da Fase 03 (sem gravar default durante `loading`).
5. Implementar as actions de mutação (`addProject`, `updateProject`, `removeProject`, `reorder`, `redetect`, `addRoot`, `updateRoot`, `removeRoot`, `rescanRoot`, `acceptDiscovered`, `dismissDiscovered`), aguardando o comando IPC + `resync()` e tratando o caso em que o comando commitou mas o `resync()` falhou (`"Alteração salva; atualização da lista pendente"`).
6. Implementar os pontos de extensão para a Fase 07 (`attachTab`, `detachTab`, `setActiveTab`).

### Passo 4 — Implementar Composables `useProjectEvents`, `useProjectTabFocus` e `useLayoutPersistence`
1. Criar `src/composables/useProjectEvents.ts`:
   - Registrar os 4 listeners antes do snapshot inicial, tratar falha parcial liberando listeners já criados e executar `unlisten()` imediato se resolver após unmount.
   - Coalescer `invalidate` em 50 ms para disparar `resync()`.
   - Configurar intervalo visível de 30 s e listeners de `focus` / `visibilitychange` com throttle de 2 s.
2. Criar `src/composables/useProjectTabFocus.ts`:
   - Integrar `selectedId` + `activeTabByProject` + `ProjectPresentationSource` ao `useTabSeenTracker` da Fase 05.
   - Garantir que foco sem conclusão renderizada não envia `SeenRequest` e que perda de foco/visibilidade cancela pendências imediatamente.
3. Criar `src/composables/useLayoutPersistence.ts`:
   - Integrar com `useSetting('layout.columns.left_size', 22, { debounceMs: 300 })`.
   - Implementar `onResized` compatível com o payload do `splitpanes`, clamp `15..45`, `ready` e `flush`.

### Passo 5 — Escrever Testes Unitários Vitest da Camada de Estado e Composables
1. Criar `tests/frontend/projects/projects.store.spec.ts`:
   - Testar `applySnapshot` idempotente, descarte de revisão antiga na mesma `epoch`, troca de `epoch`, e atualização de `selectedId` para o vizinho ao remover projeto.
   - Testar `reorder` rejeitado com `project.reorder_mismatch` recarregando o catálogo.
   - Testar `countersByProject` com `useTabStatusStore`: todas as combinações de `badgeBucket`, aba com 3 painéis contando apenas 1 vez para o projeto, soma dos 5 buckets igual ao número de abas vinculadas com snapshot conhecido, e estabilidade referencial dos objetos de contadores.
2. Criar `tests/frontend/projects/useProjectTabFocus.spec.ts`:
   - Testar com `useTabSeenTracker` da Fase 05: selecionar projeto sozinho não marca `seen`; apresentar conclusão com janela visível e focada envia `SeenRequest` exato; janela oculta, pane oculto ou perda de foco não confirmam; ACK de geração antiga é ignorado.
3. Criar `tests/frontend/projects/useProjectEvents.spec.ts`:
   - Testar chegada de evento durante `projectSnapshot` em voo (repete resync até alcançar a revisão), resposta antiga após exclusão e unmount durante registro assíncrono dos listeners.
4. Criar `tests/frontend/projects/useLayoutPersistence.spec.ts`:
   - Testar 10 eventos `@resized` seguidos resultando em apenas 1 gravação após o debounce de 300 ms, clamp nos limites `15..45`, payload inválido/NaN ignorado e `flush()`.

---

## 2. Comandos de Verificação

```bash
cd "$(git rev-parse --show-toplevel)"

# Verificar tipagem estrita TypeScript com tsgo
npm run typecheck

# Executar os testes unitários Vitest dos stores e composables de projetos
npx --no-install vitest run tests/frontend/projects/projects.store.spec.ts tests/frontend/projects/useProjectTabFocus.spec.ts tests/frontend/projects/useProjectEvents.spec.ts tests/frontend/projects/useLayoutPersistence.spec.ts

# Executar lint nos arquivos TypeScript criados
npm run lint
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 06)

- **CA-7 (Parcial):** `tsgo --noEmit` e ESLint passam sem erros para toda a camada de IPC, stores e composables.
- **CA-8 (Parcial — store e composables):** Testes Vitest de agregação de contadores, ACK via `useTabSeenTracker`, `useProjectEvents`, `useLayoutPersistence` e `useProjectBadgeRegistry` passam integralmente.
- **CA-11 (Completo):** Evento durante snapshot, resposta antiga após exclusão e listener concluído após unmount não deixam estado obsoleto nem listeners órfãos; soma dos 5 buckets = abas vinculadas com snapshot conhecido.
