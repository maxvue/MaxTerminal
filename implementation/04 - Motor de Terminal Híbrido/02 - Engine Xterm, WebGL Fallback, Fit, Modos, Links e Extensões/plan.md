# Plano de Execução — Implementação 02: Engine Xterm, WebGL Fallback, Fit, Modos, Links e Extensões

> Especificação de referência: [`./spec.md`](./spec.md)
> Fase 04: Motor de Terminal Híbrido

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Implementar `RendererManager` (`src/terminal/engine/rendererManager.ts`)
1. Implementar a máquina de estados de renderização (`webgl` → `canvas` opcional → `dom`, `suspended`, `dom_locked`):
   - Envolver a criação e `xterm.loadAddon(webglAddon)` em `try/catch`.
   - Assinar `webglAddon.onContextLoss`: descartar o addon, chamar `xterm.refresh(0, xterm.rows - 1)`, contabilizar a perda na janela móvel de `60_000 ms` e, ao atingir 3 perdas, travar em `'dom'` (`lockedToDom = true`).
   - Implementar `setVisible(false)` / `suspend()` descartando o `WebglAddon` imediatamente em instâncias ocultas e restaurando-o em `setVisible(true)`.
   - Expor `forceContextLossForDebug()` para o botão de teste de perda de contexto do playground.

### Passo 2: Implementar `FitController` (`src/terminal/engine/fitController.ts`)
1. Implementar `FitController` com `ResizeObserver` no elemento host:
   - Aplicar debounce trailing de `50 ms` com `maxWait` de `100 ms`.
   - Ignorar host desconectado, oculto, `0×0` ou dimensões propostas `< 2 cols` / `< 1 row`.
   - Truncar dimensões em `1..=1000`.
   - Separar o estado `(xterm.cols, xterm.rows)` do estado confirmado pelo IPC `(confirmedCols, confirmedRows)` e serializar chamadas a `ptyClient.resize`.
   - Reagir também a `document.fonts?.ready` e chamadas explícitas `requestFit()`.

### Passo 3: Implementar `ModeTracker` (`src/terminal/engine/modeTracker.ts`)
1. Implementar `ModeTracker`:
   - Assinar `xterm.buffer.onBufferChange` para detectar imediatamente `active.type === 'alternate'` (`?1049h`/`?1049l`).
   - Implementar `sampleModes()` lendo `altScreen`, `mouseTrackingMode`, `bracketedPasteMode` e `applicationCursorKeysMode`, disparando `onModesChange` apenas quando houver diferença em relação ao estado anterior.
   - Assinar `xterm.onTitleChange`, removendo caracteres de controle e truncando em 256 caracteres antes de emitir `onTitleChange`.

### Passo 4: Implementar Registros de Contexto, Extensões e Links
1. Criar `src/terminal/extensions/terminalContext.ts` com `registerContextProvider` e `resolveTerminalContext`.
2. Criar `src/terminal/extensions/terminalExtensions.ts` com `registerTerminalExtension` e `attachExtensionsToInstance`, isolando falhas em `activate` com `try/catch` + `console.error`.
3. Criar `src/terminal/links/linkHandlerRegistry.ts` com `isSafeHttpUrl`, `hasActivationModifier`, `registerLinkHandler`, `dispatchTerminalLink` e `createOsc8LinkHandler`, garantindo allowlist `http:`/`https:` via `new URL()`, exigência de `Ctrl+Clique` e integração com `@tauri-apps/plugin-opener`.

### Passo 5: Implementar `TerminalInstance` e `TerminalInstanceRegistry`
1. Criar `src/terminal/engine/terminalInstance.ts`:
   - Instanciar `Terminal` do `@xterm/xterm` com as opções canônicas (`allowProposedApi: true`, `allowTransparency: false`, `scrollback`, `convertEol: false`, `rightClickSelectsWord: true`, `macOptionIsMeta: false`, `minimumContrastRatio: 1`, `drawBoldTextInBrightColors: true` e `linkHandler` OSC 8).
   - Carregar addons na ordem: `Unicode11Addon` (`unicode.activeVersion = '11'`) → `FitAddon` → `SearchAddon({ highlightLimit: 2000 })` → `WebLinksAddon`.
   - Conectar `WriteBatcher`, `FlowController`, `InputPipeline`, `RendererManager`, `FitController`, `ModeTracker` e `ptyStreamHub`.
   - Implementar `attach(host)` (reutilizando `innerContainer` em remontagens), `detach()` (suspendendo WebGL sem desconectar o stream), `setVisible(v)` e `dispose()` idempotente.
   - Implementar `applyTheme(theme)` atribuindo `xterm.options.theme = toXtermTheme(theme)` **e** chamando `callbacks.onThemeChange?.(theme)` (expondo `theme.ui`), mais `getTheme()`; cada `attach`, inclusive remontagem após troca de aba, notifica `onThemeChange` com o tema vigente.
2. Criar `src/terminal/engine/instanceRegistry.ts`:
   - Implementar `TerminalInstanceRegistry` sobre `Map<PaneId, TerminalInstance>` não reativa.
   - Validar conflito de `ptyId` para o mesmo `paneId` em `getOrCreate`.

### Passo 6: Criar e Executar Testes Unitários (`tests/terminal/`)
1. Expandir `tests/terminal/mocks/xtermMock.ts` caso necessário para simular `buffer.onBufferChange`, `onTitleChange`, `loadAddon` e `ResizeObserver`.
2. Criar os 5 arquivos de especificação de testes:
   - `tests/terminal/rendererManager.spec.ts`
   - `tests/terminal/fitController.spec.ts`
   - `tests/terminal/modeTracker.spec.ts`
   - `tests/terminal/linkHandlerRegistry.spec.ts`
   - `tests/terminal/instanceRegistry.spec.ts`

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificação de tipos TypeScript
npm run typecheck

# 2. Execução dos testes unitários da Implementação 02
npx --no-install vitest run tests/terminal/rendererManager.spec.ts tests/terminal/fitController.spec.ts tests/terminal/modeTracker.spec.ts tests/terminal/linkHandlerRegistry.spec.ts tests/terminal/instanceRegistry.spec.ts

# 3. Execução acumulada de todos os testes de terminal até aqui
npx --no-install vitest run tests/terminal

# 4. Lint
npm run lint
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 04)

- **[CA-5.1.3d — `fitController.spec.ts`]** 10 eventos de resize em 30 ms produzem exatamente **1** chamada a `pty_resize`; dimensões idênticas às já confirmadas produzem **0** chamadas; container `0×0` ou oculto produz **0** chamadas; falha transitória de IPC mantém resize pendente para a próxima tentativa.
- **[CA-5.1.3e — `rendererManager.spec.ts`]** Disparo de `onContextLoss` descarta o addon WebGL, aciona o fallback (`canvas` ou `dom`) e chama `xterm.refresh`; **3 perdas de contexto em 60 s** fixam o renderer em `'dom'` até recarregar; `setVisible(false)` descarta o addon WebGL para liberar contexto GPU.
- **[CA-5.1.2 / §15.1 — `modeTracker.spec.ts`]** Transições `?1049h`/`?1049l` (`buffer.onBufferChange`), mudanças de `mouseTrackingMode`, `bracketedPasteMode` e `applicationCursorKeysMode` emitem `onModesChange` sem duplicatas; títulos OSC 0/2 são truncados em 256 caracteres.
- **[CA-5.2.7 / §14 — `linkHandlerRegistry.spec.ts`]** URLs `http://` e `https://` válidas são ativadas somente com `Ctrl+Clique` (ou `Meta+Clique`); esquemas `javascript:`, `file://` e `data:` são bloqueados tanto no `WebLinksAddon` quanto no `linkHandler` OSC 8.
- **[CA-5.2.9 / §15.1 — `instanceRegistry.spec.ts`]** Troca de aba (`detach` seguido de `attach` em novo elemento host) preserva a mesma instância xterm e o scrollback sem recriar assinatura no `ptyStreamHub`; 100 ciclos de criação/`dispose` consecutivos retornam a contagem de instâncias e assinaturas ativas no hub exatamente a `0`.
