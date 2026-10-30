# Implementação 05 — Componentes Vue do Editor de Grupos, Painel de Execução e Saída ao Vivo (`spec.md`)

## 1. Contexto, Objetivo e Escopo

Esta implementação entrega toda a camada de interface visual em Vue 3.6 + TypeScript + UnoCSS + SCSS Scoped da **Fase 15: Grupos de Comandos (`>>`)**, conectando os composables e a store da Implementação 04 aos 7 componentes canônicos especificados em `instructions.md` (`I-13`) e `spec.md` (§9.3–§9.5).

### 1.1. Escopo Incluído
- **Itens da Fase 15 cobertos:** `I-13` (`CommandGroupModal.vue`, `CommandGroupItemRow.vue`, `CommandGroupSuggestPanel.vue`, `GroupRunPanel.vue`, `GroupRunItemRow.vue`, `GroupRunOutputView.vue`, `GroupRunIndicator.vue`) e integração no workspace da coluna direita.
- **Critérios de Aceite cobertos:** `CA-10`, `CA-11` (cobertura ≥ 80% em `src/components/commandGroups/**`), `CA-16`, `CA-17` (UI/UX), `CA-18` (UI/UX), `CA-19` (UI/UX), `CA-20` (UI/UX), `CA-22`, `CA-26` (UI de conflito de revisão).

### 1.2. Diretrizes Obrigatórias de Código Frontend (`AGENTS.md` §3)
1. **Ordem estrita de blocos nos SFCs (`.vue`):**
   1. `<template>`
   2. `<script setup lang="ts">`
   3. `<style lang="scss" scoped>`
2. **Tags de abertura em linha única:** No `<template>`, toda tag de abertura (incluindo todos os atributos, props, diretivas, eventos e o fechamento `>` ou `/>`) deve permanecer em uma única linha.
3. **Formatação:** Recuo de 4 espaços (`baseIndent: 1` dentro de `<script setup>`), aspas simples (`'single'`), ponto e vírgula obrigatório (`;`) e sem vírgula final (`comma-dangle: 'never'`).
4. **Tokens Semânticos (`@maxvue/max-components-ui`):**
   - Executando: `--max-success-500` (Verde)
   - Execução Contínua: `--max-info-500` (Azul-Teal)
   - Finalizado: `--max-warning-500` (Âmbar)
   - Com Erro / Falha: `--max-danger-500` (Vermelho)
   - Pendente / Cancelado / Skipped: tokens neutros (`--background-*` / texto secundário) acompanhados sempre de ícone (`MaxIcon`) e rótulo textual em `pt-BR`.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho Absoluto / Relativo ao Workspace | Descrição |
|------|------------------------------------------|-----------|
| Criar | `src/components/commandGroups/CommandGroupModal.vue` | Modal completo de criação, edição, cópia de escopo ("Copiar para…") e exclusão de Grupo de Comandos. |
| Criar | `src/components/commandGroups/CommandGroupItemRow.vue` | Linha editável de item do grupo (título, comando, `cwd`, pares `env`, flag `continuous`, duplicar, remover, arrastar e reordenar por botões / `Alt+↑/↓`). |
| Criar | `src/components/commandGroups/CommandGroupSuggestPanel.vue` | Painel lateral/expansível de sugestão assistida via IA (Fase 12) e chips de comandos recorrentes do histórico (Fase 13) com preview separado e ações "Aplicar sugestão" / "Desfazer". |
| Criar | `src/components/commandGroups/GroupRunPanel.vue` | Painel de acompanhamento de execução (ativa e histórico da aba/projeto) com contadores, duração, status agregado, botão "Cancelar tudo", resolução de conflito de painel ocupado ("Executar em nova aba" / "Encerrar e reiniciar painel") e lista de itens. |
| Criar | `src/components/commandGroups/GroupRunItemRow.vue` | Linha de item de uma execução com badge semântico, duração, código de saída/sinal, motivo de `skipped`/`interrupted` e ações (`GroupRunItemAction`: `show_output`, `focus_tab`, `cancel`). |
| Criar | `src/components/commandGroups/GroupRunOutputView.vue` | Visualizador de saída ao vivo e replay em terminal somente leitura (`TerminalView.vue` em modo read-only), exibindo avisos de truncamento (256 KiB) e lacunas (`gap`). |
| Criar | `src/components/commandGroups/GroupRunIndicator.vue` | Indicador compacto na barra de ferramentas do workspace do projeto exibindo execuções em andamento e permitindo abrir/focar o `GroupRunPanel.vue`. |
| Modificar | `src/components/workspace/ProjectWorkspaceView.vue` (ou componente equivalente da Coluna Direita da Fase 07) | Montar `GroupRunIndicator.vue`, `GroupRunPanel.vue` e `CommandGroupModal.vue` integrados à `useCommandGroupsStore`. |
| Criar | `tests/commandGroups/CommandGroupModal.spec.ts` | Testes de componente Vitest + Vue Test Utils cobrindo criação local/global, validação por campo, reordenação `Alt+↑/↓`, alerta de comando contínuo fora do fim no modo Sequencial, sugestão IA com Aplicar/Desfazer e conflito de revisão (`CA-11`, `CA-16`, `CA-20`, `CA-26`). |
| Criar | `tests/commandGroups/GroupRunPanel.spec.ts` | Testes de componente Vitest cobrindo renderização de estados (`running`, `continuous`, `succeeded`, `failed`, `partial`, `cancelled`, `interrupted`, `skipped`), tokens semânticos, ações `show_output` / `focus_tab` / `cancel` e ofertas de fallback quando o painel está ocupado (`CA-11`, `CA-17`, `CA-18`, `CA-19`). |
| Criar | `tests/commandGroups/GroupRunOutputView.spec.ts` | Testes de componente Vitest cobrindo attach na abertura, renderização de frames `replay`/`data`/`gap`/`end`, bloqueio de input de teclado/colagem e `detach` no fechamento/unmount sem cancelar a execução (`CA-11`, `CA-18`). |

---

## 3. Contratos de Props, Emits e Comportamento dos Componentes Vue

### 3.1. `src/components/commandGroups/CommandGroupModal.vue`

```ts
import type { CommandGroup, GroupScope } from '@/types/commandGroups';

export interface CommandGroupModalProps {
    modelValue: boolean;
    group: CommandGroup | null;
    scope: GroupScope;
    projectId: string | null;
    initialName?: string;
}

export interface CommandGroupModalEmits {
    (e: 'update:modelValue', value: boolean): void;
    (e: 'saved', group: CommandGroup): void;
    (e: 'deleted', groupId: string): void;
}
```

- **Regras do Modal (`CA-16`, `CA-20`, `CA-26`):**
  - Na criação (`group === null`), permite alternar entre escopo `Local` (habilitado somente se `projectId !== null`) e `Global`.
  - Na edição (`group !== null`), o escopo é exibido como somente leitura; oferece o botão **"Copiar para Global"** (se local) ou **"Copiar para este Projeto"** (se global e houver projeto ativo), que clona os dados para um novo rascunho de criação.
  - Exibe seleção de modo (`Sequencial na mesma aba`, `Paralelo em Background`, `Multi-Abas`). Quando `Sequencial` está selecionado, exibe o seletor de política:
    - **`&&` (Parar na primeira falha)**
    - **`;` (Continuar mesmo após falha)**
  - Exibe nota explicativa contextual sobre o isolamento de `cwd`/`env`: no modo Sequencial, itens sem override mantêm o estado do shell (`cd`, `export`), enquanto itens com override rodam em shell filho isolado (`env -C`).
  - Se o modo for `Sequencial` e algum item que **não** seja o último tiver `continuous === true`, exibe um alerta preventivo (`--max-warning-500`): *"Este item está marcado como Execução Contínua e impedirá que os itens seguintes iniciem enquanto não for encerrado."*
  - Caso `update` retorne `commands.group.revision_conflict`, exibe banner de conflito de revisão sem apagar as alterações do usuário, oferecendo **"Carregar versão mais recente em paralelo"** ou **"Forçar salvamento sobre a revisão atual"**.
  - Caso `create`/`update` retorne `commands.group.secret_detected`, destaca exatamente os campos listados em `details.fields` e orienta usar referências a variáveis de ambiente (`$TOKEN`) em vez de segredos literais.
  - **Salvar nunca executa o grupo.**

### 3.2. `src/components/commandGroups/CommandGroupItemRow.vue`

```ts
import type { EditableGroupItem } from '@/types/commandGroups';

export interface CommandGroupItemRowProps {
    item: EditableGroupItem;
    index: number;
    total: number;
    fieldErrors: Record<string, string>;
}

export interface CommandGroupItemRowEmits {
    (e: 'update:item', item: EditableGroupItem): void;
    (e: 'move-up', index: number): void;
    (e: 'move-down', index: number): void;
    (e: 'duplicate', index: number): void;
    (e: 'remove', index: number): void;
}
```

- Suporta atalho de teclado `Alt+ArrowUp` e `Alt+ArrowDown` diretamente na linha para mover o item mantendo o foco no elemento reordenado e anunciando a nova posição em `aria-live="polite"` (`"Item movido para a posição X de Y"`).
- Pares `env` chave/valor mascaram visualmente (`type="password"` com toggle de visualização) valores de chaves que contenham `TOKEN`, `SECRET`, `PASSWORD`, `KEY` ou `AUTH`, e orientam o uso de referências `$VAR` dentro do comando.

### 3.3. `src/components/commandGroups/CommandGroupSuggestPanel.vue`

```ts
import type { CommandGroupInput, GroupScope, RecurringCommandEntry } from '@/types/commandGroups';

export interface CommandGroupSuggestPanelProps {
    scope: GroupScope;
    projectId: string | null;
    paneId: string | null;
    draftRevision: number;
    currentDraft: CommandGroupInput;
    hasUndoSnapshot: boolean;
}

export interface CommandGroupSuggestPanelEmits {
    (e: 'apply-suggestion', suggestion: CommandGroupInput): void;
    (e: 'undo-suggestion'): void;
    (e: 'append-recurring-item', entry: RecurringCommandEntry): void;
}
```

- Permite digitar o objetivo em `pt-BR` e clicar em **"Sugerir com IA"** (com botão **"Cancelar"** enquanto a requisição da Fase 12 estiver em voo).
- Exibe a sugestão retornada em uma área de preview separada antes de tocar no formulário principal; clicar em **"Aplicar ao rascunho"** atualiza o formulário e habilita **"Desfazer aplicação"**.
- Se ` draftRevision` mudar enquanto a IA responde, descarta ou sinaliza que o rascunho foi editado depois do pedido, nunca sobrescrevendo silenciosamente o que o usuário digitou (`CA-20`).
- Exibe a seção **"Comandos recorrentes do projeto"** somente quando `recurringCommands.available === true`; clicar em um chip adiciona um novo item editável ao final do rascunho.

### 3.4. `src/components/commandGroups/GroupRunPanel.vue` e `GroupRunItemRow.vue`

```ts
import type { GroupRun, GroupRunItem, GroupRunItemAction } from '@/types/commandGroups';

export interface GroupRunPanelProps {
    run: GroupRun | null;
    busyPaneFallback?: {
        groupId: string;
        projectId: string;
        reason: string;
    } | null;
}

export interface GroupRunPanelEmits {
    (e: 'cancel-run', runId: string): void;
    (e: 'cancel-item', payload: { runId: string; itemId: string }): void;
    (e: 'show-output', payload: { runId: string; itemId: string }): void;
    (e: 'focus-tab', tabId: string): void;
    (e: 'run-in-new-tab', groupId: string): void;
    (e: 'restart-busy-pane'): void;
}

export interface GroupRunItemRowProps {
    item: GroupRunItem;
    mode: GroupRun['mode'];
}

export interface GroupRunItemRowEmits {
    (e: 'action', payload: { action: GroupRunItemAction; item: GroupRunItem }): void;
}
```

- **Mapeamento Visual Semântico (`CA-18`):**
  - Item `running` com `snapshot.continuous === false`: badge Verde (`--max-success-500`), rótulo `"Executando"`.
  - Item `running` com `snapshot.continuous === true`: badge Azul-Teal (`--max-info-500`), rótulo `"Execução Contínua"`.
  - Item `succeeded`: badge Âmbar (`--max-warning-500`), rótulo `"Finalizado (0)"`.
  - Item `failed` / `interrupted`: badge Vermelho (`--max-danger-500`), rótulo `"Falhou (exit X)"` ou `"Interrompido"`.
  - Item `pending` / `starting` / `cancelling` / `cancelled` / `skipped`: badge Neutro com rótulo correspondente em `pt-BR`.
- Para itens em modo `Parallel` (headless), exibe a nota `"Modo em background sem entrada interativa — use Multi-Abas para comandos interativos"`.
- Quando um lançamento Sequencial é recusado com `commands.group.pane_busy` ou `commands.group.tracking_unavailable`, exibe ação imediata **"Executar em nova aba"**; quando um cancelamento Sequencial permanece em `cancelling` porque o processo em foreground ignora `Ctrl+C`, exibe o botão **"Encerrar e reiniciar painel"** (`WorkspaceManager::restart_pane`, `spec §4.4`).

### 3.5. `src/components/commandGroups/GroupRunOutputView.vue`

```ts
import type { GroupRunId, GroupRunItem } from '@/types/commandGroups';

export interface GroupRunOutputViewProps {
    runId: GroupRunId;
    item: GroupRunItem;
}

export interface GroupRunOutputViewEmits {
    (e: 'close'): void;
}
```

- Reutiliza o `TerminalView.vue` (Fase 04) em modo estritamente **somente leitura** (`readonly: true`, bloqueando teclado e colagem).
- Utiliza `useGroupRunOutput` para anexar o `Channel<GroupOutputFrame>` na abertura e desanexar no fechamento/desmontagem (`onBeforeUnmount`) sem encerrar o processo em background.
- Se `isTruncated` for verdadeiro ou se chegar um frame `GroupOutputFrameKind::Gap`, exibe um banner informativo no topo do visualizador indicando a quantidade de bytes omitidos e que a visualização mostra os últimos 256 KiB sanitizados.
