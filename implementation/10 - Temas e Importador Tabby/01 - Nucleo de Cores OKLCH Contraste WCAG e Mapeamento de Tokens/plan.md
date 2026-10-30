# Implementação 01 — Núcleo de Cores, OKLCH, Contraste WCAG e Mapeamento de Tokens (`plan.md`)

> Fase 10 — Temas e Importador Tabby
> Especificação desta unidade: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Execução

### Passo 1 — Extração das Fixtures Canônicas de Tokens do `MaxComponentsUi`
- [ ] Inspecionar `@maxvue/max-components-ui/src/themes/tokens.scss` e `@maxvue/max-components-ui/src/themes/colors.scss` (ou `src/styles/theme.scss` da Fase 01) para extrair os valores hexadecimais exatos das 99 variáveis (`--background-0..950` e `--max-{primary,success,warning,danger,info}-{50..950,content}`) para os modos escuro e claro.
- [ ] Criar `src-tauri/crates/mt-themes/tests/fixtures/max-components-ui/dark-tokens.json` e `light-tokens.json` com as chaves ordenadas e valores `#rrggbb` em minúsculas.

### Passo 2 — Criação do Crate `mt-themes`, `ids.rs` e `error.rs`
- [ ] Adicionar `crates/mt-themes` ao `src-tauri/Cargo.toml` e criar `src-tauri/crates/mt-themes/Cargo.toml` com as dependências listadas em `spec.md` §3.1.
- [ ] Implementar `src-tauri/crates/mt-themes/src/ids.rs` com `ThemeId`, `BUILTIN_NAMESPACE`, `DEFAULT_THEME_ID` e `ContentHash`, incluindo teste unitário que comprova `ThemeId::builtin("max-navy-teal") == DEFAULT_THEME_ID`.
- [ ] Implementar `src-tauri/crates/mt-themes/src/error.rs` com `codes::*`, `ThemeError` e `From<ThemeError> for MtError` com mensagens em `pt-BR` e truncamento de valores inválidos a 64 caracteres.

### Passo 3 — Implementação de `color.rs` e `oklch.rs`
- [ ] Implementar `parse_color`, `Rgb`, `Rgba` e `Rgba::composite_over` em `src-tauri/crates/mt-themes/src/color.rs`.
- [ ] Escrever testes parametrizados (`rstest`) e `proptest` em `color.rs` cobrindo `#ABC`, `#aabbcc`, `#AABBCCDD`, `rgb(170, 187, 204)`, `rgba(170,187,204,0.5)`, `rgb(66.7%, 73.3%, 80%)` e erros (`#ggg`, `rgb(300,0,0)`, `hsl(0,0%,0%)`, string vazia, mistura inteiro/percentual).
- [ ] Implementar `rgb_to_oklch`, `oklch_to_rgb_clamped`, `in_srgb_gamut` e `mix_rgb_oklch` em `src-tauri/crates/mt-themes/src/oklch.rs`, com `proptest` de roundtrip sRGB → OKLCH → sRGB (±1 em 8 bits) e invariante de gamut.

### Passo 4 — Implementação de `scheme.rs`, `contrast.rs` e `scale.rs`
- [ ] Implementar `AnsiPalette`, `ColorScheme`, `ThemeAppearance`, `ThemeOrigin`, `ThemeWarningCode`, `ThemeWarning`, `ColorScheme::appearance()` e `ColorScheme::content_hash()` em `src-tauri/crates/mt-themes/src/scheme.rs`.
- [ ] Implementar `relative_luminance`, `contrast_ratio`, `WcagLevel`, `ContrastEntry`, `ContrastReport` e `build_report` em `src-tauri/crates/mt-themes/src/contrast.rs`.
- [ ] Implementar `generate_scale`, `background_ramp` e `pick_content` em `src-tauri/crates/mt-themes/src/scale.rs` garantindo monotonicidade estrita de luminosidade OKLCH (`50` mais claro → `950` mais escuro), `--background-500` acromático e contraste `-content` ≥ 4.5:1.

### Passo 5 — Implementação de `mapping/xterm.rs`, `mapping/tokens.rs` e `builtin/`
- [ ] Implementar `XtermTheme` (`From<&ColorScheme>` e `TryFrom<&XtermTheme>`) em `src-tauri/crates/mt-themes/src/mapping/xterm.rs`.
- [ ] Implementar `ThemeTokens`, `allowed_token_keys`, `tokens_from_scheme` e `scheme_from_tokens` em `src-tauri/crates/mt-themes/src/mapping/tokens.rs`.
- [ ] Criar os 6 arquivos YAML em `src-tauri/crates/mt-themes/src/builtin/` (`max-navy-teal.yaml`, `max-light.yaml`, `dracula.yaml`, `nord.yaml`, `solarized-dark.yaml`, `solarized-light.yaml`) com cabeçalho de licença (`MIT`) e implementar `load_builtins()` em `builtin/mod.rs`.

### Passo 6 — Testes de Integração e Snapshots (`mapping_roundtrip.rs` e `scale_snapshots.rs`)
- [ ] Criar `src-tauri/crates/mt-themes/tests/mapping_roundtrip.rs` validando roundtrip `ColorScheme` → `XtermTheme` → `ColorScheme` (`proptest` 256 casos preservando `ContentHash` e cores canônicas) e proteção semântica de `success`/`warning`/`danger`/`info` com paleta invertida.
- [ ] Criar `src-tauri/crates/mt-themes/tests/scale_snapshots.rs` validando CA-14, CA-15 e CA-16 (comparação exata de `max-navy-teal` contra `dark-tokens.json` e snapshots `insta` dos 6 builtins).

---

## 2. Comandos de Verificação e Validação

```bash
# Formatação e linting estrito do crate mt-themes
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-themes --all-targets -- -D warnings

# Execução dos testes unitários, proptest e snapshots insta
cargo test --manifest-path src-tauri/Cargo.toml -p mt-themes --lib
cargo test --manifest-path src-tauri/Cargo.toml -p mt-themes --test mapping_roundtrip
cargo test --manifest-path src-tauri/Cargo.toml -p mt-themes --test scale_snapshots
```

---

## 3. Critérios de Aceite Atendidos

| Critério | Descrição | Verificação |
|----------|-----------|-------------|
| **CA-01** (parcial) | `cargo fmt` e `cargo clippy -p mt-themes -- -D warnings` passam sem avisos. | Comandos acima retornam exit 0. |
| **CA-07** (cores) | Formatos `#ABC`, `#aabbcc`, `#AABBCCDD`, `rgb()`, `rgba()`, percentuais aceitos; inválidos rejeitados. | Testes unitários em `color.rs`. |
| **CA-11** (xterm) | Roundtrip `ColorScheme` ⇄ `XtermTheme` preserva todas as cores canônicas e `ContentHash`. | `tests/mapping_roundtrip.rs`. |
| **CA-12** | `contrast_ratio(#ffffff, #000000) == 21.0`, `contrast_ratio(#777777, #ffffff) ∈ [4.47, 4.49]`, `contrast_ratio(x, x) == 1.0`. | Testes unitários em `contrast.rs`. |
| **CA-13** | ANSI importado não altera `success`/`warning`/`danger`/`info`; apenas `primary` deriva de `cyan` com ajuste de luminância documentado. | `tests/mapping_roundtrip.rs`. |
| **CA-14** | Escalas monotônicas em luminosidade OKLCH (`50` → `950`), dentro do gamut sRGB e `-content` ≥ 4.5:1. | `tests/scale_snapshots.rs` e `scale.rs`. |
| **CA-15** | Rampa `--background-*` com passo `0 = background`, luminância crescente no escuro e decrescente no claro, snapshots estáveis. | `tests/scale_snapshots.rs`. |
| **CA-16** | Builtin "Max Navy/Teal" produz `--max-primary-500 = #00768e`, `--max-primary-950 = #00152a`, `--background-0 = #17293d` e demais tokens idênticos ao `MaxComponentsUi`. | `tests/scale_snapshots.rs`. |
