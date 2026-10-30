# Fase 04 — Motor de Terminal Híbrido

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) (§2 "Motor Híbrido de Terminal") e
> [`../README.md`](../README.md) (contratos compartilhados §2.1–§2.6).
> Especificação técnica detalhada: [`spec.md`](./spec.md).

---

## 1. Objetivo da Fase

Entregar o **motor de terminal do frontend**: o componente que renderiza um PTY real (criado pela
Fase 02) com `@xterm/xterm` acelerado por GPU, fiel a TUIs interativos (`claude`, `opencode`, `agy`,
`codex`, `vim`, `htop`), e a **camada híbrida estilo Warp** sobre ele: barra de **input rico**,
**registro extensível de gatilhos** (`>`, `>>`, `@`, `?`, `/`) com popup de autocompletar genérico e a
**estrutura visual de blocos de comando** com slot de ações por bloco.

A fase **não** decide quando um comando começa ou termina (isso é OSC 133, Fase 05) e **não** implementa
nenhum gatilho concreto (Fases 12, 14 e 15). Ela entrega a infraestrutura, os contratos TypeScript e os
pontos de extensão que essas fases preencherão.

Dependência direta: **02**, transitivamente **01**. O playground deve abrir e operar um shell real
sem módulos 05/07, sem projeto cadastrado e sem SQLite. Blocos são demonstrados por feed simulado;
o terminal continua funcional sem qualquer feed ou provider registrado.

---

## 2. Escopo

### 2.1. Incluído

1. **`TerminalView.vue`** — host do `@xterm/xterm`:
   - Renderer `@xterm/addon-webgl` com cadeia de fallback em perda de contexto WebGL
     (`webgl` → `canvas` quando compatível com a versão fixada do xterm → `dom`).
   - `@xterm/addon-fit` + `ResizeObserver` com debounce → `pty_resize` (deduplicado; ignora 0×0).
   - `@xterm/addon-search` com barra de busca (`TerminalSearchBar.vue`): próximo/anterior, diferenciar
     maiúsculas, palavra inteira, regex, contador `n de m`.
   - `@xterm/addon-web-links` com **registro de handlers de link** (handler padrão apenas `http`/`https`).
   - `@xterm/addon-unicode11` (`term.unicode.activeVersion = '11'`) para largura Unicode 11;
     clusters emoji ZWJ exigem validação específica, não garantia automática desse addon.
   - Fidelidade a TUIs: alt-screen, mouse tracking (X10/VT200/SGR), bracketed paste, true color
     (24 bits), `onBinary` para relatórios de mouse, foco/teclas especiais.
2. **Pipeline de saída**: `PtyStreamHub` (roteamento do `Channel` da Fase 02 por `PtyId`, decodificação dos
   frames binários e reanexação via `pty_attach` com replay), **write-batching por `requestAnimationFrame`**
   com orçamento por frame, um write em voo e **ACK cumulativo de Data + Replay** por `attachmentId`
   após parse; `Gap`/`End` não consomem crédito. Spawn é desanexado; Channel só existe no attach.
3. **Pipeline de entrada**: `onData`/`onBinary` → `pty_write`, coalescência por microtask, colar com
   bracketed paste quando o modo estiver ativo no aplicativo.
4. **Registro de instâncias** (`TerminalInstanceRegistry`): instâncias xterm sobrevivem à troca de aba
   (reanexadas ao DOM), WebGL liberado em instâncias ocultas para respeitar o limite de contextos.
5. **Camada de Input Rico** (`RichInput.vue`) com modos **`rich`** e **`passthrough`**, resolvidos
   automaticamente (alt-screen/mouse tracking ⇒ `passthrough`, acima de override) com escolha manual fora desses modos e
   **ponto de extensão de dicas de modo** (`InputModeHintProvider`) para as Fases 05/13.
6. **`TriggerRegistry`** extensível (match por prefixo mais longo, multi-etapa, seções, itens de ação
   fixados ao final) e **`CompletionPopup.vue`** genérico, navegável por setas, acessível (ARIA
   combobox/listbox).
7. **Camada de Blocos**: `CommandBlock.vue` (cabeçalho do bloco: comando, cwd, duração, status, slot de
   ações), `CommandBlockLayer.vue` (ancoragem via `registerMarker`/`registerDecoration` do xterm),
   store `useCommandBlocksStore` com a **interface `CommandBlockFeed`** que a Fase 05 preencherá, e
   `BlockActionRegistry` com duas ações nativas: "Copiar comando" e "Copiar saída" (texto renderizado
   ainda retido, sem promessa de transcrição completa após trim).
8. **Interface `TerminalTheme`** + tema padrão **Max Navy/Teal** derivado dos tokens `--max-*`
   (sem importador; ver Fase 10).
9. **Extensões de terminal** (`registerTerminalExtension`) para fases futuras acoplarem addons,
   handlers de parser e decorações sem editar o núcleo.
10. **Store `useTerminalStore`** (estado reativo serializável de cada painel) e playground de
    desenvolvimento `/dev/terminal` (apenas em `import.meta.env.DEV`).
11. **Integração `vue-web-terminal` via `VueWebTerminalAdapter`** na apresentação de blocos/comandos,
    com spike obrigatório de compatibilidade/composição e ADR (ver `spec.md` §3); xterm permanece o único emulador VT.

### 2.2. Fora de escopo (e para onde vai)

| Item | Fase responsável |
|------|------------------|
| Spawn/kill/supervisão do PTY, `/proc`, Channel no Rust, watermarks/pausa do leitor e ring de replay | 02 (contrato-base consumido) |
| Persistência de scrollback/comandos e sanitização | 03 (gravação disparada pela 05/07) |
| Parser OSC 133, hooks zsh/bash/fish, início/fim/exit code de blocos, `TabStatus` | 05 |
| Aba, template, split panes, sessões, "Iniciar nova Sessão", botão "Reiniciar" processo | 07 |
| Indicadores Git/Worktree e detector de portas na barra da aba | 08 |
| Links de arquivo `caminho:linha` → editor, notificações nativas, tray, Quake | 09 |
| Importador de temas Tabby e persistência de preferências de aparência | 10 |
| Botão "Inspecionar" (ação de bloco) e integração MaxCode | 11 |
| Gatilhos `@` e `?`, Diagnóstico Rápido Inline (ação de bloco) | 12 |
| Detecção de agente CLI em primeiro plano, alerta de permissão, StatusLine | 13 |
| Gatilhos `>` (Artisan, flags `-`) e `/` (atalhos universais) | 14 |
| Gatilho `>>` (Grupos de Comandos) | 15 |
| Busca global FTS5, E2E completos, benchmarks formais de rajada, empacotamento | 17 |

---

## 3. Pré-requisitos

### 3.1. Fases
- **Fase 01**: projeto Vue 3.6 + Vite 8 + UnoCSS + `@maxvue/*`, ESLint/Stylelint, Vitest, `tsgo`,
  `tauri-specta` gerando `src/types/bindings.ts`, ids de `mt-core` (`PaneId`, `PtyId`, `CommandBlockId`).
- **Fase 02**: `pty_spawn(spec)` desanexado, `pty_write(ptyId, input: PtyInput)`, `pty_resize`, `pty_kill`,
  `pty_detach(ptyId, attachmentId)`, `pty_attach` (reanexação com
  replay de 1 MiB) e `pty_ack` (backpressure por bytes confirmados: o Rust pausa o leitor com 1 MiB não
  confirmado e retoma em 256 KiB), `Channel<ArrayBuffer>` com frames binários `Data`/`Replay`/`Gap`/`End`
   (`src/ipc/ptyFrame.ts`, cabeçalho v1 de 24 bytes) e evento `pty://exit`. Assinaturas finais da base estão
   em `spec.md` §4.1–§4.2; tipos de `bindings.ts`, adaptador em `src/terminal/stream/ptyClient.ts`.
- **Contratos obrigatórios da 02 final:** `PtyInput` text/bytes; `OutputCursor` com nextSeq/byteOffset
  decimais; `AttachInfo.attachmentId`; ACK `consumedBytes` cumulativo de Data + Replay; detach explícito.
  Wrappers pós-unwrap respeitam `ErrorHandlingMode::Result` e `ipc/core` da 01. Validar esses contratos
  na implementação, sem pressupor que documentação é código já disponível.

### 3.2. Ferramentas
- Node ≥ 22, `npm` (Fase 01); Rust stable + `cargo tauri dev` (Tauri v2, WebKitGTK no CachyOS).
- Binários para validação manual: `zsh`, `bash`, `fish`, `vim`/`nvim`, `htop`, `claude`, `opencode`,
  `agy`, `codex`, `tmux` (opcional), `python3` (scripts de fixture).
- Dependências npm adicionadas nesta fase: `@xterm/xterm`, `@xterm/addon-webgl`, `@xterm/addon-fit`,
  `@xterm/addon-search`, `@xterm/addon-web-links`, `@xterm/addon-unicode11`
  e `@xterm/addon-canvas` **somente se** compatível com a versão major do `@xterm/xterm` fixada.
   `vue-web-terminal` entra na camada de apresentação pelo adaptador; fixar versão após o spike (ADR em `spec.md` §3).
- CSS de xterm carregado uma vez; pares de versões dos addons fixados no lockfile. Fontes oficiais
  e indisponibilidade do Context7 registradas em `spec.md` §3.1.

---

## 4. Entregáveis

### 4.1. Frontend — núcleo do terminal (`src/terminal/`)
- `src/terminal/stream/ptyStreamHub.ts` — roteamento do Channel por `PtyId`, decodificação de frames,
  (re)anexação via wrapper `ptyAttach(id, onFrame, cursor)` com cursor confirmado e barreira de geração
  (sem buffer pré-assinatura: o replay é do Rust).
- `src/terminal/stream/ptyClient.ts` — adaptador fino sobre `src/ipc/pty.ts` (Fase 02); único ponto que
  conhece nomes de comandos `pty_*`.
- `src/terminal/engine/writeBatcher.ts` — batching por rAF, orçamento por frame, fallback oculto.
- `src/terminal/engine/flowController.ts` — total cumulativo Data + Replay (`pty_ack` agregado ≥ 32 KiB / 16 ms)
  após parse; não confirmar Gap/End nem usar callbacks antigos na geração atual.
- `src/terminal/engine/inputPipeline.ts` — coalescência de entrada, bracketed paste.
- `src/terminal/engine/rendererManager.ts` — WebGL/canvas/DOM, perda de contexto, liberação em ocultas.
- `src/terminal/engine/fitController.ts` — ResizeObserver + debounce + dedupe → `pty_resize`.
- `src/terminal/engine/terminalInstance.ts` — classe `TerminalInstance` (encapsula `Terminal` + addons).
- `src/terminal/engine/instanceRegistry.ts` — `TerminalInstanceRegistry` (Map não reativa).
- `src/terminal/engine/modeTracker.ts` — alt-screen, mouse tracking, bracketed paste, título.
- `src/terminal/extensions/terminalExtensions.ts` — `registerTerminalExtension`.
- `src/terminal/links/linkHandlerRegistry.ts` — `registerLinkHandler` + handler `http(s)`.
- `src/terminal/input/inputModeResolver.ts` — resolução `rich`/`passthrough` + `InputModeHintProvider`.
- `src/terminal/triggers/triggerRegistry.ts` — `TriggerRegistry` (singleton + factory para testes).
- `src/terminal/triggers/completionSession.ts` — máquina de estados do popup (etapas, abort, debounce).
- `src/terminal/blocks/blockActionRegistry.ts` — registro de ações de bloco + ações nativas.
- `src/terminal/blocks/vueWebTerminalAdapter.ts` — projeção de blocos e intenções de entrada, isolando APIs públicas da biblioteca; sem leitor ou execução de PTY próprios.
- `src/terminal/theme/defaultTheme.ts` — tema Max Navy/Teal a partir de tokens CSS.
- `src/terminal/keybindings.ts` — atalhos do terminal (constantes configuráveis).
- `src/types/terminal.ts` — contratos TS **exclusivos do frontend** (não espelham structs Rust).

### 4.2. Stores e composables
- `src/stores/terminal.ts` — `useTerminalStore`.
- `src/stores/commandBlocks.ts` — `useCommandBlocksStore` (+ `CommandBlockFeed`).
- `src/composables/terminal/useTerminal.ts`, `useTerminalSearch.ts`, `useRichInput.ts`,
  `useCompletion.ts`, `useCommandBlocks.ts`.

### 4.3. Componentes (`src/components/terminal/`)
- `HybridTerminal.vue` (composição: TerminalView + CommandBlockLayer + RichInput + SearchBar + banner de saída)
- `TerminalView.vue`, `TerminalSearchBar.vue`, `RichInput.vue`, `CompletionPopup.vue`,
  `CompletionItem.vue`, `CommandBlock.vue`, `CommandBlockLayer.vue`, `TerminalExitBanner.vue`,
  `InputModeToggle.vue`.
- `src/components/terminal/__dev__/TerminalPlayground.vue` + rota `/dev/terminal` (somente DEV).

### 4.4. Testes e fixtures
- `tests/terminal/*.spec.ts` (Vitest) — ver `spec.md` §15.
- `tests/fixtures/terminal/truecolor.py`, `unicode-width.txt`, `burst.sh`, `mouse-report.py`.
- O checklist manual de TUIs vive em §5.3 deste arquivo (sem arquivo separado).

### 4.5. Rust
- **Nenhum crate de domínio, struct IPC ou comando novo.** Apenas integração do plugin/capability Tauri `opener:allow-open-url` restrita a
  `http://*`/`https://*` caso o handler padrão de links use `@tauri-apps/plugin-opener`
  (se o plugin não estiver na Fase 01, adicioná-lo em `src-tauri/Cargo.toml` e `capabilities/`).

---

## 5. Critérios de Aceite

### 5.1. Automatizados (bloqueantes)
1. `npm run typecheck` (`tsgo --noEmit`) sem erros; `npm run lint` e `npm run lint:style` sem erros.
2. `npx --no-install vitest run tests/terminal` verde, com cobertura ≥ 90% de linhas em `src/terminal/**` e
   ≥ 80% em `src/components/terminal/**`.
3. Testes obrigatórios existentes e passando (nomes de arquivo exatos):
   - `writeBatcher.spec.ts`: N chunks no mesmo frame resultam em **1** `term.write`; orçamento por frame
     respeitado (resto vai para o frame seguinte); ordem de bytes preservada; instância oculta usa
     `setTimeout` em vez de rAF.
   - `flowController.spec.ts`: `pty_ack` só é chamado **depois** do callback de parse do xterm (nunca no
     recebimento); 1 ack ao acumular 32 KiB; ack por intervalo de 16 ms quando há resto; acks serializados.
   - `ptyStreamHub.spec.ts`: spawn desanexado não cria Channel nem ACK; replay antes do ao vivo,
     recuperação da mesma instância com cursor confirmado `{ nextSeq, byteOffset }`; nova instância
     usa replay completo; `Gap` gera banner externo, sem injetar bytes no parser.
     `End` aguarda callbacks anteriores, ACK não cruza gerações e u64 não é arredondado.
   - `fitController.spec.ts`: 10 resizes em 30 ms ⇒ 1 `pty_resize`; dimensões iguais ⇒ 0 chamadas;
     container 0×0 ⇒ 0 chamadas.
   - `rendererManager.spec.ts`: `onContextLoss` ⇒ descarte do addon WebGL e fallback; 3 perdas em 60 s ⇒
     fixa `dom` até recarregar.
   - `triggerRegistry.spec.ts`: `>>` vence `>`; prefixo fora da posição 0 não ativa; registro duplicado
     de prefixo lança `TriggerConflictError`; `dispose()` remove; escape `\>` desativa gatilho.
   - `completionSession.spec.ts`: digitação rápida aborta a consulta anterior (`AbortSignal`);
     `push-stage`/`pop-stage` (Backspace em query vazia) funcionam; itens `pinned: 'bottom'` ficam no fim;
     `passthrough: true` volta a `idle` sem popup; `refreshAfter` reconsulta o provider ao resolver; `Escape`
     suprime o gatilho no painel até a linha esvaziar ou o prefixo ser apagado.
   - `inputModeResolver.spec.ts`: alt-screen/mouse tracking forçam `passthrough` inclusive com override;
     fora dessas condições, override vence dicas; provider de prioridade maior vence a menor.
   - `CompletionPopup.spec.ts` / `RichInput.spec.ts`: setas, `Enter`, `Tab`, `Escape`, `Home`/`End`,
     wrap-around, `aria-activedescendant` correto, envio `texto + '\r'` ao PTY, histórico local ↑/↓.
   - `CommandBlock.spec.ts`: renderiza status (`running`/`success`/`error`), duração, slot `actions`
     recebe `{ block }`; ações do `BlockActionRegistry` filtradas por `when(block)`.
4. `cargo clippy --workspace -- -D warnings` continua verde na implementação; mudanças opcionais
   de registro do plugin/capability não adicionam lógica de domínio Rust.
5. UTF-8 e sequências CSI/OSC divididos em todos os offsets preservam resultado; um write em voo,
   filas limitadas, callbacks tardios cancelados, nenhum eco ou envio duplicado ao alternar modos.
   Validar bytes 0x80/0xff de `onBinary` end-to-end via `PtyInput.kind = 'bytes'` da 02.
6. `>>` indisponível não recai em `>`; providers assíncronos obsoletos não submetem comandos;
   IME/duplo Enter/paste não duplicam escrita. Dispose durante attach não deixa listeners tardios.
7. Spike e teste de integração usam `vue-web-terminal` real na versão fixada: projetam blocos do feed,
   comprovam um único editor/pipeline de entrada e não duplicam execução, eco ou listeners de PTY.
   Mock isolado não satisfaz este critério; incompatibilidade exige decisão arquitetural explícita.
   `HybridTerminal` → `CommandBlockLayer` → `useCommandBlocks` → `VueWebTerminalAdapter` é o pipeline
   real: um adaptador por painel, `projectedEntries` consumidas por `CommandBlock`, descarte no unmount.
8. `TerminalTheme.ui` tem os cinco campos obrigatórios (`blockBackground`, `blockBorder`,
   `blockHeaderBackground`, `richInputBackground`, `richInputBorder`), publicados por `applyTheme`/`onThemeChange`
   em `themeUi` e aplicados como variáveis CSS locais na raiz `.hybrid-terminal`. Dois painéis com temas distintos
   mantêm blocos/input isolados, inclusive após ocultar, alterar o tema e remontar; nenhuma cor é aplicada globalmente.

### 5.2. Comportamento observável (validar em `cargo tauri dev`, rota `/dev/terminal`)
1. Shell padrão abre sem fases 05/07. Registrar latência de spawn e montagem separadas; meta de
   < 300 ms até input pronto após resposta de spawn no hardware de referência, sem incluir scripts de login.
2. `python3 tests/fixtures/terminal/truecolor.py` exibe gradiente 24 bits contínuo (sem banding de 256 cores).
3. Fixture Unicode mantém CJK, combinantes e emoji simples alinhados na fonte/locale documentados;
   registrar separadamente suporte ou limitação de clusters ZWJ.
4. **Rajada**: `bash tests/fixtures/terminal/burst.sh` (`seq 1 3000000` e `yes | head -c 200M`):
   - a UI continua respondendo (barra de busca abre e `Ctrl+C` interrompe em < 500 ms);
   - a última linha de `seq` é exatamente `3000000` (sem perda nem reordenação);
   - memória do WebView não cresce indefinidamente (estável após a rajada, verificado em `htop`);
   - com consumidor artificialmente lento, `flow.paused` liga/desliga; é estimativa de pressão,
     não confirmação do estado interno Rust; bytes pendentes voltam a zero.
5. Redimensionar a janela/painel reflete `cols×rows` corretos em `tput cols; tput lines` e em `htop`
   sem artefatos; arraste contínuo gera ≤ 1 `pty_resize` a cada 50 ms.
6. `Ctrl+Shift+F` abre a busca; "n de m" atualiza; `Enter`/`Shift+Enter` navegam; `Escape` fecha e
   devolve o foco ao terminal.
7. URL `https://example.com` impressa fica sublinhada no hover e abre no navegador padrão com
   `Ctrl+Clique`; `javascript:` e `file://` **não** são clicáveis.
8. Forçar perda de contexto (`WEBGL_lose_context` no playground) troca o renderer sem perder o
   scrollback; o badge de renderer no playground mostra `canvas` ou `dom`.
9. Trocar de aba (playground com 2 instâncias) e voltar preserva scrollback, seleção e cursor;
   apenas a instância visível mantém contexto WebGL.
10. Input rico: `ls -la` + `Enter` executa uma vez; ↑ recupera histórico local; colar edita rascunho
    sem transmitir. No xterm direto, paste ativo gera um único envelope `ESC[200~…ESC[201~`.
11. Fixtures para `>`, `>>`, `@`, `?`, `/` exercitam popup, etapas, ações fixas e maior prefixo;
    `Escape` fecha sem alterar texto; `\?` não abre popup e preserva a barra no envio.
12. Blocos: o playground injeta um `CommandBlockFeed` simulado; blocos aparecem ancorados às linhas
    corretas, acompanham o scroll, somem em alt-screen e as ações "Copiar comando"/"Copiar saída"
    copiam comando e texto renderizado retido; trim informa indisponibilidade/saída parcial.
13. Reload com ring truncado sinaliza perda e visualização parcial; não promete reconstruir estado
    VT. Fechar/reiniciar no playground usa kill da 02, espera saída e gera novo PtyId sem órfãos.

### 5.3. Checklist de fidelidade a TUIs (manual, bloqueante)
Para cada um — `vim`/`nvim`, `htop`, `claude`, `opencode`, `agy`, `codex` — registrar versão,
renderer, shell, driver e resultado. Marcar comportamento não oferecido pelo aplicativo como
"não aplicável" com justificativa; indisponibilidade do binário não equivale a teste aprovado:
- [ ] entra e sai do alt-screen restaurando o buffer principal intacto;
- [ ] input rico some automaticamente (modo `passthrough`) e volta ao sair;
- [ ] mouse (clique, scroll, arrasto) funciona quando o app habilita tracking; `Shift+arrasto` seleciona texto;
- [ ] cores true color/256 corretas; caixas/bordas Unicode alinhadas;
- [ ] quando o app habilita bracketed paste, colar multilinha preserva o bloco;
- [ ] redimensionar enquanto o TUI está aberto redesenha corretamente;
- [ ] `Ctrl+C`, `Ctrl+Z`, `Esc`, `Alt+<tecla>`, `F1–F12`, setas e `Home/End` chegam ao aplicativo.

---

## 6. Riscos e Mitigação

| Risco | Impacto | Mitigação |
|-------|---------|-----------|
| WebGL instável no WebKitGTK (drivers NVIDIA/DMABUF no CachyOS) | Tela preta/artefatos | Cadeia de fallback automática; 3 perdas/60 s fixam `dom`; preferência `webglEnabled`; documentar `WEBKIT_DISABLE_DMABUF_RENDERER=1` como contorno |
| Limite de contextos WebGL por página (~16) com muitas abas/painéis | Perda de contexto em cascata | WebGL só em instâncias visíveis; ocultas liberam o addon e voltam ao `dom` |
| Rajadas saturam IPC/parser e travam a UI | App congelado | rAF batching com orçamento por frame + `pty_ack` somente após o parse, fazendo o Rust (Fase 02) pausar o leitor em 1 MiB não confirmado |
| Aba oculta/WebView suspenso deixa de confirmar por 5 s | Perda de saída antiga | Timers quando executáveis; ao retomar, reconciliar stream/replay e sinalizar Gap; suspensão do SO não é evitada por setTimeout |
| Detecção errada de modo (programa lê stdin sem alt-screen: `read`, REPL `python3`) | Input rico "rouba" teclas | Clique no terminal sempre foca o xterm (passthrough); toggle `Ctrl+Shift+I`; `InputModeHintProvider` da Fase 05 (prompt ativo) refina |
| `>` no início é redirecionamento de shell legítimo | Usuário não consegue digitar `> arquivo` | Escape fecha sem mudar texto; barra em `\>` é preservada para semântica do shell; provider indisponível mantém literal |
| Vazamento de memória ao remontar componentes | Crescimento contínuo | Instâncias fora da reatividade (`markRaw`/Map), `dispose()` determinístico, teste de montagem/desmontagem 100× |
| Divergência entre nomes de IPC esperados e os reais da Fase 02 | Retrabalho | Isolamento em `ptyClient.ts`; tipos sempre de `bindings.ts` |
| `vue-web-terminal` incompatível com composição/Vue 3.6 | Bloqueio da camada de blocos | Spike obrigatório e adaptador de apresentação; xterm mantém VT. Mudança para camada própria exige decisão arquitetural explícita |
| Links maliciosos impressos por processos | Execução de esquema perigoso | Allowlist `http`/`https` no handler padrão, `Ctrl+Clique` obrigatório e validação via `new URL()` antes de abrir |
