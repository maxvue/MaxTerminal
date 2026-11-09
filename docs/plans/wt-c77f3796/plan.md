# Plano de execução — padronização MaxComponentsUi

## 1. Objetivo

Substituir controles nativos e componentes locais por equivalentes adequados do MaxComponentsUi e corrigir usos incompatíveis com os contratos reais da biblioteca, preservando comportamento, acessibilidade e integração com o terminal.

**Base e decisões:** duas varreduras estáticas identificaram 50 botões, 4 inputs e 1 textarea nativos em 26 SFCs; a auditoria de contratos cobriu 104 SFCs, 588 ocorrências e 26 tipos Max, sem cobertura integral de credenciais. Usar a biblioteca instalada por vínculo local em `/home/johnattas/GitHub/MaxComponentsUi`, revisão limpa `85e36b5baa2e57abe414afa2d184d8418a06313b`, consultando `COMPONENTS.md`, implementação e exports compilados. Os equivalentes abaixo existem nessa biblioteca; documentação remota pode estar defasada.

Manter stores MaxPinia, helpers MaxUse e contratos de domínio existentes. Não alterar a biblioteca externa, backend, persistência ou configurações sem necessidade demonstrada. Usos válidos de slot em `MaxButton`, `@change` por valor em `MaxInputSelect`, refs expostas, instalação dos plugins e host de toast não demandam correção; não adicionar outro `MaxToast`.

## 2. Arquivos Afetados

Todos os caminhos de componentes desta seção são relativos a `src/components/`. Modificar os arquivos listados conforme a etapa correspondente; preservar seus wrappers públicos de domínio.

| Migração | Arquivos |
| --- | --- |
| Botões → `MaxButton` com label ou slot | `agents/AgentLauncherMenu.vue`; `ai/AiStatusIndicator.vue`; `bridge/BridgeStatusIndicator.vue`; `commandGroups/CommandGroupSuggestPanel.vue`; `commandGroups/GroupRunIndicator.vue`; `github/IssueListItem.vue`; `indicators/GitIndicator.vue`; `indicators/PortBadge.vue`; `projects/ProjectDiscoveryPanel.vue`; `projects/ProjectItem.vue`; `search/GlobalSearchPalette.vue` (somente botões); `search/SearchFilterBar.vue`; `skills/CommandMemoryList.vue`; `terminal/InputModeToggle.vue`; `terminal/__dev__/TerminalPlayground.vue`; `workspace/SessionHistoryDrawer.vue` (somente itens); `workspace/TabWorkspaceView.vue` (aba Issues); `workspace/TemplateLaunchPopover.vue` |
| Inputs → `MaxInputText`; toggles de busca → `MaxButton` | `agents/AgentSettingsPanel.vue`; `terminal/TerminalSearchBar.vue` |
| Seleção por etapas → `MaxButton`/`MaxListBox` | `ai/HarnessPicker.vue` |
| Seleções exclusivas → `MaxSegmentedControl`; etiquetas → `MaxInputText`/`MaxIconButton` | `commandGroups/CommandGroupModal.vue`; `skills/SkillConflictList.vue` (somente resolução exclusiva) |
| Sugestões visuais → `MaxIconButton`/`MaxButton` | `projects/ProjectIdentityForm.vue` |
| Abas simples → família `MaxTabs`, com `v-model:value` | `quick-commands/ArtisanPhpPreview.vue`; `workspace/SessionReadonlyViewer.vue` |
| Estados vazios → composição com `MaxEmptyDiv` | `shell/AppEmptyState.vue`; `projects/ProjectEmptyState.vue` |
| Tags/contadores → composição com `MaxTag`/`MaxBadge`, conforme contrato de cor/conteúdo | `github/IssueLabelChip.vue`; `projects/ProjectStatusBadges.vue`; `ai/RiskBadge.vue`; `indicators/WorktreeBadge.vue`; `indicators/SshBadge.vue`; `shell/CommandBlockStatus.vue` |
| Avisos → composição com `MaxAlert` | `agents/AgentAttentionIndicator.vue`; `github/LabelSyncBanner.vue`; `terminal/TerminalExitBanner.vue` |

**Correções dos usos Max existentes:**
- Fechamento: `workspace/TemplateEditorModal.vue`, `github/IssueCreateModal.vue`, `github/IssueOpenTabModal.vue`, `github/BugCycleConfirmModal.vue`, `agents/AgentTabOpenDialog.vue`, `agents/AgentWorktreeCloseDialog.vue`, `projects/ProjectAddModal.vue`, `projects/ProjectRootModal.vue`. Conferir consumidores `workspace/TabWorkspaceView.vue`, `github/IssuesTab.vue` e `projects/ProjectList.vue`; nos diálogos de agentes, conferir também o consumidor antes de afirmar impacto operacional.
- Nomes acessíveis: `agents/AgentTabOpenDialog.vue`, `github/IssueCreateModal.vue`, `workspace/TemplateEditorModal.vue`; estender a conferência aos demais campos com rótulos visuais sem associação demonstrada.
- Contratos numéricos a revisar, modificando somente onde faltar validação explícita: `bridge/BridgeSettingsSection.vue`, `preferences/AiPreferences.vue`, `preferences/appearance/TerminalFontSettings.vue`, `preferences/appearance/ThemeEditorDialog.vue`, `preferences/system/PreferencesSystemSection.vue`, `quick-commands/ArtisanSchedulePicker.vue`.

**Testes:** modificar `tests/lint/max-components-architecture.spec.ts` e ampliar `tests/components/modals/MaxModalCloseProtocol.spec.ts`; ajustar `tests/a11y/search-palette.a11y.spec.ts` e `tests/a11y/workspace-modals.a11y.spec.ts` conforme os controles migrados. Criar `tests/components/modals/MaxModalBusyGuards.spec.ts`, `tests/components/preferences/NumericInputContracts.spec.ts` e `tests/terminal/TerminalSearchBar.spec.ts` se não houver cobertura equivalente. Reutilizar `tests/app/plugins.spec.ts` como regressão, sem alteração obrigatória.

## 3. Etapas de Implementação

1. **Fixar contratos e testes de regressão.** Conferir props, eventos, slots, modelos, refs e exports efetivos dos equivalentes acima. Usar imports explícitos como no projeto; não pressupor auto-resolver. Registrar exceções no teste arquitetural e escrever casos que reproduzam as incompatibilidades antes das correções.
2. **Impedir fechamento enquanto ocupado.** Os oito modais acima usam `dismissable`/`closeOnEscape`, mas isso não bloqueia o X: `MaxModal.vue:219–255,311–313` executa `requestClose('button')`. Aplicar `beforeClose` assíncrono, retornando permissão e usando `preventDefault()` quando ocupado, seguindo `ThemeEditorDialog.vue:261–266`. Encaminhar Cancelar pela mesma política e tratar `update:visible` sem fechar em notificações de reabertura. Testar X, Escape, backdrop e Cancelar durante/depois da operação.
3. **Corrigir nomes acessíveis.** `MaxInputSelect.vue:9–23` usa `div role="combobox"`; spans adjacentes não o nomeiam. Associar `aria-labelledby` ou `aria-label` a Agente CLI e Localização da Worktree (`AgentTabOpenDialog.vue:5–6,21–22`), Estado Inicial (`IssueCreateModal.vue:14–15`) e Origem da Conexão/tipo de variável dinâmica (`TemplateEditorModal.vue:17–18,73`). Conferir o atributo no controle focável real; não depender só de `label for` para combobox customizado. Completar a verificação dos demais rótulos, sem presumir inválidas associações HTML legítimas.
4. **Explicitar validação numérica.** `MaxInputNumber.vue:11–23` renderiza input textual e não implementa limites/passos: atributos `min/max/step` não garantem validação. Conferir handlers e payloads nos seis arquivos listados, priorizando os `v-model` diretos de `AiPreferences.vue:62,70`. Reutilizar limites e normalizações já existentes em fontes, editor de tema e agendamento; não inventar novas faixas nem afirmar que todos os valores persistidos estão inválidos. Onde necessário, validar/normalizar antes de atualizar estado ou enviar dados.
5. **Migrar nativos por família.** Aplicar a tabela, mantendo IDs, labels, `.stop`, roles de menu/aba, listeners de gatilhos, estados disabled/loading e seleção por valor. Filtros/toggles independentes continuam independentes com `aria-pressed`. Em `TerminalSearchBar`, adaptar ref para a API exposta do `MaxInputText`, incluindo `input?.select()`. Preservar setters dos grupos e adicionar etiquetas por Enter. Nos tabs simples, preservar seleção, carregamento de chunks e identidade dos painéis.
6. **Compor os wrappers locais.** Migrar somente sua apresentação para os componentes Max indicados, preservando registry de status, cores/contraste, contagens, live regions e eventos públicos. Não trocar indicadores interativos por badges estáticos nem introduzir botões aninhados. Adaptar CSS ao `span.max-button-label` criado por `MaxButton`; `MaxIconButton` filtra listeners `on*`, portanto não utilizá-lo como substituição cega de gatilhos com `slotProps`.
7. **Fechar as guardas arquiteturais.** Cobrir botões/inputs não justificados, nomes acessíveis e contratos reais, além dos regex atuais. A exclusão de `/credentials/` é uma lacuna: revisar/remover apenas após inspeção autorizada dessa área, sem enfraquecer testes nem declarar essa cobertura concluída.

**Exceções decididas:** manter textarea de `terminal/RichInput.vue`; manter inicialmente o input combobox de `search/GlobalSearchPalette.vue` até demonstrar paridade de IME, seleção, foco e ARIA. Preservar os overlays especializados de `CommandGroupModal`/`GlobalSearchPalette`, o `aside` de `SessionHistoryDrawer`, `TabBar`/`TabBarItem` arrastáveis, autocomplete/virtualização, resultados de busca/destaque seguro, `ShortcutRecorder`, prévias/seletor de temas, `splitpanes`, xterm, scrollback e camadas de blocos. Manter `TabStatusIndicator` especializado e os SVGs do catálogo offline já registrados para `MaxIcon`. Forms, labels e fieldsets continuam com sua semântica HTML.

## 4. Validação

- Na implementação, executar testes direcionados dos componentes afetados e das suítes arquitetural e de acessibilidade; exercitar componentes reais nos testes de contratos, não somente stubs permissivos. Verificar payloads de seleção, nomes de comboboxes e fechamento por interação real, além de chamadas diretas ao hook.
- Validar números vazios/null, negativos, fora da faixa, fracionários e com vírgula decimal, verificando o valor efetivamente enviado e preservando as regras do domínio.
- Após testes direcionados, executar uma vez `npm run check`, que agrupa verificação das bibliotecas locais, lint, Stylelint, tipos TS/Vue, cobertura, build e checks Rust. Não executar sincronização corretiva automática para resolver divergências externas sem autorização.
- Conferir no aplicativo desktop foco inicial/restaurado, Tab/Shift+Tab, Enter/Escape, atalhos, estados disabled/loading, execução única por clique, teleporte dos overlays, rolagem e ausência de interferência na entrada do terminal.
- **Limitação de cobertura:** leituras de `src/components/credentials/` e `tests/components/credentials` foram bloqueadas pelo CC Safety Net (`secret.basename.credentials`), incluindo a auditoria de `VaultSetupModal.vue` e `VaultUnlockModal.vue`. Não repetir nem contornar o acesso. Próxima investigação: inspecionar contratos e testes dessa área somente após acesso autorizado ou recebimento das evidências por canal permitido; as correções fundamentadas acima não dependem desse desbloqueio.
- **Pendências sem defeito presumido:** associação dos demais rótulos, paridade integral `src`/`dist`, foco/teclado em runtime e integração Tauri exigem as verificações descritas; não foram comprovados por execução nesta etapa.
- Nesta etapa de planejamento não executar testes, build, sincronizações ou alterações de configuração.

## 5. Retomada

Implementar sequencialmente as decisões e etapas deste plano somente em `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-c77f3796`, na branch existente `wt-c77f3796`. Não criar worktree, trocar branch, fazer commit, merge ou push. Revalidar contratos caso a biblioteca instalada mude de revisão e registrar exceções e pendências de acesso antes de declarar cobertura integral.
