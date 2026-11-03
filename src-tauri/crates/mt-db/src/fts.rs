//! Infraestrutura FTS5 (`FtsSpec`, `FtsTokenizer`, `FtsRegistry`, `build_match_query`),
//! cursores de paginação keyset (`KeysetCursor`) e cache RAM de snapshots de busca (`SearchSnapshotCache`).

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use regex::Regex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{DbError, DbResult};

static IDENTIFIER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z][a-z0-9_]*$")
        .unwrap_or_else(|e| panic!("Regex de identificador SQL inválida: {e}"))
});

/// Limite máximo de caracteres processados em `build_match_query`.
pub const MAX_MATCH_QUERY_CHARS: usize = 256;

/// Limite máximo de termos extraídos em `build_match_query`.
pub const MAX_MATCH_QUERY_TERMS: usize = 16;

/// Tamanho máximo permitido para um cursor codificado (2 KiB).
pub const MAX_CURSOR_LEN: usize = 2048;

/// Tokenizador FTS5 utilizado pelo índice virtual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FtsTokenizer {
    /// Texto geral (`unicode61 remove_diacritics 2`).
    Text,
    /// Código e comandos (`unicode61 remove_diacritics 2 tokenchars '-_./:@'`).
    Code,
}

impl FtsTokenizer {
    /// Retorna a cláusula `tokenize` completa para o `CREATE VIRTUAL TABLE ... USING fts5`.
    #[must_use]
    pub const fn as_fts5_clause(self) -> &'static str {
        match self {
            Self::Text => "unicode61 remove_diacritics 2",
            Self::Code => "unicode61 remove_diacritics 2 tokenchars '-_./:@'",
        }
    }
}

/// Especificação declarativa de uma tabela virtual FTS5 no padrão *external content* + triggers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtsSpec {
    /// Nome da tabela de conteúdo base (deve possuir `rowid` inteiro).
    pub content_table: &'static str,
    /// Nome da tabela virtual FTS5 (`<tabela>_fts`).
    pub fts_table: &'static str,
    /// Colunas textuais indexadas (na mesma ordem da tabela de conteúdo).
    pub columns: &'static [&'static str],
    /// Tokenizador configurado.
    pub tokenizer: FtsTokenizer,
    /// Tamanhos de prefixo opcionais (ex.: `&[2, 3]`).
    pub prefix: &'static [u8],
}

/// Especificação FTS5 canônica da tabela `command_blocks_fts` (Fase 05).
pub const COMMAND_BLOCKS_FTS_SPEC: FtsSpec = FtsSpec {
    content_table: "command_blocks",
    fts_table: "command_blocks_fts",
    columns: &["command_text", "output_text", "error_excerpt", "cwd"],
    tokenizer: FtsTokenizer::Code,
    prefix: &[],
};

impl FtsSpec {
    /// Valida que todos os identificadores SQL (`content_table`, `fts_table`, `columns`) são seguros.
    pub fn validate(&self) -> DbResult<()> {
        if !IDENTIFIER_RE.is_match(self.content_table) {
            return Err(DbError::ConstraintViolation {
                constraint: format!(
                    "content_table inválida em FtsSpec: '{}'",
                    self.content_table
                ),
            });
        }
        if !IDENTIFIER_RE.is_match(self.fts_table) {
            return Err(DbError::ConstraintViolation {
                constraint: format!("fts_table inválida em FtsSpec: '{}'", self.fts_table),
            });
        }
        if self.content_table == self.fts_table {
            return Err(DbError::ConstraintViolation {
                constraint: String::from("content_table e fts_table não podem ter o mesmo nome"),
            });
        }
        if self.columns.is_empty() {
            return Err(DbError::ConstraintViolation {
                constraint: String::from("FtsSpec exige ao menos uma coluna indexada"),
            });
        }
        for &col in self.columns {
            if !IDENTIFIER_RE.is_match(col) {
                return Err(DbError::ConstraintViolation {
                    constraint: format!("coluna inválida em FtsSpec: '{col}'"),
                });
            }
        }
        for &p in self.prefix {
            if p == 0 {
                return Err(DbError::ConstraintViolation {
                    constraint: String::from("prefixo FtsSpec deve ser >= 1"),
                });
            }
        }
        Ok(())
    }

    /// Gera o script DDL completo (`CREATE VIRTUAL TABLE` + triggers `_ai`, `_ad` e `_au`).
    #[must_use]
    pub fn ddl(&self) -> String {
        let cols_csv = self.columns.join(", ");
        let new_cols_csv = self
            .columns
            .iter()
            .map(|c| format!("new.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let old_cols_csv = self
            .columns
            .iter()
            .map(|c| format!("old.{c}"))
            .collect::<Vec<_>>()
            .join(", ");

        let prefix_clause = if self.prefix.is_empty() {
            String::new()
        } else {
            let nums = self
                .prefix
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            format!(",\n    prefix='{nums}'")
        };

        let fts = self.fts_table;
        let content = self.content_table;
        let tok = self.tokenizer.as_fts5_clause();

        format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS {fts} USING fts5(\n    {cols_csv},\n    content='{content}',\n    content_rowid='rowid',\n    tokenize=\"{tok}\"{prefix_clause}\n);\n\
             CREATE TRIGGER IF NOT EXISTS {fts}_ai AFTER INSERT ON {content} BEGIN\n    INSERT INTO {fts}(rowid, {cols_csv}) VALUES (new.rowid, {new_cols_csv});\nEND;\n\
             CREATE TRIGGER IF NOT EXISTS {fts}_ad AFTER DELETE ON {content} BEGIN\n    INSERT INTO {fts}({fts}, rowid, {cols_csv}) VALUES ('delete', old.rowid, {old_cols_csv});\nEND;\n\
             CREATE TRIGGER IF NOT EXISTS {fts}_au AFTER UPDATE ON {content} BEGIN\n    INSERT INTO {fts}({fts}, rowid, {cols_csv}) VALUES ('delete', old.rowid, {old_cols_csv});\n    INSERT INTO {fts}(rowid, {cols_csv}) VALUES (new.rowid, {new_cols_csv});\nEND;"
        )
    }

    /// Retorna o comando SQL `rebuild` para reconstruir integralmente o índice FTS5.
    #[must_use]
    pub fn rebuild_sql(&self) -> String {
        let fts = self.fts_table;
        format!("INSERT INTO {fts}({fts}) VALUES('rebuild');")
    }

    /// Retorna o comando SQL `optimize` para mesclar b-trees internas do índice FTS5.
    #[must_use]
    pub fn optimize_sql(&self) -> String {
        let fts = self.fts_table;
        format!("INSERT INTO {fts}({fts}) VALUES('optimize');")
    }

    /// Retorna o comando SQL `integrity-check` (`rank = 1` valida contra a tabela de conteúdo).
    #[must_use]
    pub fn integrity_check_sql(&self) -> String {
        let fts = self.fts_table;
        format!("INSERT INTO {fts}({fts}, rank) VALUES('integrity-check', 1);")
    }
}

/// Catálogo de índices FTS5 registrados para manutenção periódica e checagem de integridade.
#[derive(Debug, Clone, Default)]
pub struct FtsRegistry {
    specs: Vec<FtsSpec>,
}

impl FtsRegistry {
    /// Cria um registro FTS5 vazio.
    #[must_use]
    pub fn new() -> Self {
        Self { specs: Vec::new() }
    }

    /// Constrói um `FtsRegistry` validando todas as especificações fornecidas.
    pub fn from_specs(specs: Vec<FtsSpec>) -> DbResult<Self> {
        let mut reg = Self::new();
        for spec in specs {
            reg.register(spec)?;
        }
        Ok(reg)
    }

    /// Registra uma `FtsSpec` validada (idempotente para o mesmo `fts_table`).
    pub fn register(&mut self, spec: FtsSpec) -> DbResult<()> {
        spec.validate()?;
        if !self.specs.iter().any(|s| s.fts_table == spec.fts_table) {
            self.specs.push(spec);
        }
        Ok(())
    }

    /// Retorna as especificações FTS5 registradas.
    #[must_use]
    pub fn specs(&self) -> &[FtsSpec] {
        &self.specs
    }

    /// Executa `optimize` em todas as tabelas FTS5 registradas que existam no banco.
    pub fn optimize_all(&self, conn: &Connection) -> DbResult<()> {
        for spec in &self.specs {
            if table_exists(conn, spec.fts_table)? {
                conn.execute_batch(&spec.optimize_sql())
                    .map_err(|e| DbError::from_rusqlite(e, "fts_optimize"))?;
            }
        }
        Ok(())
    }

    /// Executa `integrity-check` (`rank = 1`) em todas as tabelas FTS5 registradas que existam no banco.
    pub fn integrity_check_all(&self, conn: &Connection) -> DbResult<()> {
        for spec in &self.specs {
            if table_exists(conn, spec.fts_table)? {
                conn.execute_batch(&spec.integrity_check_sql())
                    .map_err(|e| DbError::Corrupted {
                        path: std::path::PathBuf::new(),
                        detail: format!("Falha em integrity-check de {}: {e}", spec.fts_table),
                    })?;
            }
        }
        Ok(())
    }

    /// Executa `rebuild` em todas as tabelas FTS5 registradas que existam no banco.
    pub fn rebuild_all(&self, conn: &Connection) -> DbResult<()> {
        for spec in &self.specs {
            if table_exists(conn, spec.fts_table)? {
                conn.execute_batch(&spec.rebuild_sql())
                    .map_err(|e| DbError::from_rusqlite(e, "fts_rebuild"))?;
            }
        }
        Ok(())
    }
}

fn table_exists(conn: &Connection, table_name: &str) -> DbResult<bool> {
    let exists: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = ?1);",
            [table_name],
            |row| row.get(0),
        )
        .map_err(|e| DbError::from_rusqlite(e, "check_fts_table_exists"))?;
    Ok(exists != 0)
}

/// Verifica se um caractere é seguramente parte de um token do `unicode61` (incluindo `tokenchars '_-./:@'`) no SQLite FTS5.
#[inline]
fn is_safe_fts_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(ch, '_' | '-' | '.' | '/' | ':' | '@')
        || (ch.is_alphabetic() && !ch.is_whitespace() && !ch.is_control())
}

#[inline]
fn has_alphanumeric_char(token: &str) -> bool {
    token
        .chars()
        .any(|c| c.is_ascii_alphanumeric() || c.is_alphabetic())
}

/// Converte entrada arbitrária do usuário em uma expressão `MATCH` FTS5 estritamente segura:
/// - Limita a 256 caracteres e no máximo 16 termos.
/// - Extrai apenas tokens contendo letras/dígitos e `tokenchars` (`_-./:@`), neutralizando operadores
///   (`AND`, `OR`, `NOT`, `NEAR`), curingas soltos (`*`, `^`) e aspas.
/// - Envolve cada termo entre aspas duplas e adiciona sufixo de prefixo `*` ao último termo.
/// - Retorna `None` quando a entrada não contém nenhum termo pesquisável.
#[must_use]
pub fn build_match_query(input: &str) -> Option<String> {
    let mut terms: Vec<String> = Vec::new();
    let mut current = String::new();

    for ch in input.chars().take(MAX_MATCH_QUERY_CHARS) {
        if is_safe_fts_token_char(ch) {
            current.push(ch);
        } else if !current.is_empty() {
            let candidate = std::mem::take(&mut current);
            if has_alphanumeric_char(&candidate) {
                terms.push(candidate);
                if terms.len() >= MAX_MATCH_QUERY_TERMS {
                    break;
                }
            }
        }
    }

    if !current.is_empty() && terms.len() < MAX_MATCH_QUERY_TERMS && has_alphanumeric_char(&current)
    {
        terms.push(current);
    }

    if terms.is_empty() {
        return None;
    }

    let last_idx = terms.len() - 1;
    let mut out = String::new();
    for (idx, term) in terms.iter().enumerate() {
        if idx > 0 {
            out.push_str(" AND ");
        }
        let escaped = term.replace('"', "\"\"");
        if idx == last_idx {
            let _ = write!(&mut out, "\"{escaped}\"*");
        } else {
            let _ = write!(&mut out, "\"{escaped}\"");
        }
    }

    Some(out)
}

/// Variante que retorna `Err(DbError::QueryInvalid)` quando a consulta não possui termos válidos.
pub fn build_match_query_checked(input: &str) -> DbResult<String> {
    build_match_query(input).ok_or_else(|| DbError::QueryInvalid {
        reason: String::from("A consulta de busca não contém termos pesquisáveis válidos."),
    })
}

/// Formata um snippet seguro com delimitadores `«` e `»` sem jamais cortar um marcador `[REDACTED:*]`.
#[must_use]
pub fn format_safe_snippet(sanitized_text: &str, term: &str, max_chars: usize) -> String {
    let limit = max_chars.max(32);
    let truncated: String = sanitized_text.chars().take(limit).collect();

    // Se o corte truncou um marcador `[REDACTED:...`, remove o sufixo parcial `[REDACTED:...`
    let safe_base = if let Some(open_idx) = truncated.rfind("[REDACTED:") {
        if truncated[open_idx..].contains(']') {
            truncated
        } else {
            truncated[..open_idx].to_string()
        }
    } else {
        truncated
    };

    let clean_term = term.trim();
    if clean_term.is_empty() || clean_term.starts_with('[') || clean_term.contains("REDACTED") {
        return safe_base;
    }

    if let Some(pos) = safe_base.to_lowercase().find(&clean_term.to_lowercase())
        && let Some(end) = pos.checked_add(clean_term.len())
        && safe_base.is_char_boundary(pos)
        && safe_base.is_char_boundary(end)
    {
        // Nunca destaca dentro de um marcador [REDACTED:*]
        let before = &safe_base[..pos];
        if let Some(last_redacted) = before.rfind("[REDACTED:")
            && !before[last_redacted..].contains(']')
        {
            return safe_base;
        }
        let mut highlighted = String::with_capacity(safe_base.len() + 4);
        highlighted.push_str(&safe_base[..pos]);
        highlighted.push('«');
        highlighted.push_str(&safe_base[pos..end]);
        highlighted.push('»');
        highlighted.push_str(&safe_base[end..]);
        return highlighted;
    }

    safe_base
}

/// Cursor opaco versionado para paginação keyset determinística (`(sort_key, tie_breaker)` + `watermark`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeysetCursor {
    /// Escopo/operação dona do cursor (ex.: `"commands_by_session"`).
    pub scope: String,
    /// Hash canônico dos filtros aplicados na consulta.
    pub filter_hash: String,
    /// Marca d'água (`max(sort_key)` ou timestamp da 1ª página) para isolar inserções posteriores.
    pub watermark: i64,
    /// Chave primária de ordenação do último item entregue (ex.: `started_at`).
    pub sort_key: i64,
    /// Desempate único do último item entregue (ex.: `rowid` ou `id`).
    pub tie_breaker: i64,
}

impl KeysetCursor {
    /// Cria um novo cursor keyset.
    #[must_use]
    pub fn new(
        scope: impl Into<String>,
        filter_hash: impl Into<String>,
        watermark: i64,
        sort_key: i64,
        tie_breaker: i64,
    ) -> Self {
        Self {
            scope: scope.into(),
            filter_hash: filter_hash.into(),
            watermark,
            sort_key,
            tie_breaker,
        }
    }

    /// Codifica o cursor em formato textual opaco assinado com SHA-256.
    #[must_use]
    pub fn encode(&self) -> String {
        let payload = format!(
            "v1|{}|{}|{}|{}|{}",
            hex_encode(self.scope.as_bytes()),
            hex_encode(self.filter_hash.as_bytes()),
            self.watermark,
            self.sort_key,
            self.tie_breaker
        );
        let sig = short_sha256(&payload);
        format!("{payload}|{sig}")
    }

    /// Decodifica e valida o cursor contra o escopo e `filter_hash` esperados.
    pub fn decode(
        encoded: &str,
        expected_scope: &str,
        expected_filter_hash: &str,
    ) -> DbResult<Self> {
        if encoded.is_empty() || encoded.len() > MAX_CURSOR_LEN {
            return Err(DbError::CursorInvalid {
                reason: String::from("Tamanho de cursor fora dos limites permitidos"),
            });
        }

        let parts: Vec<&str> = encoded.split('|').collect();
        if parts.len() != 7 || parts[0] != "v1" {
            return Err(DbError::CursorInvalid {
                reason: String::from("Formato ou versão de cursor inválida"),
            });
        }

        let unsigned = parts[..6].join("|");
        let expected_sig = short_sha256(&unsigned);
        if parts[6] != expected_sig {
            return Err(DbError::CursorInvalid {
                reason: String::from("Assinatura de integridade do cursor inválida"),
            });
        }

        let scope_bytes = hex_decode(parts[1]).ok_or_else(|| DbError::CursorInvalid {
            reason: String::from("Escopo de cursor malformado"),
        })?;
        let filter_bytes = hex_decode(parts[2]).ok_or_else(|| DbError::CursorInvalid {
            reason: String::from("Filtro de cursor malformado"),
        })?;

        let scope = String::from_utf8(scope_bytes).map_err(|_| DbError::CursorInvalid {
            reason: String::from("Escopo de cursor não é UTF-8 válido"),
        })?;
        let filter_hash = String::from_utf8(filter_bytes).map_err(|_| DbError::CursorInvalid {
            reason: String::from("Filtro de cursor não é UTF-8 válido"),
        })?;

        if scope != expected_scope || filter_hash != expected_filter_hash {
            return Err(DbError::CursorInvalid {
                reason: String::from("Cursor pertence a outra operação ou filtro diferente"),
            });
        }

        let watermark = parts[3]
            .parse::<i64>()
            .map_err(|_| DbError::CursorInvalid {
                reason: String::from("Watermark inválido no cursor"),
            })?;
        let sort_key = parts[4]
            .parse::<i64>()
            .map_err(|_| DbError::CursorInvalid {
                reason: String::from("Sort key inválida no cursor"),
            })?;
        let tie_breaker = parts[5]
            .parse::<i64>()
            .map_err(|_| DbError::CursorInvalid {
                reason: String::from("Tie breaker inválido no cursor"),
            })?;

        Ok(Self {
            scope,
            filter_hash,
            watermark,
            sort_key,
            tie_breaker,
        })
    }
}

/// Página retornada por uma consulta paginada por keyset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeysetPage<T> {
    /// Itens da página atual.
    pub items: Vec<T>,
    /// Cursor opaco para buscar a próxima página (`None` quando chegou ao fim).
    pub next_cursor: Option<String>,
    /// Indica se há mais itens após esta página.
    pub has_more: bool,
}

/// Página retornada a partir de um snapshot de busca FTS5 com ranking congelado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedSearchPage<T> {
    /// Itens da fatia solicitada.
    pub items: Vec<T>,
    /// Cursor opaco apontando para o próximo `offset` dentro do mesmo snapshot.
    pub next_cursor: Option<String>,
    /// Indica se existem mais resultados no snapshot.
    pub has_more: bool,
}

#[derive(Debug, Clone)]
struct SnapshotEntry<T> {
    id: u64,
    query_hash: String,
    items: Vec<T>,
    approx_bytes: usize,
    created_at: Instant,
}

#[derive(Debug)]
struct SnapshotStore<T> {
    next_id: u64,
    total_bytes: usize,
    entries: VecDeque<SnapshotEntry<T>>,
}

/// Cache em memória RAM de snapshots de busca FTS5 (máx. 8 snapshots, 8 MiB e TTL de 30 s).
#[derive(Debug)]
pub struct SearchSnapshotCache<T> {
    max_snapshots: usize,
    max_bytes: usize,
    ttl: Duration,
    store: Mutex<SnapshotStore<T>>,
}

impl<T: Clone> Default for SearchSnapshotCache<T> {
    fn default() -> Self {
        Self::new(8, 8 * 1024 * 1024, Duration::from_secs(30))
    }
}

impl<T: Clone> SearchSnapshotCache<T> {
    /// Cria um novo cache de snapshots com os limites especificados.
    #[must_use]
    pub const fn new(max_snapshots: usize, max_bytes: usize, ttl: Duration) -> Self {
        Self {
            max_snapshots,
            max_bytes,
            ttl,
            store: Mutex::new(SnapshotStore {
                next_id: 1,
                total_bytes: 0,
                entries: VecDeque::new(),
            }),
        }
    }

    /// Armazena um novo snapshot e retorna o cursor inicial (apontando para `offset = 0`) ou de próxima página.
    pub fn insert_and_paginate(
        &self,
        query_hash: &str,
        items: Vec<T>,
        approx_bytes: usize,
        page_size: usize,
    ) -> CachedSearchPage<T> {
        let step = page_size.clamp(1, 200);
        let first_items: Vec<T> = items.iter().take(step).cloned().collect();
        let has_more = items.len() > step;

        if !has_more {
            return CachedSearchPage {
                items: first_items,
                next_cursor: None,
                has_more: false,
            };
        }

        let mut guard = self.store.lock();
        Self::evict_expired_locked(&mut guard, self.ttl);

        let item_bytes = approx_bytes.max(items.len() * std::mem::size_of::<T>());
        while guard.entries.len() >= self.max_snapshots
            || (guard.total_bytes.saturating_add(item_bytes) > self.max_bytes
                && !guard.entries.is_empty())
        {
            if let Some(removed) = guard.entries.pop_front() {
                guard.total_bytes = guard.total_bytes.saturating_sub(removed.approx_bytes);
            }
        }

        let snap_id = guard.next_id;
        guard.next_id += 1;
        guard.total_bytes = guard.total_bytes.saturating_add(item_bytes);
        guard.entries.push_back(SnapshotEntry {
            id: snap_id,
            query_hash: query_hash.to_string(),
            items,
            approx_bytes: item_bytes,
            created_at: Instant::now(),
        });

        let next_cursor = encode_snapshot_cursor(snap_id, query_hash, step);
        CachedSearchPage {
            items: first_items,
            next_cursor: Some(next_cursor),
            has_more: true,
        }
    }

    /// Obtém uma página subsequente a partir de um cursor de snapshot.
    pub fn get_page(
        &self,
        cursor: &str,
        expected_query_hash: &str,
        page_size: usize,
    ) -> DbResult<CachedSearchPage<T>> {
        let (snap_id, offset) = decode_snapshot_cursor(cursor, expected_query_hash)?;
        let step = page_size.clamp(1, 200);

        let mut guard = self.store.lock();
        Self::evict_expired_locked(&mut guard, self.ttl);

        let entry = guard
            .entries
            .iter()
            .find(|e| e.id == snap_id && e.query_hash == expected_query_hash)
            .ok_or_else(|| DbError::CursorInvalid {
                reason: String::from(
                    "cursor_expired: o snapshot de busca expirou ou foi invalidado",
                ),
            })?;

        if offset >= entry.items.len() {
            return Ok(CachedSearchPage {
                items: Vec::new(),
                next_cursor: None,
                has_more: false,
            });
        }

        let end = (offset + step).min(entry.items.len());
        let slice = entry.items[offset..end].to_vec();
        let has_more = end < entry.items.len();
        let next_cursor = if has_more {
            Some(encode_snapshot_cursor(snap_id, expected_query_hash, end))
        } else {
            None
        };

        Ok(CachedSearchPage {
            items: slice,
            next_cursor,
            has_more,
        })
    }

    /// Invalida imediatamente todos os snapshots em memória (chamado após deleção/retenção de histórico).
    pub fn invalidate_all(&self) {
        let mut guard = self.store.lock();
        guard.entries.clear();
        guard.total_bytes = 0;
    }

    fn evict_expired_locked(store: &mut SnapshotStore<T>, ttl: Duration) {
        let now = Instant::now();
        while let Some(front) = store.entries.front() {
            if now.saturating_duration_since(front.created_at) >= ttl {
                if let Some(removed) = store.entries.pop_front() {
                    store.total_bytes = store.total_bytes.saturating_sub(removed.approx_bytes);
                }
            } else {
                break;
            }
        }
    }
}

fn encode_snapshot_cursor(snap_id: u64, query_hash: &str, offset: usize) -> String {
    let payload = format!(
        "snap1|{snap_id}|{}|{offset}",
        hex_encode(query_hash.as_bytes())
    );
    let sig = short_sha256(&payload);
    format!("{payload}|{sig}")
}

fn decode_snapshot_cursor(cursor: &str, expected_query_hash: &str) -> DbResult<(u64, usize)> {
    if cursor.is_empty() || cursor.len() > MAX_CURSOR_LEN {
        return Err(DbError::CursorInvalid {
            reason: String::from("Cursor de busca inválido"),
        });
    }
    let parts: Vec<&str> = cursor.split('|').collect();
    if parts.len() != 5 || parts[0] != "snap1" {
        return Err(DbError::CursorInvalid {
            reason: String::from("Formato de cursor de busca inválido"),
        });
    }

    let unsigned = parts[..4].join("|");
    if parts[4] != short_sha256(&unsigned) {
        return Err(DbError::CursorInvalid {
            reason: String::from("Assinatura de cursor de busca inválida"),
        });
    }

    let snap_id = parts[1]
        .parse::<u64>()
        .map_err(|_| DbError::CursorInvalid {
            reason: String::from("ID de snapshot inválido"),
        })?;
    let qh_bytes = hex_decode(parts[2]).ok_or_else(|| DbError::CursorInvalid {
        reason: String::from("Hash de consulta malformado no cursor"),
    })?;
    let qh = String::from_utf8(qh_bytes).map_err(|_| DbError::CursorInvalid {
        reason: String::from("Hash de consulta inválido"),
    })?;

    if qh != expected_query_hash {
        return Err(DbError::CursorInvalid {
            reason: String::from("Cursor de busca pertence a outra consulta"),
        });
    }

    let offset = parts[3]
        .parse::<usize>()
        .map_err(|_| DbError::CursorInvalid {
            reason: String::from("Offset inválido no cursor de busca"),
        })?;

    Ok((snap_id, offset))
}

fn short_sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(16);
    for b in &digest[..8] {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn hex_decode(hex_str: &str) -> Option<Vec<u8>> {
    if hex_str.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(hex_str.len() / 2);
    let bytes = hex_str.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        let sub = std::str::from_utf8(&bytes[idx..idx + 2]).ok()?;
        let val = u8::from_str_radix(sub, 16).ok()?;
        out.push(val);
        idx += 2;
    }
    Some(out)
}
