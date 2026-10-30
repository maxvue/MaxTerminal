# Implementação 03 — Persistência SQLite, ThemeService, Comandos IPC e Eventos Tauri (`spec.md`)

> Fase 10 — Temas e Importador Tabby
> Referências: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§3.16, §3.17, §4, §5, §6, §12.1).
> Pré-requisito: Implementações 01 e 02 concluídas.

---

## 1. Contexto, Objetivo e Escopo

Esta terceira implementação integra o crate `mt-themes` à camada de persistência SQLite (`mt-db`) e à ponte IPC/eventos do Tauri v2 (`src-tauri`), entregando:

1. **Migration SQLite (`mt-db/migrations/NNNN_10_themes.sql`)**: tabelas `STRICT` (`themes`, `appearance_settings`, `project_themes`), índices de listagem/deduplicação/FK, chaves estrangeiras com `ON DELETE SET DEFAULT` e `ON DELETE CASCADE`, e triggers para proteção de builtins (`trg_themes_builtin_no_delete`) e atualização automática de `updated_at` (`trg_themes_updated_at`).
2. **Repositório SQLite (`mt-db/src/repos/themes.rs`)**: tipos primitivos (`ThemeRow`, `AppearanceRow`, `UpsertKind`) sem dependência circular `mt-db → mt-themes`, com todas as queries parametrizadas via `prepare_cached`.
3. **Cache LRU (`mt-themes/src/cache.rs`)**: `ResolvedCache` de 64 entradas indexadas por `(ThemeId, ContentHash, overridesHash, mappingVersion, canonicalTokensVersion)`, evitando recomputar rampas OKLCH e relatórios WCAG e nunca misturando metadados entre temas de mesmo `ContentHash`.
4. **Serviço de Domínio (`mt-themes/src/service.rs`)**: `ThemeStore`, `SqliteThemeStore`, `ThemeEventSink`, `ThemeService` com barreira de readiness assíncrona no `start()`, seed/upsert idempotente dos 6 builtins, deduplicação atômica intra-lote e contra o banco por `ContentHash`, mutações serializadas com incremento monotônico de `revision`, validação estrita de `TerminalFontSettings` e emissão de eventos somente após commit da transação.
5. **Comandos IPC e Eventos Tauri (`src-tauri/src/commands/themes.rs`, `src-tauri/src/events/themes.rs`)**: os 9 comandos `#[tauri::command] #[specta::specta]`, os 2 eventos (`theme://changed`, `theme://library-changed`), permissões de diálogo em `src-tauri/capabilities/default.json` e regeneração de `src/types/bindings.ts`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Modificados
- `src-tauri/crates/mt-db/src/lib.rs` (e `src-tauri/crates/mt-db/src/repos/mod.rs`) — exposição de `pub mod repos;` e `pub mod themes;`.
- `src-tauri/crates/mt-themes/src/lib.rs` — exposição de `cache` e `service` + re-exports públicos de DTOs.
- `src-tauri/Cargo.toml` — dependência `mt-themes.workspace = true` no crate binário.
- `src-tauri/src/commands/mod.rs` — inclusão de `pub mod themes;`.
- `src-tauri/src/events/mod.rs` — inclusão de `pub mod themes;`.
- `src-tauri/src/specta.rs` — registro dos 9 comandos `themes::theme_*` e dos 2 eventos `ThemeChangedEvent` / `ThemeLibraryChangedEvent`.
- `src-tauri/src/lib.rs` — inicialização de `Arc<ThemeService>`, registro no `app.manage(...)` e disparo de `svc.start()`.
- `src-tauri/capabilities/default.json` — adição de `"dialog:allow-open"`, `"dialog:allow-save"` e permissões dos comandos `theme_*` na janela `main`.
- `src/types/bindings.ts` — regenerado automaticamente via `npm run bindings`.

### 2.2. Arquivos Criados
- `src-tauri/crates/mt-db/migrations/NNNN_10_themes.sql` (`NNNN` = próximo número sequencial livre em `mt-db/migrations/`)
- `src-tauri/crates/mt-db/src/repos/themes.rs`
- `src-tauri/crates/mt-themes/src/cache.rs`
- `src-tauri/crates/mt-themes/src/service.rs`
- `src-tauri/crates/mt-themes/tests/service_db.rs`
- `src-tauri/src/commands/themes.rs`
- `src-tauri/src/events/themes.rs`

---

## 3. Especificação Técnica e Assinaturas Rust

### 3.1. Migration SQLite (`src-tauri/crates/mt-db/migrations/NNNN_10_themes.sql`)

```sql
-- Fase 10: biblioteca de temas, configurações de aparência e sobreposição de tema por projeto.
CREATE TABLE themes (
    id                    TEXT    PRIMARY KEY NOT NULL,
    slug                  TEXT    NULL UNIQUE
                          CHECK (slug IS NULL OR (length(slug) BETWEEN 2 AND 64 AND slug NOT GLOB '*[^a-z0-9-]*')),
    name                  TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    appearance            TEXT    NOT NULL CHECK (appearance IN ('dark', 'light')),
    origin                TEXT    NOT NULL
                          CHECK (origin IN ('builtin', 'tabby_config', 'tabby_scheme', 'xterm_json', 'windows_terminal')),
    builtin               INTEGER NOT NULL DEFAULT 0 CHECK (builtin IN (0, 1)),
    builtin_version       INTEGER NULL CHECK (builtin_version IS NULL OR builtin_version >= 1),
    source_name           TEXT    NULL CHECK (source_name IS NULL OR length(source_name) <= 255),
    content_hash          TEXT    NOT NULL CHECK (length(content_hash) = 64 AND content_hash NOT GLOB '*[^0-9a-f]*'),
    schema_version        INTEGER NOT NULL DEFAULT 1 CHECK (schema_version = 1),
    scheme_json           TEXT    NOT NULL CHECK (json_valid(scheme_json) AND length(scheme_json) <= 16384),
    token_overrides_json  TEXT    NULL CHECK (token_overrides_json IS NULL OR json_valid(token_overrides_json)),
    warnings_json         TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(warnings_json)),
    fg_bg_contrast        REAL    NOT NULL CHECK (fg_bg_contrast BETWEEN 1.0 AND 21.0),
    created_at            INTEGER NOT NULL,
    updated_at            INTEGER NOT NULL,
    CHECK ((builtin = 1) = (origin = 'builtin')),
    CHECK ((builtin = 1) = (slug IS NOT NULL AND builtin_version IS NOT NULL))
) STRICT;

CREATE INDEX idx_themes_listing ON themes (builtin DESC, name COLLATE NOCASE);
CREATE INDEX idx_themes_content ON themes (content_hash);
CREATE UNIQUE INDEX idx_themes_imported_hash ON themes (content_hash) WHERE builtin = 0;

CREATE TABLE appearance_settings (
    id                         INTEGER PRIMARY KEY CHECK (id = 1),
    active_theme_id            TEXT    NOT NULL DEFAULT 'b5907b11-f15c-5886-b803-f1fb94bc4c9d'
                               REFERENCES themes(id) ON DELETE SET DEFAULT,
    apply_project_theme_to_ui  INTEGER NOT NULL DEFAULT 1 CHECK (apply_project_theme_to_ui IN (0, 1)),
    font_family                TEXT    NOT NULL DEFAULT 'JetBrainsMono Nerd Font, monospace'
                               CHECK (length(font_family) BETWEEN 1 AND 200),
    font_size                  REAL    NOT NULL DEFAULT 13   CHECK (font_size BETWEEN 8 AND 32),
    line_height                REAL    NOT NULL DEFAULT 1.15 CHECK (line_height BETWEEN 1.0 AND 2.0),
    letter_spacing             REAL    NOT NULL DEFAULT 0    CHECK (letter_spacing BETWEEN -2 AND 10),
    font_weight                TEXT    NOT NULL DEFAULT 'normal'
                               CHECK (font_weight IN ('normal','bold','100','200','300','400','500','600','700','800','900')),
    font_weight_bold           TEXT    NOT NULL DEFAULT 'bold'
                               CHECK (font_weight_bold IN ('normal','bold','100','200','300','400','500','600','700','800','900')),
    ligatures                  INTEGER NOT NULL DEFAULT 0 CHECK (ligatures IN (0, 1)),
    revision                   INTEGER NOT NULL DEFAULT 0 CHECK (revision BETWEEN 0 AND 4294967295),
    updated_at                 INTEGER NOT NULL
) STRICT;

CREATE TABLE project_themes (
    project_id  TEXT    PRIMARY KEY NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    theme_id    TEXT    NOT NULL REFERENCES themes(id) ON DELETE CASCADE,
    updated_at  INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE INDEX idx_project_themes_theme ON project_themes (theme_id);

CREATE TRIGGER trg_themes_builtin_no_delete BEFORE DELETE ON themes
FOR EACH ROW WHEN OLD.builtin = 1
BEGIN
    SELECT RAISE(ABORT, 'theme.builtin_readonly');
END;

CREATE TRIGGER trg_themes_updated_at AFTER UPDATE ON themes
FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
BEGIN
    UPDATE themes SET updated_at = CAST(unixepoch('subsec') * 1000 AS INTEGER) WHERE id = NEW.id;
END;
```

### 3.2. Repositório SQLite (`src-tauri/crates/mt-db/src/repos/themes.rs`)

```rust
use crate::DbError;
use rusqlite::{Connection, Transaction};

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeRow {
    pub id: String,
    pub slug: Option<String>,
    pub name: String,
    pub appearance: String,
    pub origin: String,
    pub builtin: bool,
    pub builtin_version: Option<i64>,
    pub source_name: Option<String>,
    pub content_hash: String,
    pub schema_version: i64,
    pub scheme_json: String,
    pub token_overrides_json: Option<String>,
    pub warnings_json: String,
    pub fg_bg_contrast: f64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceRow {
    pub active_theme_id: String,
    pub apply_project_theme_to_ui: bool,
    pub font_family: String,
    pub font_size: f64,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub font_weight: String,
    pub font_weight_bold: String,
    pub ligatures: bool,
    pub revision: u32,
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpsertKind {
    Inserted,
    Updated,
    Unchanged,
}

pub fn list(conn: &Connection) -> Result<Vec<ThemeRow>, DbError>;
pub fn get(conn: &Connection, id: &str) -> Result<Option<ThemeRow>, DbError>;
pub fn find_by_hashes(conn: &Connection, hashes: &[String]) -> Result<Vec<(String, String, String)>, DbError>;
pub fn insert(tx: &Transaction<'_>, row: &ThemeRow) -> Result<(), DbError>;
pub fn upsert_builtin(tx: &Transaction<'_>, row: &ThemeRow) -> Result<UpsertKind, DbError>;
pub fn ensure_appearance_row(tx: &Transaction<'_>, now: i64) -> Result<(), DbError>;
pub fn delete(tx: &Transaction<'_>, id: &str) -> Result<bool, DbError>;
pub fn projects_using(tx: &Transaction<'_>, theme_id: &str) -> Result<Vec<String>, DbError>;
pub fn appearance(conn: &Connection) -> Result<AppearanceRow, DbError>;
pub fn save_appearance(tx: &Transaction<'_>, row: &AppearanceRow) -> Result<(), DbError>;
pub fn project_overrides(conn: &Connection) -> Result<Vec<(String, String)>, DbError>;
pub fn set_project_theme(
    tx: &Transaction<'_>,
    project_id: &str,
    theme_id: Option<&str>,
    now: i64,
) -> Result<(), DbError>;
```

Regras de `find_by_hashes`:
- Quando `hashes` possui múltiplos registros no banco (ex.: um builtin e um importado pré-existente com o mesmo `content_hash`), ordena por `builtin DESC, created_at ASC, id ASC` para priorizar deterministicamente o builtin.

### 3.3. Cache de Temas Resolvidos (`src-tauri/crates/mt-themes/src/cache.rs`)

```rust
use crate::ids::{ContentHash, ThemeId};
use crate::service::ResolvedTheme;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub id: ThemeId,
    pub content_hash: ContentHash,
    pub overrides_hash: String,
    pub mapping_version: u32,
    pub canonical_tokens_version: u32,
}

pub struct ResolvedCache {
    inner: Mutex<LruCache<CacheKey, ResolvedTheme>>,
}

impl ResolvedCache {
    pub fn new(capacity: usize) -> Self;
    pub fn get(&self, key: &CacheKey) -> Option<ResolvedTheme>;
    pub fn put(&self, key: CacheKey, value: ResolvedTheme);
    pub fn remove_by_id(&self, id: ThemeId);
    pub fn clear(&self);
}
```

### 3.4. Serviço e DTOs (`src-tauri/crates/mt-themes/src/service.rs`)

Implementa integralmente todas as structs e traits de `spec.md` §3.17 e §5:
- `ThemeSummary`, `ResolvedTheme`, `FontWeight`, `TerminalFontSettings`, `TerminalFontPatch`, `ProjectThemeOverride`, `AppearanceState`, `AppearancePatch`, `ThemeScope`, `ThemeActivation`, `ThemeDeleteResult`, `ThemeExportResult`, `ImportReport`, `ThemeRecord`.
- `ThemeChangeReason`, `ThemeChangedPayload`, `ThemeLibraryChangedPayload`.
- `ThemeStore`, `SqliteThemeStore`, `ThemeEventSink`, `ThemeService`.

Validação de `TerminalFontSettings` (`validate_font_settings`):
- `font_family`: regex `^[A-Za-z0-9 ,'"._-]{1,200}$` (implementada por validação caractere a caractere + comprimento `1..=200`, sem proibir espaços ou hifens legítimos, mas proibindo `;`, `{`, `}`, `\`, `/`, `<`, `>`, `:`, `(`, `)` e caracteres de controle), no máximo 8 famílias separadas por `,` (cada família não vazia após `.trim()`). Caso inválido → `ThemeError::InvalidFont`.
- `font_size`: finito, em `[8.0, 32.0]` e múltiplo de `0.5` (`((size * 2.0).round() - size * 2.0).abs() < 1e-4`). Caso inválido → `ThemeError::InvalidAppearance { field: "fontSize", ... }`.
- `line_height`: finito, em `[1.0, 2.0]`. Caso inválido → `ThemeError::InvalidAppearance { field: "lineHeight", ... }`.
- `letter_spacing`: finito, em `[-2.0, 10.0]`. Caso inválido → `ThemeError::InvalidAppearance { field: "letterSpacing", ... }`.

Barreira de Readiness em `ThemeService`:
- `ThemeService` possui `ready: tokio::sync::OnceCell<()>` (ou `tokio::sync::RwLock<bool>` + `Mutex` de inicialização).
- `start(&self) -> MtResult<()>` é idempotente: executa `sync_builtins` + garante a linha `id = 1` em `appearance_settings` numa única transação e marca o serviço como pronto. Se qualquer comando (`list`, `get`, `appearance`, etc.) for chamado antes de `start()` concluir, ele aguarda `self.ensure_ready().await?` sem ler tabelas parcialmente semeadas.

### 3.5. Comandos e Eventos Tauri (`src-tauri/src/commands/themes.rs`, `src-tauri/src/events/themes.rs`)

```rust
// src-tauri/src/events/themes.rs
use mt_themes::{ThemeChangedPayload, ThemeEventSink, ThemeLibraryChangedPayload};
use serde::Serialize;
use tauri::AppHandle;
use tauri_specta::Event;

#[derive(Clone, Debug, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "theme://changed")]
pub struct ThemeChangedEvent(pub ThemeChangedPayload);

#[derive(Clone, Debug, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "theme://library-changed")]
pub struct ThemeLibraryChangedEvent(pub ThemeLibraryChangedPayload);

pub struct TauriThemeSink {
    app: AppHandle,
}

impl TauriThemeSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ThemeEventSink for TauriThemeSink {
    fn changed(&self, payload: ThemeChangedPayload) {
        let _ = ThemeChangedEvent(payload).emit(&self.app);
    }

    fn library_changed(&self, payload: ThemeLibraryChangedPayload) {
        let _ = ThemeLibraryChangedEvent(payload).emit(&self.app);
    }
}
```

```rust
// src-tauri/src/commands/themes.rs
use mt_core::MtError;
use mt_themes::{
    AppearancePatch, AppearanceState, ImportReport, ResolvedTheme, ThemeActivation,
    ThemeDeleteResult, ThemeExportFormat, ThemeExportResult, ThemeId, ThemeScope,
    ThemeService, ThemeSummary,
};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
#[specta::specta]
pub async fn theme_list(svc: State<'_, Arc<ThemeService>>) -> Result<Vec<ThemeSummary>, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn theme_get(svc: State<'_, Arc<ThemeService>>, id: ThemeId) -> Result<ResolvedTheme, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn theme_appearance_get(svc: State<'_, Arc<ThemeService>>) -> Result<AppearanceState, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn theme_appearance_update(
    svc: State<'_, Arc<ThemeService>>,
    patch: AppearancePatch,
) -> Result<AppearanceState, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn theme_import_file(
    svc: State<'_, Arc<ThemeService>>,
    path: String,
) -> Result<ImportReport, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn theme_import_dir(
    svc: State<'_, Arc<ThemeService>>,
    path: String,
) -> Result<ImportReport, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn theme_set_active(
    svc: State<'_, Arc<ThemeService>>,
    scope: ThemeScope,
    theme_id: Option<ThemeId>,
) -> Result<ThemeActivation, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn theme_delete(
    svc: State<'_, Arc<ThemeService>>,
    id: ThemeId,
) -> Result<ThemeDeleteResult, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn theme_export(
    svc: State<'_, Arc<ThemeService>>,
    id: ThemeId,
    format: ThemeExportFormat,
    dest_path: Option<String>,
) -> Result<ThemeExportResult, MtError>;
```

---

## 4. Regras de Negócio, Segurança e Casos de Borda

1. **Atomicidade de Exclusão e Fallback (CA-20)**: Ao excluir o tema ativo global, `SqliteThemeStore::delete` lê os `project_id` afetados antes do `DELETE`, atualiza `appearance_settings.active_theme_id = DEFAULT_THEME_ID` e incrementa `revision` na mesma transação. O serviço invalida o cache e emite `theme://library-changed` (`Deleted`) e `theme://changed` (`ActiveThemeDeleted` ou `ProjectOverrideChanged` se apenas projetos usavam o tema).
2. **Revisão Monotônica (CA-34)**: Toda escrita em `appearance_settings` ou `project_themes` incrementa `appearance_settings.revision` (`checked_add(1)`) dentro da transação SQLite. Leituras nunca incrementam `revision`.
3. **Validação de Caminhos no Comando (CA-22, CA-23)**: Caminhos relativos ou vazios em `theme_import_file`, `theme_import_dir` ou `theme_export` são rejeitados imediatamente com `theme.path_invalid` (ou `theme.export_failed` para extensão não permitida).
