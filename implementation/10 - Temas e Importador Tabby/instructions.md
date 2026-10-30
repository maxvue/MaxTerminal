# Fase 10 — Temas e Importador Tabby (`instructions.md`)

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) (§2 "Temas e Cores", Diretriz §3 tokens semânticos) e contratos
> compartilhados de [`../README.md`](../README.md) (§2.1–§2.6).
> Especificação técnica detalhada: [`./spec.md`](./spec.md).

---

## 1. Objetivo da Fase

Entregar o subsistema completo de **aparência** do MaxTerminal:

- Crate `mt-themes` com **importador nativo de esquemas de cores do Tabby Terminal** (YAML/JSON), validação e
   normalização de cores, e motor de mapeamento entre um esquema de terminal (16 cores ANSI +
  foreground/background/cursor/seleção) e:
  - (a) o `ITheme` do xterm (interface `TerminalTheme` da Fase 04);
  - (b) os tokens semânticos do `@maxvue/max-components-ui` (`--background-*`, `--max-primary-*`, `--max-success-*`,
    `--max-warning-*`, `--max-danger-*`, `--max-info-*`), gerando escalas 50–950 a partir das cores ANSI, com
     cálculo de contraste WCAG 2.x. O terminal preserva sua paleta; a UI preserva as escalas canônicas de status.
     A conversão de tokens para paleta é uma aproximação de exportação, não uma inversa sem perdas.
- Persistência em SQLite (tabela `themes` com flag `builtin`, origem e JSON normalizado; configurações de aparência;
  sobreposição de tema por projeto).
- Tema builtin padrão **"Max Navy/Teal"** (reproduz exatamente os tokens atuais do `MaxComponentsUi`, sem regressão
  visual em relação à Fase 01) e um pequeno conjunto de builtins adicionais.
- Aplicação **em tempo real** do tema na UI (CSS custom properties + `useDark`) e em todos os terminais xterm vivos,
  sem recarregar a janela e sem reiniciar PTYs.
- Configurações de **fonte do terminal** (família, tamanho, altura de linha, espaçamento, pesos e ligaduras).
- Tela **Preferências > Aparência** com seletor de temas, preview ao vivo, importação (arquivo/pasta), exportação,
  exclusão, tema por projeto e ajustes de fonte.

---

## 2. Escopo

### 2.1. Incluído

| # | Item | Detalhe |
|---|------|---------|
| I-01 | Crate `mt-themes` | Módulos `color` (parse/format/OKLCH/WCAG), `scheme` (modelo normalizado), `formats/` (parsers Tabby e similares), `normalize`, `mapping/` (xterm e tokens), `scale` (rampas 50–950 e `--background-*`), `contrast` (relatório WCAG), `builtin/` (YAML embutidos), `import` (arquivo e pasta), `export`, `service` (`ThemeService`), `error`. |
| I-02 | Formatos de entrada | (1) `config.yaml` do Tabby: `terminal.colorScheme`, `terminal.lightColorScheme` (quando existir) e `terminal.customColorSchemes[]`; (2) esquema Tabby individual YAML/JSON (`name`, `foreground`, `background`, `cursor`, `colors[16]`, opcionais `cursorAccent`, `selection`, `selectionForeground`); (3) JSON no formato `ITheme` do xterm (`black`…`brightWhite`); (4) JSON de esquema no formato Windows Terminal (`purple`, `cursorColor`, `selectionBackground`) — "similares" compatíveis com as coleções tabby-*. |
| I-03 | Validação/normalização de cores | Aceita `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()` (inteiros ou percentuais, com ou sem espaços); normaliza para `#rrggbb` minúsculo (alfa preservado apenas na seleção); paleta de 8 cores expandida para 16 com aviso; avisos de contraste. |
| I-04 | Importação | `theme_import_file` (um arquivo pode gerar N temas) e `theme_import_dir` (varredura com limites de profundidade, quantidade e tamanho), relatório `ImportReport` com importados, duplicados e falhas por arquivo. Deduplicação por hash de conteúdo normalizado. |
| I-05 | Persistência | Migration `0006_10_themes.sql` em `mt-db/migrations/` com tabelas `themes`, `appearance_settings`, `project_themes` e índices; repositório `mt-db/src/repos/themes.rs`. |
| I-06 | Builtins | "Max Navy/Teal" (padrão, escuro), "Max Claro" (claro), "Dracula", "Nord", "Solarized Dark", "Solarized Light" — embutidos via `include_str!`, semeados/atualizados no startup por `slug` + `builtin_version`, imutáveis. |
| I-07 | Mapeamento xterm | `ColorScheme` → `XtermTheme` (espelho dos campos usados de `ITheme`); `toTerminalTheme` adapta à estrutura da Fase 04. Reconversão preserva cores, não metadados como nome. |
| I-08 | Mapeamento tokens | `ColorScheme` → `ThemeTokens`: superfícies e escala primary derivadas; success/warning/danger/info provenientes dos tokens Max canônicos para o modo claro/escuro, sem usar ANSI arbitrário. Rampa `--background-0…950` em passos de 25, escalas 50–950 e `-content`; `token_overrides` somente de builtins confiáveis. Exportação de tokens para paleta é aproximada. |
| I-09 | Contraste WCAG | Luminância relativa e razão de contraste WCAG 2.x; `ContrastReport` por tema (fg/bg, ANSI/bg, `-500`/`-content`) com níveis `AAA`/`AA`/`AA_LARGE`/`FAIL`; escolha automática de `-content` ≥ 4.5:1. |
| I-10 | Tema por projeto | Sobreposição opcional por `ProjectId` (Fase 06): aplica-se aos terminais das abas do projeto e, quando `applyProjectThemeToUi = true`, aos tokens da UI enquanto o projeto estiver selecionado. |
| I-11 | Fonte do terminal | `fontFamily`, `fontSize`, `lineHeight`, `letterSpacing`, `fontWeight`, `fontWeightBold`, `ligatures`, aplicados ao vivo nos terminais (com re-fit). |
| I-12 | Comandos IPC | `theme_list`, `theme_get`, `theme_appearance_get`, `theme_appearance_update`, `theme_import_file`, `theme_import_dir`, `theme_set_active`, `theme_delete`, `theme_export`. |
| I-13 | Eventos | `theme://changed`, `theme://library-changed`. |
| I-14 | Frontend | `src/ipc/themes.ts`, `src/stores/theme.ts` (`useThemeStore`), `src/composables/{useThemeApplier,useTerminalAppearance,useThemePreview}.ts`, `src/theme/{cssTokens,bootstrap,terminalBridge}.ts`, componentes em `src/components/preferences/appearance/` (`AppearancePreferences.vue`, `ThemePicker.vue`, `ThemePreview.vue`, `ThemeSwatch.vue`, `TerminalFontSettings.vue`, `ProjectThemeOverrides.vue`, `ThemeImportReport.vue`). |
| I-15 | Preferências | Se ainda não existir um contêiner de Preferências criado por fase anterior, criar `PreferencesDialog.vue` com registro de seções (`registerPreferencesSection`) — ponto de extensão para as Fases 12, 13 e seguintes. |
| I-16 | Sem FOUC | Último conjunto de tokens aplicado é cacheado em `localStorage` (`mt.theme.lastApplied`) e reaplicado antes do `mount()` da aplicação. |

### 2.2. Fora de escopo (e destino)

| Item | Destino |
|------|---------|
| Cores/ícones de identidade de projeto (auto-detecção, `max-big-icon-pack`) | Fase 06 (já entregue; esta fase apenas lê `ProjectId`/lista de projetos) |
| Motor xterm, addons WebGL/fit/search/web-links e camada de blocos | Fase 04 (esta fase apenas fornece valores de tema/fonte via ponto de extensão) |
| Significado dos estados e identidade dos projetos | Fases 05/06; esta fase mantém verde/sucesso, azul-teal/contínuo, âmbar/finalizado, vermelho/erro e cinza/parado, mesmo com ANSI importado monocromático ou invertido |
| Cores das 24 labels de Issues | Fase 16 (consome `useThemeStore().effectiveTokens` se desejar) |
| Cores de telemetria/StatusLine de agentes | Fase 13 |
| Busca global FTS5 (inclusive por nome de tema), testes E2E de troca de tema, benchmarks e empacotamento | Fase 17 |
| Editor visual de cores, imagem/transparência de fundo, download de coleções online, formato `.itermcolors` | Não previstos no `AGENTS.md` — não implementar |
| Sincronizar tema com o VS Code/MaxCode | Não previsto no `AGENTS.md`; a Fase 11 pode consumir `theme_get` se vier a ser especificado |

---

## 3. Pré-requisitos

### 3.1. Fases
| Fase | Contrato consumido |
|------|--------------------|
| 01 | `mt-core` (`MtError`, `MtResult`, `AppPaths`), `tauri-specta` + `bindings.ts`, `src/ipc/core.ts`, `src/styles/theme.scss` (fallback Max Navy/Teal), `useIpcError`. O crate `mt-themes` é criado nesta fase. |
| 03 | `mt-db` (conexão WAL, executor de migrations versionadas, padrão de repositórios, convenção de timestamps). |
| 04 | Interface TS estruturada `TerminalTheme`, provider, registry, extensões e `applyTheme/applyPreferences`; não é um espelho plano de `ITheme`. |
| 06 | Tabela `projects` (FK de `project_themes`), `useProjectsStore` (lista e seleção), `project://changed` com `kind = deleted`. |
| 07/09 (migrations) | `0004_07_workspace_templates_tabs_sessions.sql` e `0005_09_os_settings.sql` presentes no runner antes de `0006_10_themes.sql` (D-01). Essa dependência de persistência não exige acoplar a UI de temas aos serviços de workspace/SO. |

Antes de iniciar, verificar nos `spec.md` das Fases 03, 04 e 06 os nomes exatos desses contratos e ajustar apenas
os adaptadores (`terminalBridge.ts`, `repos/themes.rs`) — nunca o modelo desta fase.

### 3.2. Ferramentas e dependências novas
| Item | Uso | Verificação |
|------|-----|-------------|
| `serde_yaml_ng` + scanner de eventos YAML (`yaml-rust2` ou equivalente validado) | Parse YAML com limites anteriores à construção de objetos; versão fixada no lockfile | `cargo tree -p mt-themes` |
| `serde_json`, `sha2`, `walkdir`, `rustix` (`fs`), `lru`, `uuid` (v5 + v7) | JSON, hash, varredura, abertura segura por descritor, cache, ids | idem |
| `insta`, `proptest` (dev) | Snapshots de rampas/tokens e testes de propriedade de cores | `cargo test -p mt-themes` |
| `tauri-plugin-dialog` (+ `@tauri-apps/plugin-dialog`) | Diálogos de abrir arquivo/pasta e salvar exportação (registrar se ainda não registrado) | `capabilities/default.json` contém `dialog:allow-open`, `dialog:allow-save` |
| `@xterm/addon-ligatures` | Ligaduras (carregado sob demanda) | `npm ls @xterm/addon-ligatures` |
| Fixtures Tabby | `config.yaml` real e esquemas individuais em `crates/mt-themes/tests/fixtures/` | presentes no repositório |

---

## 4. Entregáveis

### 4.1. Backend
- `src-tauri/crates/mt-themes/Cargo.toml`
- `src-tauri/crates/mt-themes/src/{lib.rs,error.rs,ids.rs,color.rs,oklch.rs,contrast.rs,scheme.rs,normalize.rs,scale.rs,service.rs,import.rs,export.rs,cache.rs}`
- `src-tauri/crates/mt-themes/src/formats/{mod.rs,tabby_config.rs,tabby_scheme.rs,xterm_json.rs,windows_terminal.rs,detect.rs}`
- `src-tauri/crates/mt-themes/src/mapping/{mod.rs,xterm.rs,tokens.rs}`
- `src-tauri/crates/mt-themes/src/builtin/{mod.rs,max-navy-teal.yaml,max-light.yaml,dracula.yaml,nord.yaml,solarized-dark.yaml,solarized-light.yaml}`
- `src-tauri/crates/mt-themes/tests/{import_tabby.rs,mapping_roundtrip.rs,scale_snapshots.rs,service_db.rs}` + `tests/fixtures/**`
- `src-tauri/crates/mt-db/migrations/0006_10_themes.sql` (número fixado pela decisão D-01: `0006`, após `0005_09_os_settings`)
- `src-tauri/crates/mt-db/src/repos/themes.rs`
- `src-tauri/src/commands/themes.rs` (+ registro em `commands/mod.rs` e no builder do `specta.rs`)
- `src-tauri/src/events/themes.rs`
- Atualização de `src-tauri/capabilities/default.json` (diálogos)

### 4.2. Frontend
- `src/ipc/themes.ts`
- `src/types/bindings.ts` (regenerado) e `src/types/theme.ts` (apenas utilitários derivados/re-exports)
- `src/stores/theme.ts`
- `src/composables/useThemeApplier.ts`, `src/composables/useTerminalAppearance.ts`, `src/composables/useThemePreview.ts`
- `src/theme/cssTokens.ts`, `src/theme/bootstrap.ts`, `src/theme/terminalBridge.ts`, `src/theme/fonts.ts`
- `src/components/preferences/appearance/{AppearancePreferences,ThemePicker,ThemePreview,ThemeSwatch,TerminalFontSettings,ProjectThemeOverrides,ThemeImportReport}.vue`
- (Condicional) `src/components/preferences/{PreferencesDialog.vue,registry.ts}`
- Testes: `tests/stores/theme.spec.ts`, `tests/theme/cssTokens.spec.ts`, `tests/theme/bootstrap.spec.ts`,
  `tests/composables/useThemeApplier.spec.ts`, `tests/components/preferences/appearance/*.spec.ts`

---

## 5. Critérios de Aceite

Todos os comandos na raiz da worktree devem retornar **exit 0**, salvo indicação.

### 5.1. Qualidade
- [ ] CA-01 `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test --workspace` passam.
- [ ] CA-02 `cargo test -p mt-themes` executa ≥ 60 testes, incluindo snapshots `insta` sem pendências (`cargo insta test --check`).
- [ ] CA-03 `npm run lint`, `npm run lint:style`, `npm run typecheck`, `npm run typecheck:vue` e `npm run test` passam; cobertura ≥ 85% de linhas em `src/stores/theme.ts`, `src/theme/` e `src/composables/useThemeApplier.ts`.
- [ ] CA-04 `npm run bindings && git diff --exit-code src/types/bindings.ts` retorna 0; `bindings.ts` exporta `commands.themeList`, `themeGet`, `themeAppearanceGet`, `themeAppearanceUpdate`, `themeImportFile`, `themeImportDir`, `themeSetActive`, `themeDelete`, `themeExport` e os tipos `ThemeSummary`, `ResolvedTheme`, `ColorScheme`, `XtermTheme`, `ThemeTokens`, `ImportReport`, `AppearanceState`, `ThemeScope`, `ThemeExportFormat`, `TerminalFontSettings`.
- [ ] CA-05 Um teste TS de compilação verifica `XtermTheme` atribuível a `ITheme` e o retorno de `toTerminalTheme` atribuível ao `TerminalTheme` estruturado da Fase 04; falha quebra o typecheck.

### 5.2. Parse e normalização (testes Rust)
- [ ] CA-06 Fixture `tabby/config.yaml` com `terminal.colorScheme` + 2 `customColorSchemes` gera exatamente 3 temas com `origin = tabby_config`.
- [ ] CA-07 Cores `#ABC`, `#aabbcc`, `#AABBCCDD`, `rgb(170, 187, 204)`, `rgba(170,187,204,0.5)`, `rgb(66.7%, 73.3%, 80%)` são aceitas; `#abcd` (fg) gera aviso `alpha_ignored`; `#ggg`, `rgb(300,0,0)`, `hsl(0,0%,0%)` e string vazia geram `theme.invalid_color` com o caminho do campo (ex.: `colors[3]`).
- [ ] CA-08 `colors` com 8 entradas é expandido para 16 com aviso `palette_expanded`; com < 8 entradas falha com `theme.invalid_palette`; com > 16 usa as 16 primeiras com aviso `palette_truncated`.
- [ ] CA-09 Arquivo > 1 MiB falha sem leitura integral; aliases/âncoras, tags customizadas, chaves duplicadas e profundidade > 32 são recusados antes de materializar a árvore YAML. JSON tem os mesmos limites estruturais. Rejeição da fixture de expansão em < 100 ms é meta de benchmark release, não garantia portátil de segurança.
- [ ] CA-10 Detecção de formato: cada fixture (`tabby_config`, `tabby_scheme` YAML e JSON, `xterm_json`, `windows_terminal`) é reconhecida; JSON arbitrário (`{"foo":1}`) falha com `theme.unsupported_format`.
- [ ] CA-11 Roundtrip xterm preserva todas as cores canônicas e `ContentHash` (nome fornecido separadamente); roundtrip Tabby preserva também o nome. `selectionInactiveBackground` derivado e metadados de origem ficam fora da identidade.

### 5.3. Mapeamento de tokens e contraste
- [ ] CA-12 `contrast_ratio(#ffffff, #000000) = 21.0`, `contrast_ratio(#777777, #ffffff) ∈ [4.47, 4.49]`, `contrast_ratio(x, x) = 1.0`.
- [ ] CA-13 ANSI importado não altera success/warning/danger/info da UI; essas escalas seguem a fixture Max do modo efetivo. Apenas primary deriva de cyan, com ajuste documentado em spec §3.8. Paleta com red verde e green vermelho mantém as cores ANSI no terminal e mantém badges semanticamente corretos.
- [ ] CA-14 Toda escala gerada é monotônica em luminosidade OKLCH (50 mais claro → 950 mais escuro), todas as cores dentro do gamut sRGB, e cada `-content` tem contraste ≥ 4.5:1 com o respectivo `-500` (teste para os 6 builtins + 20 fixtures).
- [ ] CA-15 Rampa `--background-*`: passo `0` = `background` do esquema; em tema escuro a luminância cresce com o número do passo, em tema claro decresce (mesma semântica do `MaxComponentsUi`); snapshots `insta` estáveis.
- [ ] CA-16 O builtin "Max Navy/Teal" produz `--max-primary-500 = #00768e`, `--max-primary-950 = #00152a`, `--background-0 = #17293d` e os demais valores idênticos aos de `@maxvue/max-components-ui/src/themes/{tokens,colors}.scss` (modo escuro) — verificado por teste que compara com fixture extraída dessas fontes.

### 5.4. Persistência e IPC
- [ ] CA-17 Runner recebe a sequência completa `0001`–`0006` (D-01) e aplica sem erro em banco vazio, até Fase 06 (`0003`) e até Fase 09 (`0005`); `PRAGMA foreign_key_check` vazio. Banco em `0003` aplica `0004` e `0005` antes de `0006`; não pular versões nem usar placeholders.
- [ ] CA-18 Após o primeiro startup existem os 6 builtins (`SELECT count(*) FROM themes WHERE builtin = 1` = 6); reiniciar não duplica; incrementar `builtin_version` de um YAML atualiza a linha no próximo startup.
- [ ] CA-19 Importar o mesmo arquivo duas vezes: a segunda chamada retorna `imported = []` e `duplicates.len() = N`.
- [ ] CA-20 `theme_delete` em builtin retorna `theme.builtin_readonly`; em tema ativo global, o ativo volta para "Max Navy/Teal" e é emitido `theme://changed`; projetos que usavam o tema perdem a sobreposição (cascade).
- [ ] CA-21 Remover um projeto (Fase 06) remove sua linha em `project_themes`.
- [ ] CA-22 `theme_import_dir` em pasta com 500 arquivos válidos conclui em < 3 s (release) e não bloqueia a UI (executa em `spawn_blocking`); symlinks não são seguidos; profundidade > 4 é ignorada; arquivos não `.yaml/.yml/.json` são ignorados sem erro.
- [ ] CA-23 `theme_export` nos formatos `tabby_yaml`, `tabby_json`, `xterm_json`, `css_tokens` retorna conteúdo; o YAML exportado é reimportável e gera hash idêntico (detectado como duplicado). Com `destPath` de extensão não permitida, retorna `theme.export_failed`.

### 5.5. Comportamento observável
- [ ] CA-24 Em Preferências > Aparência, confirmar **Aplicar** atualiza os tokens e terminais elegíveis no próximo frame após resolver a resposta IPC. Terminais de projetos com override mantêm seu tema; panes ocultas e novas recebem a revisão atual antes de exibir. PTYs, scrollback e seleção são preservados.
- [ ] CA-25 Passar o mouse/foco por um tema na lista atualiza somente o `ThemePreview` (escopo local); sair da lista restaura o preview do tema ativo; nada global muda até "Aplicar".
- [ ] CA-26 Escolher um tema claro (ex.: "Solarized Light") remove a classe `dark` de `<html>` (via `useDark`) e ajusta `color-scheme: light`; voltar a um escuro restaura.
- [ ] CA-27 Definir tema do projeto A: ao selecionar A, UI e terminais de A usam o tema de A; ao selecionar B (sem sobreposição), volta ao global. Com `applyProjectThemeToUi = false`, apenas os terminais de A mudam.
- [ ] CA-27 também cobre os cinco campos de `TerminalTheme.ui` e suas variáveis CSS locais em `.hybrid-terminal`: blocos/input de dois painéis com temas distintos permanecem isolados, inclusive após ocultar e remontar um painel.
- [ ] CA-28 Alterar fonte/tamanho/altura de linha reflete nos terminais em ≤ 200 ms (debounce), dispara re-fit e o PTY recebe o novo tamanho (cols/rows) — verificável com `tput cols` antes/depois.
- [ ] CA-29 Ligaduras: ativar com uma fonte com ligaduras (ex.: "JetBrains Mono") renderiza `=>` e `!=` ligados; desligar ou encerrar instância/bridge descarta o addon mantido no `WeakMap<Terminal, IDisposable>`. Reativar não duplica addon; desativar durante importação pendente impede reativação tardia. Se o addon vigente falhar ao carregar, a opção é revertida e é exibida mensagem pt-BR "Não foi possível ativar as ligaduras nesta fonte".
- [ ] CA-30 Reiniciar o app com um tema claro ativo não exibe flash do tema escuro (tokens do `localStorage` aplicados antes do `mount`).
- [ ] CA-31 Importar via botão "Importar arquivo" / "Importar pasta" exibe `ThemeImportReport` com contagens e erros pt-BR por arquivo; temas com contraste fg/bg < 4.5:1 exibem badge "Contraste baixo". Em lote com mais de 500 detalhes, o resumo usa `totalFailures`/`totalWarnings` exatos e informa quantos detalhes foram omitidos.
- [ ] CA-32 Todos os textos de UI, mensagens de `MtError` e comentários estão em pt-BR; nenhum `--max-*` é escrito com valores literais fora de `cssTokens.ts`/builtins.
- [ ] CA-33 Cancelar/fechar o preview não grava SQLite nem snapshot. Falha de persistência restaura fonte/tokens/tema anteriores; falha de renderer após commit é reconciliada com o banco e não apresentada como rollback persistido.
- [ ] CA-34 Respostas/eventos fora de ordem, duas alterações no mesmo milissegundo e exclusão de tema usado somente por projeto convergem pela revisão monotônica; reinício restaura a última configuração confirmada.
- [ ] CA-35 Fixtures cobrem config com seleção por nome, coleção com item inválido entre válidos, paleta nomeada, 9–15 cores, duplicatas de chaves, campos CSS desconhecidos e segredo em seção alheia do config (nunca persistido/logado).

---

## 6. Riscos e Mitigação

| # | Risco | Impacto | Mitigação |
|---|-------|---------|-----------|
| R-01 | Variações não documentadas do formato Tabby (chaves novas, `colors` com nomes, versões antigas). | Médio | Parsers tolerantes (campos desconhecidos ignorados), fixtures reais de várias versões, registro extensível de parsers (`SchemeParser`), avisos em vez de erro sempre que houver normalização segura. |
| R-02 | Expansão YAML e estruturas muito profundas. | Médio | Scanner limitado antes da desserialização; rejeitar aliases/âncoras e tags customizadas; limites de bytes, nós, profundidade e itens; não confiar em contagem textual de `&`/`*`. |
| R-03 | Escalas geradas feias/ilegíveis para paletas extremas (pastel, neon, monocromáticas). | Alto | OKLCH com âncora no 500, clamp de luminância da base, gamut mapping por redução de croma, `-content` por contraste WCAG, relatório de contraste visível na UI e snapshots de 20 paletas. |
| R-04 | Regressão visual do tema padrão ao trocar CSS estático por tokens gerados. | Alto | Builtin "Max Navy/Teal" com `token_overrides` idênticos ao `MaxComponentsUi` (CA-16); `theme.scss` da Fase 01 permanece como fallback antes do carregamento. |
| R-05 | Contrato da Fase 04 (atualização de tema/fonte em instâncias vivas) diferente do assumido. | Médio | Toda a integração isolada em `src/theme/terminalBridge.ts`; teste de tipo `satisfies TerminalTheme`; ajustar apenas o adaptador. |
| R-06 | Troca de tema/fonte com addon WebGL causa artefatos (atlas de glifos antigo). | Médio | Após trocar fonte chamar `clearTextureAtlas()` + `fit()`; troca de cores via `options.theme` (suportado nativamente pelo WebGL); teste manual documentado. |
| R-07 | `@xterm/addon-ligatures` incompatível com WebKitGTK/WebGL ou com fontes locais. | Médio | Carregamento dinâmico sob demanda, fallback automático para desligado com mensagem pt-BR, opção desligada por padrão. |
| R-08 | Injeção de CSS via nome de fonte ou valores de cor. | Alto | Tokens só são gerados a partir de `Rgb` validado (nunca strings cruas); `fontFamily` validada por regex e serializada com aspas escapadas; aplicação via CSSOM (`style.setProperty`), compatível com a CSP restritiva da Fase 01. |
| R-09 | Varredura de pasta enorme (`~`) travar ou consumir memória. | Médio | Limites (profundidade 4, 500 arquivos, 1 MiB/arquivo), `follow_links(false)`, execução em `spawn_blocking`, relatório indicando `theme.dir_limit_exceeded`. |
| R-10 | Licença das paletas builtin de terceiros. | Baixo | Usar apenas paletas MIT/domínio público (Dracula, Nord, Solarized), registrar autoria/licença no cabeçalho de cada YAML. |
| R-11 | Flash do tema errado no startup (FOUC). | Baixo | `bootstrap.ts` aplica `mt.theme.lastApplied` antes do `mount`; reconciliação com o banco logo após `init()`. |
| R-12 | Custos de recomputar tokens e cache obsoleto. | Baixo | LRU Rust (64 entradas) por id/hash/overrides/versões de mapeamento; frontend fixa temas em uso e mantém 24 inativos. Invalidar por eventos/revisão. |
