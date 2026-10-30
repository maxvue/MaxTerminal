# Implementação 05 — Plano de Execução Passo a Passo (`plan.md`)

> Parte 05 de 05 da **Fase 06: Projetos e Layout de 2 Colunas**.
> Especificação técnica correspondente: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Implementação

### Passo 1 — Implementar `ProjectStatusBadges.vue` e `ProjectItem.vue`
1. Criar `src/components/projects/ProjectStatusBadges.vue` seguindo a ordem de blocos `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>` e tags de abertura em linha única:
   - Renderizar os 5 badges nativos (`running`, `continuous`, `finished`, `error`, `idle`) ocultando valores `<= 0`.
   - Consumir `useProjectBadgeRegistry()` para renderizar providers externos (`order >= 60`) após os nativos quando `count(projectId)` retornar inteiro positivo.
   - Aplicar `font-variant-numeric: tabular-nums`, `border-radius: 9999px` e `aria-label` descritivo em pt-BR.
2. Criar `src/components/projects/ProjectItem.vue`:
   - Renderizar `MaxIcon` com `resolveSafeProjectIcon(project.icon)` sobre fundo `color-mix(in srgb, var(--project-color) 15%, transparent)`.
   - Exibir título truncado, tooltip com `project.path`, indicador `"Pasta não encontrada no disco"` quando `project.missing === true`, `<ProjectStatusBadges>` e menu de ações (`Editar projeto`, `Re-detectar identidade`, `Remover do MaxTerminal` com `MaxPopoverConfirm`).

### Passo 2 — Implementar `ProjectDiscoveryPanel.vue` e `ProjectEmptyState.vue`
1. Criar `src/components/projects/ProjectDiscoveryPanel.vue` com lista colapsável `"Novas pastas encontradas (N)"`, preview de ícone/cor, botões `"Adicionar"`, `"Dispensar"` e `"Adicionar todas"`.
2. Criar `src/components/projects/ProjectEmptyState.vue` com mensagem `"Nenhum projeto cadastrado"` e botões de ação rápida para `"Adicionar projeto"` e `"Adicionar pasta de projetos"`.

### Passo 3 — Implementar os 3 Modais (`ProjectAddModal.vue`, `ProjectRootModal.vue`, `ProjectEditModal.vue`)
1. Criar `src/components/projects/ProjectAddModal.vue`:
   - Integrar `pickFolder` e `projectPreviewIdentity` com debounce de `250 ms` e token de requisição para descartar respostas fora de ordem.
   - Rastrear `titleTouched`, `iconTouched` e `colorTouched` separadamente para enviar `icon: undefined` e `color: undefined` quando não editados manualmente pelo usuário.
   - Exibir erros do backend (`project.duplicate_path`, `project.path_forbidden`, etc.) inline em pt-BR e bloquear submissão duplicada (`submitting`).
2. Criar `src/components/projects/ProjectRootModal.vue`:
   - Suportar criação e edição de `ProjectRoot`, ocultando o checkbox `"Importar subpastas existentes agora"` quando `discoveryMode === 'auto_import'`, e oferecendo `"Varrer agora"` e `"Remover pasta de projetos"` no modo edição.
3. Criar `src/components/projects/ProjectEditModal.vue`:
   - Permitir editar título, ícone (com sugestões de `STACK_IDENTITY_SUGGESTIONS` e `FALLBACK_PROJECT_ICONS`), cor (color picker + `FALLBACK_PROJECT_COLORS`) e botão `"Restaurar identidade automática"` (`resetIdentity: true`).

### Passo 4 — Implementar `ProjectList.vue`, `ProjectWorkspace.vue`, `AppLayout.vue` e Integrar em `App.vue`
1. Criar `src/components/projects/ProjectList.vue`:
   - Montar cabeçalho com ações, indicadores de status de raízes (incluindo botão `"Tentar novamente"` quando `watchStatus === 'error'` ou `'missing'`), `<ProjectDiscoveryPanel>`, lista de `<ProjectItem>` com HTML5 Drag-and-Drop e atalhos `↑/↓/Enter` e `Alt+↑/↓` (ignorados quando o foco estiver em inputs), `<ProjectEmptyState>` e os 3 modais.
2. Criar `src/components/projects/ProjectWorkspace.vue`:
   - Implementar estados para `project === null` (`"Selecione um projeto"`), projeto selecionado e projeto `missing`, expondo `<slot />` para a Fase 07.
3. Criar `src/components/layout/AppLayout.vue`:
   - Configurar `<Splitpanes class="mt-columns" :horizontal="false" :dbl-click-splitter="false" @resized="onResized">` com `<Pane :size="leftSize" :min-size="15" :max-size="45">` e `<Pane :size="100 - leftSize">`, controle de divisor acessível por teclado e inicialização de `provideProjectBadgeRegistry`, `useProjectEvents`, `useProjectTabFocus` e `useLayoutPersistence`.
4. Atualizar `src/app/App.vue` para renderizar `<AppLayout />`.

### Passo 5 — Escrever Testes de Componentes Vitest e Executar Validação Final da Fase 06
1. Criar `tests/frontend/projects/ProjectStatusBadges.spec.ts`:
   - Verificar que contadores com valor `0` não são renderizados; verificar ordem exata dos 5 badges, variáveis CSS semânticas, `aria-label` em pt-BR, renderização de provider registrado no `useProjectBadgeRegistry` (`order: 60`) após os nativos, atualização reativa do provider sem mutar o projeto, funcionamento do `dispose()` e rejeição de ID duplicado.
2. Criar `tests/frontend/projects/ProjectItem.spec.ts`:
   - Verificar passagem de `project.icon` ao `MaxIcon` (e fallback para `mdi:folder` quando inválido), renderização do estado `missing` e emissão de `select`, `edit`, `redetect` e `remove`.
3. Criar `tests/frontend/projects/ProjectAddModal.spec.ts`:
   - Verificar preview com debounce de 250 ms, descarte de resposta fora de ordem ao trocar caminho rápido, cancelamento de `pickFolder` retornando `null`, preservação de `icon: undefined` e `color: undefined` quando não editados manualmente, exibição inline de `project.duplicate_path` e bloqueio de double-click no botão de salvar.
4. Criar `tests/frontend/projects/ProjectList.spec.ts`:
   - Verificar renderização do `ProjectEmptyState`, abertura dos modais, aceitação/dispensa no `ProjectDiscoveryPanel`, navegação por teclado, reordenação via `Alt+↑/↓` e renderização do `AppLayout.vue` com `ProjectWorkspace.vue` reagindo à seleção.

---

## 2. Comandos de Verificação Completa (Backend + Frontend)

```bash
cd "$(git rev-parse --show-toplevel)"

# 1. Verificação completa do Backend Rust
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db -p mt-projects -p maxterminal
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-db -p mt-projects -p maxterminal --all-targets -- -D warnings
! rg -n "process::Command" src-tauri/crates/mt-projects/src

# 2. Verificação de tipos TypeScript e Vue
npm run typecheck

# 3. Verificação de Lint (ESLint + Stylelint)
npm run lint

# 4. Execução de todos os testes Vitest da Fase 06
npx --no-install vitest run tests/frontend/projects/

# 5. Build de produção do frontend
npm run build
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 06)

- **CA-7 (Completo):** `tsgo --noEmit` e lint (ESLint + Stylelint) passam sem nenhum erro em todos os componentes Vue e arquivos TS.
- **CA-8 (Completo):** Toda a suíte Vitest em `tests/frontend/projects/*.spec.ts` passa cobrindo stores, composables, badges, registry de extensão, itens, lista, layout e modais.
- **CA-13 a CA-18 (Completos):**
  - Importação de Pasta de Projetos e exibição de ícones/cores por stack (`logos:tauri`, `logos:laravel`, etc.);
  - Painel de sugestões em modo `suggest`, importação direta em `auto_import` e persistência de dispensa;
  - Indicação visual de `"Pasta não encontrada no disco"` (`missing`) e restauração mantendo o mesmo `id`;
  - Persistência e restauração da proporção das colunas (`15%..45%`) no `splitpanes`;
  - Atualização do badge de erro/finalizado somente após ACK real da conclusão via `useTabSeenTracker` da Fase 05;
  - Textos e atributos `aria-*` 100% em `pt-BR`, navegação e reordenação operáveis por teclado sem colisão com inputs e restauração de foco nos modais.
