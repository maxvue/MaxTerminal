# Plano — padronização de componentes MaxComponentsUi

## Objetivo

Substituir controles nativos e componentes locais quando houver equivalente funcional seguro no MaxComponentsUi e corrigir seus usos inadequados, preservando comportamento, acessibilidade e a experiência do terminal. A referência principal será a biblioteca local vinculada, versão 1.1.36; conferir o `dist` efetivamente importado quando divergir da documentação ou do código-fonte.

## Arquivos afetados

- `src/components/commandGroups/{CommandGroupModal,CommandGroupSuggestPanel,GroupRunIndicator}.vue`;
- `src/components/{bridge/BridgeStatusIndicator,ai/AiStatusIndicator,ai/AiPromptInput,ai/HarnessPicker,ai/InlineDiagnosis,ai/AiSuggestionBlock}.vue`;
- `src/components/indicators/{GitIndicator,PortBadge}.vue` e `src/components/skills/CommandMemoryList.vue`;
- `src/components/search/{SearchFilterBar,GlobalSearchPalette,SearchResultItem}.vue`;
- `src/components/projects/{ProjectDiscoveryPanel,ProjectIdentityForm,ProjectItem}.vue`;
- `src/components/terminal/{InputModeToggle,TerminalSearchBar}.vue` e `src/components/terminal/__dev__/TerminalPlayground.vue`;
- `src/components/workspace/{TemplateLaunchPopover,SessionHistoryDrawer,SessionReadonlyViewer,TabBar}.vue`, `src/components/quick-commands/{ArtisanPhpPreview,QuickCommandsHost}.vue` e `src/components/github/IssueListItem.vue`;
- `src/components/agents/{AgentLauncherMenu,AgentSettingsPanel}.vue`, `src/components/horizon/HorizonModal.vue`, `src/components/preferences/appearance/{ThemeEditorDialog,AppearancePreferences}.vue`, `src/components/preferences/AiPreferences.vue` e demais consumidores identificados na auditoria;
- Testes existentes e novos em `tests/` para as telas afetadas; eventualmente `package.json` e lockfile, somente se a investigação de compatibilidade justificar alteração de versões.

## Etapas de implementação

1. Concluir inventário exaustivo de `src` para controles nativos, controles locais e todos os usos `Max*`: confrontar props, modelos, eventos, slots, atributos, foco e imports com fonte e `dist` vinculados. Registrar por ocorrência substituição, adaptação ou exceção; a análise inicial não comprovou todos os usos existentes. Verificar também testes e estilos dependentes do DOM atual.
2. Corrigir fechamento de `MaxModal` em `HorizonModal.vue`, `ThemeEditorDialog.vue` e `AppearancePreferences.vue`: a implementação atual deixa pendente `before-close` síncrono não cancelado. Adaptar os consumidores ao protocolo assíncrono suportado, mantendo bloqueio quando ocupado; não alterar a biblioteca externa nesta tarefa. Testar botão, Escape e backdrop. Não confundir a prop com o evento usado em `ProjectSettingsDialog.vue`.
3. Corrigir nome acessível do `MaxInputText` em `AiPromptInput.vue` e nome específico do diálogo `MaxPopover` em `AgentLauncherMenu.vue`. Auditar `MaxInputNumber`: `min`, `max` e `step` não são impostos pelo input textual da biblioteca; garantir validação explícita por consumidor, mantendo a validação antes de salvar em `AiPreferences.vue` e evitando alterações de comportamento arbitrárias. Testar limites, decimais, vazio e sincronização do valor.
4. Migrar por domínio botões, chips e indicadores para `MaxButton`/`MaxIconButton`, entradas simples para `MaxInputText` e seleções mutuamente exclusivas para `MaxInputRadio` quando apropriado; usar `MaxIcon` para ícones. Priorizar os arquivos de grupos, indicadores, projetos, busca e terminal listados acima. Preservar tipo `submit`, nomes acessíveis, estados desabilitados, `aria-pressed`/`aria-expanded`, estilos, propagação e emissão única. Em `TabBar.vue`, avaliar mover a ação do `span` para o próprio `MaxButton`.
5. Avaliar `MaxModal`, `MaxPopover` e `MaxTabs` para os diálogos, popovers e abas locais listados; migrar apenas mediante equivalência de slots, âncora, confirmação, navegação, foco e teclado. Testar especialmente o gatilho de `AgentLauncherMenu.vue`, pois o repasse de listeners do slot ao botão Max pode não ser equivalente. Preservar semântica de opções/listbox em resultados e menus; não substituir mecanicamente por botões.
6. Manter controles especializados enquanto não houver equivalência comprovada: `terminal/RichInput.vue`, superfície xterm e completions, combobox com IME de `search/GlobalSearchPalette.vue`, captura de atalhos, diálogos críticos e abas dinâmicas arrastáveis do workspace. Documentar cada exceção no código ou testes apropriados, sem alterar seu contrato.
7. Verificar incompatibilidade declarada: MaxComponentsUi exige Pinia `^4` e Vue Router `^5.2`, enquanto o projeto declara `3.0.1` e `4.5.0`. Não atualizar automaticamente dependências nem modificar biblioteca externa; comprovar impacto e compatibilidade, e tratar eventual alinhamento como dependência separada caso necessário.

## Validação

- Criar testes com componentes reais para modelos, eventos, slots, limites, fechamento de modal e nomes acessíveis; não confiar exclusivamente em stubs. Cobrir Tab, Enter, Espaço, setas, Escape, foco inicial/restaurado, clique fora, estados desabilitados e propagação.
- Testar fluxos de grupos, agentes, projetos, busca, sessões e terminal; conferir IME, cursor, colagem, completions, TUI, abas fixas e arrastáveis, além de execução de ações perigosas apenas após confirmação.
- Executar testes direcionados e `npm run check`, que inclui lint, Stylelint, typechecks, cobertura, build e verificações Rust; registrar bloqueios ambientais e complementar com inspeção visual no aplicativo Tauri.

## Retomada

Seguir as decisões e etapas acima, começando pelo inventário completo e priorizando correções comprovadas antes das migrações condicionais. Não houve implementação nem execução de testes nesta investigação.
