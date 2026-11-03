# Padronização dos componentes de interface com MaxComponentsUi

## 1. Identificação e objetivo

- Projeto: **MaxTerminal**. Data: **03/11/2026**.
- Worktree e branch: **wt-8e0aaf7f**.
- Raiz exclusiva: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-8e0aaf7f`.
- Revisão investigada: `dacfaa00fafab4f3a6d2ed06378229524b89dbe8`; árvore inicialmente limpa.
- Objetivo: substituir controles nativos e apresentações locais que tenham equivalentes adequados na MaxComponentsUi e corrigir integrações incorretas dos componentes Max já utilizados.
- Este documento é um plano, não uma implementação. A investigação utilizou dois subagentes independentes, somente em leitura. Nenhum teste, instalação, servidor ou build foi executado nesta sessão.

## 2. Escopo e preservação de comportamento

**Incluído:** controles, botões, ícones de ação, seleções, formulários, overlays, feedback, badges e apresentações equivalentes inventariadas abaixo; correções de bootstrap, Toast, modais e estilos; testes das substituições e contratos reais. Incluir superfícies de desenvolvimento em `src`, não apenas telas principais.

**Preservar:** componentes de domínio, props/emits externos, DTOs e chamadas IPC/Tauri, validações, confirmações destrutivas, segredos mascarados, drafts, revisão otimista, cache já existente, textos pt-BR, navegação, atalhos, IME, drag/drop, seleção, streaming e ciclo de vida dos PTYs/xterm. Não transformar esta entrega em redesenho visual ou migração de persistência.

**Excluído:** backend Rust, schema SQLite, endpoints novos, atualização automática de dependências, alterações nos repositórios compartilhados das bibliotecas, substituição indiscriminada de containers HTML, Splitpanes ou motor de terminal. Não criar outra worktree, trocar branch, commitar, integrar, fazer push ou remover esta worktree; essas ações pertencem ao MaxCode.

**Ecossistema obrigatório:** usar MaxComponentsUi para os equivalentes adequados, a instância Pinia já instalada com `createMaxPinia()` para estado compartilhado e stores existentes, e MaxUse para novos helpers/composables genéricos. Não adicionar GET/salvamento direto em componentes. O plugin MaxPinia está registrado, mas `settings`, `projects` e `tabs` são setup stores IPC sem `isCached`; o registro não habilita automaticamente cache/autosave. Preservar esses contratos: não ativar `isCached`, inventar rotas HTTP ou duplicar salvamentos para realizar uma troca visual. Estado transitório de apresentação continua local quando apropriado.

## 3. Evidências e viabilidade

### 3.1 Fontes e ambiente

- Auditoria completa dos **117 SFCs** de `src`: **85** com controles nativos e **32** sem esses controles. Contagem estática de tags, não de elementos renderizados: **281 button, 124 input, 27 select e 5 textarea = 437 controles**. Há também **7 form**, cuja semântica deve permanecer. Um textarea, de `RichInput`, tem exceção técnica explícita.
- Uso Max atual: **194 MaxIcon, 3 MaxModal, 3 MaxInputText, 1 MaxButton, 1 MaxIconButton e 1 MaxInputSelect**. Ocorrências corretas não precisam ser reescritas.
- Biblioteca real: `/home/johnattas/GitHub/MaxComponentsUi`, versão **1.1.36**. Consultados catálogo em `package.json`/`src/index.ts`, SFCs, tipos de seleção/feedback/progresso/Markdown/código e declarações/artefatos `dist`; os imports da aplicação executam `dist`, não os SFCs fonte.
- MaxUse **2.0.1**, MaxPinia **0.2.0**, Node **24.18.0**, Vue **3.6.0-rc.13**, Pinia **3.0.1**, router **4.5.0**. A UI declara peers Pinia `^4.0.0` e router `^5.2.0`: há divergência declarada, não falha funcional reproduzida. Compatibilidade dos componentes reais é gate da execução; falha não autoriza upgrade amplo.
- `node_modules` desta worktree é symlink para `/home/johnattas/GitHub/MaxTerminal/node_modules`; os três pacotes Max são links para seus repositórios. Não instalar, sincronizar, reconstruir libs nem limpar caches compartilhados sem autorização específica.
- Inspecionados `package.json`, `vite.config.ts`, `vitest.config.ts`, `tsconfig.vue.json`, `src/app/{main,plugins}.ts`, `src/app/App.vue`, `tests/setup.ts`, `src/constants/bundledIcons.ts`, `src/styles/theme.scss`, `scripts/{check,sync-local-libs}.mjs`, `.vscode/run-dev.sh`, stores `settings/projects/tabs`, `useSetting`, `ipc/{core,db}`, configuração Tauri e resolução nativa de paths/DB.
- `npm run dev` **não é um preflight seguro**: sincroniza libs, apaga cache compartilhado e controla processos. `sync-local-libs.mjs --check` é somente leitura, mas valida presença dos peers, não seus intervalos de versão.
- `AppPaths::resolve` suporta `MAXTERMINAL_HOME` absoluto para isolar config/data/state/cache; `DbConfig::from_paths` aceita `MAXTERMINAL_DB_PATH` absoluto, que tem precedência para o banco. Em validação nativa futura, ambos devem apontar para a sandbox da worktree, nunca para dados reais. Não copiar `.env`.
- Documentação Vue consultada confirma que attrs/listeners não declarados dependem da raiz/fallthrough. Não concluir que todo atributo não declarado como prop é inválido. Documentação Vite e CLI instalada confirmam `--configLoader runner`; `cacheDir` sozinho não isola `.vite-temp` do loader padrão.

### 3.2 Contratos de substituição confirmados

| Origem | Destino e contrato obrigatório |
|---|---|
| Ação textual / somente ícone | `MaxButton` / `MaxIconButton`; preservar `type`, disabled/loading, nome acessível e `MouseEvent`. `MaxIconButton` filtra attrs `on*`: não depender de encaminhamento automático de foco/keydown/mousedown. |
| Texto / busca / número | `MaxInputText`, `MaxInputSearch` ou `MaxInputNumber`. Usar valores de `update:modelValue`, não `Event.target`; número pode emitir `null/undefined`. Testar vazio, limites, inteiro e fração. `MaxInputText` expõe `input`, `focus`, `setFocus`; selecionar texto pela referência ao input exposto. |
| Booleano / escolha exclusiva | `MaxInputCheckbox` ou `MaxInputSwitch` para ativação; `MaxInputRadio` com modelo de grupo e `value`. `MaxSegmentedControl` para escolhas compactas, sempre com `ariaLabel`. |
| Select | `MaxInputSelect`: `options`, `optionValue`, `optionLabel`, **optionName**; `change` entrega valor. Preservar valores vazios, números, seleção múltipla onde aplicável e opções dinâmicas. Expõe `isOpen`. |
| Senha / cor / texto longo | `MaxInputPassword`, `MaxColorPicker` escalar, `MaxInputTextArea`. Não usar `MaxInputColorPalette` para cor única. Textarea Max não expõe cursor/foco/elemento nativo; aplicar exceção do RichInput. |
| Campos de ícone Iconify | `MaxInputText` com catálogo/sugestões locais. Não introduzir `MaxInputIconPicker`: seus endpoints HTTP padrão não existem neste desktop. |
| Modal / drawer / popover | `MaxModal` com `visible/update:visible` e veto `before-close`; `MaxDrawer` com `v-model:visible` somente onde o aprisionamento de foco for adequado; `MaxPopover` via `show/hide/toggle`, sem inventar prop `visible`. `MaxPopoverMenu` para menu de ações simples. |
| Confirmação | Preferir `MaxModal` para regras de domínio complexas. Confirmação curta pode usar `MaxButtonConfirm`/`MaxIconConfirm` com hospedeiro global `MaxPopoverConfirm`; este é ligado a `useConfirmStore`, não controlado por `open` local. Instalar hospedeiro único somente se essa infraestrutura for usada. |
| Feedback e conteúdo | `MaxAlert`, `MaxBadge`/`MaxTag`, `MaxEmptyDiv`, `MaxQueryState`, `MaxLoader`, `MaxProgressBar`, `MaxCodeBlock`, `MaxMarkdownViewer`, conforme semântica. QueryState não representa estado de processo. Markdown exige `source` e `renderMarkdown`; reutilizar `renderSafeMarkdown` e manter defesa XSS. |

Importar explicitamente por entrypoints públicos existentes, como os consumidores atuais. O plugin público da UI instala diretiva/opções, **não registra todos os componentes**. Não introduzir auto-import ou PrimeVue para realizar esta tarefa.

## 4. Inventário executável de substituições

Todos os nomes abaixo têm extensão `.vue` e ficam em `src/components/`, salvo `src/app/App.vue`. As linhas são referências da revisão investigada; reler trechos se o conteúdo mudar. Em cada arquivo migrar todos os controles elegíveis, não somente a primeira ocorrência.

| Grupo | Arquivos e referências principais |
|---|---|
| Aplicação e shell | `src/app/App.vue:7`; `shell/AppTitleBar:7`, `shell/DbRecoveryPanel:13–30`. |
| Agentes | `agents/AgentAttentionIndicator:7–8`, `AgentLauncherMenu:3–21`, `AgentSettingsPanel:9–78`, `AgentTabOpenDialog:2–82`, `AgentWorktreeCloseDialog:2–42`. |
| IA | `ai/AiPromptInput:2–19`, `AiStatusIndicator:2`, `AiSuggestionBlock:12–72`, `DestructiveConfirm:2–35`, `HarnessPicker:4–40`, `InlineDiagnosis:11–47`, `RagResults:11–36`. |
| Bridge e indicadores | `bridge/BridgeSettingsSection:17–81`, `BridgeStatusIndicator:2`, `InspectButton:2`; `indicators/GitIndicator:3–12`, `PortBadge:4–28`; `os/ShortcutStatusBadge:2`. |
| Grupos | `commandGroups/CommandGroupItemRow:8–61`, `CommandGroupModal:2–138`, `CommandGroupSuggestPanel:7–50`, `GroupRunIndicator:2`, `GroupRunItemRow:32–40`, `GroupRunOutputView:14`, `GroupRunPanel:9–44`. |
| GitHub | `github/BugCycleConfirmModal:2–38`, `BugCycleProgress:10–12`, `GithubAuthPanel:2–43`, `IssueActionsMenu:3–15`, `IssueCreateModal:2–36`, `IssueFilters:5–31`, `IssueLabelEditor:6–22`, `IssueListItem:2`, `IssueOpenTabModal:2–54`, `IssuesTab:10–32`, `LabelSyncBanner:9–13`. |
| Preferências | `preferences/AiPreferences:17–112`; `preferences/appearance/AppearancePreferences:11–43`, `TerminalFontSettings:6–52`, `ThemeImportReport:10`, `ThemePicker:5–15`, `ThemeSwatch:2–28`; `preferences/system/CloseBehaviorSelector:7`, `EditorSelector:5–14`, `PreferencesSystemSection:11–99`, `ShortcutRecorder:4–14`, `WaylandShortcutHelp:11–21`. |
| Projetos | `projects/ProjectAddModal:2–60`, `ProjectDiscoveryPanel:4–26`, `ProjectEmptyState:9–12`, `ProjectIdentityForm:2–49`, `ProjectItem:3–34`, `ProjectList:6–35`, `ProjectRootModal:2–64`, `ProjectWorkspace:17–40`. |
| Artisan e atalhos | `quick-commands/ArtisanCommandModal:2–93`, `ArtisanParamRow:8–63`, `ArtisanParamsBuilder:6–10`, `ArtisanPhpPreview:3–54`, `ArtisanSchedulePicker:4–80`, `QuickCommandsHost:2–22`. |
| Busca | `search/GlobalSearchPalette:2–34`, `SearchFilterBar:6–47`, `SearchResultSection:16`. |
| Skills/memória | `skills/CommandMemoryList:6–37`, `SkillConflictList:15–17`, `SkillsSyncPanel:8–66`. |
| Terminal e desenvolvimento | `terminal/CommandBlock:15`, `InputModeToggle:2`, `RichInput:8` (exceção), `TerminalExitBanner:4`, `TerminalSearchBar:3–26`, `terminal/__dev__/TerminalPlayground:6–46`; `dev/PerfOverlay:6–8`. |
| Workspace | `workspace/PaneView:6–23`, `SessionHistoryDrawer:2–36`, `SessionReadonlyViewer:9–20`, `TabBar:12–34`, `TabBarItem:2–8`, `TabWorkspaceView:5–42`, `TemplateEditorModal:2–63`, `TemplateLaunchPopover:2–32`. |

### 4.1 Estruturas compostas e apresentação

- Migrar **13 overlays artesanais** para MaxModal: AgentTabOpenDialog, AgentWorktreeCloseDialog, DestructiveConfirm, CommandGroupModal, BugCycleConfirmModal, GithubAuthPanel, IssueCreateModal, IssueOpenTabModal, ProjectAddModal, ProjectRootModal, ArtisanCommandModal, GlobalSearchPalette e TemplateEditorModal. Preservar wrappers de domínio, guardas de fechamento e contratos externos; remover traps/listeners/scroll locks antigos quando passarem a ser responsabilidade Max.
- SessionHistoryDrawer mantém o painel lateral especializado e migra seus controles internos: o MaxDrawer atual ativa focus trap mesmo com `modal=false`, sem prop pública para desativá-lo (`MaxDrawer.vue:58–110,266–285`, também confirmado no dist). Logo, não é equivalente ao uso simultâneo do terminal por teclado. GitIndicator e lista de PortBadge → MaxPopover. TemplateLaunchPopover e AgentLauncherMenu → MaxPopover com conteúdo customizado, pois possuem ações secundárias por item. Menu de ProjectItem → MaxPopoverMenu. Confirmações de porta, QuickCommandsHost, AiSuggestionBlock e InlineDiagnosis devem manter alvo capturado, revalidação e resolução de Promise; DestructiveConfirm mantém token digitado obrigatório.
- CommandGroupModal: três segmented controls para escopo, modo e separador. IssueFilters: aberta/fechada/todas; ThemePicker: polaridade; SkillConflictList: resolução exclusiva por item. Filtros múltiplos de fontes não viram seleção exclusiva.
- ArtisanPhpPreview e SessionReadonlyViewer: usar família de abas de conteúdo Max, conferindo seu modelo real (`MaxTabs` legado usa `value`, não presumir `modelValue`), sem alterar paginação/histórico. HarnessPicker conserva controlador multietapas e migra botões, não vira select único.
- Em todos os arquivos do inventário, migrar badges/chips/avisos/estado vazio locais equivalentes para MaxBadge/MaxTag/MaxAlert/MaxEmptyDiv; preservar severidades, contadores, cores GitHub e regiões live. Usar MaxQueryState nos estados de consulta de ProjectList/IssueList, não nas saídas em streaming.
- Trocar símbolos de ação `↑`, `↓`, `✕` de TerminalSearchBar e `×` de remoção de tag em CommandGroupModal por MaxIconButton; o `+` de TemplateEditorModal vira ícone de MaxButton com texto. Usar ícones já embutidos ou adicionar somente SVGs confiáveis e necessários em `bundledIcons.ts`, sem novas dependências de rede.

Os **32 SFCs sem controles nativos** também foram classificados:

| Decisão | Arquivos |
|---|---|
| Apresentação Max a migrar, mantendo wrapper/contrato | `agents/AgentStatusLine` (MaxProgressBar); `ai/AiContextPreview` (tag e código estático), `ai/RiskBadge`; `github/IssueComments`, `IssueDetail`, `IssueLabelChip`, `IssueList`, `IssuePlanSection`; `indicators/WorktreeBadge`; `preferences/appearance/ProjectThemeOverrides` (vazio); `projects/ProjectSettingsDialog` (alerta), `ProjectStatusBadges`; `quick-commands/QuickCommandItem`; `search/SearchResultItem` (badge); `shell/AppEmptyState`, `CommandBlockStatus`, `TabStatusIndicator`; `terminal/HybridTerminal` (somente banner). |
| Já Max; corrigir apenas achados da seção 5 | `preferences/PreferencesDialog`, `projects/ProjectEditModal`. |
| Preservar composição/semântica especializada | `ai/AiPaneResults`, `indicators/TabIndicators`, `layout/AppLayout`, `preferences/SkillsAndMemoryPreferences`, `preferences/appearance/ThemePreview`, `search/HighlightedText`, `shell/AppShell`, `terminal/CommandBlockLayer`, `CompletionItem`, `CompletionPopup`, `TerminalView`, `workspace/PaneLayout`. |

Nos três renderizadores GitHub, usar MaxMarkdownViewer com o Markdown original e `renderSafeMarkdown`; `useIssuePlan` já expõe `plan.markdown`. Não passar HTML já renderizado como Markdown. Preservar o fallback de descrição vazia. Usar MaxCodeBlock somente em trechos técnicos estáticos; não substituir logs vivos ou scrollback.

### 4.2 Exceções obrigatórias, não pendências de migração

- **RichInput.vue:8 permanece textarea nativa:** usa HTMLTextAreaElement, selectionStart, paste, clique/keyup, IME e combobox; MaxInputTextArea não fornece contrato equivalente. Manter seus testes e explicar a exceção no teste arquitetural.
- Manter DOM/virtualização/listbox de CompletionPopup e CompletionItem, realces escapados de HighlightedText e snippets FTS5, host xterm/CommandBlockLayer e Splitpanes/PaneLayout. Não remontar o terminal ao abrir overlays ou mudar conteúdo.
- Preservar raiz especializada arrastável de TabBarItem e preview/seleção de ThemeSwatch; migrar ações filhas sem criar botão dentro de botão. ShortcutRecorder mantém área de captura de teclas, migrando os botões auxiliares.
- Preservar o `<aside>` de SessionHistoryDrawer: não contornar o trap do MaxDrawer por campos privados nem alterar a biblioteca externa. Testar Tab entre histórico e terminal; revisar esta exceção somente se uma versão futura oferecer contrato público equivalente.
- `form`, `label`, `fieldset`, landmarks, listas, `<mark>`, `<kbd>` e containers sem equivalente funcional não são violações. Prompt `❯`, amostras de cursor `█/▏`, rótulos `Aa/.*`, instruções `↑↓`, contagem `+N` e multiplicação da grade são conteúdo, não ícones defeituosos.

## 5. Correções dos usos Max existentes

| ID | Evidência e decisão | Aceite dirigido |
|---|---|---|
| C1 | `src/app/plugins.ts:16–28` não instala `tooltip`, exigido pelo render de MaxIcon. Instalar plugin público MaxComponentsUi após Pinia, mantendo catálogo offline e registros explícitos. Não importar PrimeVue. | Bootstrap real sem warnings de diretiva/componente ausente; ícones offline continuam renderizando. |
| C2 | `src/app/App.vue` não monta MaxToast; `Toast.show` apenas enfileira em useToastStore. Montar uma instância persistente na raiz, inclusive nos estados loading/erro, com o mesmo Pinia. | Toast real aparece, é anunciado, pode ser descartado e expira; não duplicar instância ao trocar projetos/preferências. |
| C3 | `useAgentShortcuts.ts:55–78`, `terminal/blocks/blockActionRegistry.ts:36–44` e `composables/search/useSearchNavigation.ts:72–96` enviam `warn`, mas contrato aceita `warning`. Retirar casts permissivos e tipar pelo contrato público; usar a normalização já correta em `composables/quick-commands/adapters.ts:316–325`. | Aviso armazenado/renderizado como warning; success/error/info continuam iguais. |
| C4 | `useSearchNavigation.ts:75–92` inventa `actionLabel/onAction`, que a store Toast descarta. Remover payload não suportado e disponibilizar ação explícita “Atualizar resultados” na busca quando houver consulta; chamar `searchStore.run()` existente, mantendo consulta/filtros. Não ampliar a biblioteca. | Resultado removido gera aviso e a busca pode ser atualizada pelo controle real; sem callback fictício no Toast. |
| C5 | `projects/ProjectSettingsDialog.vue:2,49,77–80` apenas rejeita update:visible durante operação; MaxModal ainda pode remover sua entrada da pilha. Vetar `before-close` com `preventDefault()` quando ocupado, mantendo backdrop/Escape bloqueados. | Promise de save pendente + fechar cabeçalho não remove modal da pilha nem o torna inert/aria-hidden; depois do save, fecha normalmente. |
| C6 | Select fecha lista com Escape sem impedir propagação; MaxModal escuta document sem verificar defaultPrevented. Composição atual em ProjectThemeOverrides dentro de ProjectSettingsDialog e AppearancePreferences precisa de teste real. | Primeiro Escape fecha somente lista, segundo fecha modal; foco retorna corretamente. |
| C7 | Seletores `.classe-do-dialogo { :deep(.max-modal) }` em PreferencesDialog:104–120, ProjectSettingsDialog:113–127 e ProjectEditModal:35–43 pressupõem ancestral que não acompanha Teleport. Corrigir com classes globais exclusivas do diálogo teleportado; usar props públicas para dimensões. | Cor/padding/max-height/scroll computados corretos nos três modais; sem CSS global genérico afetando outros overlays. |
| C8 | Classes de ícone em ProjectWorkspace:186–188, ProjectItem:161–163, AppEmptyState:39–42 e WorktreeBadge:76–79 tentam dimensionar com font-size; MaxIcon fixa dimensões padrão em rem e oferece size. Revisar os 194 usos e aplicar size onde a expectativa CSS for ineficaz. | Dimensões computadas equivalentes à intenção visual; preservar cores e ARIA. `theme.scss:75–89` já corrige currentcolor: não reportar falso defeito de cor. |

C1–C5 são divergências confirmadas por inspeção dos contratos. C6 tem conflito de handlers identificado, mas requer reprodução; C7–C8 exigem validação de estilos computados para dimensionar o impacto visual, sem inventar regressões observadas.

**Solução local para C6:** rastrear `isOpen` público das instâncias MaxInputSelect e desabilitar o Escape do MaxModal enquanto uma lista filha estiver aberta. Propagar esse estado do componente de domínio ao hospedeiro; combinar com o bloqueio busy e testar a ordem real dos eventos/atualizações. Reutilizar composição local quando vários modais precisarem dela, com helpers MaxUse. Não acessar campos privados nem editar a biblioteca externa; se o contrato publicado não permitir o aceite, bloquear essa tarefa e registrar a limitação, sem declarar correção fictícia.

**Preservar contratos corretos:** `visible/update:visible`, `no-button`, `max-width`, `dismissable`, `close-on-escape`, `sub-title`; `option-name` e o payload escalar change de ProjectThemeOverrides; `maxlength` encaminhado ao input; os v-model atuais de MaxInputText. Não tratar bubbling de `@input` em ProjectIdentityForm como bug confirmado. A biblioteca já implementa restauração de foco; testar composição, não duplicar esse mecanismo indiscriminadamente.

## 6. Sequência e matriz de aceite

O executor lê este plano na admissão, confere branch/HEAD/status e trabalho já feito, compara hashes e relê contratos que tenham mudado. Não sobrescrever alterações de terceiros. Implementar o escopo e os testes de cada frente antes da validação em lote final; testes dirigidos podem ser usados durante desenvolvimento para diagnosticar regressões.

| Tarefa | Requisito / arquivos afetados | Testes pertinentes e evidência de conclusão |
|---|---|---|
| T0 — preflight, responsável integrador | Verificar links/peers, artefatos dist, versões e isolamento. Se necessário para evitar caches compartilhados, ajuste mínimo de cacheDir em `vite.config.ts` e `vitest.config.ts` para `.tmp` da worktree; usar configLoader runner nos comandos. Sem instalar/atualizar libs. | Leitura confirma caminhos isolados; gates de compatibilidade com componentes reais em T1/T7. Registrar bloqueio se faltar recurso, não copiar .env ou usar dados reais. |
| T1 — contratos compartilhados, integrador | C1–C4 em `src/app/{plugins.ts,App.vue}`, três adaptadores Toast e GlobalSearchPalette; estabilizar imports, hospedeiros e padrão de modelos/refs/overlays. Coordenar App.vue/GlobalSearchPalette antes das demais frentes. | Atualizar `tests/app/{plugins,bootstrap}.spec.ts`; criar testes de integração Max reais para bootstrap/Toast e atualização de resultado ausente. Sem warning de registro nem HTTP inesperado. |
| T2 — projetos/preferências/shell | Todos os arquivos desses grupos nas seções 4/4.1; C5–C8 dos três modais, ProjectThemeOverrides e aparência. Inclui os componentes os e AppTitleBar/DbRecoveryPanel. | Suites existentes `tests/frontend/projects/`, `tests/components/preferences/appearance/`, PreferencesDialog, AppShell, `useLayoutPersistence`; adicionar sistema, RootModal/Discovery, modais reais, busy/Escape, estilos e dimensões. Persistência/drafts/tema por projeto mantidos. |
| T3 — agentes/IA/bridge/skills | Grupos agents, ai, bridge, skills e indicadores Git/Port; progresso/tags, popovers e confirmações. | Suites existentes de agentes/bridge; adicionar formulários de IA, harness multietapas, token destrutivo, PAT/segredos, skills/memória, confirmações por porta. Não executar comandos/processos reais nem chamadas externas. |
| T4 — GitHub/grupos/Artisan | Todos os grupos github, commandGroups, quick-commands; Markdown e abas/segmented conforme seção 4. | `tests/frontend/github/`, `tests/commandGroups/`; acrescentar Artisan/horários/params/preview, Markdown XSS e labels, reordered por botões/Alt+setas, conflitos, sugestão/undo, masked variables e Promise de confirmação. |
| T5 — busca/workspace/terminal/dev | Todos os grupos search/workspace/terminal/dev, respeitando exceções; coordenar GlobalSearchPalette com T1. Adaptar refs/eventos de TerminalSearchBar e busca global. | `tests/terminal/`, `tests/frontend/workspace/workspace-components.spec.ts`, `tests/a11y/{search-palette,workspace-modals}.a11y.spec.ts`; novos drag/drop/close sem interativos aninhados e overlays. IME, Ctrl+Enter, Enter/Escape, focus/select, scroll, restart e instância xterm preservados. |
| T6 — auditoria de fechamento e testes | Revisar os 117 SFCs novamente; verificar todos os 437 candidatos e 194 ícones; remover apenas CSS obsoleto desta migração. Completar testes novos em `tests/` conforme organização existente. | Teste arquitetural com exceções documentadas identifica controles elegíveis remanescentes; nenhum consumidor Max desconhecido, modelo/evento inventado ou referência DOM obsoleta. Relatório de inventário conciliado, sem exigir contagem final igual à inicial. |
| T7 — validação integrada, integrador | Rodar lote de qualidade e cenários manuais isolados somente após frentes e testes completos; registrar estado e evidências. | Comandos e critérios da seção 7; falha/pendência explícita impede aceite integral. |

**Delegação da execução:** T0 e contratos T1 vêm primeiro. Depois, T2/T3/T4/T5 podem rodar em paralelo somente com donos de arquivos distintos; integrador retém plugins/App.vue, adaptadores Toast, helpers comuns, configs e GlobalSearchPalette até terminar C4. Se outro agente precisar desses arquivos, aguardar handoff, não editar concorrentemente. Compartilhar versão/fontes e qualquer indisponibilidade definitiva uma única vez. T6/T7 são sequenciais após integração das frentes, sem operações Git implícitas.

## 7. Validação, riscos e operação

### 7.1 Testes observáveis

- Manter testes unitários existentes, atualizando seletores por papel/nome/contrato público, não por índice de `button/select`. Adicionar camada pequena de testes com **componentes reais**: os mocks atuais de ProjectSettingsDialog não reproduzem pilha/Teleport/veto; ProjectIdentityForm inventa emit input; PreferencesDialog/AppearancePreferences emitem eventos manualmente. Stubs permissivos sozinhos não comprovam integração.
- Cobrir inputs vazios, booleanos, radios, números, select scalar e múltiplo, submit/cancel, disabled/loading, limpeza de valores e um único save por ação. Preservar debounce/revisões/erros via stores e useSetting, sem habilitar cache MaxPinia inadvertidamente.
- Abrir overlays por teclado; Tab/Shift+Tab, foco inicial, Escape aninhado, clique no backdrop, fechamento busy, retorno ao acionador/terminal, empilhamento, viewport estreito e conteúdo longo. O histórico lateral permite navegação por Tab de volta ao terminal sem fechamento obrigatório. Verificar estilos computados no navegador, não inferir CSS por snapshots de happy-dom.
- Preservar IME/composition, navegação/listbox, cursor e histórico do RichInput, pesquisa terminal com Enter/Shift+Enter/Escape e seleção do texto; não encaminhar teclas dessas interfaces ao PTY.
- Não perder contadores, ordem/tokens de status, labels canônicas, realce escapado FTS5, truncamento/gaps, estado stale/desconhecido e bloqueios do visualizador de saída. Não anunciar cada token de stream em região live.
- Markdown deve continuar bloqueando HTML/scripts/URLs perigosas e mantendo texto/links seguros; não introduzir endpoints de ícones, Axios acidental ou exposição de token/password por autocomplete, Toast, logs ou snapshots.

### 7.2 Comandos confirmados, somente na execução

Executar a partir da raiz desta worktree, após conferir isolamento de caches/config loader. Os scripts foram identificados no manifesto; **não foram rodados no Plan**:

1. `node scripts/sync-local-libs.mjs --check` — diagnóstico somente leitura, não substitui o gate de versões.
2. `npm run lint` e `npm run lint:style` — sem modos de correção automática.
3. `npm run typecheck` e `npm run typecheck:vue`.
4. `npm run test:coverage -- --configLoader runner` — suíte completa frontend com limiar atual de linhas de 80%; não reduzir limiar/exclusões para passar.
5. `npm run build -- --configLoader runner`.

`npm run test -- --configLoader runner <arquivo.spec.ts>` está disponível para diagnóstico dirigido. `npm run check` é o orquestrador canônico de dez passos, mas chama loaders padrão e também Rust; não o usar cegamente com node_modules compartilhado. Se for exigido no fechamento, primeiro comprovar isolamento de todos os artefatos e executá-lo sem efeitos fora da worktree. Não alegar aprovação do check completo com base no lote frontend.

Para cenários manuais nativos, conferir `MAXTERMINAL_HOME` e `MAXTERMINAL_DB_PATH` isolados, perfil/storage de navegador e caches por worktree, ícones offline e IPC mockado quando bastar. Não abrir o app contra o banco real nem disparar GitHub/bridge/skills/comandos reais para testar apresentação. Não iniciar `npm run dev` automaticamente. Se não houver runtime gráfico isolado, marcar validação manual pendente; DOM a11y atual não equivale a auditoria completa de navegador.

### 7.3 Riscos e recuperação

Maiores cuidados são sobreposição de handlers/foco, refs DOM após componentes compostos, coerção de valores, estilos teleportados e dependências compartilhadas. A divergência de peers requer teste real e pode bloquear a execução, mas não comprova por si só erro existente. Não há migração de dados prevista. Em falha, manter alterações e resultados parciais identificados, inspecionar o diff e solicitar decisão se houver necessidade de mudar dependências ou biblioteca externa; não reverter trabalho alheio, limpar caches compartilhados ou restaurar/resetar banco automaticamente.

## 8. Registro de execução e retomada

Criar **somente durante Execute** `docs/plans/wt-8e0aaf7f/execution-state.json`. Não criar esse arquivo no Plan. Seguir o contrato version 2 estrito já documentado no projeto (`docs/plans/wt-e42cace3/plan.md:158–178`), sem chaves extras:

- Raiz: `{ version: 2, planRevision, tasks, verification, updatedAt?, nextAction? }`.
- `planRevision`: SHA256 real dos bytes deste plan.md.
- `tasks`: `{ id, status, files, validations, revision?, nextAction? }[]`, usando T0–T7; `validations: []` obrigatório. Status: `pending | running | completed | blocked | manualPending | validationFailed`.
- `verification`: `{ revision, checks }`; cada check: `{ command, revision, result, exitCode? }`. Resultado somente `passed | failed | pending`. Passed exige inteiro 0; failed exige inteiro diferente de 0; pending omite exitCode. Não usar null ou blocked como resultado.
- `command` registra comando efetivamente executado (até 4.096 caracteres), incluindo falhas, não um comando sugerido ou resultado inferido. Não inventar comando para uma checagem manual: registrar sua pendência/conclusão na tarefa e sua evidência em handoff, sem fabricar exit code.
- Revisões de tarefas/verificação/checks: SHA256 real de manifesto determinístico dos caminhos relativos ordenados e SHA256 dos conteúdos; representar arquivos removidos com marcador fixo documentado. Não incluir execution-state no próprio hash, evitando autorreferência. Registrar o método no handoff, sem adicionar campos ao JSON.
- `updatedAt`, quando presente, é inteiro positivo em milissegundos; `nextAction` até 512 caracteres. Não adicionar schemaVersion, branch, worktree, head, planPath, title/evidence de tarefa ou checks soltos na raiz.

Em retomada, comparar hashes reais, reaproveitar apenas verificações vigentes do mesmo conteúdo, examinar alterações preexistentes e continuar lacunas. Alterações relevantes invalidam checks dependentes. Checks vazios/falhos/pendentes e testes manuais não realizados impedem declarar aceite integral. Registrar progresso e bloqueios a cada marco; usar notificações de conclusão de subagentes, sem consultas repetidas de status.

## 9. Revisão do plano

O plano cobre as duas varreduras solicitadas, diferencia substituições adequadas de exceções técnicas e separa bugs confirmados de riscos não reproduzidos. Reutiliza bibliotecas e contratos existentes, prevê testes reais e não exige alterações externas ou de persistência. A única escrita autorizada nesta sessão é este documento; a implementação depende de uma sessão Execute posterior.
