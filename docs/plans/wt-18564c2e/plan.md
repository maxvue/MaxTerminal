# Plano: padronização com MaxComponentsUi

## Objetivo
Substituir controles nativos e locais quando houver equivalente adequado no MaxComponentsUi e corrigir os usos incompatíveis com os contratos reais da biblioteca, preservando comportamento e acessibilidade.

## Arquivos
- `src/components/{projects,workspace,commandGroups,quick-commands}/**/*.vue` — formulários, botões, diálogos, abas e popovers candidatos.
- `src/components/{ai,agents,bridge,github,search,indicators,terminal}/**/*.vue` — controles e componentes compostos candidatos, inclusive a tela de desenvolvimento.
- `src/components/shell/DbRecoveryPanel.vue`, `src/components/preferences/appearance/{ThemePicker,ProjectThemeOverrides,ThemeEditorDialog,TerminalFontSettings}.vue` — correções de contratos.
- `src/components/preferences/{AiPreferences,system/PreferencesSystemSection}.vue` e `src/components/horizon/HorizonBadgeButton.vue` — correções numéricas e de eventos.
- Testes existentes relacionados aos componentes afetados e novos testes onde necessário; configuração de dependências apenas se a resolução local exigir ajuste.

## Passos
1. Conferir o catálogo e os contratos da versão local de MaxComponentsUi, a resolução da dependência e todas as ocorrências de controles nativos e componentes locais; registrar exceções sem equivalente funcional adequado.
2. Migrar formulários para os inputs Max apropriados e botões para `MaxButton`/`MaxIconButton`, adaptando modelos, eventos, tipos, validação e referências. Avaliar modal, popovers, abas e seletores compostos conforme seus contratos reais.
3. Preservar controles especializados de terminal e combobox quando a troca comprometer cursor, composição, foco, teclado ou semântica; manter stores MaxPinia e utilitários MaxUse conforme os padrões existentes.
4. Corrigir `MaxEmptyDiv` para usar `label` ou slot; adequar o fechamento de `MaxModal` durante salvamento; validar limites e passos numéricos explicitamente; corrigir parsing localizado e eventos duplicados em `TerminalFontSettings` e a alternância em `HorizonBadgeButton`.
5. Revisar os demais usos MaxComponentsUi contra props, eventos e slots da biblioteca e corrigir incompatibilidades adicionais. Atualizar testes para os fluxos alterados, incluindo acessibilidade e regressões dos problemas identificados.
6. Executar testes pertinentes, verificação de tipos e lint disponíveis no projeto; revisar visualmente os fluxos alterados quando possível.

## Aceite
- Controles substituíveis usam componentes Max adequados; exceções especializadas permanecem justificadas.
- Estados vazios, modal durante salvamento, limites numéricos, valores decimais e alternância do badge funcionam corretamente.
- Navegação por teclado, foco, validação e comportamento dos fluxos existentes são preservados; testes e verificações aplicáveis passam.
