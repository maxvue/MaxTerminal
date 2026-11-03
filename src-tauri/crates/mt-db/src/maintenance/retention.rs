//! Regras declarativas de retenção (`RetentionRule`) e expurgo em lotes limitados (`RetentionRegistry`).

use std::sync::LazyLock;

use regex::Regex;

use crate::error::{DbError, DbResult};

static IDENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z][a-z0-9_]*$")
        .unwrap_or_else(|e| panic!("Regex de identificador inválida: {e}"))
});

/// Regra de retenção temporal aplicada em lotes pelo `MaintenanceHandle`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionRule {
    /// Nome identificador da regra (ex.: `"command_blocks_retention"`).
    pub name: &'static str,
    /// Tabela alvo (deve possuir `rowid`).
    pub table: &'static str,
    /// Coluna de timestamp UNIX em milissegundos utilizada para o corte.
    pub timestamp_column: &'static str,
    /// Chave no `SettingsRegistry` que define a quantidade de dias de retenção.
    pub setting_key: &'static str,
    /// Predicado SQL estático opcional para filtrar linhas elegíveis (ex.: `"pinned = 0"`).
    pub extra_where: Option<&'static str>,
    /// Quantidade máxima de linhas deletadas por transação individual (`> 0`).
    pub batch_size: usize,
    /// Fallback opcional de dias de retenção caso a setting ainda não esteja registrada.
    pub default_days: Option<i64>,
}

impl RetentionRule {
    /// Valida os identificadores de tabela/coluna e `batch_size`.
    pub fn validate(&self) -> DbResult<()> {
        if self.name.trim().is_empty() {
            return Err(DbError::ConstraintViolation {
                constraint: String::from("RetentionRule.name não pode ser vazio"),
            });
        }
        if !IDENT_RE.is_match(self.table) {
            return Err(DbError::ConstraintViolation {
                constraint: format!("Tabela inválida em RetentionRule: '{}'", self.table),
            });
        }
        if !IDENT_RE.is_match(self.timestamp_column) {
            return Err(DbError::ConstraintViolation {
                constraint: format!(
                    "Coluna de timestamp inválida em RetentionRule: '{}'",
                    self.timestamp_column
                ),
            });
        }
        if self.batch_size == 0 {
            return Err(DbError::ConstraintViolation {
                constraint: String::from("RetentionRule.batch_size deve ser maior que zero"),
            });
        }
        Ok(())
    }

    /// Monta o comando `DELETE ... WHERE rowid IN (SELECT rowid ... LIMIT ?2)` para um lote.
    #[must_use]
    pub fn delete_batch_sql(&self) -> String {
        let table = self.table;
        let col = self.timestamp_column;
        if let Some(extra) = self.extra_where {
            format!(
                "DELETE FROM {table} WHERE rowid IN (SELECT rowid FROM {table} WHERE {col} < ?1 AND ({extra}) LIMIT ?2);"
            )
        } else {
            format!(
                "DELETE FROM {table} WHERE rowid IN (SELECT rowid FROM {table} WHERE {col} < ?1 LIMIT ?2);"
            )
        }
    }
}

/// Catálogo de regras de retenção registradas na abertura do banco.
#[derive(Debug, Clone, Default)]
pub struct RetentionRegistry {
    rules: Vec<RetentionRule>,
}

impl RetentionRegistry {
    /// Cria um registro de retenção vazio.
    #[must_use]
    pub const fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Constrói um `RetentionRegistry` validando todas as regras.
    pub fn from_rules(rules: Vec<RetentionRule>) -> DbResult<Self> {
        let mut reg = Self::new();
        for rule in rules {
            reg.register(rule)?;
        }
        Ok(reg)
    }

    /// Registra uma `RetentionRule` validada.
    pub fn register(&mut self, rule: RetentionRule) -> DbResult<()> {
        rule.validate()?;
        if !self.rules.iter().any(|r| r.name == rule.name) {
            self.rules.push(rule);
        }
        Ok(())
    }

    /// Retorna as regras de retenção registradas.
    #[must_use]
    pub fn rules(&self) -> &[RetentionRule] {
        &self.rules
    }
}
