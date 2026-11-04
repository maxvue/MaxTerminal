# Plano — gerenciamento de temas e MaxApp em modo livre

- Data: 04/11/2026.
- Projeto: MaxTerminal.
- Worktree/branch: `wt-c4210302`.
- Raiz exclusiva: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-c4210302`.
- Natureza: planejamento; nenhuma implementação ou validação executável foi realizada nesta sessão.

## 1. Objetivo e escopo

Permitir adicionar, editar, duplicar e excluir temas de cores no catálogo de Aparência e envolver a aplicação com `MaxApp` do MaxComponentsUi, usando `blank-mode` e uma prop `colors` derivada do tema efetivo.

### Comportamentos decididos

1. **Adicionar tema:** botão abre editor preenchido a partir do tema efetivo, com nome novo obrigatório. Salvar cria tema `custom`, sem ativá-lo automaticamente. Cancelar não persiste nada.
2. **Editar:** temas `custom` e `imported` podem ser editados no mesmo ID; alterações aparecem imediatamente se esse tema estiver efetivo. Nativos continuam imutáveis: mostrar ação desabilitada com explicação para duplicar antes de editar.
3. **Duplicar:** disponível para qualquer tema, inclusive nativos. Cria cópia `custom` com identidade independente e nome único, como `Nome (cópia)` e sufixo incremental, respeitando 80 caracteres. Preserva cores, cursor e paleta indexada; conteúdo idêntico é permitido nessa operação. Não ativa automaticamente. A cópia fica disponível para edição.
4. **Excluir:** disponível para não nativos, inclusive o ativo, após confirmação. Explicar o impacto sobre tema global, preferências claro/escuro e overrides. Manter política existente: fallback global `max-navy-teal`, preferências retornam aos padrões correspondentes e overrides removidos voltam a herdar o global. Não transformar exclusão em substituição indiscriminada de overrides.
5. Editor com nome, autor opcional, texto/fundo, cursor/cor sob cursor/estilo, seleção com alfa, texto selecionado/acento de aba opcionais e as 16 cores ANSI. Paleta indexada existente deve sobreviver a edição/duplicação sem perda; edição visual dos 240 índices não é necessária neste escopo.
6. Salvar explicitamente; não auto-salvar rascunho inválido. Fechar/cancelar editor mantém catálogo, aparência e bootstrap intactos. Não exigir prévia global de rascunho: preservar a prévia por hover existente, suspendendo-a durante editor/confirmação.
7. `MaxApp` não substitui o layout de terminal: conter `AppShell` e os hosts atuais no slot `#blank`, sem menu, login ou shell web adicional. Não duplicar toast/confirmação globais fornecidos pela biblioteca.

### Preservar e excluir do escopo

Preservar importação/exportação Tabby, filtros, nativos, tipografia, xterm/WebGL, sessões, recuperação de banco, temas por projeto, sincronização com SO, bootstrap sem flash e atalhos/playground. Não reformular layout, migrar todas as stores, editar bibliotecas compartilhadas, adicionar serviço HTTP, copiar `.env`, introduzir instalação/migração ou alterar dados reais para testar.

## 2. Evidências e viabilidade

### Código atual inspecionado

| Fonte | Fato observado e consequência |
| --- | --- |
| `src/components/preferences/appearance/{AppearancePreferences,ThemePicker,ThemeSwatch}.vue` | Catálogo com seleção, hover, exportação e exclusão sem confirmação. A lixeira está oculta para o ativo. Não existem editor/adicionar/duplicar. |
| `src/stores/theme.ts`, `src/ipc/themes.ts` | Store faz IPC direto, cache de resolvidos e aplicação UI/terminais/bootstrap. Já há exclusão/importação/exportação. Evento de catálogo apenas limpa cache; não re-resolve o efetivo. Não há opt-in de cache/Get/Save MaxPinia nessa store. |
| `src/composables/useThemeApplier.ts`, `src/theme/{cssTokens,bootstrap}.ts` | Tema efetivo controla `.dark`, tokens, bootstrap e janela nativa. `cssTokens.ts` produz escalas `--max-*`, `--background-*` e aliases `--mt-*`; não substituir esse pipeline por paleta genérica. |
| `src-tauri/crates/mt-themes/src/{service,scheme,normalize,cache}.rs` | Serviço e modelos completos. Normalização valida cores, nome até 80, contraste mínimo 3:1, deriva polaridade/hash. Cache invalida resolvido ao atualizar esquema. Importador deduplica por SHA; não serve para duplicação intencional. |
| `src-tauri/crates/mt-db/src/repos/themes.rs`, `migrations/0006_10_themes.sql` | Transações e upsert prontos; `origin=custom` permitido. Nome tem índice único NOCASE; SHA não é único. Exclusão atualiza preferências e remove overrides atomicamente. Nenhuma migração é necessária para o escopo previsto. |
| `src-tauri/src/commands/themes.rs`, `src-tauri/src/specta.rs`, `src/types/{bindings,theme}.ts` | Comandos tipados existentes; novas operações precisam entrar no registro Specta e nos bindings gerados, sem edição manual do arquivo gerado. Escritas usam `spawn_blocking`. |
| `src/app/{App.vue,main.ts,router.ts,plugins.ts}` | AppShell e MaxToast próprios; plugin MaxComponentsUi e MaxPinia instalados. Não há `app.use(router)`; `router.ts` só detecta `/dev/terminal`. Axios MaxPinia já tem adapters IPC Horizon/seleção de workspace. |
| `src/stores/workspaceSelection.ts`, `src/ipc/horizonMaxPiniaAdapter.ts` | Padrões locais de opt-in MaxPinia e adapter Axios→IPC reutilizáveis. Não copiar o casamento permissivo por `includes`; novas rotas devem ter correspondência exata. |
| `package.json`, `vite.config.ts`, `vitest.config.ts`, `tests/setup.ts`, `scripts/check.mjs` | Comandos reais e happy-dom; cache Vite local `.tmp/vite`; `check` é a bateria canônica de 10 gates. |
| `src-tauri/crates/mt-themes/tests/service_db.rs`, `tests/theme/`, teste de PreferencesDialog | Há integração SQLite em `TempDir`, sink de eventos e testes de tokens/bootstrap/terminais para extensão. |

### Biblioteca local confirmada (somente leitura)

Fonte: `/home/johnattas/GitHub/MaxComponentsUi`, versão declarada `1.1.36`.

- `src/components/MaxApp.vue` e `dist/components/MaxApp.vue.d.ts`: props `blankMode`, `noLogin`, `controlledTheme`, `colors`; slot `blank`. Conteúdo principal depende de `route.name`, mesmo em blank mode. Usar slot padrão não resolve esse requisito.
- `MaxApp` monta `MaxToast`, `MaxPopoverConfirm` e `MaxLoadScreen`; instancia stores de usuário/login/menus mesmo em blank mode. `noLogin` sozinho **não impede** buscas dessas stores.
- `src/helpers/maxAppConfig.ts`, `src/stores/{useUser.Store,useListMenus.Store}.ts`: rotas configuráveis, defaults web `user.data`, `user.save`, `menus`. Configurar rotas vazias antes de qualquer store do shell: `routeUser`, `routeUserSave`, `routeMenus`; testar ausência de requests. MaxPinia e MaxUse retornam sem GET quando rota é vazia (fontes consultadas: `MaxPinia/src/plugin.ts`, `MaxUse/src/Composables/useRefCachedApi.ts`). Não usar usuário fictício autenticado ou endpoint web para contornar isso.
- `src/types/colors.ts`, `src/stores/useColors.Store.ts`, `src/helpers/colorPalette.ts`: `colors` recebe `PartialColorPalette` de cores-base, não `MaxUiTokens`. Gera estilo `#max-dynamic-colors` com `--primary-*`, `--warn-*`, `--background-*` etc., escala de 0 a 900 e inversão no dark; algoritmo diferente do Rust. A store aplica defaults na criação e guarda CSS por assinatura no localStorage.
- Componentes consultados: `MaxColorPicker` recebe/emite hex RGB com `#`, sem suporte real a alfa; combinar com `MaxInputText` para hex e seleção RGBA. `MaxModal` tem `visible`, `beforeClose`, `ariaLabel`, slots `content/footer`; `MaxButtonConfirm` emite `confirm/cancel` e usa confirmação global. Usar MaxComponentsUi nos controles novos.
- `MaxPinia/src/types.ts` e `plugin.ts`: opções `get/save`, `getSaveData`, `pauseSave`, `saveInServer`, `reload`, status de servidor; GET só ocorre com rota válida. `saveInServer` pode ser chamado explicitamente enquanto auto-save está pausado.

### Limitações e isolamento

- `git status --short` estava limpo; `.max-code-worktrees/` e `.worktrees/` são ignorados.
- `node_modules` é symlink para o repositório principal; as três bibliotecas Max também apontam para diretórios compartilhados. Binários de Vitest/tsgo existem. Não instalar, sincronizar, rebuildar libs ou escrever dentro desses destinos compartilhados. Presença dos arquivos não prova execução bem-sucedida.
- Não foram iniciados app, browser, SQLite real, testes ou builds. Não há resultado de validação a declarar. Testes nativos visuais exigem ambiente desktop e armazenamento isolado a serem conferidos em Execute.

## 3. Solução e contratos propostos

Os contratos abaixo são **novos**, não APIs existentes. Implementar no MaxTerminal; nenhuma alteração nas bibliotecas externas está autorizada.

### 3.1 Domínio Rust e IPC

- Criar DTO `ThemeDraftInput` com campos editáveis do esquema, sem `id`, `origin`, `polarity`, hash, datas ou proveniência controlados pelo cliente. Exigir nome não vazio e 16 ANSI; preservar `indexedColors` válidos no roundtrip.
- Novos comandos: `theme_create(input)`, `theme_update(id, input)` e `theme_duplicate(id)`; retorno `ResolvedTheme` nas três operações. Backend gera ID único e nome da cópia, normaliza, calcula SHA real e datas. Reexportar DTOs em `mt-themes/src/lib.rs`, registrar comandos em `specta.rs`, gerar bindings pelo script existente e criar wrappers em `src/ipc/themes.ts`/reexports em `types/theme.ts`.
- Reaproveitar normalização sem passar pela importação em arquivo: adaptar DTO→`RawColorScheme`, exigir campos do editor e reaproveitar `normalize_raw_scheme`. Reutilizar erros `InvalidField`, `Conflict`, `BuiltinReadOnly`, `NotFound`, cores/contraste; não criar interpretação CSS permissiva no IPC.
- Atualização mantém ID e `createdAt`; mantém origem `imported` quando aplicável e metadados de proveniência existentes; recalcula hash/polaridade e `updatedAt`. Criação/cópia têm origem `custom` e não inventam arquivo de origem. Duplicação deve manter cores canônicas exatamente, sem mudanças acidentais por defaults de importação.
- Proteger nativos no backend, não apenas escondendo botões. Conflitos de nome devem incluir nativos e persistidos. Criar/duplicar não devem sobrescrever registros por colisão de slug. Serializar checagem+persistência+cache das mutações de catálogo contra concorrência com importação/exclusão, ou garantir atomicidade equivalente no repositório; conflitos SQLite viram erro de domínio, nunca sucesso parcial.
- Persistir antes de alterar caches ou emitir eventos. Aproveitar repositório existente; ampliar somente se necessário à proteção contra colisões/concorrência, sem mudar schema. Falha deixa catálogo e aparência anteriores intactos.
- Ampliar `LibraryChangeReason` com `created`, `updated`, `duplicated`. Emitir catálogo após sucesso; manter semântica dos eventos existentes. Atualizar tema efetivo também quando seu ID não muda: store invalida resolvido e sincroniza pelo contexto atual, evitando aplicar evento global sobre override de outro projeto.

### 3.2 MaxPinia e helpers

- Integrar leituras do catálogo/tema editável e gravação do editor via MaxPinia com adapter IPC, não chamadas de componente a Axios/Tauri. Manter `useThemeStore` como fachada para aplicação/terminais e contratos públicos existentes.
- Proposta de rotas locais exatas no adapter: `GET /app/themes/catalog`→`themeList`, `GET /app/themes/scheme` com `id`→`themeGet`, `POST /app/themes/scheme` com envelope `{kind: 'create'|'update', id?, input}`→novo comando correspondente. São rotas virtuais do adapter, não servidor HTTP. Registrar em `appMaxPiniaAdapter` antes do fallback HTTP e validar método/payload.
- Catálogo como recurso MaxPinia com `isCached`, `id` estável, `data`, `options.get`, `reload`; detalhes/editor podem usar store dedicada `src/stores/themeEditor.ts`. Evitar migrar preferências e subsistemas alheios.
- Editor mantém rascunho separado do esquema ativo; usar `getSaveData` para envelope e `options.save` para salvamento. Pausar auto-save **antes** de hidratar/editar; botão Salvar chama `saveInServer`, verifica status/erro real e só fecha após sucesso. Não interpretar resolução da Promise como sucesso sem verificar contrato do plugin. Não persistir rascunho inválido como catálogo ou bootstrap.
- Duplicação/exclusão são ações explícitas da store, reutilizando wrappers IPC, seguidas de invalidação/reload MaxPinia. Não auto-repetir mutação ao cancelar requisição: abortar frontend não desfaz escrita nativa. Travar duplo clique enquanto salva e descartar respostas de editor/projeto antigo.
- Eventos e respostas de mutação devem atualizar/inutilizar os caches pertinentes sem laço de reload ou retorno de dados antigos. Preview de tema excluído deve ser limpo. Não salvar snapshot de hover nem tema de projeto sobre bootstrap global.
- Usar helpers do `@maxvue/max-use` (por exemplo `_` para clone profundo) em todo helper novo; sem novas dependências e sem imports diretos de lodash/VueUse nos arquivos adicionados.

### 3.3 MaxApp e cores

- Registrar Vue Router real antes de montar: rota nomeada para `/` e rota DEV nomeada para `/dev/terminal`; fallback nomeado compatível com URLs de inicialização. Usar histórico apropriado ao Tauri (preferir `createWebHistory` para manter pathname atual), aguardar `router.isReady()` antes de mount. Não criar ciclo importando `App.vue` como componente de rota: conteúdo continua no slot `#blank`, com componente marcador mínimo definido em `router.ts` quando necessário. Preservar exports atuais do playground e carga DEV lazy.
- Em `plugins.ts`, configurar MaxApp sem endpoints de usuário/menus antes de instanciá-lo. Verificar outras stores do shell na versão consumida e testar que modo desktop local não gera acesso a login/usuário/menus. Não alterar adapter Axios global de outros subsistemas.
- Em `App.vue`, usar `<MaxApp blank-mode no-login controlled-theme :colors="...">` e `#blank` envolvendo o AppShell atual. Remover apenas `MaxToast` redundante e manter demais hosts/fluxos. Ajustar wrappers locais para ocupar viewport sem rolagem extra e preservar splitpanes/resize/overlays.
- Criar mapper puro `src/theme/maxAppPalette.ts`, tipado por `NonNullable<ComponentPropsDeMaxApp['colors']>` ou `PartialColorPalette` exportado pela versão instalada, sem import de caminho interno não exportado. Mapeamento: primary/success/danger/info das respectivas escalas `p500`, warn de `warning.p500`, error de danger; background de `scheme.background`; cores-base ANSI correspondentes e violet de magenta. Demais chaves permanecem defaults da biblioteca. Testar cores-base, não pressupor que os dois algoritmos gerem escalas iguais.
- `colors` acompanha `effectiveResolved` (tema global, override e hover); antes de IPC usar cores deriváveis do snapshot de bootstrap ou fallback canônico existente, sem sobrescrever os tokens de bootstrap. O pipeline atual segue dono de `.dark`, `--max-*`, `--mt-*` e rampas de background inline. CSS da paleta fornece compatibilidade adicional; não remover tokens Rust/contraste para “deixar MaxApp controlar tudo”.
- Garantir com teste de DOM e revisão visual que atualização do `<style id="max-dynamic-colors">` não substitui `--background-*` inline, não cria flash claro e que preview cancelado restaura tanto prop `colors` quanto tokens/terminais.

## 4. Etapas sequenciais e matriz de aceite

Executar na ordem abaixo. Implementar todo o escopo e seus testes antes da bateria final em lote. Não delegar por padrão; não distribuir trabalho dependente antes de estabilizar DTOs, eventos e ownership do tema.

| ID / requisito | Arquivos previstos | Testes pertinentes | Evidência de conclusão |
| --- | --- | --- | --- |
| T1 — domínio criar/editar/duplicar | `mt-themes/src/{scheme,service,lib}.rs`, eventualmente módulo `editor.rs`; `cache.rs`/repo somente se necessário | Ampliar `mt-themes/tests/service_db.rs`: criar, atualizar, duplicar nativo/custom, mesma SHA permitida, nomes/IDs únicos, conflitos, nativo protegido, contraste/cores, persistência/reabertura e falha sem evento/cache parcial | SQLite temporário contém campos preservados; hash real; ID estável na edição; cópia independente; nenhum nativo alterado |
| T2 — transporte e estado | `src-tauri/src/{commands/themes,specta}.rs`, bindings gerados, `src/ipc/themes.ts`, novo `themeMaxPiniaAdapter.ts`, `src/app/plugins.ts`, `src/stores/theme.ts`, novo `themeEditor.ts`, `src/types/theme.ts` | Testes de wrappers/adapter/stores em `tests/ipc/`, `tests/stores/`: roteamento exato, envelope, falha, cancelamento, save explícito, reload e eventos; testar override ativo e edição no mesmo ID | Frontend usa MaxPinia Get/Save/cache sem HTTP real; erro mantém rascunho; catálogo e tema efetivo coerentes |
| T3 — controles e editor | `appearance/{AppearancePreferences,ThemePicker,ThemeSwatch}.vue`, novo `ThemeEditorDialog.vue`; ajustes localizados no composable de preview | Novos testes de componentes: adicionar/cancelar/salvar, editar importado/custom, duplicar nativo, botões e erros; confirmação/exclusão ativa e inativa; teclado/foco | Quatro ações acessíveis em pt-BR; cliques/Enter nas ações não selecionam o cartão; fechar descarta rascunho; confirmação cancelada não chama delete |
| T4 — MaxApp blank e paleta | `src/app/{App.vue,main.ts,router.ts,plugins.ts}`, novo `src/theme/maxAppPalette.ts`, CSS local se necessário | Testes em `tests/app/` e `tests/theme/`: rota nomeada/root/playground, slot blank real, no-login/controlled-theme, ausência de requests web, toast único, mapper e tokens após mount/troca/preview/reversão | Shell renderiza em boot frio/offline/erro/recuperação; nenhuma barra web; colors reativo; xterm e UI preservados |
| T5 — fechamento integrado | Testes existentes de tema, preferências e bootstrap; registro Execute | Bateria canônica e roteiro visual isolado abaixo | Todos os requisitos e gates com resultado observado ou impedimento concreto; nenhuma declaração de aceite integral com gate pendente |

T1 antecede T2; T2 antecede T3; T4 usa os contratos de tema estabilizados em T2; T5 encerra. Releia trechos afetados após alteração/falha de patch antes de ajustar dependentes.

## 5. Preflight e validação em Execute

### Preflight somente leitura

1. Ler este plano e AGENTS; conferir branch, status/diff e implementação já existente. Não criar worktree, trocar branch ou descartar alterações alheias.
2. Conferir `package.json`, scripts e resolução real das libs/exportações/dist, Axios e plugin MaxPinia. Não executar instalação/sync/rebuild nas libs compartilhadas. Se dist não corresponder à API inspecionada, registrar evidência e bloquear integração dependente até disponibilização autorizada.
3. Conferir destinos de cache/build/testes antes de executar: Vite `.tmp/vite`, cobertura/build locais, Cargo target temporário exclusivo desta worktree. Como `node_modules` é compartilhado, impedir escrita de caches nele; se algum runner exigir modificação compartilhada, parar e registrar bloqueio, não substituir symlink por conta própria.
4. Confirmar que testes usam fixtures e mocks, SQLite de `TempDir` e localStorage/IndexedDB isolados por teste. Para MaxPinia usar `cacheName/storeName` exclusivos no harness e limpar só esse namespace. Não copiar `.env` nem conectar a dados do usuário.
5. Para roteiro nativo, ler a resolução de diretórios de dados do app e confirmar perfil sandbox sem dados reais antes de iniciar. Se isolamento desktop não estiver disponível, manter gate manual pendente; não usar banco de produção como alternativa.

### Comandos confirmados do projeto (somente na execução futura)

Todos na raiz da worktree, salvo indicação contrária:

- `npm run bindings` — regeneração intencional dos bindings após T1/T2, não é teste puramente read-only.
- `npm run test -- tests/theme` — suíte existente de tema; incluir também os novos testes de app/IPC/store/componentes com seus caminhos reais.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked -p mt-themes --test service_db` — integração de domínio com banco temporário.
- `npm run check` — fechamento canônico: verificação de libs `--check`, lint, stylelint, typecheck, typecheck:vue, cobertura, build, fmt `--check`, clippy e testes Rust. Não usar `lint:fix` ou sync como “validação”.

Configurar destinos locais de saída na execução conforme o preflight; não gravar artefatos nas libs/checkout principal. Não rodar as mesmas suítes repetidamente sem mudança. Ao primeiro bloqueio de ambiente, diagnosticar e registrar; não ocultar falhas nem alterar código alheio apenas para passar a bateria.

### Roteiro observável de aceite integrado

- Criar tema escuro e claro; editar várias cores e nome; reabrir app sandbox e comprovar persistência. Rejeitar nome vazio/duplicado e contraste inferior a 3:1 sem gravar.
- Duplicar nativo duas vezes: nomes/IDs distintos, cores idênticas e SHA de conteúdo igual; editar uma cópia não afeta original/outra cópia. Preservar paleta indexada e alfa de seleção.
- Editar tema ativo global e de projeto: UI, paleta MaxApp e terminal passam ao novo conteúdo sem reeleger ID. Tema de outro projeto não substitui o atual por evento atrasado.
- Excluir tema ativo e um tema usado por overrides/preferências: confirmação prévia e referências corretas; cancelar não muda dados. Preview de item excluído não deixa tokens órfãos.
- Abrir/fechar editor, hover e cancelar preview: restaurar cores e não persistir snapshot transitório. Operações com falha exibem erro em pt-BR e permitem correção sem perda do rascunho.
- Boot frio, offline, falha de bootstrap e recuperação mostram shell no blank slot. Ausência de login/menu/requisições web de usuário; apenas um host toast/confirmação. Testar redimensionamento, terminal, modais e `/dev/terminal` DEV sem rolagem adicional.
- Reexecutar regressões de importação/exportação, temas por projeto, followSystem, tokens/contraste/bootstrap e terminal bridge. Nenhum teste deve gravar no perfil normal do app.

## 6. Riscos e operação

- **Cascata de cores:** escalas MaxApp e Rust são diferentes e compartilham background; manter autoria explícita e testar propriedades computadas, não apenas props mockadas.
- **Stores web involuntárias:** blank/no-login não evitam instanciação; configurar antes do setup e testar com biblioteca real, não somente stub de MaxApp.
- **Cancelamento versus persistência:** AbortSignal não reverte SQLite; mutações não têm retry automático. Bloquear duplo envio e reconciliar catálogo após resposta/evento.
- **Integridade:** proteger identidade/metadados no backend; persistir atomicamente antes de caches/eventos. Duplicação não passa pela deduplicação do importador. Não alterar migração existente.
- **Compatibilidade:** não modificar MaxComponentsUi, MaxUse, MaxPinia, `node_modules` compartilhado, contratos de terminal/PTY nem dados reais. Falta de dependência/dist/desktop é bloqueio registrado, não autorização para ampliar escopo.

## 7. Retomada e registro de execução

Na admissão de Execute, ler este plano integralmente, conferir trabalho já feito e retomar pela primeira tarefa incompleta. Revalidar somente evidências afetadas por mudança. Esta sessão Plan escreve exclusivamente este `plan.md`; não criar registro de execução agora.

Somente em Execute, registrar marcos/handoff em `docs/plans/wt-c4210302/execution-state.json`, schema version 2 estrito, sem chaves extras:

- Raiz: `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`. `planRevision`: SHA256 real dos bytes deste plano; `updatedAt`, se presente, inteiro positivo em milissegundos.
- Tarefa: `{id, status, files, validations, revision?, nextAction?}`; IDs T1–T5; `validations` sempre presente. Status: `pending`, `running`, `completed`, `blocked`, `manualPending`, `validationFailed`. Revisões são SHA256 reais de conteúdo factual, nunca nome de branch/rótulo.
- `verification`: `{revision, checks: [{command, revision, result, exitCode?}]}`. `result`: `passed`, `failed` ou `pending`. `passed` exige `exitCode: 0`; `failed` exige inteiro não zero; `pending` omite `exitCode`. Não usar `null` nem inferir código de saída.
- `validations` referencia os comandos pertinentes e seus resultados sem duplicar objetos de check fora de `verification.checks`; resultados também apenas `passed|failed|pending`. Comando até 4096 caracteres; `nextAction` até 512.
- Para revisões de arquivos, usar lista ordenada de caminhos relativos e SHA256 real dos respectivos bytes, serialização estável documentada no handoff. Vincular cada check à revisão efetivamente validada e registrar todas as falhas e gates manuais pendentes.
- Proibidos `schemaVersion`, `blocker`, `diagnostic`, `branchSync`, `durableSync`, `followUp`, `title/evidence` em tarefa e `worktree/branch/head/planPath/status/checks` soltos na raiz. Motivos e impedimentos ficam em `nextAction`.

Não declarar conclusão integral enquanto houver gate obrigatório pendente ou falha vigente. Não fazer commit, merge, push nem remover esta worktree: integração e limpeza pertencem ao MaxCode mediante solicitação do usuário.
