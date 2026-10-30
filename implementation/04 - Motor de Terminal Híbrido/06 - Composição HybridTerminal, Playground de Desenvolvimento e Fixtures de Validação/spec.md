# Implementação 06 — Composição HybridTerminal, Playground de Desenvolvimento e Fixtures de Validação

> Fase 04: Motor de Terminal Híbrido
> Referências: [`../instructions.md`](../instructions.md) (§2.1 item 10; §4.3–§4.4; §5.1–§5.3) e [`../spec.md`](../spec.md) (§10, §11.4, §12, §13, §15.1–§15.3).

---

## 1. Contexto, Objetivo e Escopo

Esta **Implementação 06** conclui a **Fase 04 (Motor de Terminal Híbrido)** integrando todos os subsistemas das Implementações 01 a 05 no componente principal `HybridTerminal.vue`, entregando o **Playground de Desenvolvimento (`/dev/terminal`)** capaz de operar um shell real sobre a Fase 02 sem depender das Fases 05 ou 07, fornecendo os 4 scripts/arquivos de **fixtures de validação** (`truecolor.py`, `unicode-width.txt`, `burst.sh`, `mouse-report.py`) e garantindo o cumprimento integral de todos os critérios de aceite automatizados e observáveis da Fase 04.

Escopo exato desta implementação:
1. **`HybridTerminal.vue` (`src/components/terminal/HybridTerminal.vue`):**
   - Componente orquestrador que compõe `TerminalView.vue`, `CommandBlockLayer.vue` (repassando o slot `block-actions`), `TerminalSearchBar.vue` (aberto via `Ctrl+Shift+F`), banner não-intrusivo de `Gap` de replay (`"[MaxTerminal: N bytes descartados antes da exibição — visualização parcial]"`), `TerminalExitBanner.vue` (exibido após drenagem de `End`) e `RichInput.vue` (exibido quando `richInput !== false` e `inputMode === 'rich'`).
   - Intercepta os atalhos de escopo de painel (`Ctrl+Shift+F` para abrir/focar busca e `Ctrl+Shift+I` para alternar `inputModeOverride` entre `'rich'` e `'passthrough'`) e preserva/restaura o foco adequadamente ao entrar e sair de TUIs (`altScreen`).
2. **Playground de Desenvolvimento (`src/components/terminal/__dev__/TerminalPlayground.vue` + rota `/dev/terminal`):**
   - Carregado **exclusivamente** quando `import.meta.env.DEV === true`.
   - Opera sobre os contratos reais da Fase 02 (`ptyStreamHub.spawn`, `ptyClient.kill`, `onExit`, `onCleanupFailed`) usando `PtyOwner = { tabId: null, paneId }`, sem exigir SQLite (Fase 03), parser OSC 133 (Fase 05) ou gerenciador de abas (Fase 07).
   - Inclui controles interativos para validar todos os requisitos de `spec.md` §15.3 e `instructions.md` §5.2:
     - **Seletor de Shell** (`zsh`, `bash`, `fish`, `/bin/sh` ou padrão `$SHELL`) + botões **Iniciar (Spawn)**, **Reiniciar** (mata o PTY antigo via `pty_kill`, aguarda saída e drenagem, limpa `registry`/`feed` e cria **novo `PtyId`**) e **Encerrar (Kill)**.
     - **Modo 2 Instâncias (Abas):** alterna entre 2 painéis (`pane-1` e `pane-2`) comprovando que a instância oculta suspende o `WebglAddon` (`webglCount === 1`), muda para `setTimeout(16)` e preserva scrollback/seleção/cursor ao voltar.
     - **Simulador de Perda de Contexto WebGL:** botão `"Perder contexto WebGL"` que aciona `WEBGL_lose_context` / `forceWebglContextLossForDebug()` na instância ativa, comprovando o fallback imediato (`canvas`/`dom`) e a trava em `dom` após 3 perdas em 60 s.
     - **Fixtures dos 5 Gatilhos (`>`, `>>`, `@`, `?`, `/`):** botão/toggle que registra provedores simulados para todos os 5 prefixos canônicos (incluindo fluxo de 3 etapas `Harness › Modelo › Nível de Raciocínio` em `@`, precedência `>>` vs `>`, item `pinned: 'bottom'` `"Criar comando"` / `"Criar grupo de comandos local"`, `passthrough` e `refreshAfter`).
     - **Simulador de `CommandBlockFeed`:** botões para injetar `begin` → `markOutputStart` → `end` com `exitCode: 0` (`success`), `exitCode: 1` (`error`) e `exitCode: null` (`aborted`) nas linhas atuais do buffer normal.
     - **Painel de Métricas e Telemetria em Tempo Real:** exibe latência de spawn/montagem (`ms`), `renderer` ativo (`webgl`/`canvas`/`dom`), `webglCount`, dimensões `cols×rows`, `inputMode` (`rich`/`passthrough`), modos VT (`altScreen`, `mouseTracking`, `bracketedPaste`), `flow.pendingBytes`, `flow.paused`, `flow.droppedBytes`, `frameErrors` e contador de `longtask` (`PerformanceObserver` quando suportado).
3. **Fixtures de Terminal (`tests/fixtures/terminal/`):**
   - `tests/fixtures/terminal/truecolor.py` — gradiente RGB contínuo de 24 bits em múltiplas faixas.
   - `tests/fixtures/terminal/unicode-width.txt` — tabela alinhada com caracteres CJK, acentos combinantes, emojis simples, clusters emoji ZWJ e bordas box-drawing (`┌─┬─┐`).
   - `tests/fixtures/terminal/burst.sh` — script de rajada intensa (`seq 1 3000000` e `yes MaxTerminal | head -c 200M`).
   - `tests/fixtures/terminal/mouse-report.py` — script interativo que habilita SGR mouse tracking (`\x1b[?1006h\x1b[?1003h`) e imprime os relatórios recebidos até `q` ou `Ctrl+C`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Código-Fonte Frontend (`src/`)
- `src/components/terminal/HybridTerminal.vue` — componente integrador completo do Motor de Terminal Híbrido.
- `src/components/terminal/__dev__/TerminalPlayground.vue` — ambiente de validação interativa e telemetria da Fase 04.
- `src/app/router.ts` (ou arquivo de rotas equivalente da Fase 01) — registro condicional da rota `/dev/terminal` protegido por `if (import.meta.env.DEV)`.

### 2.2. Fixtures e Testes (`tests/`)
- `tests/fixtures/terminal/truecolor.py`
- `tests/fixtures/terminal/unicode-width.txt`
- `tests/fixtures/terminal/burst.sh`
- `tests/fixtures/terminal/mouse-report.py`
- `tests/terminal/HybridTerminal.spec.ts` — testes de integração da composição `HybridTerminal` e fluxos do playground (atalhos `Ctrl+Shift+F`/`Ctrl+Shift+I`, banner de `Gap`, banner de `Exit`, troca de visibilidade entre 2 instâncias, unicidade de leitor/stream/pipeline de entrada e cobertura global ≥ 90% em `src/terminal/**` e ≥ 80% em `src/components/terminal/**`).

---

## 3. Especificação Técnica Detalhada e Assinaturas

### 3.1. `HybridTerminal.vue` (`src/components/terminal/HybridTerminal.vue`)

**Props, Emits e Slots (`spec.md` §10):**
```ts
import type { ExitInfo, PaneId, PtyId } from '@/types/bindings';
import type { CommandBlockModel } from '@/types/terminal';

export interface HybridTerminalProps {
    paneId: PaneId;
    ptyId: PtyId;
    visible?: boolean;    // default: true
    richInput?: boolean;  // default: true
}

export interface HybridTerminalEmits {
    exit: [exit: ExitInfo];
    'restart-requested': [];
    'title-change': [title: string];
    focus: [];
}
```
- **Slots:**
  - `block-actions`: `{ block: CommandBlockModel }` (repassado para `CommandBlockLayer.vue` → `CommandBlock.vue`).
- **Comportamento Interno:**
  - Sincroniza a prop `richInput` com o cálculo do modo de entrada do painel (se `richInput === false`, o painel opera em `'passthrough'` ocultando a barra `RichInput.vue`).
  - Exibe um banner superior de aviso (`role="status"`) quando `Number(instanceState.flow.droppedBytes) > 0`:
    `"[MaxTerminal: {{ instanceState.flow.droppedBytes }} bytes descartados antes da exibição — visualização parcial]"`.
  - Exibe `TerminalSearchBar.vue` quando `instanceState.search.open === true`.
  - Exibe `TerminalExitBanner.vue` quando `instanceState.exit !== null`, repassando `@restart-requested="$emit('restart-requested')"`.
  - Registra listener de teclado no container do painel (`@keydown.capture`) para:
    - `isSearchShortcut(event)` (`Ctrl+Shift+F`): `event.preventDefault()`, chama `terminalStore.openSearch(props.paneId)`.
    - `isToggleInputModeShortcut(event)` (`Ctrl+Shift+I`): `event.preventDefault()`, chama `terminalStore.toggleInputMode(props.paneId)`.
  - Gerencia restauração de foco ao sair de `altScreen`: guarda se o foco estava no `RichInput` antes de entrar em `altScreen`; ao sair de `altScreen` (retornando para `inputMode === 'rich'`), se o painel continuar ativo e a barra de busca não estiver aberta, devolve o foco ao `RichInput`.

---

### 3.2. `TerminalPlayground.vue` (`src/components/terminal/__dev__/TerminalPlayground.vue`)

**Funcionalidades Obrigatórias do Playground:**
1. **Gerenciamento de Ciclo de Vida de PTY (Fase 02 pura):**
   - `spawnSlot(slotIndex: 0 | 1)`: mede `performance.now()`, chama `ptyStreamHub.spawn({ shell, args: [], login: true, cwd: null, env: {}, envRemove: [], size: { rows: 24, cols: 80, pixelWidth: 0, pixelHeight: 0 }, owner: { tabId: null, paneId } })` e monta o `HybridTerminal` registrando separadamente `spawnLatencyMs` e `mountReadyLatencyMs`.
   - `restartSlot(slotIndex)`: se o PTY ainda estiver vivo, chama `ptyClient.kill(ptyId, 'graceful')` e aguarda o evento de saída/drenagem; em seguida chama `terminalInstanceRegistry.dispose(paneId)`, `ptyStreamHub.release(oldPtyId)`, `commandBlocksStore.feed.clear(paneId)`, `terminalStore.unregister(paneId)` e inicia um **novo `PtyId`** via `spawnSlot(slotIndex)`.
   - Escuta `ptyClient.onCleanupFailed` para exibir alerta vermelho caso processos sobrevivam ao encerramento (sem declarar o PTY limpo).
2. **Provedores de Demonstração dos 5 Gatilhos (`>`, `>>`, `@`, `?`, `/`):**
   - `>` (`id: 'dev.artisan'`, `prefix: '>'`, `label: 'Comandos Artisan (Simulado)'`): filtra comandos (`migrate`, `migrate:fresh`, `make:model`, `route:list`, `cache:clear`) com `fuzzyFilter`, suporta autocompletar de flags `--seed` / `--force` ao digitar `-`, e inclui item `pinned: 'bottom'` (`kind: 'action'`, `label: 'Criar comando'`).
   - `>>` (`id: 'dev.groups'`, `prefix: '>>'`, `label: 'Grupos de Comandos (Simulado)'`): seções `'Projeto Atual'` (`order: 1`) e `'Globais'` (`order: 2`) e dois itens `pinned: 'bottom'` (`'Criar grupo de comandos local'` e `'Criar grupo de comandos global'`).
   - `@` (`id: 'dev.ai.select'`, `prefix: '@'`, `label: 'Selecionar Agente / Modelo (Simulado)'`): fluxo multi-etapa de 3 estágios (`push-stage` / `pop-stage` com `Backspace`):
     - Etapa 0 (`stageLabel: 'Harness'`): `Claude Code`, `Codex`, `Antigravity (agy)`, `OpenCode`.
     - Etapa 1 (`stageLabel: 'Harness › Modelo'`): modelos do harness escolhido.
     - Etapa 2 (`stageLabel: 'Harness › Modelo › Nível de Raciocínio'`): `Baixo`, `Médio`, `Alto`, retornando `{ kind: 'replace-input', text: '? ' }` ou `{ kind: 'submit', text: 'echo "[IA Simulada]"' }`.
   - `?` (`id: 'dev.ai.quick'`, `prefix: '?'`, `label: 'Comando por Linguagem Natural (Simulado)'`): demonstra `refreshAfter` (responde inicialmente com item de carregamento e atualiza com sugestão de comando shell) e `onSubmitWithoutSelection`.
   - `/` (`id: 'dev.shortcuts'`, `prefix: '/'`, `label: 'Atalhos Universais (Simulado)'`): `/install`, `/install-force`, `/update`, `/update-force`, `/build`, `/deploy`, `/test`, retornando `{ kind: ' true', passthrough: true }` quando o texto parece um caminho POSIX (`/usr/...`, `/etc/...`, `/home/...`, `/tmp/...`).

---

### 3.3. Scripts de Fixture (`tests/fixtures/terminal/`)

1. **`tests/fixtures/terminal/truecolor.py`:**
   - Script Python 3 sem dependências externas que imprime barras horizontais de gradiente RGB 24 bits (`\x1b[48;2;R;G;Bm` e `\x1b[38;2;R;G;Bm`) de 80 colunas em 3 faixas (R→G, G→B, B→R) e reseta com `\x1b[0m`.
2. **`tests/fixtures/terminal/unicode-width.txt`:**
   - Tabela em texto UTF-8 com colunas delimitadas por `│` (`┌──────────────┬────────────────────────┬────────┐`) contendo linhas de teste com ASCII, acentos pré-compostos e combinantes (`a\u0303`, `e\u0301`), ideogramas CJK (`日本語テスト`, `中文寬度`), emojis simples (`🚀`, `✅`, `🦀`) e clusters emoji ZWJ (`👨‍👩‍👧‍👦`, `👩‍💻`), permitindo inspeção visual imediata do alinhamento da borda direita.
3. **`tests/fixtures/terminal/burst.sh`:**
   - Script Bash executável:
     ```bash
     #!/usr/bin/env bash
     set -euo pipefail
     seq 1 3000000
     yes "MaxTerminal-Burst-Line-0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ" | head -c 200M || true
     printf "\n=== BURST FINALIZADO: 3000000 ===\n"
     ```
4. **`tests/fixtures/terminal/mouse-report.py`:**
   - Script Python 3 que coloca `sys.stdin` em modo raw (`tty.setraw`), ativa SGR mouse mode (`\x1b[?1006h\x1b[?1003h`), lê e imprime cada sequência de mouse recebida e garante restauração (`\x1b[?1003l\x1b[?1006l` + `termios.tcsetattr`) no `finally` ao pressionar `q` ou `Ctrl+C`.
