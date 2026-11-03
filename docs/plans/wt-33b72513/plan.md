# Plano — tema integral e dimensões das configurações gerais

- Projeto: **MaxTerminal**.
- Data: **03/11/2026**.
- Worktree e branch: **wt-33b72513**.
- Raiz autorizada: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-33b72513`.
- Estado: investigação e planejamento; nenhuma implementação ou validação de runtime realizada.

## 1. Objetivo e limites

Corrigir a aplicação integral da polaridade do tema selecionado: temas escuros devem produzir interface escura e temas claros, interface clara, incluindo a barra de títulos nativa. Eliminar superfícies invertidas e inconsistências entre componentes próprios e MaxComponentsUi.

No modal **Configurações gerais**, aplicar `width: calc(100vw - 50px)` e limite de `1500px`, além de alinhar ícones e textos do seletor de seções à esquerda.

Incluído: tokens, bootstrap/cache visual, consumidores de cores de toda a interface, sincronização do tema da janela nativa, permissões mínimas necessárias, modal e testes de regressão. Uma correção central não dispensa revisar os consumidores com convenção invertida.

Preservar: catálogo/importação/exportação de temas, cores ANSI e cores próprias do terminal, seleção global, overrides por projeto, seguir sistema, prévia/cancelamento, tipografia, estados semânticos, atalhos, navegação e fechamento dos modais. Não recriar PTYs nem perder abas, scrollback ou sessões ao trocar tema.

Excluído: redesign, novo seletor de tema, migração de banco, alterações de APIs/contratos Rust de temas, mudança de bibliotecas irmãs, atualização de dependências, barra de títulos personalizada, publicação e integração Git. Não criar outra worktree, trocar branch, commitar, fazer merge/push ou remover esta worktree.

## 2. Evidências e diagnóstico

### Fatos observados

1. `src/app/main.ts` aplica `applyThemeBootstrapSync()` antes de montar. `src/app/App.vue` instala `useThemeApplier()`; `src/composables/useThemeApplier.ts` acompanha projeto e preferência do sistema. `src/stores/theme.ts` centraliza seleção, cache de resolvidos, eventos IPC, prévia/cancelamento e propagação a terminais.
2. `src/theme/cssTokens.ts` copia a rampa do backend diretamente para `--background-*` e aplica corretamente `.dark`, `data-theme-polarity` e `color-scheme` no elemento raiz. Apenas adicionar `.dark` novamente não resolve.
3. O contrato real em `src-tauri/crates/mt-themes/src/scale.rs:318–369` estabelece **níveis baixos para superfícies e níveis altos para texto em ambas as polaridades**. Em escuro, `0` é escuro e `850–950` são brancos. `mapping/tokens.rs:199–239` confirma essa rampa para `max-navy-teal` (`b0 = #17293d`, `b900 = #ffffff`). Não inverter o contrato Rust.
4. `src/styles/theme.scss` e `src/theme/bootstrap.ts` ainda têm a convenção oposta: fundo `900/950` escuro, texto `50/100` claro. `AppShell.vue`, `AppLayout.vue` e `PreferencesDialog.vue` usam os níveis altos como superfícies. `TabBar.vue` e `TemplateLaunchPopover.vue` mostram o mesmo padrão. Isso explica inversões após a hidratação real.
5. `src/theme/terminalBridge.ts` já usa `b0/b25` como superfícies de blocos/input e transmite as cores xterm sem inverter. Preservar esse contrato.
6. MaxComponentsUi instalada: **1.1.36**. Foram consultados `src/components/MaxModal.vue`, `MaxButton.vue`, `src/themes/colors.scss` e declarações do pacote através de `node_modules/@maxvue/max-components-ui`. A versão atual aceita `:root.dark` e `.dark`; a informação histórica de que seria obrigatório mover a classe para `body` não se aplica aqui. `MaxModal` usa `background-0` como fundo e `background-700` como texto, teleporta para `body` e replica a prop `class` no diálogo real.
7. `PreferencesDialog.vue` limita hoje a largura a `56rem`. `MaxModal` oferece `width` e `maxWidth`; a última vira `min(valor, calc(100vw - 40px))`, compatível com a largura solicitada de viewport menos 50px. O modal real recebe ambas por estilo inline.
8. A navegação já declara `justify-content: flex-start` e `text-align: left` no botão, mas `MaxButton` envolve seu slot em `.max-button-label`. Portanto é necessário alinhar também esse conteúdo interno e conferir especificidade/cascata; não repetir somente as declarações existentes.
9. `src-tauri/tauri.conf.json` mantém `decorations: true`, janela `main`, mínimo de `960×600`. A barra de títulos é nativa; não há barra personalizada montada em `App.vue`. A API instalada Tauri **2.5.0** possui `getCurrentWindow().setTheme('dark' | 'light')`; a documentação local informa que no Linux/macOS o efeito é de aplicação, não apenas da janela. A capability atual só libera `core:event:default` entre permissões core; falta `core:window:allow-set-theme`, identificador confirmado no schema local.
10. `src/app/plugins.ts` registra `createMaxPinia()`. `useThemeStore` usa IPC e cache próprio; não há razão para introduzir GET/POST HTTP ou autosave paralelo. MaxUse **2.0.1** e MaxPinia **0.2.0** estão acessíveis localmente. Axios está declarado, mas não participa deste fluxo IPC.
11. Há testes em `tests/theme/{bootstrap,cssTokens,terminalBridge}.spec.ts`, `tests/stores/theme.spec.ts` e `src/components/preferences/__tests__/PreferencesDialog.spec.ts`. Estes últimos montam MaxModal/MaxButton reais; não se deve substituir tudo por stubs e perder a regressão de anatomia.
12. `package.json`, `vitest.config.ts`, `tests/setup.ts` e `scripts/check.mjs` foram lidos. Vitest usa happy-dom; o gate `npm run check` agrega bibliotecas, lint, tipos, cobertura, build e verificações Rust. `.gitignore` já ignora ambas as pastas de worktrees. A árvore estava limpa no início.

### Limitações e hipóteses

- Não foram iniciados aplicativo, navegador, testes ou serviços. O diagnóstico é estático; o resultado visual e a decoração Linux exigem validação futura.
- A consulta Context7 à documentação Tauri confirmou o modelo de capabilities, mas não forneceu o trecho específico de `setTheme`; sintaxe, comportamento Linux e permissão acima foram confirmados na API instalada e no schema do projeto.
- Não foi realizado inventário completo de cada declaração de cor. Há ocorrências adicionais identificadas em `TemplateEditorModal.vue`, `TabWorkspaceView.vue`, `TabBarItem.vue`, `SessionReadonlyViewer.vue`, `IssuesTab.vue`, `PaneLayout.vue`, `TerminalView.vue` e `SessionHistoryDrawer.vue`; o executor deve ler seus contextos antes de alterar, assim como demais ocorrências em `src`.
- O compositor/tema GTK pode limitar a decoração nativa. Não considerar o requisito cumprido apenas pelo sucesso da Promise de `setTheme`; conferir a janela real no CachyOS e registrar eventual bloqueio, sem substituir silenciosamente a decoração.

## 3. Solução e contratos decididos

### Tema e cores

- Manter `ResolvedTheme.uiTokens.polarity` como fonte de verdade visual, inclusive para a janela durante prévia e restauração. Não deduzir polaridade pelo nome do tema nem criar booleano independente persistido.
- Preservar a rampa `--background-*` recebida do backend e a classe escura em `html`. Corrigir consumidores, fallback e cache visual; não fazer inversão global de CSS, `filter: invert`, troca indiscriminada de índices ou sobrescrita dos componentes da biblioteca.
- Introduzir, em `src/styles/theme.scss`, aliases semânticos locais para superfície base/elevada/hover, borda e texto principal/secundário, derivados da rampa canônica: por exemplo `--mt-ui-surface: var(--background-0)`, superfícies secundárias nos níveis `25/50`, bordas em `100/150` e textos nos níveis `700/900`. Definir os papéis e níveis uma única vez, conferir contraste nas duas polaridades e usá-los nos componentes próprios; detalhes específicos podem continuar usando tokens canônicos. Usar `--max-*-content` para texto sobre ações semânticas, não presumir branco fixo.
- Alinhar a rampa fallback aos valores canônicos já existentes no Rust, antes da hidratação, e remover a suposição global de escuro fixo. Não alterar geometria, fontes ou cores ANSI durante a auditoria.
- Evitar regressão por snapshots antigos: versionar a chave e o snapshot visual para v2 em `src/types/theme.ts` e `bootstrap.ts`, com rejeição explícita do formato v1. Isso invalida apenas cache visual derivado; não apaga preferências persistidas. Na primeira abertura após atualização pode haver fallback escuro até a hidratação de um tema claro salvo; minimizar esse intervalo e registrar essa limitação, sem inventar migração de banco.

### Janela nativa

- Criar um adaptador pequeno, proposto em `src/theme/nativeWindow.ts`, para aplicar a polaridade pela API Tauri instalada. Não executar IPC nativo em navegador puro: verificar disponibilidade Tauri pela API instalada antes da chamada.
- Acioná-lo na camada de aplicação (`useThemeApplier`), inicialmente com a polaridade de bootstrap do DOM e depois com `effectiveResolved.uiTokens.polarity`. Não inserir efeitos nativos na função pura de montagem de variáveis CSS.
- Deduplicar polaridades já aplicadas com sucesso e serializar/reconciliar alterações rápidas: o último tema deve vencer também na janela. Uma falha não pode marcar o valor como aplicado nem impedir a troca de CSS/terminal; registrar erro pelo mecanismo existente do aplicativo, sem rejeição não tratada e sem retry em loop.
- Adicionar somente `core:window:allow-set-theme` à capability da janela `main`. Preservar permissões existentes, decoração, drag, botões nativos e comportamento de bandeja. Não editar schemas gerados.

### Configurações gerais e bibliotecas

- Em `PreferencesDialog.vue`, usar `width="calc(100vw - 50px)"` e `max-width="1500px"` no MaxModal existente. O retângulo efetivo deve medir `min(viewport − 50px, 1500px)`.
- Estilizar o diálogo teleportado pela própria classe `.max-modal.prefs-dialog-modal` com seletor local compatível com a anatomia real, se necessário; o seletor atual descendente `.prefs-dialog-modal :deep(.max-modal)` pode não alcançar o elemento que também recebe a classe.
- Na navegação, alinhar tanto botão quanto `.max-button-label`: conteúdo interno flex, ícone e texto em linha, início à esquerda, gap e largura adequados. Escopar à navegação deste modal. Não modificar o alinhamento global de MaxButton nem outros modais.
- Preservar as seis seções, seleção, estado ativo, teclado, foco, Escape e rolagem. Manter conteúdo com `min-width: 0` quando necessário para não forçar overflow na largura mínima suportada.
- Usar **MaxComponentsUi** nos controles, **MaxPinia** já registrado nas stores existentes e **MaxUse** para helpers/composables disponíveis que sejam necessários. Não introduzir Pinia paralelo, biblioteca visual alternativa, PrimeVue, Axios novo, HTTP de configuração ou cache/autosave concorrente. Esta correção visual não exige novos endpoints nem stores.

## 4. Etapas e matriz de aceite

Executar em sequência. Implementar todo o bloco e seus testes antes da validação em lote. Não há delegação prevista.

| ID / requisito | Arquivos e trabalho | Testes pertinentes | Evidência de conclusão |
| --- | --- | --- | --- |
| T1 — contrato e bootstrap coerentes | `src/styles/theme.scss`, `src/theme/bootstrap.ts`, `src/types/theme.ts`; manter `cssTokens.ts` compatível. Definir aliases, corrigir fallback, versionar cache visual. | Ampliar `tests/theme/cssTokens.spec.ts` e `bootstrap.spec.ts` com fixtures de claro e escuro reais, cache vazio/corrompido/v1/v2 e troca nos dois sentidos. Não apenas trocar `polarity` de uma fixture escura. | Bootstrap e hidratação usam a mesma orientação; `.dark`, dataset e color-scheme corretos; cache antigo rejeitado sem apagar configurações. |
| T2 — interface integral | `src/app/App.vue`, `src/components/shell/AppShell.vue`, `src/components/layout/AppLayout.vue`, `src/components/preferences/PreferencesDialog.vue` e consumidores de cores em `src/components`, `src/styles` e demais trechos de `src` encontrados pela auditoria. Ler contexto; separar superfícies, textos, bordas e cores intencionais de terminal/status. | Regressão de tokens/componentes usando fixtures canônicas; preservar `tests/theme/terminalBridge.spec.ts` e ampliar `tests/stores/theme.spec.ts` para claro/escuro, prévia/cancelamento e troca de contexto. | Shell, projetos, abas, issues, terminais/blocos, inputs, seis seções de configurações, modais, popovers e menus teleportados têm polaridade coerente; hover, seleção, foco e erros legíveis. |
| T3 — barra de títulos | Novo `src/theme/nativeWindow.ts`, `src/composables/useThemeApplier.ts`, `src-tauri/capabilities/default.json`; integrar sem mudar IPC de temas. | Novo `tests/theme/nativeWindow.spec.ts` e teste de integração do applier em `tests/composables`: mocks de janela, bootstrap, deduplicação, transições rápidas, prévia/cancelamento, erro e navegador sem Tauri. | API recebe a polaridade efetiva e converge ao último valor; permissão mínima; barra realmente acompanha escuro/claro no CachyOS. |
| T4 — modal e alinhamento | `src/components/preferences/PreferencesDialog.vue`; props de dimensões e SCSS escopado ao diálogo/label reais. | Ampliar `src/components/preferences/__tests__/PreferencesDialog.spec.ts`: props/estilos do diálogo real, seis itens, ícone/texto no label, navegação e fechamento. | Larguras de 910px em viewport 960, 1390px em 1440 e 1500px em 1920, tolerância de 1px; itens à esquerda com ícone ao lado e sem corte. |
| T5 — fechamento | Revisar diff e completar testes de T1–T4; registrar execução e pendências. | Gate do projeto e checagem visual/nativa descritos abaixo. | Evidências vinculadas à revisão factual; nenhum aceite visual inferido exclusivamente de happy-dom. |

## 5. Preflight e validação futura

### Antes de implementar

1. Ler este plano na admissão; confirmar `pwd`, branch e `git status --short`, sem alterar branch. Relê-lo se houver revisão. Identificar trabalho já realizado e preservar edições de terceiros.
2. Conferir por leitura `package.json`, lockfile, `node_modules/@maxvue/*`, exports/artefatos efetivamente consumidos e versões da API Tauri. Não executar `sync:local-libs` em modo de escrita ou compilar/publicar bibliotecas irmãs para contornar indisponibilidade. Recurso ausente é bloqueio de ambiente, não autorização de instalação.
3. Inventariar por busca todas as cores de superfície/texto em `src` (SCSS, estilos inline e utilitários UnoCSS). Classificar pelo papel visual, ler os contextos e fechar a lista de arquivos de T2 antes da edição. Incluir estados vazios/erro, drawers e sobreposições; não trocar números cegamente.
4. Para testes, usar stores/Pinia novos por caso com MaxPinia registrado quando aplicável, mocks IPC e storage em memória limpo entre casos. Se o plugin ativar cache, configurar namespace de teste isolado conforme API instalada. Não acessar SQLite, cache, IndexedDB ou localStorage reais do usuário. Axios não precisa ser inicializado neste fluxo. Não copiar `.env`.
5. Para teste nativo futuro, ler a configuração de desenvolvimento e o isolamento de dados existente antes de iniciar qualquer app. Se não existir caminho seguro já suportado para evitar PTYs, portas ou banco compartilhados, manter a validação nativa pendente e pedir somente a autorização/ambiente indispensável. Não inventar variáveis de ambiente de isolamento.

### Lote automatizado

- Após implementar todo o escopo e seus testes, executar **`npm run check`**, comando canônico confirmado em `package.json`/`scripts/check.mjs`. Ele já agrega verificações de bibliotecas locais, ESLint, Stylelint, tipos TS/Vue, Vitest com cobertura, build e fmt/clippy/testes Rust. Não repetir os componentes sobre a mesma revisão.
- Para diagnóstico de falha, o projeto permite `npm test -- tests/theme tests/stores/theme.spec.ts src/components/preferences/__tests__/PreferencesDialog.spec.ts`; acrescentar os testes novos do applier ao recorte pertinente. Registrar diagnóstico sem tratá-lo como substituto do gate completo. Corrigir falhas em lote e repetir apenas etapas invalidadas/faltantes; o script para no primeiro erro.
- Não há comando E2E funcional confirmado no package. A configuração `e2e/frontend/wdio.frontend.conf.ts` aponta por padrão para porta 1420, diferente da 5177 do Vite, e não basta para prometer automação instalada. Não instalar runner para este escopo; validar visualmente com recurso disponível ou registrar pendência explícita.

### Matriz visual e nativa

- Temas: `max-navy-teal`, `max-light` e um tema adicional escuro já presente no catálogo, sem importar tema em produção só para teste. Repetir escuro → claro → escuro, prévia/cancelamento, global/projeto e seguir sistema quando habilitado.
- Percorrer superfícies de T2; abrir modal/popover teleportado após cada troca e com ele já aberto. Conferir textos, placeholders, bordas, hover, foco, disabled, ativo e status; texto comum deve manter contraste mínimo de 4,5:1, sem mudar as cores ANSI escolhidas pelo usuário.
- Abrir configurações nas três larguras de T4 e altura mínima suportada, conferir retângulo efetivo, alinhamento de cada item, foco por teclado e rolagem interna. Happy-dom comprova contratos/DOM, não layout ou cascata completos.
- No aplicativo Tauri Linux, conferir barra de títulos e controles em ambas as polaridades, inclusive na abertura e após troca rápida. Registrar efeitos/limitações do compositor. Troca de tema não pode reiniciar terminal nem alterar sessões.
- Não declarar verde com gate falhando, trecho não executado ou requisito nativo pendente.

## 6. Riscos e recuperação

- **Inversão dupla:** biblioteca e backend já usam superfícies baixas; corrigir o consumidor, não a rampa recebida. Comparar fallback com o contrato canônico, sem alterar o Rust.
- **Alcance da auditoria:** o pedido é global; corrigir somente Configurações gerais ou AppShell é insuficiente. Limitar mudanças aos papéis de cor e geometria solicitada, sem refatoração funcional incidental.
- **Cascata/Teleport:** garantir que overrides alcancem o diálogo e label reais sem vazar para outros componentes; conferir estilos computados em navegador.
- **Cache visual v1:** invalidar apenas snapshot derivado; preservar preferências, projetos, SQLite e catálogo. Aceitar e documentar o breve fallback na primeira hidratação após atualização, não como inversão permanente.
- **Decoração Linux:** `setTheme` é app-wide; não modificar tema do sistema, GTK global ou outros aplicativos. Em bloqueio comprovado do compositor, registrar requisito pendente e solicitar decisão antes de ampliar para barra personalizada.
- **Recuperação:** não há migração de dados. Uma correção posterior deve se limitar ao diff desta entrega, com autorização, sem reset/checkout destrutivo ou descarte de trabalho alheio.

## 7. Registro de execução e retomada

Somente durante **Execute**, manter `docs/plans/wt-33b72513/execution-state.json`; não criar esse arquivo em Plan Mode. Contrato **version 2 estrito**, obtido de `docs/plans/wt-e42cace3/plan.md:158–176`:

- Raiz, sem chaves adicionais: `{ version: 2, planRevision, tasks, verification, updatedAt?, nextAction? }`.
- `planRevision`: SHA256 real dos bytes deste `plan.md`.
- `tasks`: array de `{ id, status, files, validations, revision?, nextAction? }`, usando IDs T1–T5 e `validations: []` obrigatório; checks ficam somente em `verification.checks`.
- `status`: `pending | running | completed | blocked | manualPending | validationFailed`.
- `verification`: `{ revision, checks }`; cada check: `{ command, revision, result, exitCode? }`.
- `result`: `passed | failed | pending`. `passed` exige `exitCode: 0`; `failed` exige inteiro não zero; `pending` omite exitCode. Nunca usar null, código inventado ou resultado inferido.
- `command`: comando realmente executado, até 4096 caracteres. Registrar falhas e gates interrompidos; comando planejado ainda não executado não conta como evidência de sucesso.
- Revisões de tarefas/checks/verificação: SHA256 real de manifesto determinístico contendo caminhos relativos ordenados e SHA256 dos conteúdos correspondentes; registrar o método no handoff em texto, sem adicionar campo ao JSON. Não confundir SHA do Git com SHA256 factual.
- `updatedAt`, quando presente: inteiro positivo em milissegundos. `nextAction`, quando presente: até 512 caracteres, com próxima ação e motivo da pendência.
- Não incluir `schemaVersion`, `worktree`, `branch`, `head`, `planPath`, `status`/`checks` soltos na raiz, `title`/`evidence` em tarefas ou objetos extras de diagnóstico.

Atualizar após marcos e no handoff. Checks vazios, falha vigente ou teste visual ausente impedem validação integral. Usar `manualPending` para validação nativa/visual indisponível, sem simular conclusão. Na retomada, conferir hashes e trabalho existente, reaproveitar evidências apenas para conteúdos iguais e executar lacunas. Mudanças relevantes invalidam checks dependentes. Integração e limpeza desta worktree pertencem ao MaxCode/usuário.

## 8. Revisão do plano

Escopo cobre tema global, barra nativa e os dois ajustes do modal. A solução mantém contratos Rust, biblioteca visual e persistência, identifica a divergência real das rampas e evita repetir orientação histórica incompatível com a biblioteca atual. Plano não autoriza implantação, alteração de bibliotecas irmãs ou integração Git. Nesta sessão, a única escrita permitida é este documento.
