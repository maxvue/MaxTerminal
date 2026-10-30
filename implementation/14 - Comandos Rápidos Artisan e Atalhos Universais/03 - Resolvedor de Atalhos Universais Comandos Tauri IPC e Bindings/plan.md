# Implementação 03 — Resolvedor de Atalhos Universais (`/`), Comandos Tauri IPC e Bindings (`plan.md`)

> **Fase:** 14 — Comandos Rápidos: Artisan (`>`) e Atalhos Universais (`/`)
> **Unidade:** `03 - Resolvedor de Atalhos Universais Comandos Tauri IPC e Bindings`
> **Especificação associada:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Execução

### Passo 1 — Modelos e Leitura de Manifestos de Atalhos (`model.rs`, `manifests.rs`, `maxcode_config.rs`)
- [ ] **1.1.** Implementar `src-tauri/crates/mt-commands/src/shortcuts/model.rs` com `ShortcutId`, `PackageManager`, `YarnVariant`, `ManagerEvidence`, `DetectedManager`, `PartSource`, `ShortcutPart`, `ResolvedShortcut` e `ShortcutList`.
- [ ] **1.2.** Implementar `src-tauri/crates/mt-commands/src/shortcuts/manifests.rs` para parsing seguro (`read_capped(2 MiB)`) de `package.json`, `composer.json` e `Cargo.toml`, ignorando o script `test` padrão do `npm init` (`echo "Error: no test specified" && exit 1`).
- [ ] **1.3.** Implementar `src-tauri/crates/mt-commands/src/shortcuts/maxcode_config.rs` lendo `config-trabalho.json` (`versao: 1`, herança `geral` → `projetos[raiz]`, string vazia limpando herança, cache por `(path, mtime)` e ignorando ausência de arquivo sem erro).

### Passo 2 — Detecção Baseada em Evidências e Resolução (`detect.rs`, `resolve.rs`, `service.rs`)
- [ ] **2.1.** Implementar `src-tauri/crates/mt-commands/src/shortcuts/detect.rs` detectando `Composer`, gerenciador `JS` (`packageManager` → lockfile único → fallback `npm` quando sem lockfile, ou `conflict_reason` quando houver múltiplos lockfiles de famílias distintas sem `packageManager`) e `Cargo` (raiz ou `src-tauri/Cargo.toml`), respeitando fronteira `.git`.
- [ ] **2.2.** Implementar `src-tauri/crates/mt-commands/src/shortcuts/resolve.rs` e `service.rs` com a matriz de capacidades (Yarn Classic vs Modern `yarn up -R '*'`, flags force indisponíveis desabilitadas, `/deploy` estritamente explícito, composição `Composer → JS → Cargo` unida por ` && `, wrapping POSIX `sh -c` quando `cwd != root` e `evidence_revision`).
- [ ] **2.3.** Criar as fixtures em `src-tauri/crates/mt-commands/tests/fixtures/shortcuts/` (`laravel-vue`, `tauri-pnpm`, `npm-default-test`, `multi-lock`, `maxcode-config`) e o teste de integração `src-tauri/crates/mt-commands/tests/shortcuts.rs` validando:
  - `laravel-vue`: `/install` → `composer install && npm install`; `/update-force` → `composer update --no-cache && npm update --force`; `/build` → `npm run build`; `/test` → `php artisan test` (CA-26).
  - `tauri-pnpm`: `/install` → `pnpm install && cargo build --manifest-path src-tauri/Cargo.toml`; `/test` → `pnpm run test && cargo test --manifest-path src-tauri/Cargo.toml` (CA-27).
  - `npm-default-test`: `/test` desabilitado com `"Nenhum comando de teste encontrado"` (CA-28).
  - `multi-lock`, `npm-shrinkwrap.json`, `bun.lock`/`bun.lockb`, Yarn Classic/Modern e `packageManager` inválido (CA-29, CA-35).
  - `maxcode-config`: override de `comandoBuild`/`comandoDeploy` e fallback quando ausente (CA-30).
  - `/deploy` sem script ou config sempre desabilitado em Laravel, Tauri e Cargo (CA-35).
  - `cwd` em subdiretório e caminhos com espaços/apóstrofos gerando quoting seguro (CA-40).

### Passo 3 — Integração no Binário Tauri (`src-tauri/src/`)
- [ ] **3.1.** Implementar `TauriCommandsEventSink` (emitindo `artisan://catalog-updated`, `artisan://discovery-failed` e `artisan://command-created`), `ProjectLookupImpl` (conectado ao serviço de projetos da Fase 06) e `MaxCodeConfigLocatorImpl`.
- [ ] **3.2.** Implementar `src-tauri/src/commands/quick_commands.rs` com `QuickCommandsState` e os 10 comandos Tauri anotados com `#[tauri::command]` e `#[specta::specta]`.
- [ ] **3.3.** Registrar `QuickCommandsState` em `src-tauri/src/state.rs` e registrar todos os 10 comandos no builder do `specta.rs` e no `invoke_handler` de `src-tauri/src/lib.rs`.

### Passo 4 — Exportação de Bindings TypeScript e Cliente IPC (`src/types/bindings.ts`, `src/ipc/quickCommands.ts`)
- [ ] **4.1.** Executar `npm run bindings` para regenerar `src/types/bindings.ts` e verificar que todos os 10 comandos e tipos (`ArtisanRoot`, `ArtisanCatalog`, `ArtisanCommandForm`, `ValidationReport`, `ArtisanPreview`, `ArtisanCreateResult`, `ShortcutList`, `ResolvedShortcut`, `ShortcutId`) foram exportados (CA-04).
- [ ] **4.2.** Implementar `src/ipc/quickCommands.ts` com `quickCommandsIpc`, `onCatalogUpdated`, `onDiscoveryFailed` e `onCommandCreated`.

---

## 2. Comandos de Verificação e Validação

Execute a partir da raiz da worktree `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0`:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands --test shortcuts
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands
npm run bindings && git diff --exit-code src/types/bindings.ts
npm run typecheck
```

---

## 3. Mapeamento de Critérios de Aceite

| Critério | Cobertura nesta Implementação |
|----------|-------------------------------|
| **CA-01 / CA-02** | Workspace Rust (`mt-commands` + binário `maxterminal`) compilando sem warnings no `clippy` e passando em todos os testes do crate. |
| **CA-04** | `npm run bindings && git diff --exit-code src/types/bindings.ts` retorna 0 com os 10 comandos exportados. |
| **CA-26** | Resolução exata para projetos híbridos Laravel + Vue (npm). |
| **CA-27** | Resolução exata para projetos Tauri com `pnpm-lock.yaml` + `src-tauri/Cargo.toml`. |
| **CA-28** | Script `test` padrão do `npm init` ignorado, deixando `/test` desabilitado com `"Nenhum comando de teste encontrado"`. |
| **CA-29** | Seleção por `packageManager`, evidência de lockfiles divergentes, bloqueio em conflito multi-lock sem declaração e fixtures de shrinkwrap, Bun e Yarn Classic/Modern. |
| **CA-30** | `/build` e `/deploy` respeitando `comandoBuild`/`comandoDeploy` de `config-trabalho.json` do MaxCode. |
| **CA-35** | `/deploy` desabilitado sem configuração explícita; Yarn Modern sem `upgrade --force`; flags `force` não suportadas desabilitadas com motivo. |
| **CA-40** | `evidence_revision`, suporte a `cwd` aninhado, caminhos com espaços/apóstrofos e bloqueio de execução parcial quando uma parte obrigatória tem conflito. |
