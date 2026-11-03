//! DTOs compartilhados entre `mt-db`, a camada Tauri IPC (`src-tauri`) e os bindings TypeScript (`specta`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Fotografia de uma configuração tipada com sua revisão atual.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingSnapshot {
    /// Chave canônica (ex.: `"ui.theme"`).
    pub key: String,
    /// Valor JSON atual (default do registro ou persistido no banco).
    pub value: Value,
    /// Revisão monotônica (`0` quando ainda usa o default sem linha na tabela `settings`).
    pub revision: i64,
    /// Timestamp UNIX em milissegundos da última atualização (`0` quando default).
    pub updated_at: i64,
}

/// Origem de uma alteração de configuração.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SettingChangeOrigin {
    /// Alteração solicitada pela UI via IPC.
    Ui,
    /// Alteração aplicada por migração de esquema ou código.
    Migration,
    /// Alteração originada por sincronização ou subsistema interno.
    Sync,
}

/// Metadados de um arquivo de backup SQLite em `backups/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntryDto {
    /// Nome do arquivo (ex.: `"maxterminal-20260417T153045Z-manual.db"`).
    pub file_name: String,
    /// Caminho absoluto do arquivo de backup.
    pub path: String,
    /// Motivo da geração (`"pre_migration"`, `"pre_reset"`, `"pre_restore"`, `"manual"`, `"daily"`, `"corrupted"`).
    pub kind: String,
    /// Timestamp UNIX em milissegundos de criação do backup.
    pub created_at: i64,
    /// Tamanho do arquivo em bytes.
    pub size_bytes: i64,
}

/// Confirmação de agendamento de restauração de backup (`pending-restore.json`) para o próximo boot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreScheduledDto {
    /// Nome do arquivo de backup validado dentro de `backups/`.
    pub backup_file_name: String,
    /// Timestamp UNIX em milissegundos do agendamento.
    pub scheduled_at_ms: i64,
    /// Indica que o aplicativo precisa ser reiniciado para concluir a restauração antes de abrir os pools.
    pub requires_restart: bool,
}

/// Estado operacional do banco de dados exposto à UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DbHealthStateDto {
    /// Banco íntegro, migrado e operacional.
    Ready {
        /// Maior versão de migration aplicada.
        schema_version: i64,
        /// Versão reportada pelo SQLite (ex.: `"3.51.3"`).
        sqlite_version: String,
    },
    /// Banco bloqueado em modo de recuperação aguardando decisão explícita do usuário.
    RecoveryRequired {
        /// Código estável do erro que causou a entrada em modo de recuperação.
        code: String,
        /// Mensagem amigável em pt-BR.
        reason: String,
        /// Caminho do arquivo `.db`.
        db_path: String,
        /// Lista de backups íntegros disponíveis para restauração.
        available_backups: Vec<BackupEntryDto>,
    },
}

/// Fotografia completa do status do subsistema de persistência (`db_status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbStatusDto {
    /// Estado de saúde (`ready` ou `recovery_required`).
    pub health: DbHealthStateDto,
    /// Bytes atualmente alocados no `ByteBudget` do `BatchWriter`.
    pub pending_bytes: u32,
    /// Registros pendentes no `BatchWriter`.
    pub pending_records: i64,
    /// Total acumulado de eventos de saturação/lag do writer.
    pub writer_lag_events: i64,
}

/// Ação de recuperação escolhida pelo usuário no `DbRecoveryPanel.vue`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DbRecoveryAction {
    /// Tenta reabrir e revalidar o banco sem alterar arquivos.
    RetryOpen,
    /// Move o banco problemático para quarentena e restaura o backup informado.
    RestoreFromBackup {
        /// Nome do arquivo de backup em `backups/` (validado contra path traversal).
        backup_file_name: String,
    },
    /// Move o banco problemático para `backups/corrupted-<ts>.db` e recria um banco limpo.
    ResetDatabase,
}

/// Evento emitido quando uma configuração é alterada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChanged {
    /// Chave alterada.
    pub key: String,
    /// Novo valor JSON.
    pub value: Value,
    /// Nova revisão monotônica.
    pub revision: i64,
    /// Origem da alteração.
    pub origin: SettingChangeOrigin,
}

/// Evento emitido quando o banco é recuperado de corrupção ou backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbRecovered {
    /// Estratégia executada (`"retry_open"`, `"restore_from_backup"`, `"reset_database"`).
    pub strategy: String,
    /// Caminho do arquivo movido para quarentena, se aplicável.
    pub quarantined_path: Option<String>,
    /// Timestamp UNIX em milissegundos da recuperação.
    pub recovered_at: i64,
}

/// Evento emitido ao concluir um ciclo de manutenção (retenção, optimize, checkpoint).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbMaintenanceFinished {
    /// Total de linhas expurgadas pelas regras de retenção.
    pub rows_deleted: i64,
    /// Páginas no WAL antes ou após checkpoint.
    pub wal_pages_checkpointed: i64,
    /// Indica se `PRAGMA incremental_vacuum` foi executado no ciclo.
    pub vacuum_ran: bool,
    /// Duração total do ciclo em milissegundos.
    pub duration_ms: i64,
}

/// Evento emitido quando uma escrita ou lote demora além do limiar ou sofre contenção.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbWriterLag {
    /// Bytes pendentes no `ByteBudget`.
    pub pending_bytes: u32,
    /// Registros pendentes na fila.
    pub pending_records: i64,
    /// Idade do item mais antigo na fila em milissegundos.
    pub oldest_age_ms: i64,
}

/// Porta de saída para emissão de eventos do banco sem acoplar `mt-db` ao runtime Tauri.
pub trait DbEventSink: Send + Sync + 'static {
    /// Notifica alteração de configuração (`db://settings-changed`).
    fn on_settings_changed(&self, _ev: SettingsChanged) {}
    /// Notifica conclusão de recuperação (`db://recovered`).
    fn on_recovered(&self, _ev: DbRecovered) {}
    /// Notifica conclusão de ciclo de manutenção (`db://maintenance-finished`).
    fn on_maintenance_finished(&self, _ev: DbMaintenanceFinished) {}
    /// Notifica atraso ou pressão na fila do writer (`db://writer-lag`).
    fn on_writer_lag(&self, _ev: DbWriterLag) {}
}

/// Implementação silenciosa de `DbEventSink` usada em testes unitários e benchmarks.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopDbEventSink;

impl DbEventSink for NoopDbEventSink {
    fn on_settings_changed(&self, _ev: SettingsChanged) {}
    fn on_recovered(&self, _ev: DbRecovered) {}
    fn on_maintenance_finished(&self, _ev: DbMaintenanceFinished) {}
    fn on_writer_lag(&self, _ev: DbWriterLag) {}
}
