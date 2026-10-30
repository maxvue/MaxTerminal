# Plano de Execução — Implementação 03: Benchmarks Reprodutíveis, Profiling, Telemetria Dev e Orçamento de Bundle

> **Fase:** 17 — Busca Global, Qualidade, Performance e Empacotamento
> **Especificação de Referência:** [`spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Gerador de Corpus de 1M Linhas e Benchmark Criterion (`mt-db`)
1. Criar `src-tauri/crates/mt-db/benches/queries_v1.json` com as 200 consultas versionadas (50 termos raros, 50 termos comuns, 50 prefixos de 2–3 chars e 50 filtros combinados).
2. Criar `src-tauri/crates/mt-db/examples/gen_search_corpus.rs` usando seed fixa (`0x4D41_5854_3030_3137`) para popular exatamente 1.000.000 de linhas distribuídas entre `command_blocks`, `session_scrollback`, `sessions`, `projects`, `tab_templates`, `command_groups`/`runs` e `github_issues`/`comments`, gravando o manifesto `CorpusManifest` com SHA-256 e tamanhos de DB/WAL/FTS.
3. Criar `src-tauri/crates/mt-db/benches/search.rs` com Criterion medindo `p50`, `p95` e `p99` em cache frio e cache quente, além de throughput com writer concorrente inserindo blocos em paralelo.

### Passo 2: Spans `tracing`, `tokio-console` e `PerfOverlay.vue`
1. Criar `src-tauri/src/perf.rs` com spans padronizados (`search.queue`, `search.sql`, `search.serialize`, `pty.read`, `pty.send`, `pty.ack`, `term.consume`, `db.flush`), coletor de percentis em memória e comando `perf_snapshot` protegido por `#[cfg(debug_assertions)]`.
2. Configurar a feature opcional `tokio-console` em `src-tauri/Cargo.toml`.
3. Criar `src/composables/dev/usePerfTelemetry.ts` e `src/components/dev/PerfOverlay.vue` (carregado apenas sob `import.meta.env.DEV`) para medir `última tecla → pintura` da busca e `input → consumo xterm / pintura` do terminal usando o mesmo relógio monotônico `performance.now()`.

### Passo 3: Produtor de Rajada e Scripts de Benchmark de Sistema
1. Criar `src-tauri/crates/mt-pty/examples/gen_pty_burst.rs` gerando stream determinístico numerado com checksum BLAKE3 por bloco e total para os cenários de 1 GiB (`--bytes 1073741824`) e arquivo de 500 MiB (`--file`).
2. Criar os scripts executáveis `scripts/bench/burst-yes.sh`, `scripts/bench/burst-cat.sh`, `scripts/bench/tabs-20.sh`, `scripts/bench/tui.sh` e `scripts/bench/idle-5tabs.sh`.
3. Criar `scripts/bench/report.mjs`, `bench/environment.json` e `bench/baseline.json` para capturar metadados completos da máquina (CPU, governor, kernel, glibc, WebKitGTK, GPU/compositor, tamanhos de DB/WAL/FTS), agregar percentis de ≥ 5 execuções e comparar contra o baseline aprovado.

### Passo 4: Verificador de Orçamento de Bundle Vite 8
1. Criar `scripts/check-bundle-budget.mjs` inspecionando `dist/` após `npm run build`, calculando tamanhos brutos e gzip de entry JS, chunks assíncronos, CSS e recursos estáticos e validando contra `bench/baseline.json`.
2. Adicionar os scripts `"bench:search"`, `"bench:system"`, `"check:bundle-budget"` no `package.json`.

---

## 2. Comandos Exatos de Verificação e Testes

Executar a partir de `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0`:

```bash
# 1. Compilar os exemplos de geração de corpus (1M linhas) e rajada de PTY
cargo check --manifest-path src-tauri/Cargo.toml --examples --benches

# 2. Gerar corpus determinístico de teste e validar benchmark de busca (CA-08)
cargo run --manifest-path src-tauri/Cargo.toml --release -p mt-db --example gen_search_corpus -- --out /tmp/mt-bench-corpus.db
cargo bench --manifest-path src-tauri/Cargo.toml -p mt-db --bench search -- --quick

# 3. Verificar build de produção do frontend e executar analisador de bundle (CA-21)
npm run build
node scripts/check-bundle-budget.mjs --smoke

# 4. Verificar que o comando perf_snapshot NÃO existe no build de release
cargo check --manifest-path src-tauri/Cargo.toml --release
```

---

## 3. Mapeamento para os Critérios de Aceite da Fase 17

| Critério | Teste / Script Responsável | Resultado Esperado |
|----------|----------------------------|--------------------|
| `CA-08` | `gen_search_corpus.rs` + `cargo bench -p mt-db --bench search` | Corpus determinístico de 1.000.000 de linhas e 200 consultas versionadas; publica `p50/p95/p99` frio/quente, writer concorrente e tamanho FTS/WAL. |
| `CA-10` | `usePerfTelemetry.ts` + `scripts/bench/report.mjs` | Mede separadamente `última tecla → pintura`, debounce, fila, SQL e IPC no Tauri real; cancelar A nunca interrompe B. |
| `CA-16` | `scripts/bench/burst-yes.sh` + `burst-cat.sh` | Rajadas de 1 GiB e 500 MiB medem bytes produzidos/lidos/enviados/consumidos e hashes; zero perda/duplicação/reordenação no canal anexado e contabilidade exata de `Gap` quando desanexado. |
| `CA-17` | `scripts/bench/tabs-20.sh` | 20 abas a 200 linhas/s/aba medindo app Rust, WebKit e filhos separadamente (`PSS`/`RSS`/CPU), busca concorrente e encerramento de todos os filhos. |
| `CA-18` | `usePerfTelemetry.ts` + `PerfOverlay.vue` | Mede `input → echo` com ID de correlação e relógio monotônico único, separando consumo `xterm.write` e frame de pintura (`rAF`). |
| `CA-19` | `scripts/bench/tui.sh` | Fixture de 10.000 linhas no `vim` e `htop`; mede intervalos de frame, input e troca de aba em WebGL e fallback, identificando explicitamente quando executado sob Xvfb. |
| `CA-20` | `scripts/bench/idle-5tabs.sh` | 5 abas paradas por 60 s após warmup; mede CPU por núcleo, wakeups, PSS/RSS e backlog. |
| `CA-21` | `scripts/check-bundle-budget.mjs` | Calcula tamanhos crus e gzip de JS inicial/assíncrono, CSS e assets; sem baseline válido ou com ambiente divergente retorna `"not_comparable"` (nunca aprovação falsa). |
