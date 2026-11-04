# Plano — padronização MaxComponentsUi

## Objetivo

Substituir controles nativos e componentes locais quando houver equivalente adequado no MaxComponentsUi e corrigir usos que não respeitam o contrato da biblioteca, preservando comportamento, acessibilidade e persistência. A varredura encontrou 180 linhas com controles nativos em 49 componentes; a contagem não corresponde necessariamente a controles distintos.

## Arquivos afetados

- `src/components/{agents,ai,bridge,commandGroups,github,projects,quick-commands,search,skills,workspace}/**/*.vue`: consumidores identificados e demais ocorrências equivalentes identificadas na execução.
- `src/components/preferences/{AiPreferences.vue,system/*.vue,appearance/*.vue}` e `src/components/shell/AppTitleBar.vue`: contratos incorretos e acessibilidade.
- `src/components/terminal/**`: somente se houver substituição comprovadamente equivalente; preservar controles especializados por padrão.
- `tests/lint/max-components-architecture.spec.ts` e testes pertinentes em `tests/frontend/`, `tests/unit/` e demais testes de componentes afetados.
- Não alterar a biblioteca externa, backend nem configurações de dependências sem necessidade comprovada; registrar limitações que não possam ser corrigidas no consumidor.

## Etapas de implementação

1. Confirmar os contratos do pacote instalado `@maxvue/max-components-ui` 1.1.36, inclusive se o `dist` corresponde ao código-fonte local em `/home/johnattas/GitHub/MaxComponentsUi`, e inventariar todas as ocorrências por arquivo. A busca externa no Context7 não encontrou a biblioteca. Usar imports explícitos compatíveis com o projeto e preservar a configuração existente dos plugins.
2. Corrigir em todos os consumidores: `MaxTag` com `value`/`label` e `severity`, não slot/`variant` (`bridge/BridgeSettingsSection.vue`, `ai/HarnessPicker.vue`, `ai/AiContextPreview.vue`, `skills/SkillsSyncPanel.vue`); aviso truncado em `search/SearchResultItem.vue` com `MaxTag` e `severity="warning"`; `MaxAlert` com `severity` válido, incluindo `danger` em vez de `error`, `type` ou `variant` (agentes, projetos, skills e preferências). Substituir variantes inválidas de `MaxButton`, especialmente `ghost`, por combinação suportada de `variant` e `severity`, verificando aparência real.
3. Corrigir grupos de `MaxInputRadio` com nomes exclusivos em `agents/AgentTabOpenDialog.vue` e `projects/ProjectRootModal.vue`. Em `preferences/AiPreferences.vue`, validar explicitamente os limites de `MaxInputNumber` antes de salvar, incluindo vazio e decimais; verificar o contrato do backend sem presumir que ele valide os valores. Verificar no DOM real os nomes acessíveis dos botões em `shell/AppTitleBar.vue` e `projects/ProjectList.vue`; usar alternativa Max acessível quando possível, sem depender de atributos que a biblioteca não encaminha.
4. Substituir em lotes os controles comuns por `MaxInputCheckbox`, `MaxInputRadio`, `MaxInputText`, `MaxInputTextArea`, `MaxInputSelect`, `MaxButton` ou `MaxIconButton`, conforme contrato: primeiro `workspace/TemplateEditorModal.vue`, `agents/AgentSettingsPanel.vue` e `bridge/BridgeSettingsSection.vue`; depois formulários GitHub, projetos, comandos Artisan, grupos de comandos, skills, busca e IA. Reavaliar todos os controles restantes, inclusive wrappers locais, e documentar no próprio código apenas exceções justificadas. Adaptar handlers de `Event` para valores emitidos, opções e valores sentinela; manter `v-model`, persistência, validação, foco, atalhos, atributos, labels e CSS. Para senhas, não modificar política de revelação ou autocomplete sem equivalência comprovada.
5. Manter por padrão controles especializados de `terminal/RichInput.vue`, campos com IME/combobox e botões estruturais de abas, menus e cartões, assim como inputs de cor e tabela nativa, quando os equivalentes não preservarem semântica ou comportamento. Preservar as exceções arquiteturais existentes de overlays, `SessionHistoryDrawer` e textarea; alterar teste de exceção somente quando a equivalência for demonstrada. Não trocar busca imediata pelo evento `search` com debounce da biblioteca sem preservar o contrato atual.
6. A proteção do ambiente bloqueou a leitura de `src/components/credentials` e `tests/frontend/credentials` durante a varredura. Não contornar o bloqueio: tratar esses caminhos como cobertura pendente e, na execução, solicitar acesso autorizado antes de concluir a auditoria deles. As faixas de peer dependencies observadas na biblioteca diferem das versões do projeto; registrar eventual falha reproduzida, sem presumir erro de execução ou atualizar dependências por iniciativa própria.

## Validação

- Criar ou atualizar testes com componentes Max reais para texto e severidade de tags e alertas, badge truncado, variantes de botão, nome acessível, grupos de rádio independentes e limites numéricos; testar fluxos de formulário, payload, foco, teclado, IME, estados desabilitados e persistência nos lotes alterados.
- Atualizar `tests/lint/max-components-architecture.spec.ts` para impedir regressões dos contratos corrigidos e validar exceções justificadas, sem proibir controles nativos especializados.
- Executar testes focados e, em seguida, `npm run lint`, `npm run lint:style`, `npm run typecheck`, `npm run typecheck:vue` e `npm test`; executar `npm run check` quando os pré-requisitos locais estiverem disponíveis. Conferir visualmente os fluxos afetados na aplicação e registrar bloqueios ou falhas preexistentes.

## Retomada

Seguir as decisões e etapas acima na worktree atual, sem criar outra worktree, trocar de branch ou realizar commit, merge ou push. Nenhum teste foi executado durante esta investigação.
