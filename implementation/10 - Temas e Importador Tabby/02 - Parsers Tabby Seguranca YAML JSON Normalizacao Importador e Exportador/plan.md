# Implementação 02 — Parsers Tabby, Segurança YAML/JSON, Normalização, Importador e Exportador (`plan.md`)

> Fase 10 — Temas e Importador Tabby
> Especificação desta unidade: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Execução

### Passo 1 — Scanner Limitado YAML/JSON (`formats/mod.rs`)
- [ ] Implementar `parse_structured_document` em `src-tauri/crates/mt-themes/src/formats/mod.rs` usando `yaml_rust2::parser::Parser` para YAML e um `Visitor` limitado para JSON.
- [ ] Garantir a aplicação dos tetos antes da construção da árvore: profundidade ≤ 32, nós ≤ 65.536, escalares ≤ 16 KiB, rejeição de âncoras/aliases/merges `<<`/tags customizadas/múltiplos documentos e rejeição de chaves duplicadas.

### Passo 2 — Parsers de Formatos e Detecção (`formats/*.rs`)
- [ ] Implementar `TabbyConfigParser` em `formats/tabby_config.rs` suportando `colorScheme`, `lightColorScheme` (objeto, `null` ou referência por nome a `customColorSchemes`) e `customColorSchemes[]`.
- [ ] Implementar `TabbySchemeParser` em `formats/tabby_scheme.rs` suportando esquema único ou array de esquemas, `colors` como array ou mapa nomeado (`ANSI_NAMES`) e alias `selection`/`selectionBackground`.
- [ ] Implementar `WindowsTerminalParser` em `formats/windows_terminal.rs` e `XtermJsonParser` em `formats/xterm_json.rs`.
- [ ] Implementar `ParserRegistry` e `detect.rs` respeitando a prioridade `TabbyConfig` → `TabbyScheme` → `WindowsTerminal` → `XtermJson`.

### Passo 3 — Normalização (`normalize.rs`)
- [ ] Implementar `normalize(raw: RawScheme) -> Result<NormalizedScheme, ThemeError>` em `src-tauri/crates/mt-themes/src/normalize.rs`.
- [ ] Cobrir todos os códigos de `ThemeWarningCode` (`AlphaIgnored`, `PaletteExpanded`, `PaletteTruncated`, `LowContrast`, `UnknownFieldsIgnored`, `NameMissing`, `NameTruncated`, `CursorDefaulted`) e erros com caminho completo do campo (`terminal.customColorSchemes[1].colors[3]`).
- [ ] Conectar `builtin/mod.rs` ao `TabbySchemeParser` + `normalize`.

### Passo 4 — Importação Segura e Exportação Atômica (`import.rs` e `export.rs`)
- [ ] Implementar `read_regular_file_bounded`, `parse_file` e `scan_dir` em `src-tauri/crates/mt-themes/src/import.rs` usando `rustix::fs::openat` (`OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK`) + `fstat` e `WalkDir::new(root).max_depth(4).follow_links(false)`.
- [ ] Implementar `ThemeExportFormat`, `render_scheme` e `write_atomic` em `src-tauri/crates/mt-themes/src/export.rs`.

### Passo 5 — Fixtures e Suíte de Testes (`tests/import_tabby.rs` e `tests/fixtures/**`)
- [ ] Criar todas as fixtures em `src-tauri/crates/mt-themes/tests/fixtures/`: `tabby/config.yaml`, `tabby/config-light.yaml`, `tabby/config-by-name.yaml`, `tabby/config-with-secrets.yaml`, as 20 paletas em `tabby/schemes/`, `xterm/itheme.json`, `windows-terminal/campbell.json`, `windows-terminal/settings.json` e `invalid/*`.
- [ ] Implementar `src-tauri/crates/mt-themes/tests/import_tabby.rs` testando CA-06, CA-07, CA-08, CA-09 (incluindo `tempfile` > 1 MiB e `billion-laughs.yaml` rejeitado em < 100 ms), CA-10, CA-22 (varredura de 500 arquivos, symlink, FIFO, > 10.000 entradas ignoradas) e CA-35 (ausência de segredo sentinela nos resultados).
- [ ] Atualizar `tests/mapping_roundtrip.rs` e `tests/scale_snapshots.rs` para incluir o roundtrip Tabby e as 20 fixtures.

---

## 2. Comandos de Verificação e Validação

```bash
# Formatação e linting
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-themes --all-targets -- -D warnings

# Execução dos testes de importação, normalização, roundtrip e snapshots
cargo test --manifest-path src-tauri/Cargo.toml -p mt-themes --test import_tabby
cargo test --manifest-path src-tauri/Cargo.toml -p mt-themes --test mapping_roundtrip
cargo test --manifest-path src-tauri/Cargo.toml -p mt-themes --test scale_snapshots
cargo test --manifest-path src-tauri/Cargo.toml -p mt-themes -- --nocapture
```

---

## 3. Critérios de Aceite Atendidos

| Critério | Descrição | Verificação |
|----------|-----------|-------------|
| **CA-06** | Fixture `tabby/config.yaml` com `colorScheme` + 2 `customColorSchemes` gera 3 temas com `origin = tabby_config`. | `tests/import_tabby.rs`. |
| **CA-07** | `#abcd` em `foreground` gera aviso `alpha_ignored`; cores inválidas geram `theme.invalid_color` com caminho exato (`colors[3]`). | `normalize.rs` + `tests/import_tabby.rs`. |
| **CA-08** | Paleta com 8 (ou 9–15) cores expandida para 16 com `palette_expanded`; < 8 falha com `theme.invalid_palette`; > 16 trunca com `palette_truncated`. | `normalize.rs` + `tests/import_tabby.rs`. |
| **CA-09** | Arquivo > 1 MiB falha sem leitura integral; aliases/âncoras, tags customizadas, chaves duplicadas e profundidade > 32 recusados antes da árvore. | `tests/import_tabby.rs`. |
| **CA-10** | Detecção automática de `tabby_config`, `tabby_scheme`, `xterm_json` e `windows_terminal`; `{"foo":1}` falha com `theme.unsupported_format`. | `formats/detect.rs` + `tests/import_tabby.rs`. |
| **CA-11** | Roundtrip Tabby preserva nome, todas as cores (incluindo alfa na seleção) e `ContentHash`. | `tests/mapping_roundtrip.rs`. |
| **CA-35** | Fixtures cobrem seleção por nome, item inválido em coleção, paleta nomeada, 9–15 cores, chaves duplicadas, campos desconhecidos e segredo em seção alheia nunca exposto. | `tests/import_tabby.rs`. |
