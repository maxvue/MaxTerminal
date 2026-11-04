# Plano de execução — tema consistente em todo o MaxTerminal

Data: 04/11/2026 · Projeto: MaxTerminal · Worktree/branch: `wt-1771709d`

Raiz confirmada por `git rev-parse --show-toplevel`: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-1771709d`.

## 1. Objetivo, limites e decisões

Implementar a solicitação: ao selecionar um esquema de cores, todos os elementos da aplicação devem acompanhar sua paleta. Ler primeiro `spec.md` deste diretório. Este plano é autossuficiente em conjunto com essa especificação e os arquivos referenciados; nenhuma implementação foi realizada na sessão Plan.

Decisões propostas para execução, sem dúvidas bloqueantes de produto:
- Revisar cores e hierarquia visual de todo o layout existente, sem redesenhar navegação, fontes, dimensões, duas colunas, abas ou splits.
- Separar **cor da interface** de **cor do dado**. Preservar cor cadastrada de projeto, labels do GitHub, amostras individuais do catálogo e ANSI original; adaptar seus contêineres e estados. Preservar significado de sucesso, aviso, erro e informação.
- Reutilizar fonte de tema e persistência existentes, componentes MaxComponentsUi, plugin MaxPinia e helpers MaxUse. Não migrar store/IPC para HTTP ou cache duplicado por causa desta tarefa.
- Não editar bibliotecas irmãs, `node_modules`, banco ou bindings. Dependência externa que realmente impedir o resultado exige notificação e autorização específica, sem bypass.
- Trabalhar somente nesta worktree. Não criar outra, trocar branch, commitar, fazer merge/push ou limpar worktree. Toda comunicação e novos textos visíveis em pt-BR; identificadores em inglês; SFC template/script/style scoped, 4 espaços, aspas simples, ponto e vírgula e tags de abertura em linha única.

## 2. Evidências verificadas por leitura

| Local existente | Evidência e consequência |
| --- | --- |
| `src/components/preferences/appearance/AppearancePreferences.vue:81–87` | Seleção chama `themeStore.setActiveTheme(id, {kind:'global'})`; preservar o caminho. |
| `src/stores/theme.ts:64–97,195–266` | Store resolve prévia, projeto/global, aplica DOM e terminais, mantém cache e snapshot; ampliar cobertura sem criar segundo estado. |
| `src/theme/cssTokens.ts:77–115` | Aplica cinco escalas, rampa e aliases; não cobre tokens específicos de campos/textos/tabelas da biblioteca nem categoria magenta. |
| `src/theme/bootstrap.ts:145–219` | Snapshot v2 guarda mapa CSS e é aplicado diretamente antes do mount; snapshots anteriores precisam continuar seguros quando novos papéis surgirem. |
| `src-tauri/crates/mt-themes/src/mapping/tokens.rs:70–138` | Rust já deriva cinco famílias da paleta e preserva temas Max canônicos; não duplicar geração de escalas no frontend nem alterar contrato. |
| `src/styles/theme.scss:21–57` | Defaults e papéis locais; `text-muted=b400` e borda=b100 são os mesmos em ambas as polaridades, exigindo revisão de contraste. |
| `src/components/shell/TabStatusIndicator.vue:95–116` | Texto usa tokens, mas fundos usam RGB fixos: trocar ambos pelo mesmo papel semântico. |
| `src/components/workspace/{TabBar,TemplateLaunchPopover,TabWorkspaceView}.vue` | Teal/RGB e roxo com fallback fixos; não acompanham integralmente o esquema. |
| `src/components/search/HighlightedText.vue:30–35` | Destaque usa ciano fixo e `--max-primary-300`; revisar par fundo/texto. |
| `src/components/github/IssueLabelChip.vue:22–28` | Fundo e texto representam a label; não substituir `colorHex` por cor do tema. |
| `src/components/preferences/appearance/ThemeSwatch.vue` | Miniatura representa cada tema; footer/ações/bordas pertencem à UI ativa. |
| `src/app/plugins.ts`, `uno.config.ts`, `vite.config.ts` | MaxComponentsUi e MaxPinia já registrados, presetMaxUno existente, dedupe Vue/Pinia; não há auto-import de componentes configurado em Vite, preservar imports explícitos. |
| `src/theme/terminalBridge.ts` | Atualiza instâncias ao vivo; preservar terminal, tipografia e scrollback. |
| `src/theme/nativeWindow.ts:24–35` | Política nativa prioriza cores do SO; não impor a paleta da página nas decorações do Linux. |
| `tests/theme/*`, `tests/stores/theme.spec.ts` | Há testes de mapa, bootstrap, ponte e store; mock de store reutiliza tokens Navy entre temas, portanto não demonstra troca real de paleta. |
| `tests/setup.ts:5–8`, `vitest.config.ts` | Happy DOM e teleport stubado; suíte unitária não prova CSS computado/renderização. |
| `scripts/check.mjs`, `package.json` | Gate agregado `npm run check` inicia com integridade local e agrega lint, estilo, dois tipos, cobertura, build e checks Rust. |

Inventário por leitura de `src/components`: foram encontrados literais de cor em shell, projects, workspace, terminal, github, agents, ai, bridge, commandGroups, quick-commands, search, skills, preferences, horizon, indicators e dev. Não são todos erros: sombras, cores de dados e fallbacks precisam classificação, não substituição cega. Revisar também grupos sem literais: variáveis legadas e herança podem continuar fixando paletas.

Bibliotecas efetivas: symlinks resolvem para `/home/johnattas/GitHub/{MaxComponentsUi,MaxUse,MaxPinia}`. MaxComponentsUi lida tem versão `1.1.36`, componentes e helpers de cor disponíveis; `src/themes/tokens.scss:92–187` contém literais de campos/seleção/tabelas. Seu manifesto declara peers Pinia 4 e router 5, enquanto o app declara Pinia 3.0.1 e router 4.5.0: divergência registrada em notificação, sem provar falha de runtime. Não foram executados instalação, preflight, testes, tipos, builds ou abertura da aplicação.

Consulta oficial complementar: Context7 `/vuejs/core` sobre estilos scoped/global e Teleport; fontes retornadas do compilador SFC e exemplo oficial de modal. A versão exata `3.6.0-rc.13` não constava da lista versionada retornada; contratos locais prevalecem. Não há evidência visual da aplicação em execução nesta sessão.

## 3. Solução e contratos compartilhados

Aplicar `spec.md`, seção 3. Ordem: `ResolvedTheme` → adaptador completo de tokens CSS → raiz do documento → biblioteca/componentes/teleports; em paralelo lógico, a ponte existente aplica o payload xterm. Toda prévia e restauração percorre o mesmo adaptador.

Responsabilidade única do adaptador: produzir mapa completo, determinístico, tipado, sem buscar dados ou ler DOM para obter cores já presentes no payload. Usar valores concretos com helpers MaxUse quando calcular contraste, não passar `var(...)` ainda não aplicado. Evitar cache de helper baseado apenas no nome de variável CSS: o nome permanece igual quando o tema muda. `getColorFromVar` aceita hex concreto; `contrastColor` clareia/escurece, mas não garante razão WCAG. Não prometer contraste apenas pela polaridade.

Preservar `buildCssVarsRecord(tokens)` para consumidores existentes e permitir contexto ANSI opcional tipado. Propagar o contexto completo nos pontos com `ResolvedTheme`; salvar todos os tokens suplementares no snapshot. O fallback pode usar as cores magenta canônicas do YAML nativo (normal `#a855f7`, bright `#c084fc`), exclusivamente na fonte central, nunca espalhadas nos componentes. Snapshot antigo sem esses papéis usa fallback documentado até hidratação; salvar novo snapshot completo na próxima aplicação persistente. Sem mudar o número v2 ou remover dados antigos sem necessidade.

Papéis de texto/borda podem selecionar níveis mais contrastantes da rampa sem alterar a rampa recebida. Para casos extremos de esquema importado, ajustar a cor derivada preservando matiz quando viável; garantir par legível e distinção por texto/ícone, sem alterar `scheme`/ANSI. Centralizar algoritmo limitado, sem observers/timers por componente. Não introduzir novo helper genérico fora de MaxUse: se faltar capacidade reutilizável, registrar a lacuna e tratar como dependência; a orquestração específica do mapa permanece no adaptador da aplicação.

Manter nomes públicos da biblioteca. Mapear tokens realmente consumidos e legados necessários, sem atribuir marca a `--primary-*` neutros. Estilos de componentes ficam scoped; CSS global apenas para o contrato central e alvos globais inevitáveis. Não usar seletores universais com `!important` para forçar todas as cores, nem filtros sobre a aplicação ou regras que repintem SVG multicolorido e conteúdo de terminal.

## 4. Execução ordenada

### T1 — preflight e inventário (antes de implementar dependentes)

- Revalidar raiz, branch e alterações locais; preservar trabalho anterior e reler instruções pertinentes.
- Ler scripts/prehooks e manifestos antes de executar qualquer comando futuro. Conferir symlinks **e artefatos dist efetivos**, exports dos componentes, identidades de Vue/Pinia/Axios e incompatibilidades notificadas. A leitura de `src` da biblioteca não comprova que `dist` tem o mesmo contrato.
- Confirmar isolamento de banco, cache/IndexedDB/localStorage, filas/storage e caminhos Tauri para testes/aceite. Worktree, mocks e ambiente de teste isoladamente não comprovam isolamento. Não copiar `.env`, executar migrations/reset ou usar credenciais reais para validar cores.
- Produzir inventário de todos os arquivos de UI, estados, variáveis legadas, literais, estilos inline e consumidores da biblioteca. Classificar cada ocorrência: token de UI, cor de dado, paleta/fallback central ou efeito neutro. Preservar exceções com justificativa no teste de cobertura.
- Gate de biblioteca falho bloqueia validação dependente. Não remover prehook, usar ignoreConfig nem chamar etapa diretamente para contornar o gate. Divergência externa exige próxima ação explícita e autorização, não update automático.
- Conclusão: contratos reais e ambiente seguro identificados; inventário cobre todos os grupos listados na spec.

### T2 — contrato de tokens, contraste e bootstrap

- Arquivos: `src/theme/cssTokens.ts`, `bootstrap.ts`, `src/styles/theme.scss`, `params.scss`, `src/stores/theme.ts`; tipos suplementares em `src/types/theme.ts` se necessários.
- Estender mapa com papéis de controles, seleção, foco, tabelas e categoria de issues; distinguir conteúdo sobre superfície, sobre preenchimento sólido e destaque suave. Unificar mixin de foco com os tokens canônicos.
- Adaptar apenas chamadas necessárias e snapshot antigo/novo. Não introduzir novo listener, cache persistente ou nova rota. Preservar prévia efêmera e restauração integral.
- Escrever testes de mapa completo, contrastes, substituição A→B→A, remoção de opcionais e snapshot v2 antigo/incompleto/corrompido. Fixtures de temas devem ter cores efetivamente distintas, não somente nomes diferentes.
- Conclusão por revisão: mapa completo e consumidores estabilizados; não executar suíte entre microedições.

### T3 — shell, layout, projetos, abas e preferências

- Áreas: `src/app/App.vue`, `src/components/{shell,layout,projects,workspace,preferences,indicators}`.
- Remover tintas fixas decorativas; corrigir labels, placeholders, links, botões de ícone, painéis e divisórias em todos os estados. Preservar propriedades de projeto e miniaturas de tema.
- Usar MaxComponentsUi para controles novos/substituídos; migrações de controles nativos somente onde necessárias para esta correção e com equivalência de foco, eventos, tamanho e acessibilidade, sem refatoração massiva alheia ao tema.
- Preservar tamanhos, ordenação, persistência do layout, seleção de abas e atalhos. Revisar overlays abertos durante troca de tema.
- Conclusão: não há cor fixa de UI não justificada nessas áreas; sem alteração de comportamento.

### T4 — terminais e demais áreas

- Áreas: `src/components/{terminal,github,agents,ai,bridge,commandGroups,quick-commands,search,skills,credentials,horizon,os,dev}` e estilos relacionados fora dos SFCs identificados em T1.
- Corrigir blocos, busca/highlights, estados, progresso, diálogos, tooltips e toast. Categoria de issues usa magenta do esquema; labels de dados continuam com suas cores originais e contraste existente.
- Preservar sequência e paralelismo de comandos, prompts, conteúdo do terminal e fluxos de autenticação. O teste manual não deve acionar ações perigosas apenas para exibir seus estados.
- Conclusão: inventário inteiro classificado e tratado; layout de desenvolvimento também não contradiz o contrato.

### T5 — testes, validação em lote e aceite visual

- Completar todo o escopo e seus testes antes de suíte completa/tipos/builds. Estender testes existentes; propor `tests/theme/themeCoverage.spec.ts` para novas ocorrências não autorizadas, reconhecendo literais de dados/fixtures/paletas e CSS em SFC/TS. Não usar allowlist de diretórios inteiros nem regex como prova única.
- Testar integração de store + componentes reais Max: seleção, projeto, seguir sistema, preview/cancelamento e dropdown/modal já abertos; incluir transições com duas paletas escuras diferentes e claro↔escuro.
- Após preflight e isolamento aprovados, executar `npm run check` na raiz da worktree. Ele já cobre `lint`, `lint:style`, `typecheck`, `typecheck:vue`, `test:coverage`, `build`, fmt check, clippy e testes Rust. Não duplicar etapas aprovadas na mesma revisão. Se interromper, registrar o que rodou e as etapas não executadas.
- Diagnosticar falhas agrupadas por bootstrap/harness, biblioteca, código alterado e baseline. Corrigir o bloco e revalidar somente evidência invalidada, sem repetir a mesma falha sem informação nova. Falha externa não autoriza ampliar escopo.
- Aceite visual em ambiente seguro: aplicar Max Navy/Teal, Max Light, Dracula/Nord e importado de destaque claramente diferente; abrir previamente modal/menu/toast, trocar tema e inspecionar cores computadas. Confirmar `color-mix` no WebKit usado pelo Tauri. Importação somente de fixture não sensível para armazenamento isolado.
- Cobrir foco por teclado, hover, seleção, erro/sucesso/aviso, disabled/inválido, scrollbars, splits, terminal existente/novo e larguras reduzidas; comparar antes/depois sem mudar geometria. Medir contraste sobre fundos efetivos, inclusive alfa. Testar reinício/bootstrap no ambiente isolado e ausência de perda de scrollback/recriação de PTY.
- Se navegador nativo ou ambiente isolado não estiver disponível, registrar `manualPending` com roteiro e próxima ação; dublês não substituem aceite real. Sem conclusão integral com falha/pendência.

Dependências: T1 → T2 → T3/T4 → T5. Execução sequencial é suficiente; este plano não autoriza delegação. Caso o usuário a autorize depois, T3 e T4 só podem ocorrer em paralelo com contrato T2 estabilizado, arquivos de escrita distintos e um responsável único por tokens, store, bootstrap e testes compartilhados. Contexto mínimo por tarefa: objetivo, referências, decisões, interfaces, arquivos/revisão e aceite; não exigir releitura integral em cada retomada.

## 5. Matriz de aceite por tarefa

| Tarefa/requisito | Evidência/testes futuros | Revisão pelo risco | Verificação manual |
| --- | --- | --- | --- |
| T1: cobertura e ambiente | Inventário completo, manifestos/exports/artefatos e isolamento comprovados | Alta: não escrever em libs/storage compartilhados | Confirmar perfil/armazenamento de teste antes de abrir Tauri |
| T2: mesma paleta em toda UI | `tests/theme/cssTokens.spec.ts`, contraste dos pares, A→B→A e opcionais | Alta: herança e resíduos de tokens | Campos, labels, tabelas, seleção, foco e teleports com cores computadas |
| T2: persistência/prévia | `tests/theme/bootstrap.spec.ts`, `tests/stores/theme.spec.ts`, snapshots distintos e legados | Alta: evitar persistir hover ou apagar preferências | Reinício, hover/cancelamento, projeto/global e seguir SO |
| T3: layout e controles | Suítes existentes de componentes + casos de integração reais Max | Média: preservar eventos, foco e geometria | Duas colunas, tabs, splits, forms e modais nos estados listados |
| T4: terminais e integrações | `tests/theme/terminalBridge.spec.ts`, testes de componentes e cobertura de cores | Alta: sem efeitos em processos/segredos | Scrollback intacto; sem comando disparado por tema; label/projeto/amostra preservados |
| T5: regressão e entrega | `tests/theme/themeCoverage.spec.ts` proposto + `npm run check` na revisão final | Alta: Happy DOM não comprova CSS real | WebKit/Tauri, ≥4,5:1 texto normal e ≥3:1 texto grande/foco/contorno essencial |

Revisão independente nos marcos de contrato e entrega quando houver revisor autorizado; não adicionar agentes sem autorização. Exceções justificadas não permitem eliminar asserções ou declarar aprovação sem evidência.

## 6. Riscos, operação e recuperação

- **Cascata/escopo:** biblioteca define tokens em `:root` e `.dark`; conferir também tokens no próprio componente. Não mover `.dark` com base em documentação histórica: inspecionar artefato atual. O alvo atual é `html` e deve continuar coerente com raiz/teleports.
- **Contraste:** níveis claros da rampa não servem automaticamente como texto secundário no modo claro; fundos suaves precisam considerar composição. Ajustar papéis, não valores ANSI ou identidade cadastrada.
- **Bibliotecas compartilhadas:** atualizações externas podem mudar `dist` durante execução; registrar versões/revisões relevantes e invalidar verificações quando houver mudança. Peer incompatível é risco observado, não falha de runtime comprovada.
- **Desempenho:** um mapa por aplicação de tema; sem varrer DOM/medir layout ou recalcular por célula/ícone. Preservar terminais registrados, sem recriar WebGL/PTY por alteração visual.
- **Reversão:** sem migração de dados; manter compatibilidade v2 e defaults seguros. Reverter somente alterações próprias por patch revisado e autorizado, sem reset, descarte de trabalho ou limpeza de cache/dados do usuário.
- **Segurança:** capturas e logs de aceite usam fixtures sem tokens, dados pessoais ou histórico real. Nenhuma chamada externa autenticada é necessária para validar cores. Novos achados fora do escopo viram notificações sanitizadas.

## 7. Retomada e progresso na sessão Execute

Na primeira admissão, ler plano e spec; nas retomadas, consultar partes pertinentes. Conferir trabalho já realizado e executar lacunas, revalidando arquivos e alterações locais. Seguir dependências e decisões; não reabrir escolhas sem evidência nova ou novo requisito. Resultados só valem enquanto conteúdo e revisão factual permanecerem iguais.

Somente durante Execute, registrar marcos e handoff em `docs/plans/wt-1771709d/execution-state.json`, ligado ao SHA256 real de `plan.md` e das revisões factuais das tarefas. **Não criar esse registro em Plan.** Contrato estrito v2, sem chaves adicionais:

- Raiz: `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`. `updatedAt` é inteiro positivo em milissegundos; `planRevision` é SHA256 real do conteúdo do plano.
- Tarefa: `{id, status, files, validations, revision?, nextAction?}`; `validations: []` é sempre obrigatório, inclusive sem validação individual; status aceita somente `pending`, `running`, `completed`, `blocked`, `manualPending`, `validationFailed`. `revision`, quando presente, é SHA256 real da revisão factual dos arquivos da tarefa.
- `verification`: `{revision: SHA256, checks: [{command, revision: SHA256, result, exitCode?}]}`. Check somente nessa lista, com essas chaves. Comando até 4.096 caracteres. `result` (e resultados em validations, quando registrados) aceita somente `passed`, `failed`, `pending`: passed exige exitCode 0; failed exige inteiro diferente de 0; pending não aceita exitCode. Nunca `null` ou resultado `blocked`.
- `nextAction` até 512 caracteres, inclusive motivos/ação concreta para bloqueios e pendências. Não usar `blocker`, `diagnostic`, `branchSync`, `durableSync`, `followUp`, `schemaVersion`, `title`, `evidence`, nem `worktree`, `branch`, `head`, `planPath`, `status` ou `checks` soltos na raiz.
- Registrar comandos realmente executados e **todas** as falhas, inclusive suíte completa. Não inferir código de saída de `tool:end.ok`. Sem checks, sem resultado vigente ou com falha vigente, não há validação integral. Compatibilidade com v1 não comprova validação integral; o registro autodeclarado não prova execução nativa nem detecta omissões.

## 8. Revisão final do planejamento

Escopo cobre interface inteira, não só terminal; preserva layout funcional e cores de dados; identifica o fluxo e contratos reais, exige MaxComponentsUi/MaxPinia/MaxUse, centraliza mudanças e evita editar bibliotecas compartilhadas. Etapas têm dependências, arquivos, aceite e gates. Snapshot legado, prévia, temas por projeto, contraste, overlays e Tauri têm validações específicas. Nenhum teste foi executado nem aprovação de implementação é presumida. Pontos externos de atenção estão nas notificações desta worktree.
