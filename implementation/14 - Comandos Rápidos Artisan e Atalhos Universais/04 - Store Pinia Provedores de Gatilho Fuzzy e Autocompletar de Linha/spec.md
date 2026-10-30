# Implementação 04 — Store Pinia, Provedores de Gatilho (`>` e `/`), Fuzzy e Autocompletar de Linha (`spec.md`)

> **Fase:** 14 — Comandos Rápidos: Artisan (`>`) e Atalhos Universais (`/`)
> **Unidade:** `04 - Store Pinia Provedores de Gatilho Fuzzy e Autocompletar de Linha`
> **Referências Canônicas:** [`../../AGENTS.md`](../../../AGENTS.md) §3, §8.1 e §8.3, [`../instructions.md`](../instructions.md) (I-05 a I-09, I-12, I-13, I-15, I-17, CA-06, CA-10, CA-12 a CA-17, CA-23 a CA-25, CA-31, CA-32, CA-36) e [`../spec.md`](../spec.md) (§2, §10.2 a §10.7, §11, §12, §13, §15.2).

---

## 1. Contexto, Objetivo e Escopo

Esta quarta implementação constrói toda a camada lógica e reativa do frontend para Comandos Rápidos:
1. Ponte isolada de adaptadores (`adapters.ts`) para os contratos das Fases 04 (`TriggerRegistry`), 06 (`useProjectsStore`), 11 (`useMaxCodeBridgeStore`) e 12 (`useAiGenerate`).
2. Algoritmo de ranqueamento e destaque fuzzy O(n·m) (`fuzzy.ts`) com bônus de prefixo e fronteiras de namespace (`:`, `-`, `_`, `.`, ` `).
3. Parser de linha de comando Artisan (`artisanLineParser.ts`) que tokeniza até o cursor respeitando aspas simples/duplas, escapes `\`, operadores shell e `root.invokerTokens`, identificando o comando ativo, as opções já utilizadas e se o usuário está completando uma `--opção` ou aguardando dica de argumento posicional.
4. Store Pinia `useQuickCommandsStore` (`src/stores/quickCommands.ts`) gerenciando o cache de raízes Laravel por `(projectId, cwd)`, catálogos com `markRaw` e deduplicação de promessas em voo, atalhos por `cwd`, estado completo do modal com controle estrito de revisões (`contextRevision`, `formRevision`, `previewRevision`), integração com IA (`fillWithAi` / `undoAiSuggestion`), envio ao MaxCode (`createWithMaxCode`) e fila de confirmação (`requestConfirm`).
5. Provedores de gatilho registrados no `TriggerRegistry`: `artisanTrigger` (`>`), `artisanArgsTrigger` (ativado por tokens de invocador — `php artisan`, `./artisan`, `sail artisan`, `./vendor/bin/sail artisan` — e não por 4 prefixos fixos) e `shortcutTrigger` (`/`). O registro e a inicialização da store ficam em `src/terminal/triggers/providers/index.ts` (criado nesta fase; estendido pela Fase 15 com `>>` — D-09).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Novos Arquivos em `src/`
- `src/composables/quick-commands/adapters.ts`
- `src/composables/quick-commands/fuzzy.ts`
- `src/composables/quick-commands/artisanLineParser.ts`
- `src/stores/quickCommands.ts`
- `src/composables/quick-commands/useArtisanTrigger.ts`
- `src/composables/quick-commands/useShortcutTrigger.ts`
- `src/terminal/triggers/providers/index.ts` (`registerTriggerProviders()`: registra `>`, `artisan.args:*` e `/`; chama `useQuickCommandsStore().init()`)

### 2.2. Arquivos Modificados em `src/`
- Nenhum arquivo de bootstrap (`src/app/main.ts`) é alterado: a inicialização de `useQuickCommandsStore().init()` e o registro dos gatilhos ocorrem em `src/terminal/triggers/providers/index.ts`. Os composables em `src/composables/quick-commands/` permanecem apenas como lógica dos provedores.

### 2.3. Testes Unitários Vitest a Criar
- `src/composables/quick-commands/__tests__/fuzzy.spec.ts`
- `src/composables/quick-commands/__tests__/artisanLineParser.spec.ts`
- `src/composables/quick-commands/__tests__/useArtisanTrigger.spec.ts`
- `src/composables/quick-commands/__tests__/useShortcutTrigger.spec.ts`
- `src/composables/quick-commands/__tests__/maxcodeCacheCompat.spec.ts`
- `src/stores/__tests__/quickCommands.spec.ts`

---

## 3. Especificação Técnica e Assinaturas Exatas

### 3.1. Adaptadores de Fronteira (`src/composables/quick-commands/adapters.ts`)

```ts
import type {
    CompletionResult,
    Disposable,
    TerminalContext,
    TriggerDefinition
} from '@/types/triggers'; // ou caminho canônico da Fase 04
import type { MtError, PaneId, ProjectId } from '@/types/bindings';
import type { AiGenerateRunOptions } from '@/composables/ai/useAiGenerate';

export const TRIGGER_PASSTHROUGH: CompletionResult = Object.freeze({
    items: [],
    passthrough: true
});

export interface AiStructuredGenerator<T> {
    loading: Readonly<{ value: boolean }>;
    error: Readonly<{ value: MtError | null }>;
    /** `null` = sem resultado utilizável ou cancelado (contrato da Fase 12). */
    run: (prompt: string, options?: AiGenerateRunOptions) => Promise<T | null>;
    cancel: () => Promise<void>;
}

export interface QuickCommandsAdapters {
    registerTrigger: (def: TriggerDefinition) => Disposable;
    projectStackHint: (projectId: ProjectId | null) => string | null;
    openMaxCodePlan: (prompt: string, projectPath: string) => Promise<void>;
    createAiGenerator: <T>(
        schemaId: string,
        schema: Record<string, unknown>,
        decode: (value: unknown) => T
    ) => AiStructuredGenerator<T>;
    notifyToast: (opts: { title: string; description?: string; severity: 'success' | 'info' | 'warn' | 'error' }) => void;
    focusPaneInput: (paneId: PaneId | null) => void;
}

export function getQuickCommandsAdapters(): QuickCommandsAdapters;

/** Schema registrado na Fase 12 para a Geração Assistida do modal (D-05). */
export const ARTISAN_COMMAND_SCHEMA_ID = 'artisan.command.v1';

/**
 * Implementação padrão de `createAiGenerator`: delega a `useAiGenerate<T>(schemaId, schema, decode)` (Fase 12),
 * sempre com os 3 argumentos. `decode` valida o objeto devolvido pela IA (ex.: `decodeArtisanCommandForm`, que rejeita campos
 * desconhecidos e tipos inválidos lançando erro), `run(prompt, { signal })` repassa o `AbortSignal` e o retorno `null`
 * é propagado sem conversão — a store trata `null` como "nenhuma sugestão" (toast pt-BR, formulário intacto).
 */
export function setQuickCommandsAdaptersForTest(overrides: Partial<QuickCommandsAdapters> | null): void;
```

### 3.2. Filtro e Destaque Fuzzy (`src/composables/quick-commands/fuzzy.ts`)

```ts
import type { ArtisanCommand, ArtisanOption } from '@/types/bindings';

export interface FuzzyMatchResult {
    score: number;
    indices: number[];
}

export function fuzzyMatch(query: string, target: string): FuzzyMatchResult | null;
export function fuzzyScore(query: string, target: string): number | null;
export function rankCommands(query: string, commands: readonly ArtisanCommand[]): ArtisanCommand[];
export function rankOptions(query: string, options: readonly ArtisanOption[]): ArtisanOption[];
```

**Regras do Algoritmo Fuzzy (CA-12):**
- `query === ''` retorna `{ score: 0, indices: [] }`.
- Match exato (`t === q`): `score = 10_000`.
- Prefixo (`t.startsWith(q)`): `score = 5_000 - (t.length - q.length)` (`mig` posiciona `migrate` antes de `migrate:fresh` — CA-12).
- Subsequência: cada caractere encontrado soma `+10`; se estiver no índice `0` ou logo após fronteira (`:`, `-`, `_`, `.`, `' '`), soma `+15` (`mfr` posiciona `migrate:fresh` no topo — CA-12); se for consecutivo (`i === last + 1`), soma `+8`; penaliza lacuna `-Math.min(i - ti, 5)`.
- Em `rankCommands`, aliases recebem penalidade `-100` em relação ao match no `name`. Desempate: maior score → menor `name.length` → `a.name.localeCompare(b.name)`.

### 3.3. Parser de Linha Artisan (`src/composables/quick-commands/artisanLineParser.ts`)

```ts
import type { ArtisanCommand, ArtisanRoot } from '@/types/bindings';

export interface LineToken {
    text: string;
    rawText: string;
    start: number;
    end: number;
    quoted: boolean;
}

export interface ArtisanLine {
    invoker: string;
    command: string | null;
    current: LineToken;
    usedOptions: Set<string>;
    positionalsBefore: number;
    mode: 'option' | 'argument-hint' | 'none';
}

export function parseArtisanLine(
    input: string,
    cursor: number,
    root: ArtisanRoot,
    resolveCommand?: (name: string) => ArtisanCommand | undefined
): ArtisanLine | null;
```

**Regras Críticas do `parseArtisanLine` (CA-14, CA-15):**
1. Se `cursor < 0` ou `cursor > input.length`, ou se antes do cursor (fora de aspas) houver operadores de encadeamento/redirecionamento shell (`|`, `;`, `&&`, `||`, `>`, `<`, `\n`, `` ` ``, `$(`), retorna `null` (passthrough).
2. Tokeniza `input.slice(0, cursor)` preservando `start` e `end` no `input` original (e estende o token atual até o fim da palavra sob o cursor se o cursor estiver no meio de um token).
3. Compara os primeiros `N` tokens completos com `root.invokerTokens` (aceitando equivalentes canônicos como `php artisan`, `./vendor/bin/sail artisan`, `vendor/bin/sail artisan`, `sail artisan` quando compatíveis com `root.runner`, bem como caminhos relativos quotados em `root.invokerTokens` como `php '../Meu Projeto/artisan'`). Se não casar, retorna `null`.
4. Se o usuário ainda está digitando o nome do comando Artisan (o token seguinte ao invocador ainda não foi finalizado com espaço), retorna `ArtisanLine` com `command: null, mode: 'none'`.
5. Após identificar o token do comando (`command`), percorre os tokens completos anteriores ao token sob o cursor (`current`):
   - Se encontrar `--` isolado completo, todos os tokens subsequentes são posicionais (`mode: 'argument-hint'` ou `'none'`, nunca `'option'`).
   - Se o token começa com `--`:
     - Se contém `=` (ex.: `--database=mysql`), adiciona `--database` em `usedOptions`.
     - Se não contém `=` (ex.: `--database`), consulta a definição da opção no `ArtisanCommand`: se `acceptValue === true && isValueRequired === true`, o próximo token (quando não começar com `-`) é consumido como valor da opção (não incrementa `positionalsBefore`).
   - Se o token começa com `-` simples (ex.: `-d`), resolve pelo `shortcut` da opção no `ArtisanCommand`, adiciona o `--name` correspondente em `usedOptions` e consome o próximo token se `isValueRequired === true`.
   - Caso contrário, incrementa `positionalsBefore`.
6. Determina `mode` para o token sob o cursor (`current`):
   - Se `current.text.startsWith('-')` (e não passou por `--` terminador anterior): `mode = 'option'`.
   - Se `current.text === ''` (cursor após espaço) e o `ArtisanCommand` possui argumentos definidos (`arguments.length > 0`) onde `positionalsBefore < arguments.length` ou o último argumento tem `isArray === true`: `mode = 'argument-hint'`.
   - Caso contrário: `mode = 'none'`.

### 3.4. Store Pinia `useQuickCommandsStore` (`src/stores/quickCommands.ts`)

```ts
export function createEmptyArtisanForm(seed?: { signatureName?: string }): ArtisanCommandForm;
export function suggestClassNameFromSignature(signatureName: string): string;

export const useQuickCommandsStore = defineStore('quickCommands', () => {
    // State, Getters e Actions exatamente conforme spec §10.2
});
```

**Regras de Concorrência e Estado da Store (CA-19, CA-23, CA-24, CA-25, CA-36):**
1. **`markRaw`:** Todo `ArtisanCatalog` recebido do IPC é envolvido em `markRaw(catalog)` antes de ser gravado em `catalogs[root].catalog` para evitar overhead de proxies Vue sobre 400+ comandos.
2. **Deduplicação em Voo (`inFlightCatalogs`):** `ensureCatalog(root, { force })` reutiliza a mesma `Promise<ArtisanCatalog>` enquanto houver uma chamada em andamento para aquela raiz.
3. **Backoff no Frontend:** Quando `catalogGet(root)` falha e não há catálogo em memória, grava `catalogs[root].error` e `failedAtMs`. Durante 5 s, novas chamadas de `provide` com `force !== true` leem o erro imediatamente sem disparar IPC nem criar `refreshAfter` resolvido (evita loop infinito de re-render do popup — CA-36, CA-37).
4. **Rascunho por Raiz e Revisões (`formRevision`, `previewRevision`, `contextRevision`):**
   - `updateForm(patch)` incrementa `modal.formRevision` e agenda o preview debounced (250 ms via `useDebounceFn`).
   - Quando `quickCommandsIpc.preview(root, snapshotForm)` retorna, só aplica `modal.preview` e `modal.report` se `modal.open && modal.root === reqRoot && modal.formRevision === reqFormRevision` (descarta respostas fora de ordem — CA-36).
   - `canCreate` exige `Boolean(modal.open && modal.report?.valid && modal.preview?.php && !modal.creating && modal.previewRevision === modal.formRevision)`.
   - Fechar o modal com `Escape` ou botão Cancelar (`closeModal({ preserveDraft: true })`) guarda o rascunho atual num mapa `draftsByRoot`, cancela qualquer requisição de IA pendente e restaura o foco ao input do painel de origem (`modal.paneId`) (CA-36).
5. **Geração Assistida por IA (`fillWithAi` / `undoAiSuggestion` — CA-25):**
   - Captura `reqFormRevision = modal.formRevision` e o `form` antes de iniciar.
   - Obtém o JSON Schema via `quickCommandsIpc.formSchema()` (memorizado) e chama `aiGenerator.run(prompt, { signal })` (gerador criado com `createAiGenerator<ArtisanCommandForm>(ARTISAN_COMMAND_SCHEMA_ID, schema, decodeArtisanCommandForm)`).
   - Valida a resposta com `quickCommandsIpc.validateForm(root, candidateForm)`. Se `run` retornar `null` (sem resultado utilizável ou cancelado), se `decode` lançar, se houver qualquer issue com `severity === 'error'` ou falha na IA, exibe toast em `pt-BR` ("Não foi possível gerar uma sugestão válida com a IA.") e mantém `modal.form` intacto.
   - Se o usuário editou o formulário enquanto a IA processava (`modal.formRevision !== reqFormRevision`), não sobrescreve silenciosamente; preserva a edição do usuário e notifica.
   - Quando válida e sem colisão, salva cópia profunda do formulário anterior em `modal.aiSnapshot`, aplica a sugestão e dispara atualização imediata do preview. `undoAiSuggestion()` restaura `modal.aiSnapshot` e limpa `aiSnapshot`.
6. **Criar Comando (`create` — CA-20, CA-21, CA-23):**
   - Invoca `quickCommandsIpc.create(root, modal.form, modal.preview.revision)`.
   - Se retornar erro `artisan.preview_stale`, recalcula o preview imediatamente, preserva o formulário e avisa com toast para revisar e clicar novamente em "Criar".
   - Se retornar sucesso com `result.manualSteps.length === 0`, fecha o modal, limpa o rascunho da raiz e exibe toast de sucesso. Se houver `result.manualSteps.length > 0`, armazena `modal.lastCreateResult = result` mantendo o modal aberto na aba "Passos manuais".

### 3.5. Provedores `useArtisanTrigger.ts` e `useShortcutTrigger.ts`

- **`artisanTrigger` (`>` — CA-10, CA-12, CA-13, CA-16, CA-17):**
  - `isAvailable(ctx)`: retorna `false` se `!ctx.cwd`; consulta `store.resolveRoot(ctx.cwd, ctx.projectId)` e retorna ` root !== null` (funciona em projetos Laravel+Vue mesmo quando a stack primária é Vue — CA-10).
  - `provide(q, ctx, signal)`:
    - Se `q.query.startsWith('>')` (`>>` da Fase 15) ou `/^\s/.test(q.query)` (redirecionamento shell `> arquivo.txt`): retorna `TRIGGER_PASSTHROUGH` (CA-17).
    - Constrói sempre `createItem` com `id: 'artisan:create'`, `label: 'Criar comando'`, `kind: 'action'`, `pinned: 'bottom'` (CA-16).
    - Se o catálogo ainda está carregando, retorna `{ items: [createItem], stageLabel: 'Carregando comandos Artisan…', refreshAfter: pending }`.
    - Se houve falha em backoff sem cache, retorna `{ items: [createItem], stageLabel: errorMsg }` sem `refreshAfter`.
    - Com catálogo disponível, filtra via `rankCommands(q.query, catalog.commands).slice(0, 200)` e anexa `createItem` no fim.
  - `onSelect(item, q, ctx)`:
    - Para `'artisan:create'`: abre `store.openCreateModal(root, { signatureName: validSeed, paneId: ctx.paneId })` e retorna `{ kind: 'handled' }`.
    - Para um comando Artisan: retorna `{ kind: 'replace-input', text: `${invoker} ${name} `, cursor: text.length, keepOpen: true }` (nunca envia ao PTY — CA-13).
- **`artisanArgsTrigger` (ativação por tokens: `php artisan`, `./artisan`, `sail artisan`, `./vendor/bin/sail artisan` — CA-14, CA-15):**
   - Registrado por primeiro token de invocador (`php`, `./artisan`, `sail`, `./vendor/bin/sail`, `vendor/bin/sail`; `id: 'artisan.args:<token>'`); a decisão de ativar vem de `parseArtisanLine` (tokens completos comparados com `root.invokerTokens` e equivalentes canônicos), nunca de um prefixo textual de 3 caracteres. `./artisan` é invocador completo, sem segundo token `artisan`; demais formas exigem esse token. Correspondência inválida ⇒ `TRIGGER_PASSTHROUGH`.
  - Usa `parseArtisanLine(q.input, q.cursor, root, (name) => store.commandByName(root.root, name))`.
  - Em `mode === 'option'`: lista opções ainda não usadas (ou com `isMultiple === true`), separadas em seções `"Opções"` e `"Opções globais"`. Selecionar uma opção com `acceptValue === true` substitui apenas o intervalo `[current.start, current.end]` do token por `${opt.name}=` mantendo o cursor logo após `=` (`keepOpen: false`); selecionar flag booleana insere `${opt.name} ` (`keepOpen: true`). Se `acceptValue` for `null`/`undefined` (cache legado ainda em revalidação), insere `${opt.name} ` sem inventar `=` (CA-14).
  - Em `mode === 'argument-hint'`: retorna 1 item `disabled: true` descrevendo o argumento posicional atual (`<name>` se obrigatório ou `[name]` se opcional, com descrição e valor padrão — CA-15).
- **`shortcutTrigger` (`/` — CA-31, CA-32, CA-35, CA-40):**
  - `provide(q, ctx)`: se `q.query` contiver espaço, `/` (ex.: `/usr/bin/env`) ou se nenhum dos 7 atalhos começar com `/${q.query.toLowerCase()}`, retorna `TRIGGER_PASSTHROUGH` (CA-32).
  - `onSelect(item, q, ctx)`:
    - Chama `quickCommandsIpc.shortcutResolve(ctx.cwd, id)` para revalidar evidências no instante da seleção. Se `fresh.evidenceRevision !== item.payload.evidenceRevision` ou `!fresh.line`, atualiza a lista na store e mantém o popup sem inserir comando obsoleto (CA-40).
    - Se `fresh.requiresConfirmation` (`/install-force`, `/update-force`, `/deploy`), aguarda `store.requestConfirm(...)`. Se recusado ou se o painel mudar, retorna `{ kind: 'close' }` (CA-31).
    - Se confirmado (ou quando não requer confirmação), retorna `{ kind: 'replace-input', text: fresh.line, cursor: fresh.line.length }` sem executar no PTY (CA-31).

---

## 4. Tratamento de Erros e Casos de Borda

1. **Compatibilidade Verbatim com o Leitor do MaxCode (CA-06):** O teste `maxcodeCacheCompat.spec.ts` inclui o port fiel de `lerCacheArtisan` e `normalizarArtisanJson` do MaxCode (`artisanDiscovery.ts`), provando que `artisan.json` gerado pelo Rust é lido diretamente por `lerCacheArtisan` e que o JSON do Symfony Console é normalizado sem divergências.
2. **Troca de Painel ou Fechamento Durante Confirmação / IA:** `requestConfirm` vincula `paneId`; se o painel ativo mudar ou `dispose()` for chamado, a Promise resolve para `false` e fecha o popover sem modificar o input do novo painel (CA-36).
3. **Cursor no Meio da Linha ao Completar `--opção`:** A substituição de flag preserva integralmente o texto à direita do token atual (`input.slice(0, token.start) + replacement + input.slice(token.end)`).
