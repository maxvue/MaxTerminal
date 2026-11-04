# Especificação técnica — esquema de cores em toda a interface

Data: 04/11/2026 · Projeto: MaxTerminal · Worktree: `wt-1771709d`

## 1. Contexto & Objetivo

Ao escolher um esquema de cores, toda a interface controlada pelo MaxTerminal deve acompanhar sua paleta: superfícies, textos, labels de campos, botões, ícones, bordas, divisórias, seleção, foco, estados, menus e sobreposições. A revisão do layout é de coerência visual, contraste e hierarquia das superfícies, sem reorganizar funcionalidades nem substituir o desenho de duas colunas e abas.

O fluxo já existe: `AppearancePreferences.onSelectTheme` → `useThemeStore.setActiveTheme` → IPC/Rust → `ResolvedTheme` → `applyMaxUiTokensToRoot` + `applyThemeAndTypographyToTerminals`. O problema é a cobertura incompleta dos consumidores e tokens: fundos RGB fixos nos componentes, famílias roxas sem derivação, tokens específicos da biblioteca não atualizados e papéis de contraste que não funcionam igualmente em temas claros e escuros.

## 2. Escopo

**Incluído:** inventário de todos os componentes de produção e auxiliares de desenvolvimento; integração dos tokens MaxComponentsUi; correção de consumidores de cores; estados normal/hover/ativo/foco/desabilitado/inválido; modais, tooltips, toast e conteúdo teleportado; bootstrap, prévia, cancelamento, escopo por projeto e sincronização com o sistema; testes de regressão e aceite visual em WebKit/Tauri.

**Preservar:** contratos IPC e SQLite, importação/exportação de temas, algoritmos e paletas ANSI do terminal, tamanho das fontes, dimensões e redimensionamento do workspace, atalhos, eventos, sessões, processos e scrollback. Manter a política explícita de decorações nativas do SO de `src/theme/nativeWindow.ts`.

**Cores que representam dados:** não sobrescrever a cor cadastrada de um projeto, `RepoLabel.colorHex` ou as amostras dos temas não selecionados. Seus contêineres, bordas e interações acompanham o tema; o dado de cor e o conteúdo ANSI permanecem fiéis à origem. Estados de sucesso/erro continuam distintos, não são todos pintados com a cor primária.

**Fora:** redesign estrutural, troca de tipografia, animações novas, migração de banco, atualização/publicação ou edição das bibliotecas irmãs, nova infraestrutura de temas, operações Git e correções de baseline sem autorização.

## 3. Arquitetura & Mudanças Técnicas

### Contrato da aplicação

1. Conservar `ResolvedTheme` como fonte única. Não criar tema paralelo em componentes, novas preferências ou nova camada de persistência.
2. Expandir `buildCssVarsRecord` em `src/theme/cssTokens.ts` como adaptador de **papéis visuais**, sem alterar os valores de `MaxUiTokens` recebidos do Rust. Manter a assinatura existente compatível, acrescentando contexto ANSI opcional tipado para cores categóricas. Chamadas com tema resolvido passam `scheme.ansi`; fallback usa a paleta canônica existente no YAML nativo.
3. Mapear tokens da biblioteca efetivamente usados: `--max-content-*`, `--max-inputtext-*`, `--max-form-field-disabled-background`, `--max-floatlabel-on-active-background`, `--max-selection-*`, `--max-focus-*`, cores de borda/conteúdo de botões e `--max-table-*`. Aliases legados são estritamente de compatibilidade com consumidores confirmados; não converter toda paleta `--primary-*` em marca.
4. Manter `--mt-ui-*` como papéis de superfície/texto/borda; revisar `text-muted`, borda de campo e seleção para contraste nas duas polaridades. Escolher candidatos da rampa e escalas do esquema para texto e ações; se não houver contraste suficiente, ajustar somente o papel derivado, nunca a paleta ANSI original. Reutilizar `getColorFromVar` de MaxUse para operações de cor; `contrastColor` sozinho não comprova conformidade WCAG.
5. Introduzir papéis locais, se necessários: texto de ação primária, fundos suaves de severidade, sombra/overlay e categoria de issues. A categoria de issues deriva de `scheme.ansi.magenta/brightMagenta`, não de roxo fixo; o fallback canônico consta em `src-tauri/crates/mt-themes/src/builtin/max-navy-teal.yaml`. Não exige alterar Rust, bindings ou formato importado.
6. Centralizar no mapa aplicado ao `document.documentElement`. Assim `body`, `#app` e teleports herdam o mesmo tema. Verificar declarações locais da biblioteca que possam sombrear herança; usar apenas contratos públicos e ajustes locais mínimos, sem editar `node_modules` ou reintroduzir PrimeVue.
7. Integrar o mapa completo ao bootstrap e ao snapshot. Manter snapshot v2 compatível: carregar snapshots antigos com defaults derivados seguros para papéis ausentes antes da aplicação; nunca carregar tokens antigos por cima de um tema novo. Acrescentar testes de reconstrução dos tokens suplementares, incluindo a categoria de issues. Não persistir prévia.

### Arquivos

**Existentes, centrais:** `src/theme/cssTokens.ts`, `bootstrap.ts`, `src/stores/theme.ts`, `src/styles/theme.scss`, `params.scss`; `src/types/theme.ts` apenas se necessário para contexto suplementar. `terminalBridge.ts` e `useThemeApplier.ts` são contratos de regressão; alterar somente se a integração exigir, não refatorar por conveniência.

**Consumidores:** `src/app/App.vue` e todos os grupos de `src/components`: shell, layout, projects, workspace, preferences, terminal, github, indicators, agents, ai, bridge, commandGroups, quick-commands, search, skills, credentials, horizon, os e dev. Inspecionar também estilos em TS/composables e utilitários UnoCSS. Arquivos sem desvio confirmado permanecem intactos.

**Testes existentes:** `tests/theme/{cssTokens,bootstrap,terminalBridge,nativeWindow}.spec.ts`, `tests/stores/theme.spec.ts`, suítes de componentes e composables. **Novos propostos:** `tests/theme/themeCoverage.spec.ts` para impedir retorno de literais de UI não justificados, e testes de integração de tema na organização existente. Não adicionar dependências sem necessidade comprovada e autorização.

### Ecossistema Max

Usar os componentes existentes MaxButton, MaxIconButton, MaxInput*, MaxModal, MaxPopover, MaxTag e MaxToast; conferir props nos artefatos efetivos. Preservar o plugin MaxPinia registrado em `src/app/plugins.ts` e a store de tema atual, com IPC/cache já existentes; não inventar endpoint HTTP nem duplicar cache/autosave para alteração visual. Helpers reutilizáveis vêm de MaxUse. Lacunas reais de biblioteca devem virar notificações, não alterações nos repositórios irmãos.

## 4. Plano de Execução Faseado

1. Preflight por leitura, revisão do ambiente e inventário de consumidores/estados e exceções de dados.
2. Estabilizar adaptador de tokens, contraste, categoria de issues e compatibilidade do bootstrap; escrever os testes correspondentes.
3. Corrigir layout-base, controles e consumidores por área, preservando comportamento e estados semânticos; acrescentar testes de integração e cobertura estática.
4. Concluir todo o escopo e testes; executar validações em lote, diagnosticar falhas agrupadas e corrigir o lote.
5. Executar aceite visual seguro, inclusive Tauri, revisar diff e registrar pendências reais. A sessão Plan não executa nenhuma fase.

## 5. Critérios de Aceite & Validação

- Temas nativos escuros e claros e tema importado de destaque diferente alteram toda a UI imediatamente, inclusive controles já abertos e teleports.
- Prévia não persiste; cancelamento restaura tudo; escopo por projeto e seguir sistema não mudam de significado.
- Sem resíduos de tema anterior, flashes atribuíveis aos novos tokens ou cores fixas de decoração fora das exceções explícitas.
- Texto normal ≥ 4,5:1; texto grande e contornos essenciais/foco ≥ 3:1 sobre o fundo efetivo. Em transparências, medir a composição real. Desabilitado continua reconhecível; cor não é o único indicador de estado.
- Nenhum PTY recriado, comando executado ou dado perdido pela troca de cores; identidade de projeto, labels e miniaturas mantida.
- Validação futura: `npm run check`, após comprovar integridade de bibliotecas e isolamento das etapas Rust. O agregador já inclui lint, estilo, tipos, cobertura, build e Rust; não repetir suas etapas aprovadas na mesma revisão. Falha de gate bloqueia validação dependente.
- Happy DOM não comprova renderização/contraste/teleport reais. Aceite manual em navegador e WebKit/Tauri é obrigatório; se indisponível, registrar `manualPending`, sem declarar conclusão integral.
