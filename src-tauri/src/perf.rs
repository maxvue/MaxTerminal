//! Instrumentação de profiling assíncrono, spans padronizados e telemetria de desenvolvimento.
//!
//! Regra de segurança de telemetria: os spans (`search.queue`, `search.sql`, `search.serialize`,
//! `pty.read`, `pty.send`, `pty.ack`, `term.consume`, `db.flush`) registram exclusivamente
//! durações em microssegundos, contagens de bytes/linhas e códigos de status. É proibido
//! anexar `query`, `output_text`, `command` ou variáveis de ambiente nos spans.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Nomes canônicos dos spans de performance do MaxTerminal.
pub const SPAN_SEARCH_QUEUE: &str = "search.queue";
pub const SPAN_SEARCH_SQL: &str = "search.sql";
pub const SPAN_SEARCH_SERIALIZE: &str = "search.serialize";
pub const SPAN_PTY_READ: &str = "pty.read";
pub const SPAN_PTY_SEND: &str = "pty.send";
pub const SPAN_PTY_ACK: &str = "pty.ack";
pub const SPAN_TERM_CONSUME: &str = "term.consume";
pub const SPAN_DB_FLUSH: &str = "db.flush";

/// Catálogo completo dos spans padronizados monitorados pelo coletor.
pub const CANONICAL_SPANS: [&str; 8] = [
    SPAN_SEARCH_QUEUE,
    SPAN_SEARCH_SQL,
    SPAN_SEARCH_SERIALIZE,
    SPAN_PTY_READ,
    SPAN_PTY_SEND,
    SPAN_PTY_ACK,
    SPAN_TERM_CONSUME,
    SPAN_DB_FLUSH,
];

/// Resumo estatístico de latência de um span em microssegundos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpanSummaryDto {
    /// Nome canônico do span (ex.: `search.sql`).
    pub name: String,
    /// Total de amostras registradas.
    pub count: u64,
    /// Percentil 50 em microssegundos.
    pub p50_us: u64,
    /// Percentil 95 em microssegundos.
    pub p95_us: u64,
    /// Percentil 99 em microssegundos.
    pub p99_us: u64,
    /// Valor máximo observado em microssegundos.
    pub max_us: u64,
}

/// Contadores acumulados de controle de fluxo de bytes dos PTYs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyFlowCountersDto {
    /// Total de bytes produzidos pelos processos filhos em PTYs monitorados.
    pub bytes_produced: u64,
    /// Total de bytes lidos pelos leitores de PTY.
    pub bytes_read: u64,
    /// Total de bytes enviados por IPC para o frontend.
    pub bytes_sent: u64,
    /// Total de bytes confirmados (`pty_ack`) pelo consumidor xterm.
    pub bytes_acked: u64,
    /// Total de bytes descartados em gap quando o consumidor estava desanexado.
    pub detached_gap_bytes: u64,
    /// Total de bytes truncados ao persistir scrollback/blocos no SQLite.
    pub db_truncated_bytes: u64,
    /// Pico histórico de bytes em fila aguardando ACK.
    pub high_watermark_queue_bytes: u64,
}

/// Fotografia instantânea das métricas de performance do processo Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerfSnapshotDto {
    /// Timestamp Unix em milissegundos no momento da captura.
    pub captured_at_ms: i64,
    /// Memória RSS do processo principal Rust em bytes.
    pub rust_rss_bytes: u64,
    /// Memória PSS do processo principal Rust em bytes (lida de `/proc/self/smaps_rollup`).
    pub rust_pss_bytes: u64,
    /// Sumários de latência por span canônico.
    pub spans: Vec<SpanSummaryDto>,
    /// Contadores de fluxo fim a fim do subsistema PTY.
    pub pty_flow: PtyFlowCountersDto,
}

/// Limite máximo de amostras retidas em janela circular por span em memória.
const MAX_SAMPLES_PER_SPAN: usize = 4096;

#[derive(Debug, Default)]
struct SpanHistogram {
    count: u64,
    max_us: u64,
    samples: Vec<u64>,
    write_idx: usize,
}

impl SpanHistogram {
    fn record(&mut self, duration_us: u64) {
        self.count = self.count.saturating_add(1);
        if duration_us > self.max_us {
            self.max_us = duration_us;
        }
        if self.samples.len() < MAX_SAMPLES_PER_SPAN {
            self.samples.push(duration_us);
        } else {
            self.samples[self.write_idx] = duration_us;
            self.write_idx = (self.write_idx + 1) % MAX_SAMPLES_PER_SPAN;
        }
    }

    fn summary(&self, name: &str) -> SpanSummaryDto {
        if self.samples.is_empty() {
            return SpanSummaryDto {
                name: name.to_owned(),
                count: 0,
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
                max_us: 0,
            };
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let p50_us = percentile_sorted(&sorted, 50.0);
        let p95_us = percentile_sorted(&sorted, 95.0);
        let p99_us = percentile_sorted(&sorted, 99.0);
        SpanSummaryDto {
            name: name.to_owned(),
            count: self.count,
            p50_us,
            p95_us,
            p99_us,
            max_us: self.max_us,
        }
    }
}

fn percentile_sorted(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let idx = ((pct / 100.0) * ((sorted.len() - 1) as f64)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Coletor em memória de telemetria de spans e fluxo de bytes de PTY.
#[derive(Debug)]
pub struct PerfCollector {
    histograms: Mutex<BTreeMap<String, SpanHistogram>>,
    bytes_produced: AtomicU64,
    bytes_read: AtomicU64,
    bytes_sent: AtomicU64,
    bytes_acked: AtomicU64,
    detached_gap_bytes: AtomicU64,
    db_truncated_bytes: AtomicU64,
    high_watermark_queue_bytes: AtomicU64,
}

impl Default for PerfCollector {
    fn default() -> Self {
        let mut map = BTreeMap::new();
        for name in CANONICAL_SPANS {
            map.insert(name.to_owned(), SpanHistogram::default());
        }
        Self {
            histograms: Mutex::new(map),
            bytes_produced: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_acked: AtomicU64::new(0),
            detached_gap_bytes: AtomicU64::new(0),
            db_truncated_bytes: AtomicU64::new(0),
            high_watermark_queue_bytes: AtomicU64::new(0),
        }
    }
}

impl PerfCollector {
    /// Cria uma nova instância compartilhada do coletor.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Registra uma amostra de duração em microssegundos para um span padronizado
    /// e emite evento estruturado sem conteúdo sensível.
    pub fn record_span(&self, span_name: &str, duration_us: u64, items_or_bytes: u64) {
        tracing::trace!(
            target: "mt_perf",
            span_name = span_name,
            duration_us = duration_us,
            units = items_or_bytes,
            "Amostra de span registrada"
        );
        let mut guard = self.histograms.lock();
        guard
            .entry(span_name.to_owned())
            .or_default()
            .record(duration_us);
    }

    /// Incrementa contadores de leitura/envio de PTY e atualiza a marca d'água da fila.
    pub fn record_pty_read_and_send(&self, read_bytes: u64, sent_bytes: u64) {
        self.bytes_produced.fetch_add(read_bytes, Ordering::Relaxed);
        let total_read = self.bytes_read.fetch_add(read_bytes, Ordering::Relaxed) + read_bytes;
        let total_sent = self.bytes_sent.fetch_add(sent_bytes, Ordering::Relaxed) + sent_bytes;
        let acked = self.bytes_acked.load(Ordering::Relaxed);
        let in_flight = total_sent.saturating_sub(acked);
        self.high_watermark_queue_bytes
            .fetch_max(in_flight, Ordering::Relaxed);
        let _ = total_read;
    }

    /// Registra confirmação (`ACK`) de bytes consumidos pelo frontend.
    pub fn record_pty_ack(&self, acked_bytes: u64) {
        self.bytes_acked.fetch_add(acked_bytes, Ordering::Relaxed);
    }

    /// Registra bytes perdidos em `Gap` por consumidor desanexado.
    pub fn record_detached_gap(&self, lost_bytes: u64) {
        self.detached_gap_bytes
            .fetch_add(lost_bytes, Ordering::Relaxed);
    }

    /// Registra bytes truncados ao persistir no SQLite.
    pub fn record_db_truncated(&self, truncated_bytes: u64) {
        self.db_truncated_bytes
            .fetch_add(truncated_bytes, Ordering::Relaxed);
    }

    /// Captura um snapshot instantâneo de memória, spans e contadores de PTY.
    #[must_use]
    pub fn snapshot(&self) -> PerfSnapshotDto {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0);

        let (rust_rss_bytes, rust_pss_bytes) = read_self_memory_bytes();
        let spans = {
            let guard = self.histograms.lock();
            guard
                .iter()
                .map(|(name, hist)| hist.summary(name))
                .collect::<Vec<_>>()
        };

        PerfSnapshotDto {
            captured_at_ms: now_ms,
            rust_rss_bytes,
            rust_pss_bytes,
            spans,
            pty_flow: PtyFlowCountersDto {
                bytes_produced: self.bytes_produced.load(Ordering::Relaxed),
                bytes_read: self.bytes_read.load(Ordering::Relaxed),
                bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
                bytes_acked: self.bytes_acked.load(Ordering::Relaxed),
                detached_gap_bytes: self.detached_gap_bytes.load(Ordering::Relaxed),
                db_truncated_bytes: self.db_truncated_bytes.load(Ordering::Relaxed),
                high_watermark_queue_bytes: self.high_watermark_queue_bytes.load(Ordering::Relaxed),
            },
        }
    }
}

/// Inicializa ganchos de telemetria de runtime (incluindo suporte à feature `tokio-console`).
pub fn init_tracing() {
    #[cfg(feature = "tokio-console")]
    {
        tracing::info!(
            target: "mt_perf",
            event = "tokio_console_feature_enabled",
            "Instrumentação de tasks Tokio habilitada para profiling"
        );
    }
}

fn read_self_memory_bytes() -> (u64, u64) {
    let mut rss_bytes = 0_u64;
    let mut pss_bytes = 0_u64;

    if let Ok(rollup) = std::fs::read_to_string("/proc/self/smaps_rollup") {
        for line in rollup.lines() {
            if let Some(rest) = line.strip_prefix("Rss:") {
                rss_bytes = parse_kb_line(rest) * 1024;
            } else if let Some(rest) = line.strip_prefix("Pss:") {
                pss_bytes = parse_kb_line(rest) * 1024;
            }
        }
    }

    if rss_bytes == 0 {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if let Some(rest) = line.strip_prefix("VmRSS:") {
                    rss_bytes = parse_kb_line(rest) * 1024;
                    break;
                }
            }
        }
    }

    if pss_bytes == 0 {
        pss_bytes = rss_bytes;
    }

    (rss_bytes, pss_bytes)
}

fn parse_kb_line(raw: &str) -> u64 {
    raw.split_whitespace()
        .next()
        .and_then(|tok| tok.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Comando compilado e exposto SOMENTE em builds de desenvolvimento/debug.
#[cfg(debug_assertions)]
#[tauri::command]
#[specta::specta]
pub async fn perf_snapshot(
    collector: tauri::State<'_, Arc<PerfCollector>>,
) -> Result<PerfSnapshotDto, mt_core::error::MtError> {
    Ok(collector.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collector_records_percentiles_and_pty_counters() {
        let collector = PerfCollector::new();
        for sample in 1..=100 {
            collector.record_span(SPAN_SEARCH_SQL, sample * 10, 1);
        }
        collector.record_pty_read_and_send(4096, 4096);
        collector.record_pty_ack(2048);
        collector.record_detached_gap(512);
        collector.record_db_truncated(128);

        let snap = collector.snapshot();
        let sql_span = snap
            .spans
            .iter()
            .find(|s| s.name == SPAN_SEARCH_SQL)
            .expect("span search.sql deve existir");
        assert_eq!(sql_span.count, 100);
        assert!(sql_span.p50_us >= 490 && sql_span.p50_us <= 510);
        assert!(sql_span.p95_us >= 940 && sql_span.p95_us <= 960);
        assert_eq!(sql_span.max_us, 1000);

        assert_eq!(snap.pty_flow.bytes_produced, 4096);
        assert_eq!(snap.pty_flow.bytes_read, 4096);
        assert_eq!(snap.pty_flow.bytes_sent, 4096);
        assert_eq!(snap.pty_flow.bytes_acked, 2048);
        assert_eq!(snap.pty_flow.detached_gap_bytes, 512);
        assert_eq!(snap.pty_flow.db_truncated_bytes, 128);
        assert_eq!(snap.pty_flow.high_watermark_queue_bytes, 4096);
    }
}
