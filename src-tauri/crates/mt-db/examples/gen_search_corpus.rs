//! Gerador determinístico de corpus de Busca Global FTS5 (`1.000.000` de linhas por padrão)
//! com seed fixa (`0x4D41_5854_3030_3137`) e manifesto auditável (`CorpusManifest` — `CA-08`).

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::bool_to_int_with_if,
    clippy::map_unwrap_or,
    clippy::cast_sign_loss
)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use mt_db::{DbConfig, MigrationRegistry, MigrationRunner, open_writer, wal_path_for};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Seed canônica de 64 bits da Fase 17 (`"MAXT0017"`).
pub const CANONICAL_CORPUS_SEED: u64 = 0x4D41_5854_3030_3137;

/// Manifesto auditável gerado junto ao banco de benchmark.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusManifest {
    pub seed_hex: String,
    pub total_rows: u64,
    pub rows_by_source: BTreeMap<String, u64>,
    pub db_size_bytes: u64,
    pub wal_size_bytes: u64,
    pub fts_size_bytes: u64,
    pub corpus_sha256: String,
}

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn choose<'a>(&mut self, items: &'a [&'a str]) -> &'a str {
        let idx = (self.next_u64() as usize) % items.len();
        items[idx]
    }
}

const COMMON_WORDS: &[&str] = &[
    "artisan",
    "migrate",
    "composer",
    "cargo",
    "vitest",
    "build",
    "release",
    "worker",
    "horizon",
    "queue",
    "redis",
    "postgres",
    "sqlite",
    "bundle",
    "tauri",
    "webkit",
    "wayland",
    "cachyos",
    "systemd",
    "socket",
    "branch",
    "worktree",
    "commit",
    "rebase",
    "merge",
    "session",
    "template",
    "project",
    "terminal",
    "scrollback",
    "painel",
    "atalho",
    "comando",
    "erro",
    "falha",
    "sucesso",
    "timeout",
    "conflito",
    "indice",
    "snapshot",
    "backup",
    "restore",
    "quarentena",
    "segredo",
    "token",
    "issue",
    "label",
    "comentario",
    "plano",
    "execucao",
];

fn parse_args() -> (PathBuf, u64) {
    let mut out_path = env::temp_dir().join("mt-bench-corpus.db");
    let mut scale_total: u64 = 1_000_000;

    let args: Vec<String> = env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" if i + 1 < args.len() => {
                out_path = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--rows" if i + 1 < args.len() => {
                if let Ok(parsed) = args[i + 1].parse::<u64>() {
                    scale_total = parsed.max(1_000);
                }
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }
    (out_path, scale_total)
}

fn main() {
    let (out_path, target_total) = parse_args();
    if out_path.exists() {
        let _ = fs::remove_file(&out_path);
    }
    let wal_path = wal_path_for(&out_path);
    if wal_path.exists() {
        let _ = fs::remove_file(&wal_path);
    }

    let cfg = DbConfig::for_path(&out_path);
    let mut conn = open_writer(&cfg).unwrap().conn;
    let reg = MigrationRegistry::builtin().unwrap();
    MigrationRunner::new(reg)
        .run(&mut conn, &cfg.db_path, &cfg.backup_dir, 7)
        .unwrap();

    // Distribuição proporcional exata (padrão = 1.000.000 linhas)
    let projects_count: u64 = 100.min(target_total / 20);
    let templates_count: u64 = 500.min(target_total / 10);
    let sessions_count: u64 = 5_000.min(target_total / 10);
    let groups_count: u64 = 400.min(target_total / 20);
    let group_runs_count: u64 = 4_000.min(target_total / 10);
    let issues_count: u64 = 10_000.min(target_total / 10);
    let comments_count: u64 = 30_000.min(target_total / 5);
    let metadata_sum = projects_count
        + templates_count
        + sessions_count
        + groups_count
        + group_runs_count
        + issues_count
        + comments_count;
    let remaining = target_total.saturating_sub(metadata_sum);
    let commands_count = (remaining * 35) / 95;
    let output_count = remaining.saturating_sub(commands_count);

    let mut rng = SplitMix64::new(CANONICAL_CORPUS_SEED);
    let mut hasher = Sha256::new();
    hasher.update(CANONICAL_CORPUS_SEED.to_le_bytes());

    conn.execute_batch("PRAGMA synchronous = NORMAL;").unwrap();

    // Inserimos a sessão base que referencia os command_blocks e session_scrollback
    {
        let tx = conn.transaction().unwrap();
        tx.execute(
            "INSERT INTO sessions (
                id, tab_id, pane_id, project_id, tab_session_id, pty_id,
                shell_kind, shell_executable, initial_cwd, integration_mode, started_at
            ) VALUES (
                'sess_bench_0', 'tab_bench_0', 'pane_bench_0', 'proj_bench_0', NULL, 'pty_bench_0',
                'zsh', '/bin/zsh', '/workspace/bench', 'osc133', 1700000000000
            );",
            [],
        )
        .unwrap();

        // 1. Popula search_metadata (projects, templates, sessions, groups, group_runs, issues, comments)
        let mut meta_stmt = tx
            .prepare(
                "INSERT INTO search_metadata (
                    source, entity_key, project_id, tab_id, session_id, pane_id,
                    exit_code, source_truncated, fetched_at, title, body,
                    target_json, occurred_at, updated_at, source_revision
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, '1');",
            )
            .unwrap();

        let mut insert_meta_batch = |source: &str,
                                     count: u64,
                                     rng: &mut SplitMix64,
                                     hasher: &mut Sha256| {
            for idx in 1..=count {
                let w1 = rng.choose(COMMON_WORDS);
                let w2 = rng.choose(COMMON_WORDS);
                let rare_suffix = if idx <= 50 && source == "projects" {
                    format!(" rare_token_{idx:04}")
                } else {
                    String::new()
                };
                let key = format!("{source}_{idx}");
                let title = format!("{source} {w1} #{idx}{rare_suffix}");
                let body = format!("Descrição determinística {w1} {w2} item {idx}");
                let target_json = match source {
                    "projects" => format!(r#"{{"kind":"project","projectId":"proj_{idx}"}}"#),
                    "templates" => format!(
                        r#"{{"kind":"template","projectId":"proj_1","templateId":"tpl_{idx}"}}"#
                    ),
                    "sessions" => format!(
                        r#"{{"kind":"session","projectId":"proj_1","tabId":"tab_1","sessionId":"sess_{idx}"}}"#
                    ),
                    "groups" => {
                        format!(r#"{{"kind":"group","groupId":"grp_{idx}","projectId":"proj_1"}}"#)
                    }
                    "group_runs" => format!(
                        r#"{{"kind":"group_run","runKey":"run_{idx}","itemKey":null,"projectId":"proj_1"}}"#
                    ),
                    "issues" => format!(
                        r#"{{"kind":"issue","repoKey":"owner/repo","number":{idx},"commentKey":null,"projectId":"proj_1"}}"#
                    ),
                    _ => format!(
                        r#"{{"kind":"issue","repoKey":"owner/repo","number":1,"commentKey":"c_{idx}","projectId":"proj_1"}}"#
                    ),
                };
                let exit_code: Option<i64> = if source == "group_runs" {
                    Some(if idx % 3 == 0 { 1 } else { 0 })
                } else {
                    None
                };
                let ts = 1_700_000_000_000_i64 + (idx as i64) * 10;
                hasher.update(key.as_bytes());
                meta_stmt
                    .execute(params![
                        source,
                        key,
                        "proj_bench_0",
                        Option::<String>::None,
                        Option::<String>::None,
                        Option::<String>::None,
                        exit_code,
                        0_i64,
                        Option::<i64>::None,
                        title,
                        body,
                        target_json,
                        ts
                    ])
                    .unwrap();
            }
        };

        insert_meta_batch("projects", projects_count, &mut rng, &mut hasher);
        insert_meta_batch("templates", templates_count, &mut rng, &mut hasher);
        insert_meta_batch("sessions", sessions_count, &mut rng, &mut hasher);
        insert_meta_batch("groups", groups_count, &mut rng, &mut hasher);
        insert_meta_batch("group_runs", group_runs_count, &mut rng, &mut hasher);
        insert_meta_batch("issues", issues_count, &mut rng, &mut hasher);
        insert_meta_batch("comments", comments_count, &mut rng, &mut hasher);
        drop(meta_stmt);
        tx.commit().unwrap();
    }

    // 2. Popula command_blocks em lotes de 50.000
    let batch_size = 50_000_u64;
    let mut inserted_cmds = 0_u64;
    while inserted_cmds < commands_count {
        let chunk_end = (inserted_cmds + batch_size).min(commands_count);
        let tx = conn.transaction().unwrap();
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO command_blocks (
                        id, session_id, tab_id, pane_id, pty_id, command_seq,
                        command_text, command_text_quality, cwd, started_at, ended_at,
                        duration_ms, exit_code, status, end_reason, is_continuous,
                        source, confidence, output_text, output_kind, output_bytes,
                        output_lines, inspectable, start_pty_seq, start_byte_offset,
                        end_pty_seq, end_byte_offset
                    ) VALUES (
                        ?1, 'sess_bench_0', 'tab_bench_0', 'pane_bench_0', 'pty_bench_0', ?2,
                        ?3, 'exact', '/workspace/bench', ?4, ?5,
                        50, ?6, ?7, ?8, 0,
                        'osc133', 'exact', ?9, 'full', 64,
                        1, 0, ?2, 0,
                        ?2, 64
                    );",
                )
                .unwrap();

            for seq in (inserted_cmds + 1)..=chunk_end {
                let w1 = rng.choose(COMMON_WORDS);
                let w2 = rng.choose(COMMON_WORDS);
                let id = format!("blk_bench_{seq}");
                let cmd = format!("{w1} {w2} --seq={seq}");
                let out = format!("Saída do comando {w1} {w2} #{seq}");
                let is_err = seq % 5 == 0;
                let exit_code: i64 = if is_err { 1 } else { 0 };
                let status = if is_err { "failed" } else { "succeeded" };
                let reason = if is_err { "exit_non_zero" } else { "exit_zero" };
                let ts = 1_700_010_000_000_i64 + (seq as i64) * 5;
                hasher.update(id.as_bytes());
                stmt.execute(params![
                    id,
                    seq as i64,
                    cmd,
                    ts,
                    ts + 50,
                    exit_code,
                    status,
                    reason,
                    out
                ])
                .unwrap();
            }
        }
        tx.commit().unwrap();
        inserted_cmds = chunk_end;
    }

    // 3. Popula session_scrollback em lotes de 50.000
    let mut inserted_out = 0_u64;
    while inserted_out < output_count {
        let chunk_end = (inserted_out + batch_size).min(output_count);
        let tx = conn.transaction().unwrap();
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO session_scrollback (
                        session_id, seq, ts, text, byte_len, kind
                    ) VALUES ('sess_bench_0', ?1, ?2, ?3, 64, 'output');",
                )
                .unwrap();
            for seq in (inserted_out + 1)..=chunk_end {
                let w1 = rng.choose(COMMON_WORDS);
                let w2 = rng.choose(COMMON_WORDS);
                let text = format!("[scrollback #{seq}] stream {w1} {w2} ok");
                let ts = 1_700_020_000_000_i64 + (seq as i64) * 2;
                stmt.execute(params![seq as i64, ts, text]).unwrap();
            }
        }
        tx.commit().unwrap();
        inserted_out = chunk_end;
    }

    conn.execute_batch(
        "INSERT INTO command_blocks_fts(command_blocks_fts) VALUES('optimize');
         INSERT INTO session_scrollback_fts(session_scrollback_fts) VALUES('optimize');
         INSERT INTO search_metadata_fts(search_metadata_fts) VALUES('optimize');
         PRAGMA wal_checkpoint(TRUNCATE);",
    )
    .unwrap();

    let db_size_bytes = fs::metadata(&out_path).map_or(0, |m| m.len());
    let wal_size_bytes = fs::metadata(&wal_path).map_or(0, |m| m.len());
    let fts_size_bytes: u64 = conn
        .query_row(
            "SELECT COALESCE(SUM(pgsize), 0) FROM dbstat WHERE name LIKE '%_fts%';",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|v| v.max(0) as u64)
        .unwrap_or(db_size_bytes / 3);

    let mut rows_by_source = BTreeMap::new();
    rows_by_source.insert(String::from("commands"), commands_count);
    rows_by_source.insert(String::from("output"), output_count);
    rows_by_source.insert(String::from("sessions"), sessions_count);
    rows_by_source.insert(String::from("projects"), projects_count);
    rows_by_source.insert(String::from("templates"), templates_count);
    rows_by_source.insert(String::from("groups"), groups_count);
    rows_by_source.insert(String::from("group_runs"), group_runs_count);
    rows_by_source.insert(String::from("issues"), issues_count);
    rows_by_source.insert(String::from("comments"), comments_count);

    let total_rows: u64 = rows_by_source.values().sum();
    let corpus_sha256 = hex::encode(hasher.finalize());

    let manifest = CorpusManifest {
        seed_hex: format!("0x{CANONICAL_CORPUS_SEED:016X}"),
        total_rows,
        rows_by_source,
        db_size_bytes,
        wal_size_bytes,
        fts_size_bytes,
        corpus_sha256,
    };

    let manifest_path = out_path.with_extension("manifest.json");
    let json = serde_json::to_string_pretty(&manifest).unwrap();
    fs::write(&manifest_path, &json).unwrap();
    println!("{json}");
}
