# Implementação 03 — Benchmarks Reprodutíveis, Profiling, Telemetria Dev e Orçamento de Bundle

> **Fase:** 17 — Busca Global, Qualidade, Performance e Empacotamento
> **Itens de Escopo Cobertos:** `I-07`, `I-08`, `I-09`
> **Critérios de Aceite Cobertos:** `CA-08`, `CA-10`, `CA-16`, `CA-17`, `CA-18`, `CA-19`, `CA-20`, `CA-21`

---

## 1. Contexto, Objetivo e Escopo

Esta implementação entrega toda a infraestrutura de **engenharia de performance mensurável, profiling e controle de orçamento de bundle** da Fase 17:
- Gerador determinístico de corpus de **1.000.000 de linhas** (`gen_search_corpus.rs`) e suíte Criterion (`benches/search.rs`) com 200 consultas versionadas (raras, comuns, prefixos, filtros combinados), medindo cache frio/quente, percentis `p50/p95/p99`, throughput do writer concorrente e espaço ocupado pelo FTS5/WAL.
- Instrumentação de profiling assíncrono (`src-tauri/src/perf.rs`) com spans `tracing` padronizados (sem jamais registrar texto de consultas ou output de PTY), feature opcional `tokio-console`, comando `perf_snapshot` compilado apenas sob `#[cfg(debug_assertions)]` e componente visual de desenvolvimento `src/components/dev/PerfOverlay.vue` (`import.meta.env.DEV`).
- Suíte de benchmarks de sistema e terminal (`scripts/bench/{burst-yes.sh, burst-cat.sh, tabs-20.sh, tui.sh, idle-5tabs.sh, report.mjs}`) com produtor determinístico de rajadas finitas (1 GiB e arquivo de 500 MiB) numeradas e com checksum, contabilidade fim a fim (`produzido → lido → enviado → consumido → ACK`), medição de latência `input → echo / pintura`, `20 abas` a `200 linhas/s/aba`, TUIs (`vim`/`htop` com fixture de 10.000 linhas), ociosidade (5 abas por 60 s) e agregador estatístico para `bench/environment.json` e `bench/baseline.json`.
- Verificador de orçamento de bundle compatível com Vite 8 (`scripts/check-bundle-budget.mjs`).

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

### 2.1. Criar em `src-tauri/`
- `src-tauri/crates/mt-db/examples/gen_search_corpus.rs` — Gerador determinístico (seed fixa `0x4D41_5854_3030_3137`) de banco SQLite com exatamente 1.000.000 de linhas indexadas e manifesto de distribuição/hash SHA-256.
- `src-tauri/crates/mt-db/benches/search.rs` — Benchmark Criterion + coletor de percentis `p50/p95/p99` sobre as 200 consultas versionadas, cache frio vs. quente e writer concorrente.
- `src-tauri/crates/mt-db/benches/queries_v1.json` — Catálogo versionado de 200 consultas (50 termos raros, 50 termos comuns, 50 buscas por prefixo de 2–3 chars, 50 consultas com filtros combinados).
- `src-tauri/crates/mt-pty/examples/gen_pty_burst.rs` — Binário produtor determinístico de rajadas finitas (`--bytes 1073741824` para 1 GiB ou `--file <path>` de 500 MiB) com cabeçalho de sequência monotônica, CRC32C/BLAKE3 por linha e resumo final de bytes e checksum.
- `src-tauri/src/perf.rs` — Spans padronizados (`search.queue`, `search.sql`, `search.serialize`, `pty.read`, `pty.send`, `pty.ack`, `term.consume`, `db.flush`), agregador de histogramas em memória para dev (`PerfCollector`) e comando `perf_snapshot` restrito a `#[cfg(debug_assertions)]`.

### 2.2. Modificar em `src-tauri/`
- `src-tauri/Cargo.toml` e `src-tauri/crates/mt-db/Cargo.toml` — Adicionar `criterion` em `[dev-dependencies]`, declarar `[[bench]] name = "search" harness = false` e a feature opcional `tokio-console = ["dep:console-subscriber", "tokio/tracing"]` no binário `maxterminal`.
- `src-tauri/src/lib.rs` — Inicializar `perf::init_tracing()` e registrar `perf_snapshot` exclusivamente quando `cfg!(debug_assertions)` for verdadeiro.

### 2.3. Criar em `src/`
- `src/components/dev/PerfOverlay.vue` — Overlay flutuante carregado via `defineAsyncComponent` apenas quando `import.meta.env.DEV` for `true`, exibindo métricas de IPC, busca (` última tecla → pintura`, debounce, fila, SQL, IPC), taxa de frames (`rAF` heartbeat), contadores de bytes/ACK do PTY e botão de exportação de amostra JSON.
- `src/composables/dev/usePerfTelemetry.ts` — Medição de `input → echo (consumo xterm)` e `input → pintura (próximo requestAnimationFrame)` usando `performance.now()` monotônico do mesmo relógio JS e IDs de correlação.

### 2.4. Criar em `scripts/` e `bench/`
- `scripts/bench/burst-yes.sh` — Benchmark de rajada contínua finita de **1 GiB** via `gen_pty_burst --bytes 1073741824`.
- `scripts/bench/burst-cat.sh` — Benchmark de leitura de arquivo determinístico de **500 MiB** (`cat` / `gen_pty_burst --file`).
- `scripts/bench/tabs-20.sh` — Benchmark de 20 abas concorrentes produzindo 200 linhas/s/aba (linhas de 120 bytes) por 60 s, medindo processo principal Rust, subprocessos WebKitGTK e filhos separadamente (`PSS` via `/proc/<pid>/smaps_rollup`, `RSS` e CPU via `pidstat`), busca simultânea e encerramento limpo.
- `scripts/bench/tui.sh` — Benchmark de TUIs (`vim` abrindo fixture de 10.000 linhas, rolagem, edição e troca de aba; `htop` em atualização contínua) em modo WebGL e fallback Canvas/DOM.
- `scripts/bench/idle-5tabs.sh` — Benchmark de ociosidade com 5 abas paradas no prompt e janela visível por 60 s após warmup de 15 s (medindo CPU normalizada por núcleo, context switches/wakeups em `/proc/<pid>/status`, PSS/RSS e timers).
- `scripts/bench/report.mjs` — Coletor do perfil de hardware/SO (`bench/environment.json`), agregador de amostras brutas (`p50`, `p95`, `p99`, média, desvio padrão) e validador contra `bench/baseline.json`.
- `scripts/check-bundle-budget.mjs` — Analisador estático dos artefatos gerados pelo Vite 8 (`dist/assets/`), medindo tamanhos brutos e gzip de entry JS, chunks assíncronos, CSS e recursos e confrontando com `bench/baseline.json`.
- `bench/environment.json` — Esquema de metadados do ambiente de execução.
- `bench/baseline.json` — Contrato de métricas e limites (`status: "pending_measurement"` até calibração aprovada em máquina dedicada).

---

## 3. Especificação Técnica Exaustiva

### 3.1. Gerador de Corpus de 1.000.000 de Linhas (`gen_search_corpus.rs`)

Distribuição determinística exata do corpus de **1.000.000 de registros** (seed `ChaCha8Rng::seed_from_u64(0x4D41_5854_3030_3137)`):
- `projects`: `100` projetos distribuídos em 6 stacks.
- `sessions`: `5.000` sessões (4.000 arquivadas com vínculo `tab_sessions`, 800 legadas sem vínculo, 200 ativas).
- `command_blocks`: `350.000` blocos de comando (com `command`, `cwd`, `output_text`, `exit_code` variando entre `0`, `1..255` e `NULL`, e `command_quality`).
- `session_scrollback`: `600.000` chunks de scrollback (`seq` sequencial por `(session_id, pane_id)`, linhas médias de 160 bytes).
- `tab_templates`: `500` templates.
- `command_groups` + `command_group_runs`: `4.400` registros.
- `github_issues` + `github_issue_comments`: `40.000` registros.
- **Total indexado nos FTS5 (`command_blocks_fts` + `session_scrollback_fts` + `search_metadata_fts`):** `1.000.000` de linhas.

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusManifest {
    pub seed_hex: String,
    pub total_rows: u64,
    pub rows_by_source: std::collections::BTreeMap<String, u64>,
    pub db_size_bytes: u64,
    pub wal_size_bytes: u64,
    pub fts_size_bytes: u64,
    pub corpus_sha256: String,
}
```

### 3.2. Spans de Profiling e Telemetria Dev (`src-tauri/src/perf.rs`)

```rust
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpanSummaryDto {
    pub name: String,
    pub count: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyFlowCountersDto {
    pub bytes_produced: u64,
    pub bytes_read: u64,
    pub bytes_sent: u64,
    pub bytes_acked: u64,
    pub detached_gap_bytes: u64,
    pub db_truncated_bytes: u64,
    pub high_watermark_queue_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerfSnapshotDto {
    pub captured_at_ms: i64,
    pub rust_rss_bytes: u64,
    pub rust_pss_bytes: u64,
    pub spans: Vec<SpanSummaryDto>,
    pub pty_flow: PtyFlowCountersDto,
}

/// Comando compilado e exposto SOMENTE em builds de desenvolvimento/debug.
#[cfg(debug_assertions)]
#[tauri::command]
#[specta::specta]
pub async fn perf_snapshot(
    collector: tauri::State<'_, std::sync::Arc<PerfCollector>>,
) -> Result<PerfSnapshotDto, mt_core::error::MtError>;
```

**Regra de segurança de telemetria:** Os spans (`search.queue`, `search.sql`, `search.serialize`, `pty.read`, `pty.send`, `pty.ack`, `term.consume`, `db.flush`) registram exclusivamente durações em microssegundos, contagens de bytes/linhas e códigos de status. É **terminantemente proibido** anexar `query`, `output_text`, `command` ou variáveis de ambiente nos campos de `tracing::info_span!`.

### 3.3. Medição de Latência no Frontend (`usePerfTelemetry.ts` e `PerfOverlay.vue`)

Para medir `CA-10` (busca: última tecla → pintura) e `CA-18` (terminal: input → echo consumo vs. pintura) sem misturar relógios diferentes:
1. **Busca Global (`CA-10`):**
   - `t_key = performance.now()` no evento `input` do `GlobalSearchPalette.vue`.
   - `t_debounce_end = performance.now()` ao iniciar `searchStore.run()`.
   - `t_ipc_start` e `t_ipc_end` ao redor de `await searchGlobal(request)`, extraindo do header/metadata de dev os tempos internos Rust (`queue_us`, `sql_us`, `serialize_us`).
   - `t_paint = performance.now()` dentro de `nextTick(() => requestAnimationFrame(...))` após a atualização de `sections`.
   - Calcula separadamente: `debounceMs = t_debounce_end - t_key`, `ipcTotalMs = t_ipc_end - t_ipc_start`, `queueMs`, `sqlMs`, `keyToPaintMs = t_paint - t_key`.
2. **Terminal Input → Echo (`CA-18`):**
   - Ao enviar um marcador correlacionado em modo de medição, grava `t_input = performance.now()`.
   - No callback de `xterm.write(chunk, () => { ... })` quando o marcador é processado pelo parser do xterm, grava `t_consume = performance.now()` (`inputToConsumeMs = t_consume - t_input`).
   - No `requestAnimationFrame` imediatamente subsequente ao callback do `xterm.write`, grava `t_frame = performance.now()` (`inputToPaintMs = t_frame - t_input`).

### 3.4. Contabilidade de Rajadas e Processos (`scripts/bench/*.sh` e `report.mjs`)

- **`burst-yes.sh` (1 GiB) e `burst-cat.sh` (500 MiB) (`CA-16`):**
  - Executam `gen_pty_burst` dentro do PTY real do MaxTerminal.
  - Quando a aba está visível/anexada com fluxo de ACK saudável: verifica que `bytes_produced == bytes_read == bytes_sent == bytes_acked` e que o hash BLAKE3 acumulado no consumidor coincide 100% com o hash emitido pelo produtor (`zero perda, zero duplicação, zero reordenação`).
  - Quando o teste simula aba desanexada/consumidor lento além do buffer de replay: verifica que a diferença exata é reportada em `Gap { lost_bytes }` e que o truncamento de persistência no SQLite é contabilizado em `db_truncated_bytes`.
- **Validação de Baseline e Orçamento de Bundle (`CA-21`):**
  - `scripts/check-bundle-budget.mjs` percorre `dist/` após `npm run build`, calcula bytes crus e `zlib.gzipSync(buf, { level: 9 })` por categoria (`entryJs`, `asyncJs`, `css`, `assets`, `total`).
  - Se `bench/baseline.json` tiver `status === "pending_measurement"` ou `environmentId` divergente do `bench/environment.json` atual, o script emite status explícito `"not_comparable"` (código de saída `2` no modo `--strict-gate`, ou relatório informativo no modo `--smoke` de CI compartilhada), **nunca** reportando aprovação falsa.

---

## 4. Regras de Negócio e Casos de Borda

- **Isolamento de processos na medição (`CA-17`, `CA-20`):** `report.mjs` coleta `/proc/<pid>/smaps_rollup` para separar: (a) processo principal `maxterminal` (Rust), (b) árvore `WebKitNetworkProcess` + `WebKitWebProcess`, e (c) processos filhos dos 20 PTYs (`bash`/`gen_pty_burst`), somando `Pss` para memória efetiva sem dupla contagem de `libc`/`libwebkit2gtk` e reportando `Rss` paralelamente.
- **Honestidade de métricas em Xvfb (`CA-19`):** Quando `tui.sh` rodar sob `Xvfb` (sem compositor/VSync físico), o relatório marca `displayServer: "xvfb"` e reporta apenas `frameCallbackIntervalsMs` e `rAF` sintético, bloqueando qualquer alegação de FPS real de GPU.
