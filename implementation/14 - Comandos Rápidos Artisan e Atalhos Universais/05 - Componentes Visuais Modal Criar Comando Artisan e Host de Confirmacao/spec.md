# Implementação 05 — Componentes Visuais, Modal "Criar Comando Artisan" e Host de Confirmação (`spec.md`)

> **Fase:** 14 — Comandos Rápidos: Artisan (`>`) e Atalhos Universais (`/`)
> **Unidade:** `05 - Componentes Visuais Modal Criar Comando Artisan e Host de Confirmacao`
> **Referências Canônicas:** [`../../AGENTS.md`](../../../AGENTS.md) §3 e §8.1, [`../instructions.md`](../instructions.md) (I-09 a I-13, I-15, I-18, CA-03, CA-16, CA-18 a CA-25, CA-31, CA-33, CA-36) e [`../spec.md`](../spec.md) (§10.8, §11.3 a §11.5, §12, §13, §15.2).

---

## 1. Contexto, Objetivo e Escopo

Esta quinta e última implementação da Fase 14 entrega todos os componentes visuais Vue 3.6 + TypeScript + UnoCSS + SCSS Scoped e o composable de formulário `useArtisanForm.ts`:
1. `useArtisanForm.ts`: lógica de vinculação reativa entre os controles do modal e `useQuickCommandsStore`, sugestão automática de `className` em PascalCase a partir de `signatureName` até o usuário editar `className` manualmente, indexação de `FieldIssue` por caminho (`path`) e foco automático no primeiro campo inválido.
2. `QuickCommandItem.vue`: item customizado para o `CompletionPopup` (Fase 04) exibindo realce de caracteres do match fuzzy, badges (`aceita valor`, `global`, `obrigatório`), atalhos (`-Q`) e motivo de desabilitação.
3. `ArtisanParamRow.vue` e `ArtisanParamsBuilder.vue`: construtor visual de Argumentos e Flags (`--options`) com adição, reordenação (`↑`/`↓`), remoção e campos condicionais por `ParamKind`.
4. `ArtisanSchedulePicker.vue`: seletor visual de agendamento (`Schedule`) com todas as 13 frequências do Laravel, entradas para argumentos obrigatórios e opções da execução agendada, checkboxes para `withoutOverlapping`, `onOneServer` e `runInBackground` e visualização da cadeia `Schedule::command(...)`.
5. `ArtisanPhpPreview.vue`: visualizador somente leitura em `<pre>` seguro (interpolação de texto puro, sem `v-html`) com 3 abas (`Comando`, `routes/console.php` e `Passos manuais`).
6. `ArtisanCommandModal.vue`: modal completo em 2 colunas (`MaxModal`) com formulário à esquerda, `ArtisanPhpPreview` à direita e barra de ações (`Preencher com IA`, `Desfazer sugestão`, `Criar com MaxCode` e `Criar`).
7. `QuickCommandsHost.vue` montado em `src/App.vue`: hospeda `ArtisanCommandModal` e o diálogo/popover de confirmação (`MaxPopoverConfirm`) para `/install-force`, `/update-force` e `/deploy`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Novos Arquivos em `src/`
- `src/composables/quick-commands/useArtisanForm.ts`
- `src/components/quick-commands/QuickCommandItem.vue`
- `src/components/quick-commands/ArtisanParamRow.vue`
- `src/components/quick-commands/ArtisanParamsBuilder.vue`
- `src/components/quick-commands/ArtisanSchedulePicker.vue`
- `src/components/quick-commands/ArtisanPhpPreview.vue`
- `src/components/quick-commands/ArtisanCommandModal.vue`
- `src/components/quick-commands/QuickCommandsHost.vue`

### 2.2. Arquivos Modificados em `src/`
- `src/App.vue` — montagem de `<QuickCommandsHost />`.

### 2.3. Testes de Componentes Vitest a Criar
- `src/components/quick-commands/__tests__/ArtisanCommandModal.spec.ts`
- `src/components/quick-commands/__tests__/ArtisanParamsBuilder.spec.ts`
- `src/components/quick-commands/__tests__/ArtisanSchedulePicker.spec.ts`
- `src/components/quick-commands/__tests__/QuickCommandsHost.spec.ts`

---

## 3. Especificação Técnica e Assinaturas Exatas

### 3.1. Padrões Obrigatórios de SFC Vue (CA-33)
Todos os arquivos `.vue` desta implementação DEVEM obedecer estritamente:
1. Ordem dos blocos: `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`.
2. Tags de abertura no `<template>` em **linha única** (nome da tag, atributos, props, diretivas, eventos e `>` ou `/>` na mesma linha).
3. Recuo de 4 espaços (inclusive `baseIndent: 1` dentro de `<script setup lang="ts">`).
4. Aspas simples (`'`), ponto e vírgula obrigatório e sem vírgula final (`comma-dangle: 'never'`).
5. Uso exclusivo dos tokens semânticos do `@maxvue/max-components-ui` (`--max-primary-*`, `--max-success-*`, `--max-warning-*`, `--max-danger-*`, `--max-info-*`, `--background-*`).
6. Todos os rótulos, botões, placeholders, tooltips e mensagens visíveis em Português do Brasil (`pt-BR`).

### 3.2. Composable `useArtisanForm` (`src/composables/quick-commands/useArtisanForm.ts`)

```ts
import type {
    ArtisanCommandForm,
    ArtisanParam,
    FieldIssue,
    ScheduleSpec
} from '@/types/bindings';
import type { ComputedRef, Ref } from 'vue';

export interface UseArtisanFormReturn {
    form: ComputedRef<ArtisanCommandForm>;
    classNameTouched: Ref<boolean>;
    issuesByPath: ComputedRef<Record<string, FieldIssue[]>>;
    generalIssues: ComputedRef<FieldIssue[]>;
    canCreate: ComputedRef<boolean>;
    hasAiSnapshot: ComputedRef<boolean>;
    setClassName: (value: string) => void;
    setSignatureName: (value: string) => void;
    setDescription: (value: string) => void;
    setParams: (params: ArtisanParam[]) => void;
    setIsolatable: (value: boolean) => void;
    setPromptsForMissingInput: (value: boolean) => void;
    setSchedule: (schedule: ScheduleSpec | null) => void;
    setObjective: (value: string) => void;
    issuesFor: (path: string) => FieldIssue[];
    firstErrorMessageFor: (path: string) => string | null;
    focusFirstInvalidField: (container: HTMLElement | null) => void;
}

export function useArtisanForm(): UseArtisanFormReturn;
```

**Regras de Negócio de `useArtisanForm`:**
- Enquanto `classNameTouched.value === false`, alterar `signatureName` (ex.: `reports:send-weekly`) atualiza automaticamente `className` usando `suggestClassNameFromSignature(signatureName)` (`SendWeekly` ou `ReportsSendWeekly` em PascalCase válido). Assim que o usuário digitar diretamente no input "Nome da Classe", `classNameTouched.value` passa a `true` e `signatureName` deixa de sobrescrever `className`.
- Quando `openCreateModal` ou `fillWithAi` preenche o formulário, `classNameTouched` é sincronizado adequadamente.
- `issuesByPath` agrupa `store.modal.report?.issues ?? []` pela chave `issue.path` (`"className"`, `"signatureName"`, `"description"`, `"params[0].name"`, `"params[0].shortcut"`, `"schedule.frequency.time"`, etc.).
- `focusFirstInvalidField(container)` procura o primeiro elemento com `[data-field-path="<path>"]` correspondente à primeira issue de severidade `'error'` e chama `.focus()`.

### 3.3. Contratos de Props e Emits dos Componentes Vue

#### `QuickCommandItem.vue`
```ts
export interface QuickCommandItemProps {
    item: CompletionItem;
    active: boolean;
    highlightIndices?: number[];
}
```
- Renderiza ícone (`MaxIcon`), `label` com `<mark>` nos índices de match fuzzy, `description`, badges (`aceita valor`, `global`, `atalho`) e indicador visual quando `item.disabled === true`.

#### `ArtisanParamRow.vue`
```ts
export interface ArtisanParamRowProps {
    modelValue: ArtisanParam;
    index: number;
    total: number;
    issues: FieldIssue[];
}

export interface ArtisanParamRowEmits {
    (e: 'update:modelValue', value: ArtisanParam): void;
    (e: 'remove'): void;
    (e: 'move', delta: -1 | 1): void;
}
```
- Exibe badge `"Argumento"` ou `"Opção (--flag)"`.
- Para `Argument`: inputs `name`, `description`, `default` (desabilitado se obrigatório ou array) e checkboxes `Opcional` e `Array (*)`.
- Para `Option`: inputs `name` (com prefixo visual `--`), `shortcut` (1 letra, com prefixo visual `-`), `description`, `default` (habilitado quando `acceptsValue && !isArray`) e checkboxes `Aceita valor (=)` e `Múltiplos valores (=*)`.
- Exibe erros específicos de `params[${index}].name`, `params[${index}].shortcut`, `params[${index}].default` e `params[${index}].description`.

#### `ArtisanParamsBuilder.vue`
```ts
export interface ArtisanParamsBuilderProps {
    modelValue: ArtisanParam[];
    issues: FieldIssue[];
}

export interface ArtisanParamsBuilderEmits {
    (e: 'update:modelValue', value: ArtisanParam[]): void;
}
```
- Botões `"Adicionar argumento"` e `"Adicionar opção"`.
- Lista de `ArtisanParamRow` com suporte a reordenação (`move: -1 | 1`) e remoção.

#### `ArtisanSchedulePicker.vue`
```ts
export interface ArtisanSchedulePickerProps {
    modelValue: ScheduleSpec | null;
    params: ArtisanParam[];
    issues: FieldIssue[];
}

export interface ArtisanSchedulePickerEmits {
    (e: 'update:modelValue', value: ScheduleSpec | null): void;
}
```
- Checkbox mestre `"Agendamento (Schedule)"`.
- Seletor de frequência com todas as 13 opções em `pt-BR`:
  - `everyMinute` ("A cada minuto"), `everyFiveMinutes` ("A cada 5 minutos"), `everyFifteenMinutes` ("A cada 15 minutos"), `everyThirtyMinutes` ("A cada 30 minutos"), `hourly` ("A cada hora"), `hourlyAt` ("A cada hora no minuto…"), `daily` ("Diariamente à meia-noite"), `dailyAt` ("Diariamente às…"), `twiceDaily` ("Duas vezes ao dia"), `weekly` ("Semanalmente"), `weeklyOn` ("Semanalmente no dia/hora…"), `monthly` ("Mensalmente"), `monthlyOn` ("Mensalmente no dia/hora…"), `cron` ("Expressão Cron personalizada").
- Campos dinâmicos conforme a frequência (`minute`, `time` `"HH:MM"`, `first`/`second`, `day` da semana `0..6` com nomes Domingo..Sábado, `day` do mês `1..31`, `expression` cron de 5 campos).
- Campos para preencher os valores dos argumentos posicionais do comando no agendamento e ativar opções da signature.
- Checkboxes para `withoutOverlapping` ("Sem sobreposição — withoutOverlapping"), `onOneServer` ("Em um único servidor — onOneServer") e `runInBackground` ("Em segundo plano — runInBackground").

#### `ArtisanPhpPreview.vue`
```ts
export interface ArtisanPhpPreviewProps {
    preview: ArtisanPreview | null;
    loading: boolean;
}
```
- 3 abas:
  1. `"Comando PHP"` — exibe `preview.path`, a `$signature` montada e o código `preview.php` dentro de `<pre><code>{{ preview.php }}</code></pre>` (nunca usa `v-html`).
  2. `"routes/console.php"` — exibe `preview.consolePatch.unifiedDiff` (com badge indicando se o arquivo será criado ou se é somente manual).
  3. `"Passos manuais"` — lista numerada de `preview.manualSteps` com botão `"Copiar trecho"`.

#### `ArtisanCommandModal.vue`
```ts
export interface ArtisanCommandModalProps {
    root: string;
    open: boolean;
}

export interface ArtisanCommandModalEmits {
    (e: 'close'): void;
}
```
- Layout de 2 colunas dentro de `MaxModal`:
  - Coluna esquerda:
    1. `Nome da Classe` (`data-field-path="className"`)
    2. `Signature` (`data-field-path="signatureName"`)
    3. `Descrição` (`data-field-path="description"`)
    4. `ArtisanParamsBuilder`
    5. Checkboxes `Isolatable` e `PromptsForMissingInput` (exibindo warning quando `PromptsForMissingInput` está marcado sem argumentos obrigatórios)
    6. `ArtisanSchedulePicker`
    7. `Objetivo / Regras do Comando` (`MaxTextarea` com 6 linhas, `data-field-path="objective"`)
  - Coluna direita: `ArtisanPhpPreview`.
  - Rodapé:
    - Botão `"Preencher com IA"` (chama `store.fillWithAi()`, desabilitado se `objective.trim()` estiver vazio ou carregando).
    - Botão `"Desfazer sugestão"` (visível quando `store.modal.aiSnapshot !== null`, chama `store.undoAiSuggestion()`).
    - Botão `"Criar com MaxCode"` (chama `store.createWithMaxCode()`, envia `"Criar comando artisan:\n${dados_formatados_do_formulario}"` ao Plan Mode da Fase 11 sem gravar arquivos — CA-24).
    - Botão primário `"Criar"` (desabilitado quando `!store.canCreate`, chama `store.create()`).

#### `QuickCommandsHost.vue`
- Montado uma única vez em `src/App.vue`.
- Renderiza `<ArtisanCommandModal :open="store.modal.open" :root="store.modal.root ?? ''" @close="store.closeModal({ preserveDraft: true })" />`.
- Renderiza o diálogo/popover de confirmação (`MaxPopoverConfirm` / modal de confirmação) vinculado a `store.confirm` para `/install-force`, `/update-force` e `/deploy`, exibindo o título (`store.confirm.title`), a linha de comando exata (`store.confirm.line`) e os botões `"Cancelar"` (`store.resolveConfirm(false)`) e `"Inserir no terminal"` (`store.resolveConfirm(true)`).

---

## 4. Tratamento de Erros e Casos de Borda

1. **Segurança contra XSS no Preview (`ArtisanPhpPreview.vue`):** O código PHP e o diff são renderizados exclusivamente via interpolação textual `{{ ... }}` dentro de `<pre><code>`, jamais com `v-html`.
2. **Fechamento com `Escape`:** Pressionar `Escape` no modal fecha o modal preservando o rascunho daquela raiz em memória e devolve o foco ao `RichInput` do painel de origem (CA-36).
3. **Passos Manuais Pós-Criação (`manualSteps`):** Quando o comando PHP é criado com sucesso mas existem `manualSteps` (ex.: `routes/console.php` já existente ou projeto Laravel 10 com `app/Console/Kernel.php`), o modal exibe o banner de sucesso `"Comando criado em app/Console/Commands/..."` e destaca a aba `"Passos manuais"` para que o usuário possa copiar o snippet antes de fechar (CA-20, CA-22).
