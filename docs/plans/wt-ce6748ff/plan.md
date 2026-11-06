# Plano — padronização de componentes MaxComponentsUi

## Objetivo

Substituir controles nativos ou locais quando houver equivalente adequado no MaxComponentsUi e corrigir os usos incompatíveis com o contrato efetivo da biblioteca, preservando funcionalidade, acessibilidade e identidade visual. A investigação foi estática, considerando a versão instalada 1.1.36; confirmar o contrato instalado antes de implementar.

## Arquivos afetados

- `src/components/search/SearchFilterBar.vue`.
- `src/components/commandGroups/CommandGroupModal.vue`, `CommandGroupItemRow.vue` e `CommandGroupSuggestPanel.vue`.
- `src/components/projects/ProjectAddModal.vue`.
- `src/components/ai/AiPromptInput.vue` e `AiSuggestionBlock.vue`.
- `src/components/skills/CommandMemoryList.vue` e `src/components/quick-commands/ArtisanPhpPreview.vue`, quando a equivalência for comprovada.
- `src/components/preferences/appearance/AppearancePreferences.vue`, `ThemeEditorDialog.vue`, `TerminalFontSettings.vue`, `ThemeSwatch.vue` e `ThemeImportReport.vue`.
- `src/components/bridge/BridgeSettingsSection.vue`, `src/components/quick-commands/ArtisanSchedulePicker.vue`, `src/components/preferences/AiPreferences.vue` e `src/components/preferences/system/PreferencesSystemSection.vue`.
- Demais consumidores de `MaxIconButton` com `variant` ou `severity`, incluindo `src/components/workspace/TemplateEditorModal.vue` e `src/components/quick-commands/ArtisanParamRow.vue`; identificar todos na implementação.
- Testes correspondentes em `tests/`, incluindo `tests/commandGroups/CommandGroupModal.spec.ts`, `tests/frontend/projects/ProjectAddModal.spec.ts` e `tests/lint/max-components-architecture.spec.ts`.

## Etapas de implementação

1. Inventariar novamente os controles nativos, componentes locais e usos `Max*` em `src/`, comparando-os com `node_modules/@maxvue/max-components-ui/COMPONENTS.md`, tipos publicados e implementação efetiva. Registrar exceções justificadas; não modificar a biblioteca externa nem assumir equivalência apenas pelo nome.
2. Migrar controles simples: seletores de `SearchFilterBar` e `AiPromptInput` para `MaxInputSelect`; campos simples em `CommandGroupItemRow`, `CommandGroupModal` e componentes de IA para `MaxInputText`; textarea de `CommandGroupSuggestPanel` para `MaxInputTextArea`; seletor de cor de `ProjectAddModal` para `MaxColorPicker`, desde que preserve edição e acessibilidade. Adaptar opções, eventos que emitem valores em vez de `Event`, referências de foco e estados existentes.
3. Avaliar e migrar seletores exclusivos de `CommandGroupModal` para `MaxSegmentedControl`, ações simples para `MaxButton` ou `MaxIconButton` e abas de `ArtisanPhpPreview` para `MaxTabs`, somente quando preservarem interação e layout. Avaliar `MaxModal` para `CommandGroupModal` mediante equivalência de foco, fechamento e conflitos; caso contrário manter o overlay e documentar a exceção. Não usar `MaxCodeBlock` onde sua limitação inicial de linhas ou alteração de rolagem prejudique o conteúdo.
4. Preservar os controles especializados de `terminal/RichInput.vue`, `terminal/TerminalSearchBar.vue`, `search/GlobalSearchPalette.vue`, navegação do workspace e saída de processos quando sua substituição não preservar cursor, IME, teclado, ARIA, foco ou streaming. Rever cada caso restante individualmente, sem trocas mecânicas.
5. Em `AppearancePreferences.vue`, substituir `closable`, não suportado, por `dismissable`, `closeOnEscape` e `beforeClose` conforme o contrato, bloqueando também o botão de fechar durante a exclusão. Testar os três caminhos de fechamento e a retomada após a operação.
6. Revisar todos os usos de `MaxInputNumber` com `min`, `max` ou `step`: a implementação usa entrada textual e não aplica esses limites. Garantir validação de intervalo, integralidade e passo conforme cada domínio, inclusive no limite de persistência, sem duplicar ou enfraquecer validações existentes.
7. Trocar `size="small"` de `MaxIconButton` por fator numérico adequado e revisar todos os usos de `variant` e `severity`: na implementação instalada essas propriedades não produzem estilo. Aplicar classes semânticas locais quando necessárias, preservando dimensão clicável e aparência; não alterar a biblioteca externa.
8. Atualizar os testes arquiteturais para refletir substituições efetivas e exceções justificadas, sem remover proteção de acessibilidade. Acrescentar testes de regressão por componente e revisar o inventário até não restarem usos incorretos conhecidos sem justificativa.

## Validação

- Executar em lote os testes Vitest dos componentes alterados e o teste arquitetural; cobrir valores e eventos, limites numéricos, fechamento durante operação, teclado, foco, rótulos e estados desabilitados.
- Validar manualmente fluxos de busca, edição de projetos, grupos de comandos, IA e preferências, inclusive responsividade e contraste; verificar cursor, IME e saída contínua onde aplicável.
- Executar `npm run typecheck`, `npm run typecheck:vue`, `npm run lint` e `npm run lint:style`, registrando eventuais falhas preexistentes separadamente.

## Retomada

Seguir as decisões e etapas acima na worktree atual; manter as exceções funcionais e revalidar os contratos da versão instalada antes de qualquer substituição.
