# Plano: adoção e correção de componentes MaxComponentsUi

## Objetivo
Substituir controles nativos ou locais quando houver equivalente adequado no MaxComponentsUi e corrigir usos que contrariem seus contratos, preservando comportamento e acessibilidade.

## Arquivos
- `src/components/quick-commands/{ArtisanSchedulePicker,ArtisanCommandModal,ArtisanParamRow}.vue`
- `src/components/commandGroups/{CommandGroupModal,CommandGroupItemRow,GroupRunItemRow}.vue`
- `src/components/{projects/ProjectAddModal,ai/DestructiveConfirm,ai/RagResults,ai/AiSuggestionBlock}.vue`
- `src/components/{search/SearchFilterBar,github/IssueLabelEditor,bridge/BridgeSettingsSection}.vue`
- `src/components/{workspace/TabBar,workspace/SessionHistoryDrawer,indicators/GitIndicator,indicators/PortBadge}.vue`
- `src/components/preferences/{appearance/TerminalFontSettings,system/EditorSelector,system/PreferencesSystemSection,AiPreferences}.vue`
- `src/components/skills/SkillsSyncPanel.vue` e testes existentes correspondentes; outros componentes identificados na varredura de implementação.

## Passos
1. Inventariar todos os controles nativos, locais e usos de componentes Max no projeto; confrontar props, eventos, slots e acessibilidade com a versão local de `@maxvue/max-components-ui`. A inspeção da pasta de componentes do cofre foi bloqueada pela proteção de caminhos sensíveis: respeitar o bloqueio e, se necessário, solicitar acesso apropriado antes de incluí-la; não presumir que esteja correta.
2. Migrar campos, seletores e checkboxes apropriados para `MaxInputText`, `MaxInputNumber`, `MaxInputSelect` e `MaxInputCheckbox`; adaptar opções e eventos sem perder validação, foco, teclado, nomes acessíveis ou identificadores de teste. Usar `MaxButton`, `MaxIconButton` e `MaxIcon` nos controles equivalentes.
3. Avaliar `MaxModal`, `MaxPopover`, `MaxPopoverConfirm` e `MaxDrawer` nos diálogos e painéis locais, migrando somente quando preservarem semântica, posicionamento e interação. Manter implementações especializadas de terminal e busca quando o contrato Max não suportar cursor, composição ou combobox adequadamente.
4. Corrigir os contratos identificados: nome acessível dos `MaxInputSelect` nas preferências; botão de fechar no cabeçalho do modal Artisan; slot `icon` ou ícone padrão nos dois `MaxAlert`; validar limites e incrementos de `MaxInputNumber` antes da persistência, verificando os manipuladores existentes para evitar duplicação.
5. Corrigir outros usos comprovadamente incompatíveis identificados no inventário, mantendo MaxPinia e MaxUse nas integrações existentes e sem alterar contratos do backend. Atualizar testes de interação, validação e acessibilidade; executar testes pertinentes, lint e verificação de tipos disponíveis no projeto.

## Aceite
- Controles elegíveis usam componentes Max equivalentes; exceções especializadas e eventual acesso bloqueado ficam documentados.
- Props, slots e eventos respeitam os contratos instalados; rótulos, fechamento de modal e validações funcionam.
- Fluxos, navegação por teclado e testes existentes permanecem funcionais; verificações executáveis passam ou limitações são registradas.
