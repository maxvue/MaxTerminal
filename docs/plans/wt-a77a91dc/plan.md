# Plano — padronização com MaxComponentsUi

## Objetivo

Substituir controles nativos e locais por componentes MaxComponentsUi quando houver equivalência funcional e corrigir usos incompatíveis da biblioteca, preservando acessibilidade, segurança e comportamento existente. A investigação estática encontrou 156 linhas com controles nativos; esse número inclui controles especializados que não devem ser substituídos automaticamente.

## Arquivos afetados

- `src/components/workspace/{TemplateEditorModal,TabBar,TemplateLaunchPopover,PaneView}.vue`.
- `src/components/projects/{ProjectAddModal,ProjectRootModal,ProjectIdentityForm,ProjectItem}.vue`.
- `src/components/agents/AgentTabOpenDialog.vue`, `src/components/indicators/PortBadge.vue` e `src/components/bridge/BridgeSettingsSection.vue`.
- `src/components/github/{IssueCreateModal,GithubAuthPanel,IssueFilters,IssueLabelEditor}.vue`.
- `src/components/credentials/{VaultSetupModal,VaultUnlockModal}.vue`.
- `src/components/quick-commands/{ArtisanCommandModal,ArtisanParamRow,ArtisanSchedulePicker}.vue`.
- `src/components/commandGroups/{CommandGroupModal,CommandGroupItemRow,CommandGroupSuggestPanel,GroupRunItemRow}.vue`.
- `src/components/skills/{SkillsSyncPanel,CommandMemoryList}.vue`, `src/components/search/{SearchFilterBar,GlobalSearchPalette}.vue`, `src/components/terminal/TerminalSearchBar.vue` e `src/components/ai/{AiSuggestionBlock,DestructiveConfirm,AiPromptInput,RagResults}.vue`.
- `src/components/preferences/AiPreferences.vue`, `src/components/preferences/appearance/TerminalFontSettings.vue`, `src/components/preferences/system/{PreferencesSystemSection,EditorSelector}.vue` e demais consumidores identificados na revisão de `MaxIconButton` e `MaxBadge`.
- Testes existentes correspondentes em `tests/` e novos testes de componentes necessários. Outros arquivos identificados pela varredura final entram no escopo quando houver equivalência comprovada.

## Etapas de implementação

1. Usar como referência o catálogo e o código local instalado de `@maxvue/max-components-ui` 1.1.36; confirmar exports, props, eventos, slots, elementos reais e refs antes de cada troca. Manter imports explícitos, conforme o projeto. Inventariar todas as ocorrências restantes de `button`, `input`, `select`, `textarea` e controles locais em `src`, incluindo playground, classificando substituições e exceções justificadas.
2. Corrigir os usos comprovadamente incorretos: trocar `type="danger"` por `severity="danger"` nos alertas do cofre; remover os IDs externos desnecessários dos dois checkboxes em `TerminalFontSettings.vue` e confirmar associação do rótulo; ajustar o slot de `ProjectItem.vue` para deixar o wrapper de `MaxPopoverMenu` executar a ação uma única vez, fechar e restaurar foco, sem controles interativos aninhados.
3. Revisar todos os `MaxInputNumber` existentes: `min`, `max` e `step` não são implementados pelo componente, cujo input é textual. Preservar e completar validação explícita, limites, valores vazios e feedback nos consumidores; implementar incremento apenas onde for requisito existente. Não alterar a biblioteca externa: se alguma paridade não puder ser obtida localmente, documentar a exceção ou dependência no resultado da implementação.
4. Revisar as 29 ocorrências de `severity`/`variant` em `MaxIconButton`: a implementação instalada não aplica essas props visualmente. Onde necessário, usar classes locais coerentes com os tokens semânticos, especialmente em ações destrutivas, sem presumir suporte da biblioteca. Validar também o tamanho `sm` dos `MaxBadge` e corrigir apenas se o padrão visual não corresponder ao esperado.
5. Migrar por domínio os controles comuns: `MaxButton`/`MaxIconButton` e `MaxIcon`; `MaxInputText` para texto e senha, `MaxInputSelect` para seleção, `MaxInputCheckbox` para booleanos, `MaxColorPicker` para cor e `MaxInputTextArea` para texto livre. Avaliar `MaxSegmentedControl` para grupos de opções. Adaptar `v-model` e handlers: `MaxInputSelect` emite valor em `change`, não `Event`; testar encaminhamento de atributos e o DOM interativo, sem aninhar labels. Usar `MaxInputNumber` em novos casos somente se os limites e a semântica de inteiro forem preservados explicitamente.
6. Preservar `type`, submit, `disabled`, `required`, limites, `readonly`, `autocomplete`, nomes acessíveis, IDs, refs, estados ARIA, modificadores de evento, foco e testes existentes. Senhas e tokens devem continuar mascarados e sem exposição em logs. Verificar os rótulos externos dos selects, cujo controle é um combobox, e o possível disparo duplicado por `change` mais `update:modelValue`; tratar esses pontos como hipóteses até teste real.
7. Não substituir mecanicamente o input de `RichInput.vue` nem o combobox de `GlobalSearchPalette.vue`: preservar cursor, composição IME, navegação e `aria-activedescendant` até comprovar equivalência. Avaliar separadamente abas, menus, popovers e `SessionHistoryDrawer.vue`; manter implementações atuais quando papéis, teclado, ancoragem ou comportamento persistente não tiverem equivalente comprovado. Registrar as exceções justificadas.

## Validação

- Criar ou atualizar testes com os componentes reais para cliques, submissão, eventos e valores, limites numéricos, associação de labels, foco, teclado, overlays em modais, ações destrutivas, persistência, segredos e ausência de chamadas duplicadas.
- Testar visualmente variantes, estado desabilitado, ícones, layout, modais e contraste; percorrer fluxos de projetos, templates, preferências, cofre, busca e comandos.
- Executar os testes pertinentes e, após a implementação, `npm run test`, `npm run typecheck`, `npm run typecheck:vue`, `npm run lint`, `npm run lint:style` e `npm run build`; registrar falhas preexistentes ou bloqueios concretos sem ocultá-los.
- Repetir a varredura de controles e usos Max para confirmar que cada ocorrência remanescente está corrigida ou possui exceção justificada.

## Retomada

Seguir as decisões e etapas acima na worktree ativa, em lotes por domínio, consultando novamente os contratos locais se a versão da biblioteca mudar. Não alterar bibliotecas externas, realizar commit ou integrar sem autorização explícita.
