//! Repositório SQLite para `color_schemes`, `appearance_settings` e `project_theme_overrides` (Fase 10).

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::Db;
use crate::error::{DbError, DbResult};
use crate::sanitizer::SanitizedText;
use crate::time::now_unix_ms;

/// Linha persistida na tabela `color_schemes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredColorSchemeRow {
    /// Identificador único (`slug`).
    pub id: String,
    /// Nome exibido.
    pub name: String,
    /// Origem (`builtin`, `imported`, `custom`).
    pub origin: String,
    /// Polaridade (`dark`, `light`).
    pub polarity: String,
    /// Autor opcional.
    pub author: Option<String>,
    /// Formato de origem opcional.
    pub source_format: Option<String>,
    /// Nome de arquivo original opcional (apenas basename sanitizado).
    pub source_filename: Option<String>,
    /// Hash SHA-256 de 64 caracteres hexadecimais.
    pub sha256: String,
    /// JSON completo do `ColorScheme`.
    pub payload_json: String,
    /// Timestamp de criação (ms).
    pub created_at: i64,
    /// Timestamp de atualização (ms).
    pub updated_at: i64,
}

/// Linha singleton (`singleton_id = 1`) da tabela `appearance_settings`.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredAppearanceRow {
    /// ID do tema ativo global.
    pub active_theme_id: String,
    /// Se acompanha a polaridade do sistema operacional.
    pub follow_system: bool,
    /// Tema preferido para polaridade escura.
    pub preferred_dark_theme_id: String,
    /// Tema preferido para polaridade clara.
    pub preferred_light_theme_id: String,
    /// Família de fontes monoespaçadas.
    pub font_family: String,
    /// Tamanho da fonte em pixels (`8..=32`).
    pub font_size: u8,
    /// Altura de linha (`1.0..=2.0`).
    pub line_height: f32,
    /// Se ligaduras tipográficas estão habilitadas.
    pub font_ligatures: bool,
    /// Se o cursor pisca.
    pub cursor_blink: bool,
    /// Sobrescrita opcional de estilo de cursor (`block`, `underline`, `bar`).
    pub cursor_style_override: Option<String>,
    /// Contraste mínimo do xterm.js (`1.0..=21.0`).
    pub minimum_contrast_ratio: f32,
    /// Timestamp da última atualização (ms).
    pub updated_at: i64,
}

impl Default for StoredAppearanceRow {
    fn default() -> Self {
        Self {
            active_theme_id: String::from("max-navy-teal"),
            follow_system: false,
            preferred_dark_theme_id: String::from("max-navy-teal"),
            preferred_light_theme_id: String::from("max-light"),
            font_family: String::from("JetBrains Mono, Fira Code, Cascadia Code, monospace"),
            font_size: 13,
            line_height: 1.2,
            font_ligatures: true,
            cursor_blink: true,
            cursor_style_override: None,
            minimum_contrast_ratio: 1.0,
            updated_at: 0,
        }
    }
}

/// Linha persistida na tabela `project_theme_overrides`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProjectThemeOverrideRow {
    /// Identificador do projeto (`projects.id`).
    pub project_id: String,
    /// Identificador do tema vinculado ao projeto.
    pub theme_id: String,
    /// Timestamp da última atualização (ms).
    pub updated_at: i64,
}

/// Repositório de leitura e escrita para esquemas de cores e configurações de aparência.
#[derive(Clone, Debug)]
pub struct ThemesRepo {
    db: Db,
}

impl ThemesRepo {
    /// Constrói um `ThemesRepo` associado à instância `Db`.
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    /// Lista todos os esquemas persistidos no SQLite ordenados por nome (`COLLATE NOCASE`).
    pub async fn list_schemes(&self) -> DbResult<Vec<StoredColorSchemeRow>> {
        self.db.read(list_schemes_conn).await
    }

    /// Versão síncrona de `list_schemes`.
    pub fn list_schemes_blocking(&self) -> DbResult<Vec<StoredColorSchemeRow>> {
        self.db.read_blocking(list_schemes_conn)
    }

    /// Busca um esquema persistido pelo `id`.
    pub async fn get_scheme(&self, id: &str) -> DbResult<Option<StoredColorSchemeRow>> {
        let id_owned = id.to_owned();
        self.db
            .read(move |conn| get_scheme_conn(conn, &id_owned))
            .await
    }

    /// Versão síncrona de `get_scheme`.
    pub fn get_scheme_blocking(&self, id: &str) -> DbResult<Option<StoredColorSchemeRow>> {
        self.db.read_blocking(|conn| get_scheme_conn(conn, id))
    }

    /// Busca um esquema persistido pelo hash SHA-256 de conteúdo.
    pub fn find_by_sha256_blocking(&self, sha256: &str) -> DbResult<Option<StoredColorSchemeRow>> {
        self.db.read_blocking(|conn| {
            conn.query_row(
                "SELECT id, name, origin, polarity, author, source_format, source_filename, sha256, payload_json, created_at, updated_at
                 FROM color_schemes
                 WHERE sha256 = ?1
                 LIMIT 1;",
                params![sha256],
                map_scheme_row,
            )
            .optional()
            .map_err(DbError::from)
        })
    }

    /// Insere ou atualiza um lote de esquemas em uma única transação atômica.
    pub fn upsert_schemes_blocking(&self, rows: Vec<StoredColorSchemeRow>) -> DbResult<()> {
        let approx_bytes = rows
            .iter()
            .map(|r| r.payload_json.len() + 256)
            .sum::<usize>()
            .max(256);
        self.db
            .write_blocking("themes_upsert_batch", approx_bytes, move |tx| {
                for row in rows {
                    upsert_scheme_tx(tx, &row)?;
                }
                Ok(())
            })
    }

    /// Remove um esquema `imported` ou `custom` e, opcionalmente, atualiza referências em `appearance_settings`
    /// e `project_theme_overrides` para `fallback_id` na mesma transação.
    pub fn delete_scheme_with_fallback_blocking(
        &self,
        id: &str,
        fallback_id: &str,
    ) -> DbResult<bool> {
        let id_owned = id.to_owned();
        let fallback_owned = fallback_id.to_owned();
        self.db
            .write_blocking("themes_delete", 512, move |tx| {
                let origin_opt: Option<String> = tx
                    .query_row(
                        "SELECT origin FROM color_schemes WHERE id = ?1;",
                        params![id_owned],
                        |r| r.get(0),
                    )
                    .optional()?;

                let Some(origin) = origin_opt else {
                    return Ok(false);
                };

                if origin == "builtin" {
                    return Err(DbError::ConstraintViolation {
                        constraint: format!("builtin:{id_owned}"),
                    });
                }

                let now = now_unix_ms();

                tx.execute(
                    "UPDATE appearance_settings
                     SET active_theme_id = CASE WHEN active_theme_id = ?1 THEN ?2 ELSE active_theme_id END,
                         preferred_dark_theme_id = CASE WHEN preferred_dark_theme_id = ?1 THEN 'max-navy-teal' ELSE preferred_dark_theme_id END,
                         preferred_light_theme_id = CASE WHEN preferred_light_theme_id = ?1 THEN 'max-light' ELSE preferred_light_theme_id END,
                         updated_at = ?3
                     WHERE singleton_id = 1;",
                    params![id_owned, fallback_owned, now],
                )?;

                tx.execute(
                    "DELETE FROM project_theme_overrides WHERE theme_id = ?1;",
                    params![id_owned],
                )?;

                let affected = tx.execute(
                    "DELETE FROM color_schemes WHERE id = ?1;",
                    params![id_owned],
                )?;
                Ok(affected > 0)
            })
    }

    /// Lê a linha singleton de `appearance_settings` e todos os overrides por projeto.
    pub fn get_appearance_and_overrides_blocking(
        &self,
    ) -> DbResult<(StoredAppearanceRow, Vec<StoredProjectThemeOverrideRow>)> {
        self.db.read_blocking(|conn| {
            let appearance = get_appearance_conn(conn)?;
            let overrides = list_project_overrides_conn(conn)?;
            Ok((appearance, overrides))
        })
    }

    /// Salva a linha singleton de `appearance_settings`.
    pub fn save_appearance_blocking(
        &self,
        row: StoredAppearanceRow,
    ) -> DbResult<StoredAppearanceRow> {
        self.db
            .write_blocking("appearance_settings_save", 512, move |tx| {
                let now = now_unix_ms();
                let clean_font: SanitizedText = SanitizedText::new(&row.font_family);
                tx.execute(
                    "INSERT INTO appearance_settings (
                        singleton_id,
                        active_theme_id,
                        follow_system,
                        preferred_dark_theme_id,
                        preferred_light_theme_id,
                        font_family,
                        font_size,
                        line_height,
                        font_ligatures,
                        cursor_blink,
                        cursor_style_override,
                        minimum_contrast_ratio,
                        updated_at
                    ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                    ON CONFLICT(singleton_id) DO UPDATE SET
                        active_theme_id = excluded.active_theme_id,
                        follow_system = excluded.follow_system,
                        preferred_dark_theme_id = excluded.preferred_dark_theme_id,
                        preferred_light_theme_id = excluded.preferred_light_theme_id,
                        font_family = excluded.font_family,
                        font_size = excluded.font_size,
                        line_height = excluded.line_height,
                        font_ligatures = excluded.font_ligatures,
                        cursor_blink = excluded.cursor_blink,
                        cursor_style_override = excluded.cursor_style_override,
                        minimum_contrast_ratio = excluded.minimum_contrast_ratio,
                        updated_at = excluded.updated_at;",
                    params![
                        row.active_theme_id,
                        i32::from(row.follow_system),
                        row.preferred_dark_theme_id,
                        row.preferred_light_theme_id,
                        clean_font.as_str(),
                        i64::from(row.font_size),
                        f64::from(row.line_height),
                        i32::from(row.font_ligatures),
                        i32::from(row.cursor_blink),
                        row.cursor_style_override,
                        f64::from(row.minimum_contrast_ratio),
                        now,
                    ],
                )?;

                Ok(StoredAppearanceRow {
                    font_family: clean_font.into_inner(),
                    updated_at: now,
                    ..row
                })
            })
    }

    /// Define ou remove (`theme_id = None`) o tema vinculado a um projeto específico.
    pub fn set_project_override_blocking(
        &self,
        project_id: &str,
        theme_id: Option<&str>,
    ) -> DbResult<()> {
        let pid = project_id.to_owned();
        let tid = theme_id.map(str::to_owned);
        self.db
            .write_blocking("project_theme_override_set", 256, move |tx| {
                if let Some(theme) = tid {
                    let now = now_unix_ms();
                    tx.execute(
                        "INSERT INTO project_theme_overrides (project_id, theme_id, updated_at)
                         VALUES (?1, ?2, ?3)
                         ON CONFLICT(project_id) DO UPDATE SET
                             theme_id = excluded.theme_id,
                             updated_at = excluded.updated_at;",
                        params![pid, theme, now],
                    )?;
                } else {
                    tx.execute(
                        "DELETE FROM project_theme_overrides WHERE project_id = ?1;",
                        params![pid],
                    )?;
                }
                Ok(())
            })
    }
}

fn upsert_scheme_tx(tx: &Transaction<'_>, row: &StoredColorSchemeRow) -> DbResult<()> {
    let clean_name = SanitizedText::new(&row.name);
    let clean_author = row
        .author
        .as_deref()
        .map(|a| SanitizedText::new(a).into_inner());
    let clean_filename = row
        .source_filename
        .as_deref()
        .map(|f| SanitizedText::new(f).into_inner());
    let clean_payload = SanitizedText::new(&row.payload_json);

    tx.execute(
        "INSERT INTO color_schemes (
            id, name, origin, polarity, author, source_format, source_filename,
            sha256, payload_json, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            origin = excluded.origin,
            polarity = excluded.polarity,
            author = excluded.author,
            source_format = excluded.source_format,
            source_filename = excluded.source_filename,
            sha256 = excluded.sha256,
            payload_json = excluded.payload_json,
            updated_at = excluded.updated_at;",
        params![
            row.id,
            clean_name.as_str(),
            row.origin,
            row.polarity,
            clean_author,
            row.source_format,
            clean_filename,
            row.sha256,
            clean_payload.as_str(),
            row.created_at,
            row.updated_at,
        ],
    )?;
    Ok(())
}

fn list_schemes_conn(conn: &Connection) -> DbResult<Vec<StoredColorSchemeRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, origin, polarity, author, source_format, source_filename, sha256, payload_json, created_at, updated_at
         FROM color_schemes
         ORDER BY name COLLATE NOCASE ASC, id ASC;",
    )?;
    let iter = stmt.query_map([], map_scheme_row)?;
    let mut out = Vec::new();
    for item in iter {
        out.push(item?);
    }
    Ok(out)
}

fn get_scheme_conn(conn: &Connection, id: &str) -> DbResult<Option<StoredColorSchemeRow>> {
    conn.query_row(
        "SELECT id, name, origin, polarity, author, source_format, source_filename, sha256, payload_json, created_at, updated_at
         FROM color_schemes
         WHERE id = ?1;",
        params![id],
        map_scheme_row,
    )
    .optional()
    .map_err(DbError::from)
}

fn map_scheme_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredColorSchemeRow> {
    Ok(StoredColorSchemeRow {
        id: row.get(0)?,
        name: row.get(1)?,
        origin: row.get(2)?,
        polarity: row.get(3)?,
        author: row.get(4)?,
        source_format: row.get(5)?,
        source_filename: row.get(6)?,
        sha256: row.get(7)?,
        payload_json: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn get_appearance_conn(conn: &Connection) -> DbResult<StoredAppearanceRow> {
    let row_opt = conn
        .query_row(
            "SELECT
                active_theme_id,
                follow_system,
                preferred_dark_theme_id,
                preferred_light_theme_id,
                font_family,
                font_size,
                line_height,
                font_ligatures,
                cursor_blink,
                cursor_style_override,
                minimum_contrast_ratio,
                updated_at
             FROM appearance_settings
             WHERE singleton_id = 1;",
            [],
            |r| {
                let follow: i32 = r.get(1)?;
                let font_size_i64: i64 = r.get(5)?;
                let line_height_f64: f64 = r.get(6)?;
                let ligatures: i32 = r.get(7)?;
                let blink: i32 = r.get(8)?;
                let min_contrast_f64: f64 = r.get(10)?;
                Ok(StoredAppearanceRow {
                    active_theme_id: r.get(0)?,
                    follow_system: follow != 0,
                    preferred_dark_theme_id: r.get(2)?,
                    preferred_light_theme_id: r.get(3)?,
                    font_family: r.get(4)?,
                    font_size: font_size_i64.clamp(8, 32) as u8,
                    line_height: line_height_f64 as f32,
                    font_ligatures: ligatures != 0,
                    cursor_blink: blink != 0,
                    cursor_style_override: r.get(9)?,
                    minimum_contrast_ratio: min_contrast_f64 as f32,
                    updated_at: r.get(11)?,
                })
            },
        )
        .optional()?;

    Ok(row_opt.unwrap_or_default())
}

fn list_project_overrides_conn(conn: &Connection) -> DbResult<Vec<StoredProjectThemeOverrideRow>> {
    let mut stmt = conn.prepare(
        "SELECT project_id, theme_id, updated_at
         FROM project_theme_overrides
         ORDER BY project_id ASC;",
    )?;
    let iter = stmt.query_map([], |r| {
        Ok(StoredProjectThemeOverrideRow {
            project_id: r.get(0)?,
            theme_id: r.get(1)?,
            updated_at: r.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for item in iter {
        out.push(item?);
    }
    Ok(out)
}
