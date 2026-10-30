# Implementação 01 — Fundação `mt-os`, Settings SQLite, Detecção de Ambiente e Agregador de Estado (`plan.md`)

> Plano de execução passo a passo para a **Implementação 01** da Fase 09.
> Especificação técnica correspondente: [`./spec.md`](./spec.md).

---

## 1. Pré-requisitos e Verificações Iniciais

1. Confirmar na workspace Cargo (`src-tauri/Cargo.toml`) a presença dos crates `mt-core` (Fase 01) e `mt-db` (Fase 03).
2. Verificar o último número de migration em `src-tauri/crates/mt-db/migrations/` para nomear `NNNN_09_os_settings.sql` sequencialmente sem colisão.

---

## 2. Etapas de Implementação

### Etapa 2.1 — Migration SQLite e `OsSettingsRepo` no `mt-db`
- **Arquivos**:
  - `src-tauri/crates/mt-db/migrations/NNNN_09_os_settings.sql`
  - `src-tauri/crates/mt-db/src/repos/os_settings.rs`
  - `src-tauri/crates/mt-db/src/repos/mod.rs`
- **Ações**:
  1. Criar a migration SQL com a tabela `os_settings` (`STRICT`, `id INTEGER PRIMARY KEY CHECK (id = 1)`, `schema_version >= 1`, `CHECK (json_valid(value_json) AND json_type(value_json) = 'object')`, `updated_at INTEGER NOT NULL`) e `INSERT OR IGNORE INTO os_settings VALUES (1, 1, '{}', ...)`.
  2. Registrar a migration no runner de migrations do `mt-db`.
  3. Implementar `OsSettingsRow` e `OsSettingsRepo` (`get_row` e `save_row`) em `crates/mt-db/src/repos/os_settings.rs` (e alias `os_settings_repo` em `repos/mod.rs`).
  4. Garantir que `mt-db` não adicione `mt-os` como dependência no `Cargo.toml`.

### Etapa 2.2 — Criação do Crate `mt-os` e Códigos de Erro (`error.rs`)
- **Arquivos**:
  - `src-tauri/Cargo.toml` (inclusão de `crates/mt-os` em `workspace.members`)
  - `src-tauri/crates/mt-os/Cargo.toml`
  - `src-tauri/crates/mt-os/src/error.rs`
  - `src-tauri/crates/mt-os/src/lib.rs`
- **Ações**:
  1. Configurar o `Cargo.toml` do `mt-os` com as dependências de workspace e crates Linux (`zbus`, `ashpd`, `notify-rust`, `which`, `parking_lot`, `lru`, `percent-encoding`, `url`, `async-trait`, `futures`).
  2. Implementar `crates/mt-os/src/error.rs` contendo as 19 constantes `codes::*` especificadas na §2.1 da Fase 09 e o helper `os_err`.

### Etapa 2.3 — Modelo de Configurações, Patch e Validação (`settings.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/settings.rs`
- **Ações**:
  1. Implementar `OsSettings`, `CloseBehavior`, `QuakeSettings`, `QuakeMode`, `ShortcutBackendPreference`, `EditorSettings`, `EditorKind`, `NotificationSettings` com `#[derive(Serialize, Deserialize, specta::Type)]` e `#[serde(default, rename_all = "camelCase")]` (enums em `snake_case`).
  2. Implementar `OsSettingsPatch`, `QuakeSettingsPatch`, `NotificationSettingsPatch` e `SettingsDiff`.
  3. Implementar validação sintática preliminar do acelerador (delegando para `Accelerator::parse` ou validador compartilhado com `shortcut/accelerator.rs`), validação das faixas `drop_down_height_pct` (`20..=100`) e `long_command_threshold_secs` (`3..=3600`), e validação de `custom_argv` (`1..=16` itens, `<= 512` bytes por item, presença de `{file}`, ausência de tokens/credenciais via `mt_db::sanitizer`).
  4. Implementar `OsSettings::from_row_preserving_unknown` e `to_row_preserving_unknown` para rejeitar `schema_version > 1` com `os.settings_invalid`, rejeitar tipos JSON malformados e preservar campos desconhecidos na serialização.

### Etapa 2.4 — Detecção de Ambiente Linux/CachyOS (`environment.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/environment.rs`
- **Ações**:
  1. Implementar `DisplayServer`, `Compositor`, `EditorAvailability` e `OsEnvironment`.
  2. Implementar a função pura `classify_display(env: &impl Fn(&str) -> Option<String>) -> (DisplayServer, Compositor)` cobrindo `WAYLAND_DISPLAY`, `GDK_BACKEND=x11` (`XWayland`), `XDG_SESSION_TYPE`, `DISPLAY`, `HYPRLAND_INSTANCE_SIGNATURE`, `SWAYSOCK`, `XDG_CURRENT_DESKTOP` e `DESKTOP_SESSION`.
  3. Implementar `probe(env)` assíncrono com `tokio::time::timeout(Duration::from_millis(500), ...)` para consultar via `zbus`:
     - `org.kde.StatusNotifierWatcher` / `org.freedesktop.StatusNotifierWatcher` (`NameHasOwner`);
     - `org.freedesktop.portal.Desktop` interface `org.freedesktop.portal.GlobalShortcuts` (`version >= 1`);
     - `org.freedesktop.Notifications` (`GetServerInformation` e `GetCapabilities` contendo `"actions"`).

### Etapa 2.5 — Registro de Atenção, Contadores e Traits de Fronteira (`attention.rs`, `status.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/attention.rs`
  - `src-tauri/crates/mt-os/src/status.rs`
- **Ações**:
  1. Implementar `AttentionReason` (`AwaitingInput`, `AwaitingApproval`) e `AttentionRegistry` (`set`, `clear`, `count`, `get`, `contains`) protegido por `parking_lot::RwLock`.
  2. Implementar `TabStatusKind`, `TabStatusEntry`, `ActivitySnapshot` (`has_live_activity`), `TabStatusProvider` e `StatusCounters` (`aggregate`, `has_active`, `needs_attention`).
  3. Garantir que `StatusCounters::aggregate` converta `Finished { seen: true }` e `Error { seen: true }` em incremento de `idle`, mantendo a soma dos 5 buckets igual ao total de abas.

### Etapa 2.6 — Suítes de Testes Unitários e de Integração com SQLite
- **Arquivos**:
  - `src-tauri/crates/mt-db/tests/os_settings.rs` (ou módulo `tests` em `repos/os_settings.rs`)
  - `src-tauri/crates/mt-os/tests/settings.rs`
  - `src-tauri/crates/mt-os/tests/environment.rs`
  - `src-tauri/crates/mt-os/src/status.rs` (módulo `#[cfg(test)]`)
- **Ações**:
  1. Em `mt-db`: testar criação da tabela pela migration, existência da linha default `(1, 1, '{}', ...)`, rejeição de JSON inválido (`'{invalid'`) e de array (`'[]'`) pelo `CHECK`, restrição `id = 1`, e atualização via `save_row`.
  2. Em `tests/settings.rs`: testar defaults a partir de `'{}'`, `apply_patch` e todas as flags de `SettingsDiff`, rejeição de `drop_down_height_pct` (`19`, `101`), `long_command_threshold_secs` (`2`, `3601`), `custom_argv` sem `{file}`, `custom_argv` com token `Bearer secret`, rejeição de `schema_version = 2` e preservação de campos desconhecidos no JSON.
  3. Em `tests/environment.rs`: testar `classify_display` com mapas simulados para KDE Plasma Wayland, Hyprland Wayland, GNOME Wayland, Sway Wayland, KDE X11, XWayland (`WAYLAND_DISPLAY=wayland-1` + `GDK_BACKEND=x11`) e ambiente vazio (`Unknown` / `Other`).
  4. Em `status.rs`: testar `StatusCounters::aggregate` com combinações de abas vistas/não vistas e `AttentionRegistry`, além de `ActivitySnapshot::has_live_activity` com split oculto ativo.

---

## 3. Checklist de Validação

- [ ] A migration `NNNN_09_os_settings.sql` cria a tabela `os_settings` com `STRICT` e insere a linha default `'{}'`.
- [ ] `mt-db` compila e passa nos testes sem depender de `mt-os`.
- [ ] `OsSettings::validate()` rejeita valores fora de faixa, aceleradores inválidos, `custom_argv` sem `{file}` e argumentos contendo segredos/tokens.
- [ ] `schema_version > 1` retorna erro `os.settings_invalid` sem sobrescrever o banco.
- [ ] `classify_display` distingue corretamente `Wayland`, `XWayland`, `X11` e `Unknown`, além de `KdePlasma`, `Hyprland`, `Gnome`, `Sway` e `Other`.
- [ ] `StatusCounters::aggregate` contabiliza abas `Finished`/`Error` já visualizadas (`seen: true`) no bucket `idle`.
- [ ] `ActivitySnapshot::has_live_activity` considera `active_panes`, `pending_spawns`, `supervised_busy`, `incomplete` e `awaiting_input`.

---

## 4. Comandos de Verificação (Mapeados para `CA-*`)

```bash
# CA-01: Lint estrito do crate mt-os
cd src-tauri && cargo clippy -p mt-os --all-targets -- -D warnings

# CA-02 (parcial): Testes unitários de settings, environment e status/attention
cd src-tauri && cargo test -p mt-os --test settings --test environment
cd src-tauri && cargo test -p mt-os status::tests

# CA-03: Testes de migration e repositório os_settings no mt-db
cd src-tauri && cargo test -p mt-db os_settings
```
