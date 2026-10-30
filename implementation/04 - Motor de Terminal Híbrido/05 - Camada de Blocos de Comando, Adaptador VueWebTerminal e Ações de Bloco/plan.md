# Plano de Execução — Implementação 05: Camada de Blocos de Comando, Adaptador VueWebTerminal e Ações de Bloco

> Especificação de referência: [`./spec.md`](./spec.md)
> Fase 04: Motor de Terminal Híbrido

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Implementar e Validar `VueWebTerminalAdapter` (`src/terminal/blocks/vueWebTerminalAdapter.ts`)
1. Inspecionar os `.d.ts` e exports reais de `vue-web-terminal` instalado em `node_modules/vue-web-terminal`.
2. Criar `src/terminal/blocks/vueWebTerminalAdapter.ts` implementando `VueWebTerminalAdapter` e `createVueWebTerminalAdapter`:
   - Projetar `CommandBlockModel` para entradas de bloco compatíveis com `vue-web-terminal` (garantindo texto puro sem HTML).
   - Encaminhar qualquer evento de execução (`handleExecCmd`) ao callback `onInputIntent` conectado ao `InputPipeline`, sem criar segundo leitor de PTY ou eco duplicado.

### Passo 2: Implementar `BlockActionRegistry` e Ações Nativas (`src/terminal/blocks/blockActionRegistry.ts`)
1. Criar `src/terminal/blocks/blockActionRegistry.ts` implementando:
   - `registerBlockAction(action): Disposable`, `getBlockActionsFor(block)` (ordenado por `order asc` e filtrado por `action.when(block)`) e `executeBlockAction(actionId, block, ctx)` com tratamento de erro via `Toast.show({ title, severity: 'error' })`.
   - `extractRenderedBlockOutput(xterm, block, markers)` percorrendo `xterm.buffer.normal.getLine(i).translateToString(true)` entre os markers do bloco, unindo linhas com `isWrapped === true` sem nova quebra e sinalizando `partial: true` quando o marker inicial tiver sofrido trim.
   - Registro automático das 2 ações nativas: `'copy-command'` (`"Copiar comando"`) e `'copy-output'` (`"Copiar saída"`).

### Passo 3: Implementar `useCommandBlocksStore` (`src/stores/commandBlocks.ts`)
1. Criar `src/stores/commandBlocks.ts` com `useCommandBlocksStore`:
   - Implementar `feed: CommandBlockFeed` (`begin`, `markOutputStart`, `end`, `patchMeta`, `clear`).
   - Manter os objetos `IMarker` do xterm em uma estrutura não-reativa de módulo (`Map<PaneId, Map<CommandBlockId, BlockMarkersRecord>>`), assinando `marker.onDispose` para remover blocos descartados pelo scrollback (`pruneEvicted`).
   - Garantir idempotência por `CommandBlockId`, ignorar `end` sem `begin` correspondente com `console.warn` e impor o teto de `maxBlocksPerPane = 500`.

### Passo 4: Implementar Composable `useCommandBlocks` e Componentes `CommandBlock.vue` e `CommandBlockLayer.vue`
1. Criar `src/composables/terminal/useCommandBlocks.ts` com assinatura `useCommandBlocks(paneId, { getContext, onInputIntent })`: importar `createVueWebTerminalAdapter` de `@/terminal/blocks/vueWebTerminalAdapter`, criar um adaptador por painel, expor `adapter` e `projectedEntries` (`computed(() => adapter.projectBlocks(blocks.value))`) e chamar `adapter.dispose()` em `onScopeDispose`. Este é o único ponto de instanciação do adaptador em produção.
2. Criar `src/components/terminal/CommandBlock.vue`:
   - Receber `block` + `entry` (`VueWebTerminalProjectedEntry`) e renderizar badge de status com tokens `--max-success-500`, `--max-danger-500` e `--max-warning-500`.
   - Exibir comando (truncado a 4096 caracteres), `cwd`, duração em `pt-BR`, botões de ações registradas e `<slot name="actions" :block="block" />`.
3. Criar `src/components/terminal/CommandBlockLayer.vue`:
   - Gerenciar decorações do xterm (`xterm.registerDecoration`) apenas para blocos no **viewport ± 1 tela** reagindo a `xterm.onRender`.
   - Obter `blocks` e `projectedEntries` de `useCommandBlocks(paneId, ...)` e renderizar `CommandBlock.vue` (passando `block` e a `entry` correspondente) via `<Teleport>` dentro do elemento DOM de cada decoração ativa.
   - Receber `getContext` e `onInputIntent` como props do `HybridTerminal` e ser a única chamada do composable por painel; renderizar apenas pares `block`/`entry` válidos. Manter destinos do Teleport dentro do `.hybrid-terminal` correspondente para preservar o tema local.
   - Ocultar toda a camada quando `altScreen === true`, preservando os markers no buffer normal.

### Passo 5: Criar e Executar Testes Unitários, de Integração e de Componentes (`tests/terminal/`)
1. Criar `tests/terminal/vueWebTerminalAdapter.spec.ts` (usando o pacote real `vue-web-terminal`).
2. Criar `tests/terminal/blockActionRegistry.spec.ts`.
3. Criar `tests/terminal/commandBlocksStore.spec.ts`.
4. Criar `tests/terminal/CommandBlock.spec.ts`.
5. Criar `tests/terminal/useCommandBlocks.spec.ts` (adaptador instanciado 1x por painel, `projectedEntries` consistente com o feed e `dispose()` ao desmontar).
6. Confirmar por busca (`grep -rn "createVueWebTerminalAdapter" src/`) que o adaptador é importado por `useCommandBlocks.ts` (e não fica órfão) e, na `HybridTerminal.spec.ts` (`../06`), que a composição `HybridTerminal` → `CommandBlockLayer` → `useCommandBlocks` → `VueWebTerminalAdapter` projeta ao menos 1 bloco do feed simulado.
7. Não adicionar `overrides` de `vue-web-terminal` ao `package.json`; só registrar nota e adicionar o mínimo se `npm ci` provar conflito de peer.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificação de tipos TypeScript
npm run typecheck

# 2. Execução dos testes da Implementação 05
npx --no-install vitest run tests/terminal/vueWebTerminalAdapter.spec.ts tests/terminal/blockActionRegistry.spec.ts tests/terminal/commandBlocksStore.spec.ts tests/terminal/CommandBlock.spec.ts tests/terminal/useCommandBlocks.spec.ts

# 3. Verificação de lint (ESLint + Stylelint)
npm run lint
npm run lint:style
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 04)

- **[CA-5.1.3j — `CommandBlock.spec.ts`]** Renderiza estados `running`, `success`, `error` e `aborted`, duração formatada em `pt-BR`, passa `{ block }` ao slot `actions` e exibe ações do `BlockActionRegistry` filtradas por `when(block)`.
- **[CA-5.1.7 — `vueWebTerminalAdapter.spec.ts` / `useCommandBlocks.spec.ts`]** Exercita `vue-web-terminal` real na versão instalada: projeta blocos do `CommandBlockFeed`, comprova um único pipeline de entrada (`InputPipeline`) e garante que não há duplicação de execução, eco ou listeners de PTY. Além disso, `useCommandBlocks` instancia o adaptador (1 por painel) e `CommandBlockLayer` renderiza `CommandBlock` a partir de `projectedEntries`, provando que o adaptador faz parte do pipeline real.
- **[CA-5.2.12 — `commandBlocksStore.spec.ts` / `blockActionRegistry.spec.ts`]** Blocos ancorados via `IMarker` acompanham o scroll/reflow, ocultam-se em `altScreen` sem perder os markers do buffer normal, `"Copiar comando"` copia o comando bruto e `"Copiar saída"` copia o texto renderizado retido (tratando linhas `isWrapped` e informando quando houver trim parcial).
