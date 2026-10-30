# Implementação 04 — Store Pinia, Provedores de Gatilho (`>` e `/`), Fuzzy e Autocompletar de Linha (`plan.md`)

> **Fase:** 14 — Comandos Rápidos: Artisan (`>`) e Atalhos Universais (`/`)
> **Unidade:** `04 - Store Pinia Provedores de Gatilho Fuzzy e Autocompletar de Linha`
> **Especificação associada:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Execução

### Passo 1 — Adaptadores (`adapters.ts`), Fuzzy (`fuzzy.ts`) e Parser de Linha (`artisanLineParser.ts`)
- [ ] **1.1.** Implementar `src/composables/quick-commands/adapters.ts` isolando a comunicação com `TriggerRegistry` (Fase 04), `useProjectsStore` (Fase 06), `useMaxCodeBridgeStore` (Fase 11), `useAiGenerate` (Fase 12 — `useAiGenerate<T>(schemaId, schema, decode)` com `schemaId = 'artisan.command.v1'`, `decode` validando o objeto, repasse de `{ signal }` e tratamento de retorno `null`) e `Toast` (`@maxvue/max-components-ui`), incluindo `setQuickCommandsAdaptersForTest`.
- [ ] **1.2.** Implementar `src/composables/quick-commands/fuzzy.ts` com `fuzzyMatch`, `fuzzyScore`, `rankCommands` e `rankOptions`.
- [ ] **1.3.** Criar `src/composables/quick-commands/__tests__/fuzzy.spec.ts` validando que `mig` ranqueia `migrate` antes de `migrate:fresh`, `mfr` ranqueia `migrate:fresh` no topo, aliases sofrem penalidade `-100` e índices destacados são retornados corretamente (CA-12).
- [ ] **1.4.** Implementar `src/composables/quick-commands/artisanLineParser.ts` suportando `root.invokerTokens` (inclusive caminhos quotados com espaços), aspas simples/duplas, escapes `\`, terminador `--`, opções com `=` ou valor separado, atalhos curtos `-d` e modos `'option' | 'argument-hint' | 'none'`.
- [ ] **1.5.** Criar `src/composables/quick-commands/__tests__/artisanLineParser.spec.ts` cobrindo todos os formatos de invocador (`php artisan`, `php ../artisan`, `php '../Meu Projeto/artisan'`, `./vendor/bin/sail artisan`, `sail artisan`), preservação de sufixo com cursor no meio da linha, operadores shell (`|`, `&&`, `>`) retornando `null` e contagem de argumentos posicionais.

### Passo 2 — Store Pinia `useQuickCommandsStore` (`src/stores/quickCommands.ts`)
- [ ] **2.1.** Implementar `src/stores/quickCommands.ts` com cache de raízes por `(projectId, cwd)`, catálogos com `markRaw`, deduplicação de promessas em voo (`ensureCatalog`), backoff de erro de 5 s, listeners IPC (`artisan://catalog-updated`, `artisan://discovery-failed`, `artisan://command-created`), controle de revisões do modal (`contextRevision`, `formRevision`, `previewRevision`), `updateForm` com debounce de 250 ms, `create`, `createWithMaxCode`, `fillWithAi`, `undoAiSuggestion` e `requestConfirm`.
- [ ] **2.2.** Criar `src/stores/__tests__/quickCommands.spec.ts` testando:
  - Deduplicação de chamadas concorrentes a `ensureCatalog`.
  - Atualização automática ao receber `artisan://catalog-updated` e `artisan://command-created` (CA-23).
  - Descarte de respostas de preview e IA fora de ordem quando `formRevision` ou `contextRevision` mudam (CA-36).
  - `fillWithAi` rejeitando resposta inválida com toast em `pt-BR` sem alterar o formulário, aplicando resposta válida e restaurando o snapshot em `undoAiSuggestion` (CA-25).
  - `createWithMaxCode` invocando `artisan_format_maxcode_prompt` e repassando a string para `openMaxCodePlan(prompt, root, 'artisan_create')` (CA-24).

### Passo 3 — Provedores de Gatilho (`useArtisanTrigger.ts`, `useShortcutTrigger.ts`, `src/terminal/triggers/providers/index.ts`)
- [ ] **3.1.** Implementar `artisanTrigger` e `artisanArgsTrigger` em `src/composables/quick-commands/useArtisanTrigger.ts`.
- [ ] **3.2.** Implementar `shortcutTrigger` em `src/composables/quick-commands/useShortcutTrigger.ts`.
- [ ] **3.3.** Criar `src/terminal/triggers/providers/index.ts` com `registerTriggerProviders()` (registra `>`, `artisan.args:*` por token de invocador e `/` no `triggerRegistry` e chama `useQuickCommandsStore().init()`); não registrar via `src/composables/quick-commands/` nem alterar `src/app/main.ts`. A Fase 15 estende este arquivo com `>>`.
- [ ] **3.4.** Criar `src/composables/quick-commands/__tests__/useArtisanTrigger.spec.ts`, `useShortcutTrigger.spec.ts` e `maxcodeCacheCompat.spec.ts` validando:
  - `>>` e `> arquivo.txt` retornando `TRIGGER_PASSTHROUGH` (CA-17).
  - Projeto sem raiz Laravel retornando `isAvailable: false`, e projeto Laravel+Vue com stack primária Vue retornando `isAvailable: true` (CA-10).
  - Seleção de comando Artisan substituindo o input por `php artisan migrate ` sem nenhuma escrita no PTY (CA-13).
  - Autocompletar de `--database` inserindo `--database=` com cursor após `=`, `--force` inserindo `--force `, e opções não-múltiplas já usadas não reaparecendo (CA-14).
  - Dica não selecionável do argumento posicional em `php artisan make:model ` (CA-15).
  - Ativação de `artisanArgsTrigger` por tokens de invocador (`php artisan`, `./artisan`, `sail artisan`, `./vendor/bin/sail artisan`) e passthrough para `php -v`/`sail up` (T-18).
  - Opção `"Criar comando"` sempre no final da lista (inclusive em loading, erro e `>zzzzz`) abrindo o modal com `signatureName` pré-preenchida quando válida (CA-16, CA-36).
  - `/usr/bin/env` retornando `TRIGGER_PASSTHROUGH` (CA-32).
  - `/install-force`, `/update-force` e `/deploy` solicitando confirmação antes de inserir e fechando sem inserir quando recusado (CA-31).
  - Compatibilidade do `artisan.json` gerado pelo Rust com `lerCacheArtisan` do MaxCode (CA-06).

---

## 2. Comandos de Verificação e Validação

Execute a partir da raiz da worktree a raiz da worktree ativa:

```bash
cd "$(git rev-parse --show-toplevel)"
npx --no-install vitest run src/composables/quick-commands/__tests__/ src/stores/__tests__/quickCommands.spec.ts
npm run typecheck
npm run lint
```

---

## 3. Mapeamento de Critérios de Aceite

| Critério | Cobertura nesta Implementação |
|----------|-------------------------------|
| **CA-06** | Teste `maxcodeCacheCompat.spec.ts` validando leitura direta do `artisan.json` pelo port de `lerCacheArtisan()` do MaxCode. |
| **CA-10** | `isAvailable` baseado na raiz Laravel validada, independente da stack primária. |
| **CA-12 / CA-13** | Ranqueamento fuzzy (`>mig`, `>mfr`) e substituição de input (`php artisan migrate `) sem escrita no PTY. |
| **CA-14 / CA-15** | Autocompletar de flags (`--database=`, `--force `) e dica de argumento posicional (`make:model `). |
| **CA-16 / CA-17** | Item fixo `"Criar comando"` sempre no rodapé e passthrough imediato para `>>`. |
| **CA-23 / CA-24 / CA-25** | Reação da store a `artisan://command-created`, envio para MaxCode Plan Mode e fluxo completo de `"Preencher com IA"` + `"Desfazer sugestão"`. |
| **CA-31 / CA-32** | Confirmação obrigatória para `*-force` e `/deploy`; passthrough para caminhos como `/usr/bin/env`. |
| **CA-36** | Controle de revisão contra condições de corrida em preview/IA/troca de painel e preservação de rascunho. |
