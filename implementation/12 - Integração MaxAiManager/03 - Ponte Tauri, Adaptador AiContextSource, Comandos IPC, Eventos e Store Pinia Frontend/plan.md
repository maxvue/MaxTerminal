# Implementação 03 — Plano de Execução Passo a Passo (`plan.md`)

> **Fase:** 12 — Integração MaxAiManager (`@` / `?`)
> **Unidade:** `03 - Ponte Tauri, Adaptador AiContextSource, Comandos IPC, Eventos e Store Pinia Frontend`
> **Especificação técnica:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1 — Conectar `mt-ai` ao Binário `maxterminal` e Implementar `TauriAiContextSource`:**
  1. Adicionar `mt-ai = { path = "crates/mt-ai" }` em `src-tauri/Cargo.toml`.
  2. Criar `src-tauri/src/ai_context.rs` implementando `TauriAiContextSource` (`AiContextSource`) integrado aos serviços das Fases 05, 06 e 07.
  3. Conectar o listener de ciclo de vida do `WorkspaceObserver` (Fase 07) para chamar `ai_service.requests.cancel_pane(pane_id)` ao fechar painel/aba, substituir PTY ou arquivar sessão.

- [ ] **Passo 2 — Implementar os 17 Comandos Tauri IPC e Emissão de Eventos (`src-tauri/src/commands/ai.rs`):**
  1. Criar `src-tauri/src/commands/ai.rs` com os 17 comandos anotados com `#[tauri::command]` e `#[specta::specta]`: `ai_health`, `ai_settings_get`, `ai_settings_update`, `ai_catalog_get`, `ai_catalog_refresh`, `ai_context_preview`, `ai_suggest_command`, `ai_cancel`, `ai_classify_risk`, `ai_diagnose_block`, `ai_diagnosis_dismiss`, `ai_rag_list_slugs`, `ai_rag_search`, `ai_generate_structured`, `ai_history_list`, `ai_history_clear`, `ai_cache_clear`.
  2. Configurar em `src-tauri/src/state.rs` a inicialização de `AiService` e uma task leve que observa `ai_service.subscribe_status()` e emite `ai://status-changed` via `AppHandle::emit` em toda transição de `GatewayStatus`.
  3. Registrar os 17 comandos e os 3 eventos (`ai://status-changed`, `ai://catalog-updated`, `ai://settings-changed`) em `src-tauri/src/specta.rs` e regerar `src/types/bindings.ts` (`CA-02`, `CA-04`).

- [ ] **Passo 3 — Criar o Wrapper IPC (`src/ipc/ai.ts`) e o Adaptador Frontend (`src/composables/ai/adapters.ts`):**
  1. Criar `src/ipc/ai.ts` exportando todas as funções tipadas e criando `Channel<AiStreamEvent>` em `suggestCommand`.
  2. Criar `src/composables/ai/adapters.ts` implementando `getPaneContext`, `isRichShellPane`, `insertIntoInput`, `writeToPty`, `pushRichBlock`, `removeRichBlock`, `clearPaneBlocks` e `usePaneRichBlocks`.

- [ ] **Passo 4 — Implementar a Store Pinia `useAiStore` (`src/stores/ai.ts`):**
  1. Criar `src/stores/ai.ts` com estado, getters e actions conforme §3.5 da `spec.md`.
  2. Implementar controle de revisão monotônica (`configRevision`, `editRevision`), cancelamento precoce antes do aceite IPC, debounce de `150 ms` em `editCommand` com `editedRisk = null` enquanto pendente, e limpeza de cards ao encerrar sessão/painel.

- [ ] **Passo 5 — Implementar o Composable `useAiGenerate<T>` (`src/composables/ai/useAiGenerate.ts`):**
  1. Criar `src/composables/ai/useAiGenerate.ts` com `loading`, `error`, `result`, `repaired`, `run()` e `cancel()`.
  2. Garantir execução obrigatória do `decode(response.value)` de runtime (capturando exceções do decoder como erro `AI_SCHEMA_VALIDATION`) e validação de `getFormRevision()` antes de escrever em `result.value` (`CA-28`, `CA-36`).

- [ ] **Passo 6 — Escrever e Executar Testes Unitários (Rust Workspace + Vitest):**
  1. Criar `tests/stores/ai.spec.ts` cobrindo `init()`, `updateSettings()`, getters de catálogo, ciclo `Started -> Delta -> Completed / Failed / Cancelled`, cancelamento antes do aceite IPC, substituição de sugestão no mesmo painel, `editCommand` reclassificando risco com debounce e limpeza ao desligar diagnóstico (`CA-24`, `CA-33`).
  2. Criar `tests/composables/ai/useAiGenerate.spec.ts` simulando um consumidor da Fase 14 (`artisan.command.v1`), testando `loading`, `result`, `error`, `cancel()`, falha do decoder de runtime e descarte de resposta quando a revisão do formulário muda (`CA-28`, `CA-36`).

---

## 2. Comandos de Verificação e Validação

Execute os comandos abaixo a partir da raiz da worktree:

```bash
# 1. Compilar e testar todo o workspace Rust incluindo o binário maxterminal e verificar bindings.ts (CA-02, CA-04)
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --workspace

# 2. Verificar typecheck TypeScript (tsgo / vue-tsc)
npm run typecheck
npm run typecheck:vue

# 3. Executar testes unitários da store Pinia e de useAiGenerate no Vitest (CA-28, CA-33, CA-36)
npx --no-install vitest run tests/stores/ai.spec.ts tests/composables/ai/useAiGenerate.spec.ts
```

---

## 3. Mapeamento dos Critérios de Aceite da Fase 12

| Critério | Cobertura nesta Implementação | Teste / Verificação |
|----------|-------------------------------|---------------------|
| `CA-02` | `cargo test --workspace` passa e geração de `src/types/bindings.ts` não produz divergência. | `cargo test --manifest-path src-tauri/Cargo.toml --workspace` |
| `CA-04` | `bindings.ts` exporta todos os 17 comandos (`aiHealth` até `aiCacheClear`). | `npm run typecheck` + `tests/stores/ai.spec.ts` |
| `CA-28` | `useAiGenerate<T>()` expõe `loading`, `error`, `result`, `run()`, `cancel()` e funciona com consumidor fictício da Fase 14. | `tests/composables/ai/useAiGenerate.spec.ts` |
| `CA-33` | Cancelar antes do aceite IPC, trocar URL, iniciar nova sessão, substituir PTY ou fechar painel descarta respostas antigas e não ressuscita cards nem formulários. | `tests/stores/ai.spec.ts` + `tests/composables/ai/useAiGenerate.spec.ts` |
| `CA-36` (frontend) | `useAiGenerate<T>` aplica decoder de runtime sobre o JSON retornado e respeita a revisão do formulário consumidor. | `tests/composables/ai/useAiGenerate.spec.ts::validates_runtime_decoder_and_form_revision` |
