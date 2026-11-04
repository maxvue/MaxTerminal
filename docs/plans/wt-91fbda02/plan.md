# Plano de execução — Gestão de temas e substituição de AppShell

## Identificação e retomada

- Data: 2026-11-04. Projeto: MaxTerminal. Worktree/branch esperada: `wt-91fbda02`.
- Raiz: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-91fbda02`.
- Especificação: `docs/plans/wt-91fbda02/spec.md`.
- Ler este plano na primeira admissão. Em retomadas, consultar etapas pertinentes, conferir trabalho realizado e revalidar conteúdo/alterações locais antes de editar. Não reabrir decisões sem evidência nova ou mudança de requisito.
- Usar somente esta worktree; não criar outra, trocar branch, commitar, integrar ou publicar. Não modificar bibliotecas compartilhadas. Se houver bloqueio nelas, registrar notificação sanitizada e próxima ação.

## 1. Pedido, resultado e restrições

Entregar gestão de temas em Configurações Gerais → Aparência & Temas e substituir AppShell por MaxApp `blank-mode` com paleta `colors`. Público: usuários do terminal que personalizam temas globais e por projeto.

O CRUD e o MaxApp já estão presentes. A implementação futura deve reaproveitá-los, completar a remoção de AppShell e corrigir as lacunas de integração descritas abaixo. Não criar endpoints, tabelas ou uma segunda implementação de temas.

Decisões de preservação: nativos somente leitura; duplicação livre; criação/duplicação sem ativar automaticamente; salvar explicitamente e cancelar sem persistir; manter importação/exportação, tipografia, sincronização com SO, overrides por projeto, preview temporário e SQLite como fonte persistente. Não alterar processos/PTYs ao trocar cores.

SFCs: template → script setup TypeScript → style SCSS scoped, quatro espaços, aspas simples, ponto e vírgula, sem vírgula final e tags de abertura em linha única. Componentes MaxComponentsUi, stores MaxPinia e helpers MaxUse. Textos de interface e comentários em pt-BR.

## 2. Evidências e limites da investigação

| Fonte existente | Constatação |
| --- | --- |
| `src/components/preferences/PreferencesDialog.vue` | Registra AppearancePreferences na categoria appearance. |
| `src/components/preferences/appearance/AppearancePreferences.vue:17-57,148-229` | Novo Tema, ações do catálogo, editor e confirmação já ligados; após salvar faz `themeList()` diretamente e altera estado externo. Exclusão fecha confirmação antes de concluir, sem estado de operação. |
| `ThemePicker.vue` e `ThemeSwatch.vue:23-28` na mesma pasta | Busca, filtros, preview e botões; nativo não edita/exclui, mas duplica; ações interrompem propagação de seleção. |
| `ThemeEditorDialog.vue`, `src/stores/themeEditor.ts` | Editor usa MaxColorPicker e MaxUse; store declara contrato MaxPinia, porém possui fallback Axios direto e usa recibos globais do adapter/resultado anterior. |
| `src/stores/theme.ts:105-125,359-439` | Catálogo carregado via MaxUse/IPC, cache Map de resolvidos e ações CRUD; não declara `isCached/options` para o catálogo MaxPinia. |
| `src/ipc/themeMaxPiniaAdapter.ts` | Rotas virtuais de catálogo/esquema, GET e POST/PUT convertidos para IPC; não são endpoints HTTP reais. |
| `src-tauri/src/commands/themes.rs` | Todos os comandos CRUD já registrados no módulo; mutações usam spawn_blocking. |
| `src-tauri/crates/mt-themes/src/service.rs:461-621` | Persistência precede eventos; bloqueio de nativos, nomes únicos e duplicação; exclusão com fallback. |
| `src-tauri/crates/mt-db/src/repos/themes.rs:195-212` | Exclusão redefine preferências de modo e remove overrides; projetos passam a herdar tema global. |
| `src/app/App.vue:1-27` e `src/components/shell/AppShell.vue` | MaxApp já envolve shell; somente slot workspace usado na aplicação; shell fornece tamanho/overflow e atributo loading. |
| `src/components/layout/AppLayout.vue:79-100` | Splitpanes depende da altura de 100% do ancestral. |
| `src/app/plugins.ts`, `src/app/router.ts` | Configuração offline, plugin MaxPinia com Axios adaptado e rotas nomeadas já disponíveis. |
| `src/theme/maxAppPalette.ts`, `src/theme/cssTokens.ts`, `src/composables/useThemeApplier.ts` | Paleta derivada, tokens canônicos, sincronização de projeto/SO e janela nativa já implementados. |
| `package.json`, `vite.config.ts`, `vitest.config.ts`, `scripts/check.mjs`, `tests/setup.ts` | Scripts de validação, dedupe de Vue/Pinia/Axios, happy-dom, mocks IPC e stub de teleport. |

Bibliotecas lidas por links reais: MaxComponentsUi, MaxPinia e MaxUse em `/home/johnattas/GitHub`. MaxComponentsUi declara versão 1.1.36 e exporta artefatos dist. Fonte e tipos dist confirmam props solicitadas. O entrypoint dist de MaxApp reexporta chunk; a equivalência completa do bundle não foi certificada nesta sessão e faz parte do preflight futuro.

MaxPinia `src/plugin.ts` ignora stores sem `isCached`, reinicializa `data` e carrega cache/servidor; `saveInServer` captura erros em `status.server.save` em vez de necessariamente rejeitar. Testes devem montar Vue com plugin efetivamente instalado, não somente chamar `setActivePinia`. Não confundir cadastro do plugin com sua execução.

MaxComponentsUi `src/components/MaxApp.vue`: exige rota nomeada, fornece slot blank e singletons; `controlledTheme` não bloqueia `colors`. Store `useColors.Store.ts` observa paleta e gera CSS/cache. MaxColorPicker e helpers MaxUse `_.cloneDeep`, `_.isEqual`, `getColorFromVar` existem; não foi identificada necessidade de componente/helper novo na biblioteca.

Context7 foi consultado em `/maxvue/maxcomponentsui`; retornou documentação antiga sem blankMode/colors. Prevalecem fonte e contrato local efetivo. Divergência registrada em notificação específica.

Estado inicial: `git status --short` vazio; diretório de plano ainda inexistente. `.gitignore` já contempla worktrees. Nesta sessão não houve testes, instalações, builds, leitura de banco de produção ou execução nativa. Nenhuma falha de runtime foi reproduzida; riscos de concorrência e cascata CSS devem ser verificados, não tratados como incidentes comprovados.

## 3. Contratos e escolhas de implementação

### A. Catálogo e editor

1. Criar `src/stores/themeCatalog.ts` como store MaxPinia cacheada com `data: ThemeSummary[]`, default vazio, GET em `THEME_CATALOG_VIRTUAL_ROUTE`, sem autosave. Expor `refreshCatalog(): Promise<ThemeSummary[]>` e `replaceCatalog(themes: ThemeSummary[]): void`.
2. `theme.ts` mantém API usada pelos componentes e delega catálogo à store. Eventos e refresh atualizam a mesma coleção, evitando cópias divergentes. Após mutação, invalidar/recarregar catálogo e cache resolvido afetado. Evitar que resposta anterior sobrescreva catálogo recebido de evento mais recente; serializar refresh ou usar geração de requisição no domínio, além das proteções do plugin.
3. Retirar GET direto de `AppearancePreferences.onThemeSaved`: aplicar resolved retornado e chamar ação de refresh da store. Falha de refresh após gravação confirmada é erro de atualização do catálogo, não falha da mutação; não reenviar gravação.
4. Editor mantém `ThemeDraftInput` e salvamento manual com `block_save`. Usar exclusivamente a instância MaxPinia configurada para enviar. Se plugin/adapter não estiver instalado, falhar claramente; não mascarar configuração com Axios alternativo. Adaptar harness para esse contrato.
5. Confirmar que get da edição usa a forma efetivamente suportada pelo plugin: objeto `options.get.data` com campo `id` reativo, não depender de iteração de um computed objeto pelo plugin. Isolar a identidade do editor por tema/modo e impedir hidratação atrasada de sobrescrever rascunho aberto. Preservar formato de `ThemeEditorState`.
6. O resultado de submit deve ser da solicitação atual: zerar recibo antes, verificar status e resultado; não usar resultado de GET ou save anterior como prova. Como há um editor único, bloquear submissão reentrante na store e ações enquanto salva. Se for necessário ajustar adapter, manter as rotas e payload `{kind, id, input}` atuais e correlacionar o retorno à solicitação; não ampliar backend.

### B. Interação

- Reutilizar MaxButton/MaxIconButton, MaxModal, MaxAlert, MaxInputText/Select/Number e MaxColorPicker já presentes.
- Novo tema nasce do esquema efetivo conforme código atual; cópia mantém todas as cores, incluindo indexadas e alfa.
- Não ativar ao editar/duplicar/excluir pela propagação do cartão. Teclado e foco devem operar os botões individualmente.
- Impedir repetição de duplicação/exclusão enquanto operação está em curso; desabilitar/indicar loading nos controles pertinentes. Limpar estado em finally.
- Manter confirmação aberta e alvo identificado durante delete; fechar após sucesso; erro permite tentar novamente ou cancelar. Usar o alvo real do catálogo, sem inventar ThemeSummary quando o ID desapareceu.
- Corrigir mensagem de impacto de exclusão: ativo global retorna a Navy/Teal quando aplicável; preferências escuro/claro retornam aos respectivos padrões; projetos perdem override e seguem global. Nenhuma alteração na transação Rust.
- Cancelamento/fechamento do editor sem submit descarta alterações em memória na próxima abertura. Não introduzir autosave de tema durante digitação.

### C. MaxApp e paleta

- `App.vue` deve renderizar conteúdo diretamente no slot blank, com um único contêiner local de viewport. Remover import e uso de AppShell. Transferir apenas estilos/atributos necessários à altura, flex, overflow, fundo/texto e loading; não recriar slots vazios nem titlebar redundante.
- Manter `blank-mode no-login controlled-theme :colors="maxAppColors"`, router/configuração offline e todos os hosts existentes. Não adicionar outro MaxToast. Não passar styles ao MaxApp esperando herança automática: ele tem múltiplas raízes.
- Manter `resolveMaxAppColors` como adaptador entre domínio e PartialColorPalette. Preservar prioridade effectiveResolved → bootstrap → fallback, mapping `warn` e magenta/violet. Garantir cores reativas após editar tema ativo, escolher tema, preview/cancelamento, exclusão e troca de projeto.
- Preservar tokens canônicos inline e sua polaridade; CSS gerado pela biblioteca complementa o domínio. Não remover indiscriminadamente `applyMaxUiTokensToRoot` nem tornar o cache de paleta da biblioteca fonte de seleção ativa. Testar valor efetivo no navegador, pois happy-dom não comprova toda cascata/layout.
- Excluir `src/components/shell/AppShell.vue` somente após busca global por consumidores e atualização dos testes. Preservar testes de AppTitleBar/AppEmptyState que compartilham arquivo de teste.

Alternativas descartadas: CRUD novo no Rust (já existe), segundo shell (não atende substituição), mudança dos temas builtin (fora do pedido), autosave no editor (quebra cancelar), uso de documentação remota antiga como contrato, alteração de bibliotecas externas para contornar testes.

## 4. Etapas executáveis

### T1 — Preflight e contratos

Por leitura: conferir branch/status, arquivos do plano, scripts/pre-post hooks, symlinks/exports/dist de bibliotecas e contratos MaxPinia efetivos; revisar `scripts/sync-local-libs.mjs` antes do gate do check. Verificar identidade única de Axios/Pinia/Vue no harness e isolamento de cache/LocalForage, storage e IPC. Confirmar perfil isolado para aceite nativo, incluindo banco, logs e diretórios XDG; worktree sozinha não isola dados.

Saída: contratos confirmados ou tarefa blocked com próxima ação concreta. Integridade de biblioteca falhada bloqueia validação dependente; não usar ignoreConfig, comando direto para contornar gate, remoção de hook ou instalação/build externo não autorizado.

### T2 — Consolidar gestão de temas

Arquivos: novo `src/stores/themeCatalog.ts`; existentes `src/stores/theme.ts`, `src/stores/themeEditor.ts`, `src/ipc/themeMaxPiniaAdapter.ts` se necessário, `AppearancePreferences.vue`, `ThemePicker.vue`, `ThemeSwatch.vue`, `ThemeEditorDialog.vue` na pasta appearance conforme necessidade dos estados.

Implementar contratos A/B completos, reaproveitando todos os controles e IPC. Acrescentar testes de catálogo, plugin real instalado, cancelamento, sucesso, erro, duplo acionamento, eventos e não ativação. Não alterar backend para satisfazer mocks.

### T3 — Concluir migração da raiz

Arquivos: `src/app/App.vue`, remoção condicionada de `src/components/shell/AppShell.vue`, ajuste de `tests/components/shell/AppShell.spec.ts`; `tests/app/toastIntegration.spec.ts`, `tests/theme/maxAppPalette.spec.ts` para regressões pertinentes. Alterar `src/theme/maxAppPalette.ts`/estilos somente se os critérios revelarem lacuna concreta.

Implementar contrato C e testes de blank/props/hosts, estados de boot/erro/retry/recuperação e paleta. Preservar playground e layout de duas colunas. Após remoção, busca por AppShell deve apontar apenas documentação histórica, nunca imports vivos.

### T4 — Completar testes e revisão

Estender testes existentes em `tests/components/preferences/appearance/{AppearancePreferences,ThemeEditorDialog,ThemePicker,ThemeSwatch}.spec.ts`, `tests/stores/theme.spec.ts` e `tests/ipc/themeMaxPiniaAdapter.spec.ts`. Novos propostos: `tests/stores/themeCatalog.spec.ts` e, se necessário, `tests/stores/themeEditor.spec.ts` para integração do plugin/concorrência. Fixtures devem conter objeto ANSI real de 16 campos, não array.

Antes de rodar suítes, terminar todo o escopo e testes. Revisar em conjunto o contrato store→adapter→IPC, feedback pós-gravação e diff de layout. Revisão independente nos marcos de risco e entrega, sem revisões a cada microedição. Não há delegação ativa; execução sequencial evita conflito em stores. Eventual paralelização futura somente após autorização e contratos fixos, com arquivos de escrita distintos.

### T5 — Validar e entregar

Executar `npm run check` na raiz da worktree após T1–T4. O script já inclui gate local-libs, lint, lint:style, typecheck, typecheck:vue, test:coverage, build, cargo fmt --check, clippy e testes Rust. Não repetir esses comandos individualmente se já aprovados na mesma revisão. Como o script interrompe na primeira falha, registrar etapa e código real; diagnosticar falhas em lote, corrigir causa e executar etapas faltantes/afetadas sem burlar gate de integridade.

Realizar aceite manual em perfil de dados isolado e autorizado. Se não houver isolamento comprovado ou Tauri disponível, manter manualPending e indicar preparação necessária. Não copiar .env, migrar/resetar banco compartilhado nem executar scripts de desenvolvimento antes de inspecionar seus efeitos. Nenhum destes comandos foi executado no Plan.

## 5. Matriz de aceite por tarefa

| Tarefa/requisito | Evidência de aceite e testes | Revisão/aceite manual |
| --- | --- | --- |
| T1: integridade | Contratos fonte/dist, plugin instalado, Axios adaptado, gate local-libs e storage isolado | Bloquear dependentes se ambiente inconsistente. |
| T2: adicionar/editar | Criar e alterar cor/nome; IPC uma vez; nome vazio/conflito/cor inválida/baixo contraste mantêm rascunho; cancelar não escreve; indexedColors preservado | Reiniciar perfil isolado e reencontrar tema salvo. |
| T2: duplicar | Cada origem produz cópia custom e não ativa; duplo clique durante pending não duplica novamente | Nome/ID distintos, original intacto. |
| T2: excluir | Cancelar não escreve; falha mantém confirmação; ativo global/preferências/overrides reconciliados; item removido some do catálogo | Projeto afetado segue global; nenhum dangling ID após reinício. |
| T2: MaxPinia | Teste com Vue+plugin efetivo e adapter; erro em status não é sucesso; cache/eventos/refresh não repõem catálogo antigo; GET tardio não sobrescreve rascunho | Não há HTTP para rotas virtuais nem fallback que esconda plugin ausente. |
| T3: MaxApp | Sem AppShell, blank/no-login/controlled-theme/colors, único toast, erro/retry/recovery/playground preservados | Janela pequena/maximizada, divisórias, scroll do catálogo e modal, foco/teclado. |
| T3: cores | Paleta atualiza ao selecionar/editar/preview/cancelar/excluir; bootstrap/fallback e modo claro/escuro | Verificar cores computadas, controles Max, terminal aberto e overrides sem reiniciar PTY. |
| T4/T5: regressão | Check canônico completo, todas as falhas e resultados vigentes registrados | Não declarar conclusão integral sem aceite nativo. |

## 6. Riscos e operação

- Layout de MaxApp tem altura mínima, enquanto Splitpanes exige altura definida: aceite visual obrigatório.
- Inicialização assíncrona MaxPinia pode substituir data; editor aberto deve impedir sobrescrita pelo cache/GET anterior. Não gravar rascunho automaticamente.
- Uma mutação pode persistir antes de refresh falhar: separar feedback e não reenviar mutações automaticamente. Eventos não substituem confirmação local de resultado.
- Inline tokens e CSS de paleta coexistem: verificar contraste/cores em ambos os modos; não tratar eventual diferença como autorização para remodelar tema.
- Nenhuma migração prevista. Reversão futura é reverter apenas mudanças desta tarefa com autorização, preservando temas já salvos no schema atual. Exclusão continua definitiva e depende da confirmação existente.
- Ausência de ambiente nativo não equivale a aprovação por mocks. Não registrar conclusão total com bloqueio ou validação manual pendente.

## 7. Registro de execução e handoff (somente Execute)

Durante Execute, registrar após marcos em `docs/plans/wt-91fbda02/execution-state.json`, vinculado ao SHA256 real do conteúdo de `plan.md` e revisões factuais das tarefas. Não criar esse arquivo na sessão Plan.

Contrato version 2 estrito, sem chaves adicionais:

- Raiz: `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`. `updatedAt`, quando presente, inteiro positivo em milissegundos, não ISO. Proibidos schemaVersion, worktree, branch, head, planPath, status, checks solto, blocker, diagnostic, branchSync, durableSync e followUp.
- Tarefa: `{id, status, files, validations, revision?, nextAction?}`. `validations: []` obrigatório; não adicionar title/evidence. Estados: pending/running/completed/blocked/manualPending/validationFailed. Resultados de validações: apenas passed/failed/pending. Para registros estruturados de validations, confirmar contrato do executor antes de acrescentar estrutura não definida aqui; manter checks executados no local canônico abaixo.
- `verification`: `{revision, checks}`. Cada check: `{command, revision, result, exitCode?}`. Commands até 4.096 caracteres; hashes SHA256 reais do conteúdo/revisão, nunca rótulos. `result` apenas passed/failed/pending; passed exige exitCode 0, failed exige inteiro não zero, pending não admite exitCode. Nunca exitCode null.
- `nextAction` até 512 caracteres, contendo motivo e próxima ação em bloqueios/pendências.

Registrar somente comandos executados, incluindo todas as falhas e falhas da suíte completa; não inferir exit code por `tool:end.ok`. Checks vazios, resultado ausente ou falha vigente impedem aprovação integral. Version 1 permanece compatível sem provar validação automaticamente. Registro autodeclarado não comprova execução nativa nem verifica comandos omitidos.

Retomada executa lacunas e reaproveita checks aprovados sem mudança relevante; falha repetida sem progresso exige diagnóstico agrupado do harness/causa. Trabalho assíncrono aguarda notificação ou conclusão com espera limitada/recuo, sem polling repetitivo. Cancelamento conserva resultado parcial e pendências explícitas.
