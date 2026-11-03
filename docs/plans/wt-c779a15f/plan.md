# Tema escuro integrado e ajustes das configurações gerais

- Projeto: MaxTerminal.
- Data: 03/11/2026.
- Worktree/branch: `wt-c779a15f`.
- Raiz exclusiva: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-c779a15f`.
- Estado: planejamento; nenhuma implementação ou validação executada.

## 1. Objetivo e limites

Corrigir a aplicação do modo escuro conforme o sistema, abrangendo interface, componentes Max, terminais e barra de títulos nativa. Ajustar o modal **Configurações gerais** para largura `calc(100vw - 50px)`, máximo `1500px`, com itens da coluna esquerda alinhados à esquerda.

Preservar seleção manual de tema, sincronização configurável com o sistema, temas por projeto, pré-visualização temporária, importação, tipografia, sessões e navegação das seis seções existentes. Adota-se o controle já existente `appearance.followSystem`: quando habilitado, aplicar imediatamente a preferência atual e acompanhar mudanças; quando desabilitado por configuração persistida, respeitar o tema escolhido. Não transformar a correção visual em redefinição das preferências do usuário. O padrão atual é `false`; este plano não o altera nem migra configurações existentes.

Fora do escopo: novo seletor de tema, redesenho da aplicação, migração de banco, mudanças nas bibliotecas irmãs, atualização de dependências, reimplementação da barra de títulos, alterações no sistema operacional, commit/merge/push, troca de branch ou criação/remoção de worktree.

## 2. Evidências e diagnóstico

### Fluxo de tema

- `src/app/main.ts`: executa `applyThemeBootstrapSync()` antes da montagem.
- `src/app/App.vue`: instala `useThemeApplier()` na raiz.
- `src/composables/useThemeApplier.ts`: observa `followSystem` e eventos de `matchMedia`, mas não aplica explicitamente a preferência atual após concluir `themeStore.init()`. O observador depende de uma mudança de valor; existe lacuna quando a preferência já chega habilitada ou há variação de timing. Não há sincronização da decoração nativa.
- `src/stores/theme.ts`: store existente com cache de temas resolvidos, IPC, preferências persistidas, preview e overrides por projeto. `init()` hidrata aparência e tema; `applyResolvedToUiAndTerminals()` atualiza DOM, terminais e snapshot. `setActiveTheme()` global aplica o tema retornado diretamente: considerar isso ao preservar o override do projeto selecionado durante atualização do tema global.
- `src/app/plugins.ts`: Pinia já recebe `createMaxPinia()`. Reutilizar essa infraestrutura, sem store paralela, Axios novo ou cache concorrente.
- `src/theme/cssTokens.ts`: escreve tokens inline na raiz, `.dark`, `data-theme-polarity` e `color-scheme`.
- `src/theme/bootstrap.ts`: snapshot versão 1 e fallback escuro; tokens `--background-0` branco e `--background-950` escuro, inclusive no tema escuro. Não inverter globalmente essa escala: o frontend atual depende dela.
- `src/styles/theme.scss`: superfícies principais usam `--background-900` e texto `--background-100`.
- `src-tauri/crates/mt-themes/src/service.rs`, `src-tauri/crates/mt-db/src/repos/themes.rs` e `src-tauri/crates/mt-db/migrations/0006_10_themes.sql`: `followSystem` persistido; padrão desabilitado. Sem necessidade de modificar persistência para esta correção.

### Componentes e modal

- `src/components/preferences/PreferencesDialog.vue`: `MaxModal` com `max-width="56rem"`, sem `width`. Navegação já declara `justify-content: flex-start` e `text-align: left`; somente repetir essas regras não resolve a anatomia interna do botão.
- MaxComponentsUi instalado: `1.1.36`; fontes consultadas por `node_modules/@maxvue/max-components-ui/src/components/{MaxModal,MaxButton}.vue` e `src/themes/colors.scss`.
- `MaxModal` aceita `width`/`maxWidth` como strings CSS. `maxWidth` resulta em `min(valor, calc(100vw - 40px))`; com largura pedida e teto de 1500px isso não conflita.
- O diálogo é teleportado para `body`; a classe fornecida é aplicada ao wrapper **e ao próprio** `.max-modal`. O seletor atual `.prefs-dialog-modal :deep(.max-modal)` pressupõe ancestralidade que não corresponde ao diálogo teleportado. É preciso atingir o próprio elemento, não depender do wrapper original.
- `MaxModal` usa fundo `--background-0` e texto `--background-700`; a biblioteca normalmente remapeia sua rampa em `.dark`, mas os tokens inline do aplicativo prevalecem. Essa incompatibilidade pode produzir superfícies claras mesmo com `.dark`; não atribuir todo o problema apenas a `matchMedia`.
- `MaxButton` envolve o slot em `.max-button-label` e centraliza a raiz. Preferir seu contrato `icon`/`label` para navegação, com regras locais de especificidade suficiente na raiz e no label. Não alterar a biblioteca compartilhada.
- Outros consumidores de `MaxModal`: `src/components/projects/ProjectEditModal.vue` e `src/components/projects/ProjectSettingsDialog.vue`. Incluir sua superfície na verificação do modo escuro, sem alterar larguras ou comportamentos desses modais.

### Janela nativa e viabilidade

- `src-tauri/tauri.conf.json`: janela `main`, `decorations: true`, mínimo 960×600. `AppShell.vue` possui slot de título opcional, mas `App.vue` não o preenche: a barra externa não é corrigível apenas por CSS.
- API instalada `@tauri-apps/api` 2.5.0: `window.d.ts` declara `getCurrentWindow().setTheme('dark' | 'light' | null)`. Em Linux/macOS o efeito é global ao aplicativo.
- `src-tauri/gen/schemas/desktop-schema.json` confirma `core:window:allow-set-theme`; `src-tauri/capabilities/default.json` ainda não concede essa permissão.
- Documentação atual consultada via Context7, biblioteca `/tauri-apps/tauri`, fonte `crates/tauri/src/webview/webview_window.rs`: confirma suporte Linux a `set_theme`. `WindowEvent::ThemeChanged` não tem suporte Linux; manter a detecção pela WebView/MaxUse, não migrar para esse evento.
- Decorações dependem de GTK/compositor. Compatibilidade da API está confirmada; resultado visual no CachyOS/Wayland/X11 ainda exige teste nativo. Não alterar tema global GTK/KDE, variáveis de ambiente ou configurações pessoais para mascarar falha.

### Testes e ambiente

- `package.json`: scripts confirmados `lint`, `lint:style`, `typecheck`, `typecheck:vue`, `test`, `test:coverage`, `build`, `check`.
- `vitest.config.ts`: happy-dom, `tests/**/*.spec.ts` e `src/**/__tests__/**/*.spec.ts`; `tests/setup.ts` limpa mocks Tauri após cada teste.
- Testes lidos: `tests/stores/theme.spec.ts`, `tests/theme/cssTokens.spec.ts`, `tests/theme/bootstrap.spec.ts`, `src/components/preferences/__tests__/PreferencesDialog.spec.ts`.
- MaxUse instalado `2.0.1`: subpath público `@maxvue/max-use/vueUseCore` reexporta VueUse. Usar `usePreferredDark` desse contrato para leitura reativa do sistema; não importar diretamente `@vueuse/core`, nem usar `useDark` como segundo escritor da classe raiz.
- Dependências locais estão acessíveis para leitura. Seus `src` não substituem a necessidade de conferir o `dist` efetivamente consumido no preflight de Execute.
- `scripts/check.mjs` agrega verificação das libs locais, lint, stylelint, tipos TS/Vue, cobertura, build e checks Rust. `scripts/sync-local-libs.mjs` possui modo somente leitura `--check`.
- `git status --short` inicial vazio; `.gitignore` já ignora `.worktrees/` e `.max-code-worktrees/`. O destino deste plano não existia.
- Não foram executados testes, build, aplicação gráfica, instalações ou scripts de sincronização. Não há comprovação visual nem baseline de testes nesta sessão.

## 3. Solução e contratos

### 3.1 Sincronização única de tema

1. Manter `useThemeStore` como fonte de verdade, com MaxPinia já instalado. Get/salvamento/cache continuam no fluxo IPC existente, sem adaptar artificialmente a rotas HTTP.
2. Em `useThemeApplier`, obter preferência reativa por MaxUse; após hidratação bem-sucedida, reconciliar explicitamente a preferência atual. Observar alterações do sistema, habilitação de `followSystem` e mudanças dos IDs preferidos. Não depender somente de mudança de `followSystem` para aplicar no boot.
3. Serializar/coalescer mudanças assíncronas para que o estado final corresponda à preferência mais recente; evitar chamadas redundantes quando o ID já está correto. Capturar rejeições sem derrubar a inicialização nem deixar promises não tratadas. Desmontagem deve impedir aplicações tardias e liberar observadores.
4. Preservar precedência existente: preview > override do projeto > tema global. Se necessário, ajustar a reconciliação da store para atualizar a preferência global sem substituir visualmente um override ativo. Cancelar preview deve restaurar o tema efetivo mais recente. Não apagar overrides ou tornar preview persistente.
5. Preservar fallback e compatibilidade do snapshot versão 1. Um snapshot pode refletir uma preferência anterior ao boot: reconciliar após hidratação, sem inventar dados de preferência ausentes no cache nem afirmar ausência absoluta de flash antes do IPC.

### 3.2 Barra nativa

Adicionar adaptador pequeno em `src/theme/nativeWindowTheme.ts` (novo) para encapsular acesso ao Tauri. Observar a polaridade do tema efetivamente exibido e aplicar `setTheme('dark' | 'light')`, incluindo inicialização, troca de projeto e preview/cancelamento. Preferir polaridade resolvida, não o nome do tema. Reutilizar o padrão de detecção de ambiente Tauri encontrado no projeto na admissão; no navegador puro, não chamar IPC nativo.

Conceder somente `core:window:allow-set-theme` à capability da janela `main`. Não conceder `core:window:default` ou permissões amplas. Deduplicar e ordenar atualizações nativas; falha nativa não deve impedir o tema web e deve ser reportada pelo mecanismo de erros existente, sem logs sensíveis. Não introduzir Rust ou IPC customizado quando a API pública já atende.

### 3.3 Superfícies e configurações gerais

- Manter componentes Max. Corrigir no aplicativo a incompatibilidade das superfícies Max com seus tokens: regras globais de compatibilidade estritamente delimitadas aos componentes usados (começar por `.max-modal` e seus cabeçalhos), baseadas na polaridade efetiva e tokens existentes. Não remapear globalmente `--background-*`, editar `node_modules` ou trocar paletas de terminais.
- Usar `src/styles/theme.scss` para as superfícies compartilhadas que precisam alcançar Teleport. Eliminar ou corrigir os seletores locais ineficazes dos diálogos envolvidos, apenas onde necessário. Evitar remendos globais em todos os `button`, `span` ou elementos de `body`.
- Em `PreferencesDialog.vue`, passar `width="calc(100vw - 50px)"` e `max-width="1500px"` ao `MaxModal`.
- Navegação com `MaxButton` e `MaxIcon` conforme API existente: preferir `:icon="sec.icon"` e `:label="sec.title"` no botão em vez do ícone dentro do label; manter eventos e classes atuais. Garantir alinhamento inicial da raiz e texto do label, sem perder foco, seleção, hover ou nome acessível.
- Preservar coluna lateral de 13rem, seis seções, fechamento por Escape/backdrop, restauração de foco e rolagem. Área de conteúdo deve poder encolher (`min-width: 0`/coluna `minmax(0, 1fr)` se necessário), sem forçar overflow horizontal no mínimo suportado de 960px.

## 4. Etapas e matriz de aceite

Executar sequencialmente. Implementar o conjunto completo e seus testes antes de validar em lote; não há necessidade de delegação.

| ID | Requisito e atividade | Arquivos previstos | Testes e evidência de conclusão |
|---|---|---|---|
| T1 | Preflight somente leitura: conferir branch, status, plano, dependências consumidas, detecção Tauri e isolamento | `package.json`, `scripts/sync-local-libs.mjs`, configurações/testes citados | `git status --short`, `git branch --show-current`; confirmar disponibilidade sem instalar/revincular libs; registrar bloqueios reais |
| T2 | Aplicar tema do sistema no boot e em mudanças, respeitando modo manual, preview e projeto | `src/composables/useThemeApplier.ts`; `src/stores/theme.ts` se necessário | Novo `src/composables/__tests__/useThemeApplier.spec.ts`; ampliar `tests/stores/theme.spec.ts`: sistema já escuro na montagem, claro→escuro→claro, habilitação tardia, `followSystem=false`, IDs preferidos, init lento, mudanças rápidas e desmontagem; override/preview permanecem corretos |
| T3 | Sincronizar barra nativa com a polaridade efetiva | Novo `src/theme/nativeWindowTheme.ts`; composable raiz; `src-tauri/capabilities/default.json` | Novo `tests/theme/nativeWindowTheme.spec.ts`: mocks Tauri, dark/light, navegador sem Tauri, deduplicação e rejeição; capability mínima revisada; evidência visual nativa obrigatória |
| T4 | Corrigir superfícies claras dos componentes Max sem inverter tokens | `src/styles/theme.scss`; estilos dos três diálogos somente se necessário | `tests/theme/cssTokens.spec.ts` e `bootstrap.spec.ts` preservados/ampliados; estilos computados em navegador real e inspeção dos três modais no app escuro; terminais mantêm paleta |
| T5 | Dimensões e alinhamento da navegação | `src/components/preferences/PreferencesDialog.vue` | Ampliar `src/components/preferences/__tests__/PreferencesDialog.spec.ts`: props reais de largura, ícones/rótulos, seis seções, seleção e fechamento. Navegador real confirma dimensões e alinhamento; happy-dom não comprova layout |
| T6 | Validar lote e registrar handoff | Todos os anteriores; registro de execução | Gates automatizados aprovados ou falhas discriminadas; testes visuais abaixo concluídos ou `manualPending`; nenhuma integração Git |

## 5. Validação em Execute

### Preflight e isolamento

- Continuar na worktree existente. Não remover alterações encontradas na retomada.
- Confirmar versões e exports reais das bibliotecas locais, Vue/Pinia deduplicados e scripts antes de usá-los. Bibliotecas são recursos compartilhados: apenas leitura; não executar seus builds, lint com fix, instalação ou sincronização mutante.
- Usar IPC mockado e Pinia nova por teste; localStorage e estado DOM isolados com restauração entre testes. Não alcançar MaxAiManager, serviços HTTP, banco pessoal ou cache real.
- Inspecionar o harness nativo antes de iniciar o aplicativo. A validação desktop deve usar diretório de dados isolado suportado pelo projeto; se não houver contrato confirmado, registrar bloqueio de validação manual, sem inventar flag ou copiar `.env`. Não iniciar restaurando workspace pessoal/processos reais.

### Lote automatizado

Depois de implementar todo o escopo, executar `npm run check` na raiz da worktree. É o agregador confirmado e já inclui os gates frontend e Rust; não repetir suas etapas sobre o mesmo conteúdo. Em falha, registrar comando/etapa e exit code, diagnosticar causalidade, corrigir somente o escopo e revalidar o conjunto afetado ao fim do lote. Falhas de baseline ou ausência de dependências não autorizam instalações ou correções externas.

Se o agregador estiver bloqueado por ambiente, os scripts individuais confirmados (`npm run lint`, `npm run lint:style`, `npm run typecheck`, `npm run typecheck:vue`, `npm run test:coverage`, `npm run build`) podem comprovar apenas a parte disponível. Não declarar validação integral nessa situação. Nunca executar `lint:fix` automaticamente.

### Aceite visual e nativo

1. Sistema escuro e sincronização habilitada: abrir com tema global previamente claro; após hidratação, shell, três modais, cabeçalhos, campos, menus utilizados e terminais devem acompanhar o tema escuro, sem painéis brancos indevidos. Barra nativa também escura.
2. Alterar preferência claro/escuro em execução e reiniciar: resultado final coerente com o sistema. Com modo manual desabilitando sincronização, preservar a escolha explícita. Repetir com override de projeto e preview/cancelamento para confirmar precedência.
3. Verificar a barra no CachyOS e registrar sessão gráfica/compositor efetivamente testados. Navegador e mocks não comprovam decoração nativa. Se o gerenciador de janelas ignorar `setTheme`, registrar limitação, não dar aceite silencioso nem substituir por barra própria sem nova autorização.
4. Em viewport 960px, 1440px e 1920px, largura externa do diálogo deve ser respectivamente 910px, 1390px e 1500px (tolerância de 1px); redimensionamento mantém margem total mínima de 50px quando não limitado pelo teto. Medir `.max-modal` real, não wrapper de largura zero.
5. Todos os seis itens à esquerda em estados normal/ativo/hover/foco; ícone e texto com espaçamento consistente, sem centralização no label. Sem overflow horizontal no modal; conteúdo e navegação continuam roláveis.
6. Abrir/fechar por botão, Escape e backdrop; navegar seções e verificar foco restaurado. Terminais continuam vivos sem reiniciar PTYs ou perder output.

## 6. Riscos e recuperação

- **Tema nativo Linux:** aparência depende do compositor e do tema GTK; validação real é gate separado.
- **Escalas de tokens incompatíveis:** não inverter a rampa para corrigir um modal; isso quebraria superfícies do aplicativo e contratos já testados. Compatibilidade deve ficar no consumidor e em componentes delimitados.
- **Teleport/especificidade:** validar DOM real; seletores descendentes do wrapper não alcançam automaticamente o diálogo. Não usar `!important` indiscriminado.
- **Concorrência:** atualização global não pode sobrepor preview/override nem permitir que resposta antiga vença mudança recente.
- **Dados/permissões:** sem migração, novos arquivos de configuração pessoal ou permissões amplas. Preferências persistidas só mudam por ações já existentes no fluxo autorizado; nunca redefinir `followSystem` à força.
- Recuperação por revisão dos arquivos desta entrega; não usar reset/revert destrutivo nem descartar trabalho alheio. Manter worktree para integração pelo MaxCode.

## 7. Retomada e registro de execução

Ler este plano na admissão, conferir estado existente e implementar apenas lacunas. Revalidar evidências se arquivos ou bibliotecas mudarem. Perguntar somente se surgir ampliação indispensável de escopo, como substituição da decoração nativa.

Somente durante Execute, registrar progresso em `docs/plans/wt-c779a15f/execution-state.json`, seguindo o contrato version 2 estrito encontrado em `docs/plans/wt-e42cace3/plan.md`:

- Raiz, sem chaves adicionais: `{ version: 2, planRevision, tasks, verification, updatedAt?, nextAction? }`.
- `planRevision`: SHA256 real do conteúdo deste `plan.md`.
- `tasks`: array de `{ id, status, files, validations, revision?, nextAction? }`; IDs T1–T6; `validations: []` obrigatório, checks no local canônico abaixo.
- Status: `pending | running | completed | blocked | manualPending | validationFailed`.
- `verification`: `{ revision, checks }`. Checks: `{ command, revision, result, exitCode? }`.
- `result`: `passed | failed | pending`; `passed` exige exitCode inteiro 0, `failed` exige inteiro não zero, `pending` omite exitCode. Nunca inventar execução ou exit code.
- Cada `command` é comando efetivamente executado (até 4096 caracteres), identificado nos scripts/configurações do projeto; incluir falhas. Checks pendentes ou vazios não comprovam entrega.
- Revisões: SHA256 real do conteúdo pertinente, por manifesto determinístico de caminhos relativos ordenados e seus hashes; explicar o método no handoff sem adicionar campos JSON. Excluir o próprio registro do conjunto para evitar autorreferência. Invalidar checks dependentes quando o conteúdo mudar.
- `updatedAt`, se usado: inteiro positivo em milissegundos. `nextAction`, até 512 caracteres, comunica próxima ação/bloqueio. Evidência visual fica no handoff, sem inventar chaves extras no JSON.
- Proibidos campos como `schemaVersion`, `worktree`, `branch`, `head`, `planPath`, `status`/`checks` soltos na raiz, `title`/`evidence` em tarefa, ou objetos extras de diagnóstico.

Não criar esse registro em Plan Mode. Nesta sessão, somente este `plan.md` pode ser escrito.

## 8. Revisão do plano

Escopo restrito aos dois pedidos, contratos públicos consultados e caminhos existentes separados dos novos. Preservadas escolhas manuais e funcionalidades fora do pedido. Solução evita editar bibliotecas compartilhadas, migrations e barra própria. Implementação, gates e validação visual permanecem futuros, sem alegação de resultado aprovado.
