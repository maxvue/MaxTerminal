# Plano de execução — padronização e correção de MaxComponentsUi

## 1. Identificação e instruções de retomada

- Projeto: **MaxTerminal**. Data: **03/11/2026**. Worktree/branch: **wt-897572b7**.
- Raiz obrigatória: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-897572b7`.
- Revisão investigada: `c76d16f2ef3ecc4ef87d4c6bdb7d45d40203dd1b`. Estado inicial: `git status --short` vazio.
- Especificação: `docs/plans/wt-897572b7/spec.md`, elaborada antes deste plano.
- Entrega atual: plano, especificação e notificações de achados autorizadas pelo usuário. **Não há autorização de implementação nesta sessão.**

Na primeira admissão, ler este plano e a especificação. Em retomadas, consultar somente tarefas, contratos e dependências pertinentes. Conferir trabalho já realizado, estado de execução se existente, revisão real dos arquivos e alterações locais. Preservar trabalho alheio. Não reabrir decisões sem mudança de requisito ou evidência nova. Implementar lacunas, não repetir trabalho concluído cuja evidência permaneça vigente.

Não criar outra worktree, mudar branch, instalar dependências, modificar bibliotecas externas, fazer commit/merge/push ou remover esta worktree. A integração pertence ao MaxCode mediante pedido do usuário.

## 2. Objetivo, requisitos e preservações

**R1:** substituir elementos nativos e primitivas locais somente quando existir equivalência adequada em MaxComponentsUi. **R2:** corrigir todos os contratos Max comprovadamente incorretos. **R3:** preservar funcionalidade e acessibilidade. **R4:** manter MaxPinia na integração de estado e usar MaxUse para helpers novos/alterados. **R5:** cobrir o escopo com testes e proteção arquitetural, sem fingir aceite nativo com dublês.

Usuários afetados: quem gerencia projetos, abas, terminais, agentes, comandos, issues e preferências. A migração não altera permissões, formatos de dados ou regras de negócio.

Preservar: identidade e isolamento por projeto; confirmação explícita de formulários; autosave onde já existe; estados de carregamento/erro; payloads e eventos públicos; cache e persistência via backend; sanitização e renderização escapada; IME, seleção/colagem, histórico e controle do PTY; atalhos e foco; seleção/fechamento/arraste de abas; semântica e densidade visual do desktop.

Não converter todo HTML em componentes. Containers semânticos, conteúdo textual, desenhos genuínos, wrappers de domínio e superfícies especializadas podem continuar locais. Não trocar um controle por outro de finalidade diferente só por compartilhar nome ou aparência.

## 3. Evidências, limites e contratos compartilhados

### Ambiente e ferramentas identificados

| Evidência existente | Consequência |
| --- | --- |
| `package.json:15-35` | Scripts existentes: `check`, `test`, `test:coverage`, `typecheck`, `typecheck:vue`, `lint`, `lint:style`, `build`; não inventar scripts. |
| `scripts/check.mjs:28-88` | Gate canônico com 10 passos; começa por `scripts/sync-local-libs.mjs --check`, termina em verificações Rust. Interrompe no primeiro erro. |
| `scripts/sync-local-libs.mjs:125-325` | `--check` verifica arquivos exportados, presença de peers e destinos dos links; **não comprova compatibilidade semântica das versões**. |
| `vite.config.ts:40-56`, `vitest.config.ts:9-22` | Vue/Pinia deduplicados, libs externas ligadas, sem plugin de auto-import; manter imports explícitos. |
| `tests/setup.ts:5-13` | Harness global só substitui Teleport e tooltip; isso não garante plugin Max, foco real ou funcionamento de componentes complexos. |
| `tests/lint/max-components-architecture.spec.ts:27-150` | Auditoria parcial por texto; verifica modais/exports/toast e fixa exceções antigas. Não prova contratos de props/eventos nem ausência de botões/inputs nativos. |
| `tests/frontend/projects/ProjectIdentityForm.spec.ts:13-17`, `ProjectSettingsDialog.spec.ts:20-45` | Dublês simplificam inputs, modal e select; podem esconder bugs reais de atributos, slots e eventos. |
| `tests/components/preferences/appearance/AppearancePreferences.spec.ts:147-205` | Há testes que emitem `change` diretamente no componente; acrescentar interação pelo controle real, não usá-los como prova única. |
| `tests/frontend/workspace/workspace-components.spec.ts:36-67` | Preservar texto escapado na busca de sessões e proibição de interpretar snippets como HTML. |
| `.vscode/run-dev.sh:90-168` | `npm run dev` sincroniza links, limpa cache compartilhado e pode encerrar processos; **não é smoke test inofensivo**. |

Leituras também cobriram `eslint.config.js`, `.gitignore`, `tsconfig.vue.json`, `src/stores/projects.ts`, `src/stores/settings.ts`, os manifestos efetivos e os tipos de MaxPinia. Convenções: template/script setup/style scoped, atributos de abertura em uma linha, quatro espaços, aspas simples e ponto e vírgula; tokens semânticos Max.

Bibliotecas locais observadas: MaxComponentsUi `1.1.36`, MaxUse `2.0.1`, MaxPinia `0.2.0`. O aplicativo usa Vue `3.6.0-rc.13`, Pinia `3.0.1`, Router `4.5.0`, Axios `1.7.9`. MaxComponentsUi declara peers Pinia `^4.0.0`/Router `^5.2.0`. `node_modules` é link para `/home/johnattas/GitHub/MaxTerminal/node_modules`; as três libs apontam aos repositórios irmãos. Há risco alto de compatibilidade/efeitos compartilhados, não uma falha de runtime comprovada.

Notificações criadas nesta sessão, fora da implementação do aplicativo: `docs/notifications/wt-897572b7/notifications/max-components-ui-peer-compatibility.md` (divergência declarada e risco compartilhado) e `docs/notifications/wt-897572b7/notifications/max-components-ui-button-accessible-attributes.md` (lacuna da biblioteca, relacionada a M10). Correção nas bibliotecas e alinhamento de versões exigem demanda própria; os documentos não autorizam essas ações. Não foram identificados segredos que exigissem notificação na investigação realizada; isso não equivale a auditoria de segurança abrangente.

Notificação complementar: `docs/notifications/wt-897572b7/notifications/max-components-ui-checkbox-attribute-forwarding.md`. Na fonte de MaxInputCheckbox, id fornecido por attrs segue para wrapper e input; não pressupor equivalência com id nativo ao migrar controles. É risco de contrato estático, sem ocorrência atual/runtime afirmada. Preferir label da biblioteca ou nome acessível sem id duplicado e testar a associação no componente distribuído; não alterar a lib.

**Limitações:** não se executaram testes, builds, aplicações, instalações ou serviços; não houve aceite visual/nativo. A documentação oficial Vue sobre fallthrough de atributos foi consultada via Context7 (`https://vuejs.org/guide/components/attrs`): atributos não declarados não chegam automaticamente ao alvo certo em componentes multi-root. A versão exata `3.6.0-rc.13` não estava disponível como referência versionada; o código local é a fonte decisiva dos componentes Max.

### Contratos obrigatórios antes de distribuir implementação

- **C1 — botão:** conferir raiz HTML, prop de tipo, disabled/loading e evento; garantir uma única ação por clique/tecla/submit. Não pressupor que attrs recebidos pelo componente chegam ao botão interno.
- **C2 — input:** conferir `modelValue`, nulabilidade, coercão, debounce, eventos e elemento que recebe ID/ARIA/foco. Não usar `$event.target.value` em evento cujo payload é valor simples.
- **C3 — seletor:** conferir formato de opções, campos label/value, evento de mudança e valor vazio. Preservar tipos numéricos/booleanos e sentinelas de herança de tema.
- **C4 — ícones:** usar API exportada de MaxIcon/MaxIconButton; preservar cor, tamanho e nome acessível no controle, com ícone decorativo oculto quando adequado.
- **C5 — overlays:** conferir modelo de visibilidade, slots, fechamento, confirmação, Escape, backdrop, foco inicial/restauração e bloqueio durante ação. Não presumir que um slot existente tenha o nome esperado.
- **C6 — estado/helpers:** manter o bootstrap MaxPinia existente e ações de stores; sem novos caminhos diretos HTTP/IPC em componentes para contornar a arquitetura. `MaxPiniaConfig` (`MaxPinia/src/types.ts:44-70`) fornece Axios/rotas HTTP, não contrato nativo de transporte Tauri: não inventar `options.get` IPC nem duplicar cache SQLite. Helpers genéricos novos/alterados vêm de MaxUse, com export efetivo confirmado; lógica de domínio permanece no projeto.

## 4. Inventário de mudanças

Auditoria delegada, somente leitura: **117 SFCs**, sendo **47 com controles nativos** e **70 sem esses controles**. Contagem estática: 62 botões, 87 inputs, 22 selects, duas textareas, uma tabela e sete forms; loops podem produzir várias instâncias. Foram encontradas 411 linhas com tags Max, não necessariamente 411 instâncias. Nenhuma ocorrência foi corrigida nesta sessão.

### 4.1 Correções confirmadas de contratos Max — prioridade T1

Os caminhos desta seção são relativos a `src/components/`, salvo prefixo explícito. Linhas referem-se à revisão investigada e podem deslocar durante a execução.

| ID / achado | Arquivos e evidência | Correção e aceite específico |
| --- | --- | --- |
| M1 — texto em slot de MaxTag desaparece; variante inválida | `ai/HarnessPicker.vue:6,10,14,23`; `ai/AiContextPreview.vue:16,30`; `bridge/BridgeSettingsSection.vue:8`; `skills/SkillsSyncPanel.vue:46,47` | Mover texto para `value`/`label`; `variant` semântico → `severity`; intenção primária via `color` com token. Não presumir `size="sm"` compacto em MaxBadge. Ver texto real e cor. |
| M2 — API de tag usada em badge | `search/SearchResultItem.vue:12` | Preferir `MaxTag value="Truncado" severity="warning"`, preservando classe/title; alternativa MaxBadge com `label` e `color`. Ver texto visível. |
| M3 — mensagem ignorada | `shell/DbRecoveryPanel.vue:15`; `preferences/appearance/ThemePicker.vue:12`; `preferences/appearance/ProjectThemeOverrides.vue:7` | `MaxEmptyDiv message` → `label`; verificar mensagens específicas, não o padrão “Sem Registros”. |
| M4 — alertas com `type` | `projects/ProjectList.vue:16,23`; `ProjectRootModal.vue:33`; `ProjectAddModal.vue:43`; `ProjectIdentityForm.vue:37,41`; `agents/AgentWorktreeCloseDialog.vue:18,30`; `AgentTabOpenDialog.vue:46,59`; `AgentSettingsPanel.vue:20` | Usar `severity` conforme intenção atual: info/success/warning/danger. Ver classes, ícone, role e aria-live. |
| M5 — outras APIs inválidas de alerta | `skills/SkillsSyncPanel.vue:13`; `preferences/system/ShortcutRecorder.vue:12`; `PreferencesSystemSection.vue:3`; `preferences/appearance/AppearancePreferences.vue:3`; `TerminalFontSettings.vue:12` | No primeiro, substituir `variant` e fornecer ícone pelo slot `#icon` se necessário; nos demais `severity="error"` → `danger`. MaxAlert não tem a mesma união de severidade de MaxTag. |
| M6 — variantes/severidades fora do contrato de botão | Todos os MaxButton/MaxIconButton em `src`; exemplos: `workspace/TemplateLaunchPopover.vue:5,23-25`, `TemplateEditorModal.vue:40,49,53-54`, `PaneView.vue:6-8`; `quick-commands/ArtisanParamRow.vue:8-10`; `commandGroups/CommandGroupItemRow.vue:20-23,39,47-48`; `terminal/CommandBlock.vue:15`; `search/SearchFilterBar.vue:41`; variantes dinâmicas `ai/AiSuggestionBlock.vue:65` e `ai/InlineDiagnosis.vue:29` | MaxButton: variante somente outlined/text/link, severidade separada; ghost → text preservando intenção visual. Remover primary como variante/severidade quando significar padrão. Valores secondary/danger/warning/success em variant podem funcionar por coincidência de classe: corrigir contrato sem alegar que todo botão está quebrado visualmente. MaxIconButton não aplica aparência de variant/severity: usar color/iconColor/transparent ou CSS local comprovado. Buscar também valores dinâmicos/computed até encerrar todas as ocorrências. |
| M7 — primary indevido em severity | `src/app/App.vue:7`; `preferences/system/PreferencesSystemSection.vue:59`; `preferences/AiPreferences.vue:100`; `shell/DbRecoveryPanel.vue:21` | Omitir severity para aparência primária padrão, sem alterar intenção da ação. |
| M8 — números permitem vazio/null, fração e fora de faixa | `preferences/AiPreferences.vue:62,70,161-165` | Adaptar modelo e validação antes de salvar: debounce inteiro 0–10000, topK inteiro 1–20; vazio/inválido gera feedback e impede salvar até corrigido, sem enviar null ou NaN. Preservar último valor válido no DTO, sem silenciosamente persistir valor incorreto. Testar limites, negativo, fração, vazio e parsing pt-BR. |
| M9 — dois canais para mesma atualização | `preferences/system/EditorSelector.vue:10`; `PreferencesSystemSection.vue:27,50`; `preferences/appearance/TerminalFontSettings.vue:11,17,22,32,37,38` | Usar um canal canônico (`update:modelValue` para texto/número/checkbox); remover change DOM redundante, não o change declarado dos selects. Uma interação produz uma atualização/persistência. Preservar clamps e proteção de finitude já existentes. |
| M10 — nome acessível consumido e não repassado por MaxButton | `shell/AppTitleBar.vue:7`; `projects/ProjectList.vue:6,60`; `projects/ProjectWorkspace.vue:17,30` | Lacuna da biblioteca, não prop inventada do app. Ação só de ícone → MaxIconButton com icon e aria-label; conteúdo complexo → texto acessível real no slot. Ver nome no DOM; não modificar biblioteca. `title` em MaxIconButton não garante tooltip nativo. |

**Fontes de API verificadas:** `MaxTag.vue:2,45-63` não renderiza slot; `MaxBadge.vue:46-83` usa label/color; `MaxEmptyDiv.vue:21-43` usa label; `MaxAlert` aceita info/success/warning/danger; tipos de botões em `MaxComponentsUi/src/types/index.ts:60-99`. Achados centrais também conferidos pelo auditor no dist exportado de tags, botões, alertas, empty, checkbox e número. O coordenador releu diretamente MaxTag e MaxButton, além de AiPreferences e SuggestPanel.

### 4.2 Controles nativos: inventário completo dos 47 arquivos — T2

Todos os caminhos relativos a `src/components/`. T/N/C/R/S/A/B significam texto/número/checkbox/radio/select/textarea/botão; são ocorrências de template, não quantidade renderizada. **Decisão:** migrar controles simples indicados; preservar a superfície especializada explicitamente descrita. Itens ricos usam slot de MaxButton somente se manterem estrutura HTML válida e layout; caso contrário, compor primitivas internas sem envolver botões dentro de botão.

| Lote / arquivo:linha | Ocorrências | Destino e cuidados |
| --- | --- | --- |
| A `agents/AgentLauncherMenu.vue:5,13` | 2 B | MaxButton; popover, atalhos e disponibilidade. |
| A `agents/AgentSettingsPanel.vue:29-87` | 8 C, 1 T, tabela | MaxInputCheckbox/Text; preservar tabela sem equivalência comprovada de MaxTable. |
| A `agents/AgentTabOpenDialog.vue:6-56` | 2 S, 3 T | MaxInputSelect/Text; proposta e caminho da worktree. |
| A `ai/AiPromptInput.vue:8,12` | S, T | Select/Text; adaptar ref para focus exposto. |
| A `ai/AiStatusIndicator.vue:2-4` | B | MaxButton com conteúdo/estados; em compact=true, incluir texto acessível real no slot (visualmente oculto, não display:none/v-if) ou alternativa comprovada com MaxIconButton. Não depender apenas de aria-label consumido por MaxButton. Preservar tooltip por mecanismo efetivamente renderizado, não só pela prop title consumida. |
| A `ai/AiSuggestionBlock.vue:31` | T | Text; manter reclassificação de risco. |
| A `ai/DestructiveConfirm.vue:19` | T | Text; confirmação exata, Enter/Escape e bloqueio. |
| A `ai/HarnessPicker.vue:4-40` | 6 B | MaxButton; preservar seleção em etapas. |
| A `ai/RagResults.vue:11` | S | Select; preservar slug. |
| A `bridge/BridgeSettingsSection.vue:17-61` | 4 C, S, N | Checkbox/Select/Number; porta vazia continua null, nunca 0. |
| A `bridge/BridgeStatusIndicator.vue:2-4` | B | MaxButton; abre configurações. Em compact=true, manter nome acessível por texto real no slot visualmente oculto ou alternativa comprovada com MaxIconButton; não depender apenas de aria-label/title consumidos por MaxButton. Preservar tooltip e estados. |
| B `commandGroups/CommandGroupItemRow.vue:8-46` | 4 T, T/senha, C | Text/Checkbox; manter máscara e edição de env. |
| B `commandGroups/CommandGroupModal.vue:35-90` | 4 T, 8 B | Text/Button; `×` → IconButton lucide:x. Preservar overlay; controles exclusivos podem continuar Button, sem exigir redesenho para SegmentedControl. |
| B `commandGroups/CommandGroupSuggestPanel.vue:7,38` | A, B | TextArea, rows=3 e autoResize=false; Button. Remover só essa exceção antiga do guardrail. |
| B `commandGroups/GroupRunIndicator.vue:2` | B | Button; contadores/abertura. |
| B `commandGroups/GroupRunItemRow.vue:32,36,40` | 3 B | Button; mesmas ações de output/foco/cancelamento. |
| C `github/GithubAuthPanel.vue:34` | senha | Text type=password, sem acrescentar revelação de senha. |
| C `github/IssueCreateModal.vue:7,15,22` | T, S, C | Text/Select/Checkbox; título obrigatório, limite e categorias. |
| C `github/IssueFilters.vue:5-31` | 5 B, busca, 3 S | Button/Text type=search/Select; manter seleção exclusiva atual. |
| C `github/IssueLabelEditor.vue:6,13,22` | 2 S, C | Select/Checkbox; exclusividade das labels. |
| C `github/IssueListItem.vue:2` | B | MaxButton com slot rico e wrapper de domínio preservado. |
| C `github/IssueOpenTabModal.vue:8-41` | 5 R, C | Radio/Checkbox; mesmos values e name de grupo. |
| D `indicators/GitIndicator.vue:3` | B | Button; detalhes/popover. |
| D `indicators/PortBadge.vue:4,8,12,24` | 4 B | Button/IconButton; manter confirmação de kill e stopPropagation. |
| D `projects/ProjectAddModal.vue:7-34` | 4 T, cor | Text/MaxColorPicker; autodetecção versus manual. |
| D `projects/ProjectDiscoveryPanel.vue:4` | B | Button; não converter seção em accordion. |
| D `projects/ProjectIdentityForm.vue:15,23,27` | 2 B, cor | Button/IconButton/ColorPicker; preservar paleta e hex. |
| D `projects/ProjectItem.vue:3,21,26` | 3 B | Button/IconButton; seleção rica, menu e propagação. |
| D `projects/ProjectRootModal.vue:7` | T | Text; readonly em edição e seletor de pasta. |
| E `quick-commands/ArtisanCommandModal.vue:20-45` | 3 T, 2 C | Text/Checkbox; DTO e geração. |
| E `quick-commands/ArtisanParamRow.vue:17-57` | 4 T, 4 C | Text/Checkbox; argumentos/opções/arrays/defaults. |
| E `quick-commands/ArtisanPhpPreview.vue:4,7,13` | 3 B | Button; manter três painéis, sem troca obrigatória por Tabs. |
| E `quick-commands/ArtisanSchedulePicker.vue:4-80` | 4 C, 2 S, 4 N, 3 T | Inputs Max; manter modelo Laravel, validação e integralidade, não agendador genérico. |
| F `search/GlobalSearchPalette.vue:8-37` | T, 4 B | Migrar botões; preservar input/overlay especializado até equivalência de IME/combobox/foco demonstrada. |
| F `search/SearchFilterBar.vue:6-45` | 4 S, B | Select/Button; chips múltiplos não são radiogroup. |
| G `skills/CommandMemoryList.vue:6,10,21` | T, C, B | Text/Checkbox/Button; ocultos, recarga e draft. |
| G `skills/SkillsSyncPanel.vue:20-34` | 3 S, T | Select/Text; destino específico agy. |
| H `terminal/__dev__/TerminalPlayground.vue:6,18,22` | S, 2 B | Select/Button; demo também entra no escopo. |
| H `terminal/InputModeToggle.vue:2` | B | Button; bloqueio forçado por TUI. |
| H `terminal/RichInput.vue:8` | A | **Preservar nativa**; seleção/cursor, IME e PTY. |
| H `terminal/TerminalSearchBar.vue:3-11` | T, 3 B | Text/Button; focus exposto e aria-pressed. |
| I `workspace/SessionHistoryDrawer.vue:14,21,31` | busca, 2 B | Text type=search/Button; preservar aside não modal. |
| I `workspace/SessionReadonlyViewer.vue:12` | B | Button; geração e seleção de painel. |
| I `workspace/TabBar.vue:12,17` | 2 B | Button; lançamento padrão/popover. |
| I `workspace/TabWorkspaceView.vue:5` | B | Button apenas preservando role/integração da aba fixa Issues. |
| I `workspace/TemplateEditorModal.vue:8-48` | 6 T, 3 C, S | Text/Checkbox/Select; env literal/herdada e submit. |
| I `workspace/TemplateLaunchPopover.vue:10` | B | Button com item rico; manter ações adjacentes independentes. |

**APIs para as substituições:**

- MaxInputText (`:36-74,103-130` na fonte): `modelValue`, type/label/disabled/required/placeholder/spellcheck/noMessage; update entrega string/number/undefined; expõe input/focus/setFocus. Converter undefined para string vazia onde o domínio exige string.
- MaxInputCheckbox (`:27-60`): modelo booleano, não `event.target.checked`. MaxInputRadio (`:33-42`): modelo e value, name comum do grupo.
- MaxInputSelect (`:161-244,680`): opções `{label,value}` com campos explicitamente configurados; change/update entregam valor, não DOM Event. Preservar sentinelas e tipos. Não ligar dois canais à mesma persistência.
- MaxInputNumber (`:11-23,102-157`): input interno text/inputmode decimal; emite number/null/undefined e interpreta pt-BR. `min/max/step` não substituem validação. Preservar limites existentes dos campos migrados, lendo seus handlers/schemas antes de adaptar.
- MaxInputTextArea (`:19-62`): modelo, rows/minRows/maxRows/autoResize; autoResize padrão true. **SuggestPanel: desligar**, sem estender essa decisão à RichInput.
- MaxColorPicker (`:11-20,37-60`): defineModel, format/inputId/ariaLabel/disabled; preservar hex #RRGGBB e normalização, sem mudança de UX.
- InputBase (`:279-335`) separa attrs do controle/wrapper. Classes, data-testid/data-field-path e listeners podem mudar de nó; ajustar seletores e foco deliberadamente, sem presumir que um wrapper é editável.

### 4.3 Outros 70 SFCs: composições adequadas e preservações

**26 oportunidades de composição.** Migrar primitivas simples de badge/alerta/estado vazio dentro dos wrappers, preservando seus contratos; se uma superfície não couber nos slots reais sem perda de informação, mantê-la especializada e registrar a exceção concreta. Isso não autoriza redesenho. Caminhos relativos a `src/components/`, exceto App.

| Arquivo:linha | Proposta delimitada |
| --- | --- |
| `src/app/App.vue:6` | Código de erro em MaxTag. |
| `agents/AgentAttentionIndicator.vue:4-5` | Tag de atenção preservando fonte, suspeita/confirmação e ações. |
| `ai/InlineDiagnosis.vue:18,37`; `ai/RiskBadge.vue:2-5` | Tags de cache/risco; manter classificação/motivos/tooltips. Confirmação local não vira PopoverConfirm global automaticamente. |
| `commandGroups/GroupRunOutputView.vue:5,11,18,28`; `GroupRunPanel.vue:3,18-20` | Tags/alertas; preservar viewport contínuo, scroll e bloqueio de entrada/paste/drop. |
| `github/BugCycleConfirmModal.vue:4,8` | Loading/empty Max, mantendo prévia/fila. |
| `github/IssueComments.vue:4,9`; `IssueDetail.vue:3-5,12` | Empty/tag de plano/estado. |
| `github/IssueLabelChip.vue:2-4` | MaxTag com cor/contraste GitHub e grupo canônico; não reduzir cores arbitrárias a severidade. |
| `github/IssueList.vue:3-13`; `IssuesTab.vue:7,18`; `LabelSyncBanner.vue:2-10` | Loading/empty/alertas e contadores; preservar stale, seleção, sincronização e ações. |
| `github/IssuePlanSection.vue:7` | Tag Provisório. |
| `indicators/WorktreeBadge.vue:2-5` | Tag mantendo origem/nome/bloqueio. |
| `projects/ProjectEmptyState.vue:2-14`; `shell/AppEmptyState.vue:2-9` | Composição MaxEmptyDiv com slots, mantendo título/descrição e todas as ações. |
| `projects/ProjectSettingsDialog.vue:19` | MaxAlert de erro de tema. |
| `projects/ProjectStatusBadges.vue:3-9` | Badge/tag nas pills de status/providers; registry, contadores, cores e compactação preservados. |
| `projects/ProjectWorkspace.vue:10-11,46-48` | Tags de stack/pasta ausente e empty de workspace. |
| `quick-commands/ArtisanParamsBuilder.vue:11-13` | Empty interno sem substituir builder. |
| `quick-commands/QuickCommandItem.vue:14-16` | Tags de atalho/valor/global; manter item de completion e highlighting. |
| `quick-commands/QuickCommandsHost.vue:5-20` | MaxModal para confirmação simples, mantendo preview e resolveConfirm(false/true) inclusive fechamento/cancelamento; confirmar todos os caminhos resolvem uma vez. |
| `search/SearchResultSection.vue:5-6` | Tags de contagem/truncamento; agrupamento/paginação preservados. |
| `shell/CommandBlockStatus.vue:3-5`; `terminal/TerminalExitBanner.vue:2-4` | Primitiva de status/tag/alerta, mantendo confiança, mensagem e reinício. |

MaxEmptyDiv tem slots default/icon/label; label é a prop textual. MaxTag não encaminha slot default. MaxPopoverConfirm depende de `useConfirmStore` global, não prop visible: **não é substituto neutro de confirmação local**. MaxInputIconPicker abre drawer próprio: não trocar campos/paletas rápidas por ele neste escopo. MaxSegmentedControl é radiogroup exclusivo: não usar para filtros múltiplos nem exigir sua adoção onde Button preserva melhor a UX.

**44 SFCs restantes, sem nova substituição adequada identificada** (ainda sujeitos às correções Max da seção 4.1). Nomes abaixo têm extensão `.vue`, relativos a `src/components/`:

- `agents/`: AgentStatusLine, AgentWorktreeCloseDialog.
- `ai/`: AiContextPreview, AiPaneResults.
- `bridge/`: InspectButton; `dev/`: PerfOverlay; `github/`: BugCycleProgress, IssueActionsMenu; `indicators/`: TabIndicators.
- `layout/`: AppLayout; `os/`: ShortcutStatusBadge.
- `preferences/`: AiPreferences, PreferencesDialog, SkillsAndMemoryPreferences.
- `preferences/appearance/`: AppearancePreferences, ProjectThemeOverrides, TerminalFontSettings, ThemeImportReport, ThemePicker, ThemePreview, ThemeSwatch.
- `preferences/system/`: CloseBehaviorSelector, EditorSelector, PreferencesSystemSection, WaylandShortcutHelp, ShortcutRecorder.
- `projects/`: ProjectEditModal, ProjectList.
- `search/`: HighlightedText, SearchResultItem.
- `shell/`: AppShell, AppTitleBar, DbRecoveryPanel, TabStatusIndicator.
- `skills/`: SkillConflictList.
- `terminal/`: CommandBlock, CommandBlockLayer, CompletionItem, CompletionPopup, HybridTerminal, TerminalView.
- `workspace/`: PaneLayout, PaneView, TabBarItem.

**Preservações funcionais explícitas:** RichInput nativa; GlobalSearchPalette com input/overlay especializado; CommandGroupModal com overlay especializado; SessionHistoryDrawer aside não modal; TabBarItem com DnD e fechamento independente; Splitpanes em AppLayout/PaneLayout; captura de ShortcutRecorder; preview real de cores em ThemePreview/ThemeSwatch; hosts, completion e decorações xterm. Não forçar MaxDrawer/MaxTabs/MaxTable sem equivalência demonstrada. Preservar os sete forms semânticos e seus submits.

Não há SVG direto nos templates encontrados. `src/constants/bundledIcons.ts:1-6` alimenta catálogo **offline** em `src/app/plugins.ts:24-28`: preservá-lo, não confundir SVG de catálogo com ícone visual duplicado. Prompts `❯`, informação ahead/behind e símbolos matemáticos permanecem texto quando representam conteúdo. Não foram encontradas render functions/JSX adicionais nos padrões auditados; a div imperativa de xterm em `src/terminal/engine/terminalInstance.ts:144` permanece.

### 4.4 Usos corretos a preservar e cobertura

MaxIcon usa **icon**, não name, e aceita tamanhos numéricos. MaxButton suporta click, submit, disabled e loading; não reintroduzir action como evento. MaxIconButton suporta click.stop e nome por label/aria-label. MaxInputSelect suporta opções label/value, change(valor) e expose isOpen usado em ProjectThemeOverrides. MaxModal suporta visible/noButton/dimensões/dismissable/closeOnEscape/update:visible e before-close cancelável; manter a proteção de `projects/ProjectSettingsDialog.vue:52-56`. Accordion value, QueryState, Popover/PopoverMenu e markdown source/renderMarkdown encontrados não precisam de correção contratual. Sanitização permanece.

O bootstrap está confirmado em `src/app/plugins.ts:19-32`: createPinia → createMaxPinia → app.use(pinia) → MaxComponentsUi, CSS e catálogo de ícones. O plugin não transforma stores sem isCached/is_cached (`MaxPinia/src/plugin.ts:98`); isso não justifica migrar stores IPC existentes ou ativar cache/HTTP indiscriminadamente.

Testes adicionais existentes a adaptar por domínio: `tests/frontend/projects/ProjectAddModal.spec.ts:83-106`; `tests/frontend/workspace/workspace-components.spec.ts:62,85-88`; `src/components/ai/__tests__/DestructiveConfirm.spec.ts:48-55` e `AiSuggestionBlock.spec.ts:100-108`; `src/components/quick-commands/__tests__/ArtisanSchedulePicker.spec.ts:15-40`; `tests/terminal/TerminalSearchBar.spec.ts:43-61`; `src/components/indicators/__tests__/PortBadge.spec.ts:43-49` e `GitIndicator.spec.ts:48-51`; `tests/a11y/workspace-modals.a11y.spec.ts` e `tests/a11y/search-palette.a11y.spec.ts`. Selects reais, foco, IDs e estrutura dos inputs exigem ajustar seleção dos testes, sem reduzir assert de negócio.

## 5. Execução faseada e responsáveis

### T0 — Preflight por leitura e baseline

1. Confirmar worktree/branch, revisão, mudanças preexistentes, links e manifestos efetivos. Conferir existência/coerência de fonte, `dist`, exports e tipos de cada componente escolhido.
2. Ler scripts/prehooks e configuração de testes antes de executar qualquer validação; confirmar identidades resolvidas de Vue, Pinia e Axios para app, bibliotecas e harness. Registrar incompatibilidade declarada dos peers e interromper dependências que não possam ser validadas com segurança. Não instalar, reconstruir libs, remover hooks nem usar `ignoreConfig` como solução.
3. Comprovar que testes mockam IPC/serviços e isolam DB, cache/IndexedDB, storage, filas quando existentes, subprocessos e PTYs. Para etapas Rust da bateria canônica, ler os testes e seus diretórios de dados antes de executar. Worktree, transação e variável de ambiente isoladamente não comprovam isolamento.
4. Confirmar destinos de saída/cache e que executar não escreverá na árvore compartilhada nem alcançará dados reais. Não copiar `.env`, migrar/resetar banco ou executar suíte compartilhada sem prova e autorização operacional.

**Saída:** baseline e gates documentados; se houver bloqueio real, tarefa `blocked` e próxima ação concreta. Não apresentar incompatibilidade como resolvida pela simples existência dos arquivos.

### T1 — Estabilizar contratos e correções Max

Revalidar os achados da seção 4 nas revisões atuais, fechar C1-C6 com evidências da fonte e distribuição efetiva, implementar correções de contratos e seus testes. Um responsável decide APIs compartilhadas antes das substituições dependentes. Não alterar biblioteca externa para adaptar um uso incorreto do aplicativo. Se a equivalência exigir alteração externa, registrar a pendência e pedir decisão específica, sem implementação disfarçada.

### T2 — Substituições de primitivas por domínio

Aplicar o inventário confirmado da seção 4. Migrar controles e ícones, adaptar estilos ao DOM real e preservar classes/seletores úteis quando compatíveis. Não manter CSS para elementos que deixaram de existir nem usar seletores profundos frágeis como primeira solução. Distinguir formulário submit de ações auxiliares, clique de seleção de clique de fechamento e disabled visual de bloqueio efetivo.

Os lotes A–I da tabela são também limites de responsabilidade por diretório; composições da seção 4.3 e correções M1–M10 pertencem ao dono do mesmo diretório. `preferences/` e `shell/` formam lote J; `src/app/App.vue` e o harness compartilhado ficam com o integrador. T1 fecha contratos antes de T2; não entregar o mesmo SFC simultaneamente a dois agentes. Após estabilização, é permitido paralelizar domínios com **arquivos distintos**, incluindo seus testes próprios. Cada executor recebe tarefa, requisitos, APIs confirmadas, revisão dos arquivos, lista de escrita e critérios de aceite, com referências a este plano e à spec; não exige releitura integral a cada microetapa. Bibliotecas e dependências compartilhadas não têm dono de escrita nesta demanda.

### T3 — Testes e proteção arquitetural

Atualizar testes junto de todo o escopo antes da bateria completa. Responsável de harness coordena alterações em `tests/setup.ts` e testes compartilhados; não permitir escrita concorrente nesses arquivos.

- Evoluir `tests/lint/max-components-architecture.spec.ts` para inventariar controles em templates, preferindo AST com `vue-eslint-parser` já declarado no projeto. Não contar HTML gerado internamente por libs/xterm como violação do aplicativo. Exceções por caminho e finalidade, não permissões globais.
- Remover expectativas de exceções antigas somente após demonstrar equivalência e adicionar teste comportamental correspondente; não inverter testes para acompanhar uma mudança ainda não validada.
- Criar **`tests/components/max-components-contracts.spec.ts` (novo proposto)** para exercitar componentes Max reais nos contratos atingidos: atributos no alvo interno, submit único, evento/valor de input e select, botão bloqueado e modal com slots corretos. Montar com a mesma integração necessária do app e limpar estado/portais/cache entre casos; mockar IPC e integrações, não os contratos que estão sob teste.
- Atualizar testes específicos por domínio do inventário; preservar asserts de regras de negócio e sanitização. Não trocar tudo por snapshots, emissões manuais ou stubs permissivos.
- Em `src/components/ai/__tests__/AiStatusIndicator.spec.ts` e `tests/components/bridge/BridgeStatusIndicator.spec.ts` (existentes), acrescentar casos compact=true e false com componentes Max reais: nome acessível não vazio e coerente com o estado, tooltip preservado por mecanismo comprovado, ativação por teclado e uma única emissão de openSettings/abertura da seção correta. Os testes atuais cobrem rótulo/tooltip/clique no modo padrão, mas não o compacto; não usar apenas a existência de aria-label na instância Vue como prova.

### T4 — Revisão e validação agrupada

Com implementação e testes de todo o escopo prontos, revisar diff e riscos independentemente. Executar a bateria canônica **`npm run check`** somente após T0 aprovado; ela cobre lint, estilo, tipos TS/SFC, suíte com cobertura, build e Rust. Não executar suíte/tipos/build entre microalterações ou enquanto agentes ainda implementam.

Registrar comandos realmente executados, códigos de saída e todas as falhas. O script para no primeiro erro: passos posteriores não foram executados e não podem constar como aprovados. Diagnosticar bootstrap/harness e agrupar correções antes de repetir testes equivalentes. Reutilizar resultados aprovados somente com revisão factual relevante inalterada; não contornar gate de libs ao rodar comandos internos diretamente. Nova revisão exige revalidar os checks impactados e fechar todas as lacunas do gate.

### T5 — Aceite manual e entrega

Em ambiente desktop **isolado e autorizado**, verificar matriz abaixo nos temas claro/escuro, navegação só por teclado, zoom/densidade e fluxos de erro/carregamento. Inspecionar previamente o lançador; não executar `npm run dev` automaticamente devido aos efeitos identificados. Não usar projetos/credenciais/dados reais para provocar ações destrutivas, abrir PRs/issues ou comandos de shell.

Se ambiente nativo, credenciais ou ação externa forem necessários e indisponíveis, manter `manualPending` com procedimento seguro e próxima ação; dublês não comprovam execução nativa. Esperar conclusão de tarefas assíncronas por notificação/evento com prazo e recuo, sem polling repetitivo. Cancelamento mantém resultados parciais e pendências. Não declarar conclusão integral com gates ou aceite pendentes.

## 6. Matriz de aceite por tarefa

| Tarefa / requisito | Evidência exigida | Testes pertinentes | Revisão pelo risco / manual |
| --- | --- | --- | --- |
| T0 / ambiente seguro | Versões, links, exports e isolamento efetivos; integridade aprovada | Gate de bibliotecas da bateria após preflight | Identidades Pinia/Axios e impacto de symlinks; bloquear incompatibilidade não resolvida |
| T1 / R2 | Todos os achados Max da seção 4 com correção ou descarte justificado por fonte | Novo teste de contratos + testes dos fluxos afetados | DOM real, slots e eventos; foco e ausência de warnings em interação |
| T2 / R1 | Cada primitiva inventariada migrada ou exceção específica fundamentada | Testes por domínio + auditoria arquitetural | Comparar densidade, cores, nomes acessíveis e keyboard; nenhuma substituição inadequada |
| T2 / R3 terminal/workspace | Bytes/payloads, seleção, fechamento e arraste preservados | `tests/terminal/RichInput.spec.ts`, `CompletionPopup.spec.ts`, `HybridTerminal.spec.ts`, testes de workspace | IME, colagem, Ctrl+C, setas/Home/End/Tab, terminal nativo e foco |
| T1–T3 / R3 formulários/temas | Um submit/ação, disabled/loading efetivos, vazio/null/valores tipados preservados | Testes de projetos, aparência e formulários; contratos reais | Salvar/cancelar/reabrir; tema herdado/global; impedir clique repetido |
| T2–T3 / R3 indicadores compactos | AiStatusIndicator e BridgeStatusIndicator têm nome acessível no botão real em compact=true/false, tooltip e abertura única das preferências | `src/components/ai/__tests__/AiStatusIndicator.spec.ts`, `tests/components/bridge/BridgeStatusIndicator.spec.ts` | Leitor de tela/nome acessível e teclado; não confiar em aria-label/title consumidos por MaxButton |
| T1–T3 / R4 | MaxPinia integrado sem alterar contratos de stores; helpers MaxUse confirmados | Testes de bootstrap, stores e toast pertinentes | Nenhum autosave inesperado, cache paralelo ou chamada externa nova |
| T3 / R5 | Guardrail cobre o universo auditado e exceções justificadas | `tests/lint/max-components-architecture.spec.ts` e teste novo | Não mascarar falhas com stub/relaxamento de assert |
| T4 / qualidade | Checks vigentes de toda a bateria, sem falhas ou etapas omitidas | `npm run check` após T0 | Revisão final vinculada ao conteúdo testado |
| T5 / aceite real | Roteiro manual realizado em ambiente seguro | Não substituível por unitários | Sem aceite desktop: `manualPending`, não conclusão total |

## 7. Riscos, operação e reversão

**Situação da revisão documental:** revisão independente integral concluída sobre spec, plano e as três notificações, sem bloqueadores documentais identificados. Foram conferidos inventário, requisitos/aceite, contratos, exceções, responsabilidades, gates operacionais, registro v2 e limites das notificações. O apontamento anterior sobre nomes acessíveis dos dois indicadores compactos está incorporado às migrações, testes e matriz de aceite. A revisão parcial anterior e sua tentativa de check que falhou no parser são histórico, não uma revisão integral ainda pendente. Esta conclusão é de planejamento: não comprova execução da aplicação, compatibilidade em runtime, aprovação de T0 ou aceite desktop, que permanecem gates futuros explícitos.

- **Compatibilidade e recursos compartilhados:** risco alto; não sanar peers com instalação/upgrade automático, nem sincronizar ou limpar links/caches compartilhados. Caso dependa de mudança externa, suspender validação afetada e solicitar solução isolada/autorizada.
- **Atributos e estrutura DOM:** nomes corretos não bastam; validar IDs, ARIA, foco e eventos no alvo real. Não duplicar listeners ao combinar v-model e eventos.
- **Mudança visual:** biblioteca pode alterar tamanho/estrutura; adaptar tokens e layout sem redesenho amplo. Preservar área de clique, overflow e densidade de abas.
- **Estado e dados:** manter confirmação, isolamento por projeto e revisões de stores; nenhum reset, migração ou transferência de segredos para cache local.
- **Exceções:** testes históricos evidenciam contratos atuais, não tornam uma exceção desejada para sempre. Documentar razão concreta e reavaliar só diante de equivalência demonstrada.
- **Reversão futura:** sem migração de dados; reverter apenas o patch desta demanda no fluxo autorizado, preservando alterações alheias e contratos backend. Não usar reset/checkout destrutivo ou apagar worktree. Não há deploy nesta demanda.

## 8. Registro de progresso em Execute

**Não criar esse registro durante Plan.** Em Execute, registrar após marcos e no handoff em `docs/plans/wt-897572b7/execution-state.json`, com SHA256 real dos bytes de `plan.md` em `planRevision`, e revisões factuais SHA256 dos conteúdos relevantes de cada tarefa/check. Nunca usar rótulos, nomes de branch ou hashes inventados como revision.

Contrato de novos registros: **version 2 estrita**, sem campos adicionais:

- Raiz: `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`. `updatedAt`, se presente, inteiro positivo em milissegundos, nunca ISO string.
- Tarefa: `{id, status, files, validations, revision?, nextAction?}`. `validations: []` é obrigatório; nesta demanda, manter essa lista vazia e registrar resultados executados em `verification.checks`, evitando inventar formato adicional de validação por tarefa.
- `verification`: `{revision: SHA256, checks: [...]}`.
- Check, **somente** em `verification.checks`: `{command, revision: SHA256, result, exitCode?}`. `command` tem no máximo 4.096 caracteres.
- Status de tarefa: `pending`, `running`, `completed`, `blocked`, `manualPending` ou `validationFailed`.
- Resultado de validação/check: somente `passed`, `failed` ou `pending`. `blocked` não é resultado de check. `passed` exige `exitCode: 0`; `failed` exige inteiro não zero; `pending` não aceita `exitCode`; nunca `null`.
- `nextAction`, quando presente, tem até 512 caracteres; motivos e providências de bloqueios/pendências cabem nele.

Proibidos: `schemaVersion`; raiz com `worktree`, `branch`, `head`, `planPath`, `status` ou `checks` solto; tarefa com `title` ou `evidence`; campos `blocker`, `diagnostic`, `branchSync`, `durableSync`, `followUp` em qualquer nível.

Registrar todos os comandos realmente executados e falhas, inclusive da suíte completa; não inferir resultado ou exit code de `tool:end.ok`. Checks vazios, falta de resultado vigente ou falha vigente impedem validação integral. Version 1 permanece compatível, mas não prova validação integral automaticamente. O estado é registro autodeclarado: não prova execução nativa nem detecta verificações omitidas. Repetição da mesma falha sem progresso exige diagnóstico; tarefa bloqueada/manual exige próxima ação concreta.
