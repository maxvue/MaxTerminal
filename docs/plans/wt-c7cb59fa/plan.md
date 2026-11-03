# Plano de execução — janela inicial do MaxTerminal

## 1. Identificação, autorização e retomada

| Campo | Valor |
| --- | --- |
| Projeto/data | MaxTerminal — 03/11/2026 |
| Worktree/branch | `wt-c7cb59fa` |
| Raiz exclusiva | `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-c7cb59fa` |
| HEAD investigado | `ed45a0f514e09544d9c90cb0ca06856c80b1a703` |
| Especificação | [spec.md](./spec.md) |
| Situação | Planejamento concluído; implementação e validação de execução não iniciadas |

**Pedido:** corrigir a abertura na qual só aparece o ícone na barra inferior e nenhuma janela utilizável. Usuários afetados: quem inicia o aplicativo desktop, especialmente no ambiente KDE/Wayland observado.

**Limite desta sessão Plan:** somente estes dois documentos foram autorizados para escrita. Não houve testes, build, instalação, abertura do aplicativo, acesso ao banco, alteração de serviços ou implementação. O registro de execução descrito na seção 9 é exclusivamente para uma futura sessão Execute autorizada.

**Para o executor:** ler este plano e a spec na primeira admissão. Na retomada, consultar tarefas pertinentes e o registro de progresso, conferir o que já foi feito e revalidar o código/alterações locais antes de executar lacunas. Não recriar worktree, mudar branch, investigar outras worktrees ou realizar operações de commit/merge/push/limpeza. Preservar trabalho preexistente e decisões; reabri-las somente com evidência nova ou mudança do pedido.

## 2. Diagnóstico fundamentado e resultado esperado

### 2.1. Fatos observados

| Evidência | Constatação | Consequência |
| --- | --- | --- |
| `src/app/plugins.ts:13-17` | Cria Pinia, configura MaxPinia e registra MaxIcon; não instala Pinia no app | Defeito de bootstrap confirmado por leitura |
| `src/app/main.ts:9-14` e `App.vue:67-78` | Monta após registrar plugins; o componente raiz usa stores no setup | A falta de instalação pode impedir o conteúdo inicial |
| `tests/stores/app.spec.ts:25-28` | `setActivePinia(createPinia())` antes de cada teste | Esses testes não detectam a omissão no bootstrap real |
| `src-tauri/tauri.conf.json:14-26` | `main`, centralizada, `visible: true` | Não tratar a flag de visibilidade como causa já comprovada |
| `src-tauri/src/lib.rs:162-489` | Setup inicializa banco e diversos serviços antes de concluir | Há uma segunda fronteira de falha, anterior ao frontend utilizável |
| `src-tauri/src/lib.rs:521-525` | O erro da construção do runtime é descartado e substituído por mensagem genérica | Falta contexto diagnóstico em falhas de construção |
| `src-tauri/src/logging.rs:426-435` | Hook de pânico registra somente evento estático | Preservar privacidade; não inventar mensagem/stack ausente |
| Log local de 03/11 | 14 `app_startup` e 7 `backend_panic` nas 21 linhas consultadas | Existem ocorrências nativas, mas sem correlação com uma execução específica |
| Lançador local | Executa `~/.local/bin/maxterminal %u`, sem flags de ocultação | O lançador inspecionado não solicita abertura oculta |
| Ambiente/processos | KDE/Wayland; nenhum processo de nome exato `maxterminal`/`maxterminal-bin` encontrado | Não houve observação de uma janela travada em execução |
| `e2e/specs/tray.e2e.ts` e `tabs.e2e.ts` | Somente objetos com roteiros textuais | Não representam testes desktop executáveis nem evidência de aprovação |

**Limitações:** o binário instalado não teve origem/revisão comprovada; log sem timestamp por linha/PID não permite atribuir os pânicos ao incidente. Não se reproduziu a falha. Não há evidência suficiente para afirmar deadlock, falha de GPU, problema do compositor, corrupção do SQLite ou uma causa nativa específica.

### 2.2. Requisitos e preservações

- **R1:** inicialização normal mostra janela principal e conteúdo inicial utilizável. Ícone na barra, `is_visible()` ou processo vivo não bastam para aceite.
- **R2:** instalar o Pinia configurado com MaxPinia antes do mount e demonstrar a aplicação real do plugin em regressão isolada.
- **R3:** diagnóstico mínimo distingue avanço/falha do startup sem expor segredos nem remover a supervisão de PTYs.
- **R4:** preservar IPC, retry/erro existentes, configurações de visibilidade, Quake, fechamento seguro e dados do usuário.
- **R5:** testes e aceite usam ambiente isolado e registram resultados reais vinculados à revisão factual.

Não é objetivo garantir foco absoluto em Wayland, nem concluir toda a integração de bandeja/atalhos/single-instance. A abertura normal não deve depender de uma rede de ícones ou de um fornecedor de IA estar disponível.

## 3. Solução, contratos e decisões

### 3.1. Decisões fechadas pelo pedido e pelo código

1. Corrigir `registerAppPlugins(app: App): void` com a instalação da **mesma** instância criada e configurada com `createMaxPinia`. Preservar a ordem: criar → configurar plugin → instalar no app → montar componentes. Não usar `setActivePinia` no bootstrap como substituto de `app.use`.
2. Manter MaxComponentsUi e o registro existente de `MaxIcon`/CSS. Foram consultados os exports e a implementação local de `MaxIcon`, que usa `useIconStore` e helpers do MaxUse. Não criar UI nova para este conserto; eventual UI adicional exige MaxComponentsUi e revisão de escopo.
3. Manter MaxPinia como plugin de infraestrutura. Seu gate `isCached`/`is_cached` em `node_modules/@maxvue/max-pinia/src/plugin.ts:96-106` não deve ser contornado. Não converter stores IPC de runtime em stores HTTP/cacheadas, não alterar os aliases de configuração da biblioteca nem seu contrato de autosave/cache.
4. Helpers adicionais devem vir do MaxUse; não há necessidade de novo helper para uma chamada de instalação. Não editar bibliotecas irmãs por meio dos symlinks.
5. Não alterar DTOs, eventos públicos, bindings, SQLite ou permissões. Preservar `specta_builder.mount_events(app)`, o contexto Tokio no setup e o encadeamento dos hooks de pânico/watchdog.
6. Instrumentar com `target: "maxterminal"` e os campos já serializados `event`, `code`, `message`. Não depender de targets `mt_os`/`mt_bridge`, que não estão no filtro atual. Identificadores de etapa são estáticos, nunca derivados de argumentos ou dados do usuário.

### 3.2. Visibilidade existente a manter

`App.vue` → `windowStore.init()` → `osIpc.uiReady()` → `OsIntegration::ui_ready()` (`mt-os/src/lib.rs:421-437`). O handshake é idempotente: `--hide`/`--start-hidden` só ocultam inicialmente quando há tray disponível; nos demais casos solicita exibição. `QuakeController` já desminimiza/exibe/foca via `TauriWindowHost` e preserva fallback de minimização quando não há meio seguro de reabrir.

Preservar prioridade CLI `--show > --toggle > --hide > --start-hidden`, resolução de toggle e semântica de segunda instância já modelada, sem implementar novo mecanismo de single-instance. `tray_available` não deve ser forçado para verdadeiro. A fábrica de atalhos atual só fornece backend CLI; não prometer captura global nativa.

### 3.3. Hipóteses e decisões condicionais

- A omissão de Pinia explica falha de montagem, não necessariamente ausência de janela do SO.
- Pânicos históricos podem pertencer a binário anterior; não apagar logs nem substituir o executável instalado para descobrir isso.
- Se a reprodução isolada apontar falha nativa, corrigir o menor trecho responsável e adicionar regressão. Registrar arquivo, etapa e evidência antes de modificar. Reorganização assíncrona ampla, novas permissões/dependências, mudança de schema ou de política de janela exigem revisão do plano e eventual decisão do usuário.
- Se não houver reprodução nativa e só o executável instalado continuar falhando, manter aceite desse cenário em `manualPending`; informar a discrepância, sem recomendar reset de dados ou instalação automática.

### 3.4. Alternativas descartadas

- Mudar `visible` para true: já está assim.
- Forçar `show()` repetidamente, adicionar sleeps ou esconder/mostrar por timeout: mascara causas e pode contrariar a CLI/Quake.
- Remover MaxPinia/MaxComponentsUi: contraria o pedido e não corrige a instalação do Pinia.
- Declarar todos os `block_on` inválidos: a documentação Tauri v2 permite esse uso em setup; o erro depende da operação e do contexto reais.
- Ativar stack/payload bruto de pânico: contraria a política de privacidade existente. Os marcos estáticos devem ajudar a localizar a etapa sem relaxá-la.

## 4. Referências, versões e arquivos

- App: Vue `3.6.0-rc.13`, Pinia `3.0.1`, API/CLI Tauri `2.5.0` conforme `package.json`; Rust Tauri resolvido `2.11.6`, runtime-wry `2.11.4`, Tokio `1.54.1` conforme `Cargo.lock`. Não propor upgrade para homogeneizar números sem verificar compatibilidade real.
- Bibliotecas consultadas: MaxPinia `0.2.0`, MaxComponentsUi `1.1.36`, MaxUse `2.0.1`; manifestos, fonte relevante e declarações públicas foram lidos. Isso não substitui o gate de integridade dos artefatos.
- Configuração do harness: `vitest.config.ts` usa happy-dom e dedupe de Vue/Pinia; `vite.config.ts` também deduplica Vue/Pinia e preserva imports das bibliotecas locais. `tests/setup.ts` apenas limpa mocks Tauri.
- Scripts: `scripts/check.mjs`, `scripts/sync-local-libs.mjs`, `package.json`; toolchain `stable` com rustfmt/clippy.
- Isolamento: `mt-core/src/paths.rs`, `mt-db/src/config.rs`, `mt-pty/src/config.rs`, `mt-pty/src/manager.rs`, `mt-bridge/src/config.rs`, `client.rs`, `discovery.rs`, `src-tauri/src/agents/mod.rs`.
- Janela: `src-tauri/src/adapters/os.rs`, `commands/os.rs`, `mt-os/src/{lib,quake,cli,autostart}.rs`; testes existentes `mt-os/tests/{cli,toggle,close_decision,settings}.rs` são regressões a preservar, não comprovadas como executadas aqui.
- Fonte oficial: [Pinia — instalação](https://github.com/vuejs/pinia/blob/v3.0.4/packages/docs/getting-started.md) e [plugins em testes](https://github.com/vuejs/pinia/blob/v3.0.4/packages/docs/cookbook/testing.md), confrontadas com o Pinia `3.0.1` instalado (`dist/pinia.mjs:967-998`). A documentação consultada é `3.0.4`, não a versão exata instalada.
- Fonte oficial: [Tauri v2 — migração/runtime](https://v2.tauri.app/start/migrate/from-tauri-1/), [tray](https://v2.tauri.app/learn/system-tray/) e [CLI](https://v2.tauri.app/reference/cli/). Documentação evolutiva v2; revalidar opções na CLI `2.5.0` local. Eventos de clique do ícone de tray não são suportados no Linux segundo essa documentação; não basear o aceite em uma ação inexistente.

### Arquivos de escrita previstos durante Execute

| Arquivo | Tipo/responsabilidade |
| --- | --- |
| `src/app/plugins.ts` | Existente; correção mínima de registro |
| `tests/app/plugins.spec.ts` | Novo; instalação real e aplicação do MaxPinia |
| `tests/app/bootstrap.spec.ts` | Novo; ordem do entrypoint e montagem sem Pinia pré-ativo |
| `src-tauri/src/lib.rs` | Existente; instrumentação e falhas de construção/setup |
| `src-tauri/src/logging.rs` | Existente; testes do contrato de logging e suporte mínimo, se necessário |
| Arquivo nativo e teste a identificar | Condicionais; somente após prova causal, nos limites da seção 3.3 |
| `docs/plans/wt-c7cb59fa/execution-state.json` | Novo registro de Execute; proibido criar nesta sessão Plan |

## 5. Etapas executáveis e dependências

### T1 — Revalidar contratos e isolamento, sem executar a aplicação

1. Conferir HEAD, branch e alterações com comandos Git de leitura; comparar o conteúdo relevante com a revisão investigada e preservar alterações novas.
2. Ler scripts npm, scripts agregados, hooks e configuração Tauri antes de executar ferramentas. Não instalar pacotes, sincronizar symlinks em modo de escrita nem compilar bibliotecas externas para contornar falha de integridade.
3. Conferir destinos reais dos links e exports usados de MaxComponentsUi/MaxPinia/MaxUse; verificar que o harness e suas bibliotecas compartilham Vue e Pinia. Identificar qual Axios cada caminho usa antes de mocká-lo; MaxPinia carrega Axios de forma lazy e MaxUse também tem sua dependência. Não assumir que um mock da aplicação intercepta todas as cópias.
4. Desenhar isolamento do teste frontend: DOM/localStorage/IndexedDB/localforage descartáveis, transporte IPC e `fetch`/HTTP substituídos por fixtures. `useIconStore` pode requisitar ícones e sincronizá-los por POST; bloquear toda saída real, não apenas Axios.
5. Para futuros testes Rust/desktop, conferir que fixtures criam banco, backups, locks, filas, cache e storage privativos. `MAXTERMINAL_HOME` isola AppPaths, mas **não basta**: `MAXTERMINAL_DB_PATH` prevalece sobre ele; autostart consulta XDG_CONFIG_HOME; PTY usa XDG_RUNTIME_DIR e varre registros órfãos; bridge publica endpoints e faz handshake por padrão; agentes podem observar skills na HOME.
6. Escolher ambiente gráfico descartável/isolado. Validar HOME, bases XDG, runtime, banco e descoberta de peers; preservar acesso ao display de forma explicitamente verificada. Não trocar XDG_RUNTIME_DIR cegamente: `WAYLAND_DISPLAY` relativo pode deixar de resolver o socket gráfico. Não copiar `.env`, bancos ou credenciais do usuário nem conectar a bridge ao MaxCode real. Se o isolamento completo não for viável, bloquear o smoke e solicitar a preparação do ambiente, sem ampliar o escopo do produto.

**Saída:** revisão factual de arquivos/contratos e estratégia segura de testes. No Execute, o check `node scripts/sync-local-libs.mjs --check` é gate canônico de integridade; em caso de falha, bloquear validações dependentes e diagnosticar uma vez. Não ignorar hooks/config, executar binários diretos para burlar o gate ou usar `ignoreConfig`.

### T2 — Corrigir o bootstrap e implementar regressões frontend

Dependência: T1 aprovado por leitura. Não executar suíte, tipos ou build entre subpassos.

1. Em `plugins.ts`, instalar a instância configurada na aplicação mantendo API pública, import de estilos e registro de MaxIcon. Evitar refatoração de stores/componentes adjacentes.
2. Em `tests/app/plugins.spec.ts`, montar um componente mínimo que consuma a store real `useAppStore` e o MaxIcon real pelo registro de produção. Antes do caminho sob teste, não pode existir Pinia ativo de outro teste, `global.plugins` alternativo ou plugin simulado. Restaurar estado global e desmontar ao final.
3. Adicionar caso com store elegível de teste para demonstrar aplicação real de MaxPinia: presença de métodos/status esperados em store cacheada e preservação de `appStore.status === 'idle'` na store de runtime. Consultar o contrato público efetivo antes de escolher asserções. Dublar somente fronteiras de storage/rede e usar chave/cache privativos de teste; não ativar autosave real.
4. Em `tests/app/bootstrap.spec.ts`, atravessar o entrypoint `main.ts` com fixture de componente raiz que consuma store no setup. Não mockar `registerAppPlugins`, Pinia ou MaxPinia. Resolver estilos/UnoCSS com o harness existente, sem mudar configuração global por conveniência. A fixture deve montar conteúdo observável; um spy de `app.use` sozinho é insuficiente.
5. Cobrir ordem plugins→mount, ausência de erro de Pinia e limpeza de listeners/timers/instâncias. Manter os testes existentes de falha de `app_info`, retry e logger; não modificar o setup geral para pré-ativar Pinia, pois isso esconderia a regressão.

**Saída:** correção pequena e testes que falhariam por ausência da instalação na versão anterior, sem exigir uma execução Red intermediária nesta tarefa. Testes com fixtures não comprovam o desktop nativo.

### T3 — Delimitar falhas de inicialização nativa sem expor conteúdo

Dependência: T1. Pode ser implementada independentemente de T2, mas a execução padrão é sequencial e sem delegação implícita.

1. Marcar entrada/conclusão das etapas significativas já existentes no setup: banco; temas/shell/workspace; SO; bridge/IA/agentes; serviços restantes; construção do runtime. Usar catálogo pequeno de identificadores estáticos em `event` e `code`, sem criar um framework de observabilidade.
2. Preservar a propagação original de erros e encerramento seguro. No ramo que hoje descarta `Err(_err)`, emitir etapa/categoria segura quando disponíveis. Não imprimir cadeia arbitrária de erros sem sanitização; nunca logar CLI, ambiente, payload/stack do pânico ou caminho pessoal. Não transformar pânico em sucesso com `catch_unwind` genérico.
3. Em `logging.rs`, testar que os novos eventos sobrevivem ao filtro/formatter corretos e que não passam sentinelas secretas. Manter cobertura da FoundationRedactor e do hook estático; eventuais adições ao catálogo não podem liberar texto arbitrário.
4. Manter intactos os guards de PTY, o runtime Tokio já estabelecido e a montagem do registro de eventos Specta. Não mover todos os serviços para tarefas soltas sem contrato de prontidão.

**Saída:** implementação e regressões de diagnóstico concluídas antes da validação agrupada. A instrumentação é suporte à correção; não resolve nem encerra, sozinha, o incidente.

### T4 — Validar o lote e comprovar a abertura real

Dependências: T2 e T3 completas, T1 vigente. Implementar todos os testes previstos antes de executar gates completos. Revisar o conjunto no marco de risco antes de executar.

1. Executar a sequência agregada da seção 6 e registrar todas as falhas. Diagnosticar juntas as falhas de bootstrap/harness antes de repetir testes equivalentes. Falha de biblioteca bloqueia validação dependente; falha persistente sem alteração exige causa e próxima ação, não repetição automática.
2. Construir o desktop com a CLI Tauri, mantendo hooks e configuração oficial. Verificar o caminho/identidade do artefato gerado, sem instalar sobre o binário do usuário. Não assumir que o executável instalado e o recém-construído são iguais por terem versão `0.1.0`.
3. Após o gate de isolamento, abrir o artefato em sessão gráfica segura: observar janela mapeada, conteúdo do shell, ausência de erro de Pinia e progressão do startup. Espera limitada de até 15 s para o shell inicial, registrada como limite de observação desta verificação, não SLA do produto. Encerrar a espera com falha/pendência se não houver avanço, sem polling infinito.
4. Se houver pânico/travamento/fechamento antes do conteúdo: coletar marcos da execução isolada, identificar a etapa e inspecionar o trecho específico e suas dependências. Associar evidência ao binário/revisão. Não inferir a causa somente da última linha e não contar pânicos de logs históricos como falha desta revisão.
5. Havendo causa nativa comprovada e contida no escopo, adicionar regressão e correção mínima no mesmo lote, revisar o risco e repetir somente os gates invalidados pela mudança, incluindo nova abertura nativa. Se for necessário mudar política pública, dados reais, dependências ou arquitetura ampla, interromper com proposta de revisão do plano.
6. Validar a matriz abaixo: início normal, erro/retry de IPC e preservação de ocultação/Quake/fallback. Cenários de tray indisponível devem continuar abrindo normalmente no handshake. Cenários com tray real só recebem aceite se houver host realmente disponível; não fingir disponibilidade.

**Saída:** evidências automatizadas e observação desktop da mesma revisão. Sem teste nativo, T4 fica `manualPending`; se um pânico permanecer sem causa/correção, fica `blocked` ou `validationFailed`, com próxima ação concreta.

### T5 — Revisar, registrar e entregar sem integração automática

1. Revisar diff contra R1–R5: preservação de contratos, escopo mínimo, privacidade, nenhum dado real alterado e nenhum workaround especulativo.
2. Fazer revisão independente no marco de risco/entrega, se houver revisor autorizado/disponível; não há autorização de subagentes nesta sessão. Ausência de revisão independente deve ser informada, não inventada.
3. Atualizar o registro da seção 9, apontando tarefas, arquivos, hashes reais, comandos/exit codes e pendências manuais. Entregar resumo claro separando correção implementada, resultados e aceite do executável instalado ainda pendente, quando aplicável.
4. Não commitar, integrar, publicar, instalar ou excluir worktree. Se a adoção da correção exigir atualizar a instalação do usuário, encaminhar como operação separada ao usuário/MaxCode.

## 6. Comandos de validação e política de execução

**Confirmados no projeto; somente para Execute após os gates de isolamento/integridade:**

| Comando/cwd | Uso |
| --- | --- |
| `node scripts/sync-local-libs.mjs --check` / raiz | Gate de integridade; também é o primeiro passo de `npm run check` |
| `npm run check` / raiz | Agrega integridade, ESLint, Stylelint, tsgo, vue-tsc, Vitest com cobertura, Vite build, Rust fmt check, Clippy e testes workspace |
| `npm run tauri -- build --no-bundle` / raiz | Build desktop sem instalação; conferir opção na CLI local antes de usar |

Não executar o gate de integridade isoladamente e repetir logo em seguida sem necessidade: preferir a primeira etapa do agregado quando a leitura de T1 já permitir avançar. Falha nessa etapa encerra o agregado e bloqueia dependentes.

Os comandos internos de Rust identificados em `scripts/check.mjs` são `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings` e `cargo test --workspace --locked`, com cwd `src-tauri`. Não executá-los de novo após aprovação vigente do agregado. Se houver correção posterior, selecionar os checks afetados pelo diff factual e registrar a justificativa de reaproveitamento dos demais.

O build Tauri executa `beforeBuildCommand: npm run build`; essa repetição imposta pelo fluxo oficial é necessária para embutir os assets e não autoriza remover o hook. Não acrescentar outra chamada manual de build frontend. Se CLI/toolchain detectar incompatibilidade, diagnosticar; não usar flags para ignorar incompatibilidades.

Não há comando npm de E2E desktop no manifesto investigado, e as dependências WDIO/tauri-driver não estão declaradas nele. Não inventar runner, instalar ferramentas nem reportar roteiros exportados como testes. Na ausência de harness operacional autorizado, executar aceite manual seguro ou registrar `manualPending`.

Tarefas assíncronas aguardam conclusão/mudança com timeout e recuo; usar notificações quando disponíveis, sem consultas repetidas sem informação nova. Cancelamento preserva resultados parciais e próxima ação. Não executar operações destrutivas, resets/migrations compartilhadas ou cópia ad hoc de `.env` como preparação.

## 7. Matriz de aceite por tarefa

| Tarefa/requisito | Evidência exigida | Testes pertinentes | Revisão pelo risco | Verificação manual |
| --- | --- | --- | --- | --- |
| T1 / R5 | Resolução das libs, hooks e destinos de storage/runtime/peers documentados | Gate de libs vigente; harness isolado | Symlinks externos, identidade Pinia/Axios, POST de ícones e limpeza de órfãos | Confirmar sessão gráfica e ausência de peer/dados reais |
| T2 / R2 | Mesma instância Pinia configurada e instalada antes do mount | Novos `plugins.spec.ts` e `bootstrap.spec.ts`; regressões de stores/logger existentes | Nenhum Pinia pré-ativo escondendo erro; stores runtime sem cache forçado | Shell sem erro de Pinia, com MaxIcon registrado; rede de ícones não é pré-requisito |
| T3 / R3 | Eventos estáticos delimitam etapas e mantêm erro/pânico seguro | Testes de formatter/filtro/sentinelas e catálogo em `logging.rs` | Não vazar segredo, não perder guard/watchdog nem transformar falha em sucesso | Relacionar logs somente à execução isolada observada |
| T4 / R1 | Artefato identificado abre janela e renderiza conteúdo, sem pânico nessa execução | `npm run check` e build desktop concluídos; regressão nativa da causa, se necessária | Não confundir DOM simulado com janela do SO nem executar sobre instalação/dados reais | Abrir normalmente em KDE/Wayland; aguardar shell até limite observado; fechar seguramente |
| T4 / R4 | Contratos CLI/handshake/erro/retry e fechamento preservados | Testes existentes `mt-os/tests/{cli,toggle,close_decision,settings}.rs` e `tests/stores/app.spec.ts` | Não forçar tray/atalhos ou foco proibido pelo compositor | `--show`; abertura sem flags; fallback sem tray; erro/retry com transporte controlado; ocultação intencional só com meio real de recuperação |
| T5 / R1–R5 | Diff revisado e registro v2 coerente com conteúdo/exit codes | Checks vigentes aprovados da revisão final, sem omissões | Revisão de entrega e pendências explícitas | Aceite real separado da eventual atualização da instalação |

Casos extremos: Pinia global residual de outro teste; biblioteca com cópia distinta de Vue/Pinia; MaxPinia não aplicado por falta de instalação; falta de rede/storage de ícones; falha de IPC; startup nativo anterior ao mount; perfil vazio versus estado previamente salvo; `--start-hidden` sem tray; janela minimizada; política de foco Wayland. Perfil salvo real não deve ser copiado/testado sem autorização e prova de isolamento; usar fixtures sintéticas primeiro.

## 8. Riscos, segurança e operação

- **Dados/processos:** SQLite deve continuar WAL; nada neste plano muda migrations, sanitização ou retenção. O sweep de PTY pode afetar registros compartilhados se o runtime não for isolado. Não parar processos de outras sessões para liberar teste.
- **Rede/bridge:** o serviço de bridge é habilitado por padrão e tenta descobrir peers; testes de abertura não autorizam sincronizar projetos ou modificar MaxCode. Bibliotecas de ícones também podem emitir HTTP/POST; bloquear transporte real no harness.
- **Disponibilidade:** ausência de display, bibliotecas nativas, toolchain, artefatos de libs ou permissões de contenção bloqueia a validação pertinente; não comprova defeito nem permite bypass.
- **Compatibilidade:** não alterar CSP, tokens de tema, esquema de settings, atalhos, política de tray ou contratos gerados. Manter textos/comentários novos em pt-BR e identificadores em inglês.
- **Desempenho:** marcos são poucos e executados uma vez por etapa; não introduzir polling, sleeps artificiais ou logs por byte/evento do terminal. Instrumentação não deve atrasar o loop da UI.
- **Recuperação:** desfazer somente alterações próprias se a revisão exigir; preservar docs, trabalho de terceiros, banco, logs e instalação. Não há reversão de dados planejada porque não há migração. Atualização do binário instalado e limpeza ficam fora da autorização.
- **Delegação futura:** somente se expressamente autorizada. Compartilhar uma vez contratos de T1, versões/fontes e recursos indisponíveis; T2 possui `plugins.ts` e testes frontend, T3 possui `lib.rs`/`logging.rs`. Nenhum executor executa gates completos enquanto outro escreve. T4/T5 são sequenciais e consolidam resultados; contexto mínimo por tarefa com referências a este plano, sem releitura integral compulsória.

## 9. Progresso e handoff durante Execute — contrato v2 estrito

Destino futuro: `docs/plans/wt-c7cb59fa/execution-state.json`. **Não criar nem escrever esse arquivo em Plan Mode.** Atualizar após marcos e no handoff, não a cada microedição.

Chaves permitidas, sem quaisquer campos adicionais:

- **Raiz:** `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`.
- **Tarefa:** `{id, status, files, validations, revision?, nextAction?}`. `validations: []` é sempre obrigatório; neste plano, usar esse array vazio e concentrar checks executados em `verification.checks`, sem inventar outro formato.
- **Verification:** `{revision, checks}`.
- **Check, somente em verification.checks:** `{command, revision, result, exitCode?}`.

Regras:

1. `planRevision` é SHA256 real dos bytes vigentes de `plan.md`. `revision` é SHA256 real do conteúdo factual da tarefa/lote, incluindo testes e configuração relevante; definir uma composição determinística (caminhos relativos ordenados e respectivos bytes) e mantê-la entre retomadas. Incluir hashes dos artefatos externos relevantes no conjunto verificado quando seu conteúdo influenciar a validação. Nunca usar rótulos como `T2-final` no lugar de hashes.
2. `tasks` lista T1–T5 com `id`, arquivos relativos em `files`, `validations` e estado. Status admitidos: `pending`, `running`, `completed`, `blocked`, `manualPending`, `validationFailed`.
3. `result` de check/validação admite somente `passed`, `failed`, `pending`. `blocked` não é resultado de check. `passed` exige `exitCode: 0`; `failed` exige inteiro diferente de zero; `pending` não admite `exitCode`, nem `null`.
4. Comandos têm até 4.096 caracteres, correspondem ao que realmente foi executado e incluem cwd/contexto suficiente para reprodução segura. Registrar todas as falhas, inclusive de suíte completa; não sobrescrever histórico para esconder uma falha. Não inferir aprovação/exit code de `tool:end.ok`.
5. `updatedAt`, quando presente, é inteiro positivo em milissegundos, nunca data ISO. `nextAction`, quando presente na raiz/tarefa, contém motivo e próxima ação em até 512 caracteres.
6. Proibidos `schemaVersion`, `blocker`, `diagnostic`, `branchSync`, `durableSync`, `followUp`, `title`, `evidence` e os campos de raiz `worktree`, `branch`, `head`, `planPath`, `status` ou `checks` solto. Motivos ficam em `nextAction`.
7. Checks vazios, falha vigente, ausência de resultado vigente ou revisão divergente impedem declarar validação integral. Mudança de código/config/lib invalida os resultados afetados; reaproveitar os demais apenas com vínculo factual preservado.
8. Aceite manual fica documentado no handoff e em `nextAction`/status quando pendente; não inventar comando bem-sucedido para uma observação humana. O JSON autodeclarado não prova execução nativa nem detecta checks omitidos.
9. Version 1 permanece compatível na retomada, sem provar validação integral automaticamente. Novo registro deve ser v2; não descartar progresso antigo sem reconciliação.

## 10. Critério de encerramento e revisão do plano

O escopo só pode ser declarado concluído quando bootstrap e regressões estiverem implementados, gates pertinentes estiverem aprovados na revisão final e uma abertura desktop real segura tiver comprovado janela **e conteúdo**. Se ainda houver pânico/travamento, dúvida de identidade do binário ou falta de ambiente nativo, registrar pendência e não relatar correção integral.

Revisão deste planejamento: pedido e preservações separados; defeito confirmado de frontend distinto de hipótese nativa; uso das três bibliotecas Max preservado; nenhuma migração/instalação necessária; testes novos atravessam registro real; validação agrupada depois da implementação; isolamento cobre os caminhos que escapam de MAXTERMINAL_HOME; E2E meramente declarativo não é tratado como executável; decisões condicionais têm limites e condições de interrupção; registro de retomada segue v2 sem ser criado em Plan Mode.
