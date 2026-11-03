# Plano de execução — barra nativa do sistema e F5

## Identificação e autorização

- Projeto: MaxTerminal. Data: 03/11/2026.
- Worktree e branch: `wt-7c84dcfa`.
- Raiz: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-7c84dcfa`.
- Especificação: [spec.md](./spec.md). Leia junto com este plano na primeira admissão.
- Este documento autoriza apenas planejamento nesta sessão. Implementação depende da etapa Execute. Não criar worktree, trocar branch, fazer commit/merge/push ou limpar a worktree; essas operações pertencem ao usuário/MaxCode.

## 1. Resultado esperado e preservações

Para quem usa o aplicativo Linux: a barra superior deve continuar sendo a barra real do sistema, com suas cores; sem integração correta de cores, usar fallback escuro quando o conteúdo estiver escuro. F5 deve recarregar a página interna, não reiniciar o aplicativo nem os processos de terminal.

Preservar: botões e comportamento de janela, bandeja/fechamento, atalhos diferentes de F5 simples, configurações de aparência, tema por projeto, preview e cancelamento de tema, followSystem, sessões e PTYs existentes, sequência dos eventos e limites de replay. Não prometer preservação de campos de formulário não salvos nem de memória exclusivamente do frontend após reload.

Não criar barra HTML, nova preferência de usuário, migrations, cache paralelo, endpoint ou comando Rust de reload. Não alterar bibliotecas irmãs. MaxComponentsUi permanece sendo a biblioteca de UI; MaxPinia já está instalado e suas stores permanecem a camada de estado/leituras/salvamentos; helpers novos devem vir de MaxUse.

## 2. Evidências e situação inicial

`git status --short` retornou vazio antes da documentação. Nesta sessão só se fizeram leituras e escrita de spec/plano; nenhuma validação de execução foi realizada.

| Evidência existente | Conclusão delimitada |
| --- | --- |
| `src-tauri/tauri.conf.json:14–26` | Janela `main`, `decorations: true`, sem tema fixado. Manter. |
| `src/components/shell/AppShell.vue:2–5` e `src/app/App.vue:1–21` | Slot opcional de título existe, mas a raiz não o usa. Não há barra customizada a remover nesse fluxo. |
| `src/composables/useThemeApplier.ts:19–47` | Bootstrap e mudanças do tema efetivo atualmente forçam dark/light na janela. |
| `src/theme/nativeWindow.ts` | Guard de Tauri, deduplicação e serialização, porém só aceita dark/light; null atualmente é sentinela. |
| `src-tauri/capabilities/default.json` | Não concede set-theme, apesar da chamada JS. Isso exige corrigir a ACL, não concluir que o SO não suporta tema. |
| `src-tauri/tests/capabilities.rs:26–53` | Testa lista exata de permissões: atualizar em conjunto. |
| `node_modules/@tauri-apps/api/window.d.ts:1121–1131` | null segue sistema; efeito app-wide em Linux/macOS. |
| `src-tauri/gen/schemas/linux-schema.json` | Permissão exata `core:window:allow-set-theme` disponível. Não editar schemas gerados. |
| `src/stores/theme.ts` | Fonte atual de tema efetivo, preview, persistência e cache. Reutilizar. |
| `src/app/plugins.ts:18–22` | Pinia com createMaxPinia e MaxComponentsUi já instalados. |
| Contratos instalados MaxComponentsUi/MaxUse | MaxButton já existe e é usado; MaxUse reexporta VueUse. Não há necessidade de UI nova; usar useEventListener para helper de evento após conferir export. |
| `src/composables/useAgentShortcuts.ts:31–37,220–330` | Atalhos de agente usam captura global; F5 não consta na lista reservada. |
| `src/terminal/engine/terminalInstance.ts:291–317` | xterm tem handler próprio; F5 precisa ser capturado antes de chegar ao terminal. |
| `src/components/layout/AppLayout.vue:49–74`, `src/composables/workspace/useWorkspaceEvents.ts` | Remontagem faz snapshots e registra eventos com ordenação e resync. |
| `src/stores/tabs.ts:220–255` | Hidratação usa IDs do snapshot; não é necessário abrir abas novas para recuperar a UI. |
| `src/stores/terminal.ts`, `src/stores/sessions.ts` | Registro de instância e criação de sessão são operações distintas: reload não é nova sessão. |
| `src/terminal/stream/ptyStreamHub.ts:355–367` | release faz detach, não kill. Isso não prova limpeza de attachments após desaparecimento abrupto do documento. |
| `src-tauri/crates/mt-os/src/lib.rs:421–423` | Handshake ui_ready tem guarda atômica idempotente; conferir restante do boot antes de validar reload. |
| `package.json`, `scripts/check.mjs`, `vitest.config.ts`, `tests/setup.ts`, `scripts/sync-local-libs.mjs` | Comandos e harness conhecidos; suíte usa happy-dom, dedupe de Vue/Pinia e gate de integridade das libs. |

Versões factuais: API/CLI Tauri 2.5.0; crate Tauri 2.11.6 no Cargo.lock; Vue 3.6.0-rc.13; Vitest 5.0.5; MaxComponentsUi instalado 1.1.36 e MaxUse instalado 2.0.1. Não atualizar versões para este pedido. A documentação oficial consultada e suas limitações constam na spec.

## 3. Decisões e contratos

### Confirmadas pelo pedido

1. Decoração nativa e cores do SO são a primeira escolha. Escuro é fallback, não motivo para impor sempre o tema de conteúdo à barra nativa.
2. F5 simples significa reload da página interna. Isso intencionalmente toma precedência sobre F5 de TUIs.
3. Trabalhar exclusivamente na worktree indicada; não tocar dados reais nem publicar mudanças.

### Solução proposta

- Política separada do transporte nativo: alvo `NativeWindowThemeTarget = 'dark' | 'light' | null`, com `null` representando sistema. Caminho principal null; fallback dark apenas quando a integração correta for comprovadamente indisponível e o conteúdo estiver escuro; ao sair do fallback, null.
- Manter o adaptador serializado. Usar sentinelas distintas de null para não aplicado e sem pedido pendente. Não usar teste truthy para decidir se há alvo; null precisa entrar na fila e ser aplicado.
- Corrigir ACL com apenas `core:window:allow-set-theme` na capability local main e na asserção exata do teste. Não adicionar `core:window:default` ou `core:default`.
- `useThemeApplier` continua consumindo a mesma store. Preserve bootstrap, preview, tema por projeto e followSystem; altere apenas como deriva o alvo da janela.
- Composable novo de F5 registra listener em captura na raiz usando MaxUse. Para F5 sem modificadores e sem composição: consumir evento, inclusive repeats, e chamar `window.location.reload()` só uma vez. Usar guarda local de reload iniciado. Remover listener quando o escopo termina. Não respeitar defaultPrevented a ponto de permitir a um handler descendente sequestrar F5; registrar cedo e interromper propagação para impedir xterm/agente.
- Reservar F5 entre os atalhos da aplicação. Ctrl/Alt/Shift/Meta+F5 permanecem fora deste novo handler. F5 em formulários e modais também atualiza a página, como solicitado.
- Não usar `appStore.retry()` como reload: esse método só reexecuta bootstrap e não substitui o documento nem todo seu estado.

### Gate aberto de viabilidade, não decisão de produto

A API investigada não detecta se a cor desenhada na decoração corresponde ao sistema. Antes de implementar o fallback condicional, T1 precisa demonstrar um sinal confiável da integração nativa no backend gráfico realmente usado. Não usar o retorno de setTheme, Window.theme, matchMedia ou Linux isoladamente como prova de fidelidade visual.

Ler o código/documentação Tauri/GTK resolvido, identificar o responsável pela decoração e validar o comportamento em ambiente gráfico isolado. Preferir suporte existente, sem dependências novas. Se não houver forma confiável e simples, registrar `manualPending` com diagnóstico e próxima ação objetiva para escolher uma estratégia de fallback suportada; não alegar entrega integral. Não forçar dark incondicionalmente em todas as janelas Linux, pois isso elimina a prioridade das cores do sistema. Não ampliar para barra customizada sem decisão posterior.

Hipóteses ainda não comprovadas: fidelidade das cores do desktop atual; ausência de realimentação GTK → prefers-color-scheme; recuperação de todos os attachments depois de reload abrupto; disponibilidade de ambiente gráfico isolado. Testes simulados não resolvem essas hipóteses.

## 4. Etapas executáveis

### T1 — Preflight e contratos críticos (dependência das demais)

1. Ler AGENTS.md, esta spec/plano e alterações locais. Conferir trabalho já implementado; não sobrescrever trabalho de terceiros.
2. Conferir por leitura scripts/hooks, package/lockfiles, symlinks reais e exports das três libs. Conferir resolução única de Vue/Pinia e, onde o harness as usa, Axios. Não rodar sync sem --check nem reconstruir libs automaticamente. Gate falho bloqueia validação dependente, sem bypass.
3. Identificar por leitura recursos usados por testes e boot nativo: diretório de configuração, SQLite, cache, filas e storage. Só autorizar execução que comprove destinos descartáveis. Worktree, transações e --env=testing sozinhos não provam isolamento. Não copiar .env nem migrar/resetar storage compartilhado.
4. Resolver o gate de integração nativa descrito na seção 3. Estabilizar contrato da política e sua evidência antes de modificar o caminho de tema. Se precisar de observação nativa, realizá-la apenas durante Execute, com ambiente seguro e autorização operacional pertinente.
5. Completar por leitura o caminho de UI ready, comandos iniciais e attachments no backend: confirmar que novo documento usa processos vivos, não restauração de boot que os duplica. Não adicionar limpeza assíncrona em beforeunload como garantia de correção.

Saída: contrato de fallback sustentado, riscos conhecidos e isolamento demonstrado, ou tarefas dependentes explicitamente blocked/manualPending com próxima ação. Nenhum comando de instalação ou teste nesta fase Plan.

### T2 — Decoração e política de tema

Arquivos existentes: `src/theme/nativeWindow.ts`, `src/composables/useThemeApplier.ts`, `src-tauri/capabilities/default.json`, `src-tauri/tests/capabilities.rs`, `tests/theme/nativeWindow.spec.ts`, `tests/composables/useThemeApplier.spec.ts`.

1. Implementar alvo sistema/null e política verificada em T1, sem modificar a configuração já correta de decorations.
2. Reconciliar fila com null e preservar retentativa após erro, deduplicação e última solicitação prevalecendo. Documentar o que o booleano retornado garante; não tratá-lo como prova visual.
3. Atualizar autorização mínima e seu teste exato juntos.
4. Acrescentar testes para sistema, fallback escuro, retorno a sistema, navegador sem Tauri, API ausente/rejeitada, null concorrente, alternância rápida e reentrada após erro. Cobrir bootstrap escuro/claro, preview/cancelamento, troca de projeto e followSystem na integração.

Conclusão por conteúdo: não há força de dark/light sobre o caminho nativo funcional; null não se perde por ser sentinela. Validação executada posteriormente, em lote.

### T3 — F5 na raiz

Novos: `src/composables/usePageReloadShortcut.ts`, `tests/composables/usePageReloadShortcut.spec.ts`.
Existentes: `src/app/App.vue`, `src/composables/useAgentShortcuts.ts`; localizar e estender testes existentes de atalhos, sem inventar nome de arquivo.

1. Usar `useEventListener` exportado por MaxUse com fase de captura e lifecycle do escopo. Instalar uma vez em App.vue, inclusive durante carregamento/erro, e não em cada terminal.
2. Reconhecer somente F5 simples; consumir a tecla antes do xterm e impedir reload duplicado e autorepeat. Não interferir com composição ou combinações modificadas.
3. Reservar F5 no matcher dos agentes. Não adicionar extensões/handlers concorrentes em cada xterm se captura raiz resolver.
4. Testar chamada única, várias ocorrências, repeat, composição, modificadores, teclas diferentes, target textarea/modal/terminal e remoção do listener no unmount. Usar função de reload substituível no teste para não navegar de verdade.
5. Testar que F5 não abre agente nem produz input no terminal, e que outras teclas seguem passando.

Conclusão por conteúdo: reload real do documento, não retry de bootstrap, quit ou nova sessão. Nenhuma necessidade de componente visual novo; preservar MaxComponentsUi.

### T4 — Regressões e validação agrupada

1. Concluir todo o escopo e seus testes antes de executar suíte, tipos ou build. T2 e T3 podem ter arquivos distintos, mas um único responsável deve revisar contratos compartilhados e integração raiz. Delegação não está autorizada por este plano; se futuramente autorizada, fornecer só tarefa, contrato, revisão factual e referências necessárias.
2. Nos testes existentes de workspace/stream, acrescentar apenas lacunas relevantes: snapshots preservam IDs, novo consumidor reanexa, não chama kill/open/newSession nem comando inicial. Revalidar detach/cleanup no backend se o gate T1 encontrar necessidade. Se a correção exigir remodelar streaming ou persistência, interromper para revisão de escopo, não mascarar falha.
3. Revisar o diff em conjunto: superfície ACL, sem alteração de cores globais do SO, sem efeitos colaterais de F5, sem perda de null na fila e sem reintrodução de PrimeVue.
4. Após isolamento e integridade confirmados, executar `npm run check` na raiz. Ele já agrega gate `node scripts/sync-local-libs.mjs --check`, ESLint, Stylelint, tsgo, vue-tsc, cobertura Vitest, build Vite e fmt/clippy/test Rust. Não repetir as mesmas etapas sobre revisão idêntica.
5. Se falhar, diagnosticar logs/harness de forma agrupada, corrigir o lote e revalidar o que ficou inválido ou não executado. O agregador para na primeira falha: etapas posteriores não estão aprovadas. Não remover prehooks, ignorar configuração, reduzir asserções ou contornar bibliotecas inválidas. Falha repetida sem progresso exige causa e próxima ação, não execução idêntica indefinida.

### T5 — Aceite nativo e handoff

Em instância Tauri apontada a dados descartáveis, nunca na sessão de trabalho real do usuário:

1. Registrar SO, sessão gráfica/compositor e backend de decoração observados, sem capturar segredos. Comparar a barra com uma janela nativa do mesmo ambiente e verificar controles, mover, maximizar e fechar/bandeja.
2. Verificar sistema claro/escuro versus conteúdo claro/escuro sem alterar preferências globais do usuário. Se a matriz exigir outra configuração de SO, usar sessão de teste separada ou participação manual autorizada. Confirmar prioridade nativa e fallback real; testar troca de tema, preview/cancelamento e reload sem realimentação de followSystem.
3. Abrir processo descartável com saída contínua e TUI descartável. Anotar IDs de PTY/sessão e PID antes do reload. Pressionar F5 sobre terminal, input, modal e estado de erro; confirmar que o documento realmente reinicializou, o backend e os processos mantiveram identidade e o input/output voltou.
4. Confirmar ausência de comandos iniciais repetidos, abas/sessões/processos duplicados e attachments acumulados. Não exigir scrollback ilimitado: respeitar replay atual e sinalização de visão parcial. Repetir poucos reloads deliberados; não usar loop de polling.
5. Sem ambiente gráfico/isolamento ou sem cenário real de fallback, registrar `manualPending` e procedimento restante. Dublês não aprovam aparência ou execução nativa. Não declarar tarefa integralmente concluída com gates pendentes.

## 5. Matriz de aceite

| Tarefa/requisito | Evidência esperada | Testes pertinentes | Revisão pelo risco | Verificação manual |
| --- | --- | --- | --- | --- |
| T1: viabilidade e isolamento | Contrato de backend e destinos descartáveis identificados | Gate de integridade antes da suíte dependente | Dependências reais, identidade dos módulos, persistência | Ambiente gráfico seguro identificado |
| T2: barra nativa/sistema | decorations preservado; política envia null no caminho principal | Adaptador e useThemeApplier, incluindo bootstrap/projeto/preview | Não sobrepor cores nativas corretas | Comparar barra/controles com janela do SO |
| T2: fallback escuro | Sinal de indisponibilidade documentado e alvo dark condicional | Fallback, retorno a null, concorrência e falha | Não usar ACL/IPC como detector visual; efeito app-wide | Cenário real sem integração nativa correta |
| T2: ACL mínima | Uma permissão set-theme, main/local mantidos | Teste Rust de capabilities com lista exata | Sem grants genéricos ou remotos | Ausência de erro ACL na janela Tauri |
| T3: F5 simples | Uma chamada de reload, evento consumido na captura | Composable, targets descendentes, repeats e combinações | Sem quit/kill/newSession; não abrir agente | F5 sobre xterm, input e modal |
| T4: continuidade | Mesmos IDs/processos; reanexo ao snapshot | Workspace/stream e integração de atalhos | Corridas, attachments e comando inicial | Processo contínuo e TUI permanecem vivos |
| T4/T5: qualidade e entrega | Checks da revisão final e pendências explícitas | npm run check completo, após os gates | Revisão independente se disponível/autorizada no marco final | Sem aprovação visual simulada |

## 6. Riscos e limites operacionais

- **Cores nativas:** gerenciador de janelas e GTK podem divergir; não há detecção de fidelidade visual na API JS inspecionada. O gate de T1 é obrigatório, não detalhe a preencher inventando garantia.
- **Efeito global:** setTheme afeta a aplicação inteira no Linux/macOS. Validar interação com prefers-color-scheme e não mudar configuração persistente do SO.
- **Reload:** destrói memória JS e não garante hooks Vue de unmount. Processos e limpeza de assinaturas precisam ser corretos no backend independentemente disso. Rascunhos podem ser perdidos como em atualização de página comum.
- **Segurança:** permitir apenas set-theme à janela local existente; F5 não precisa de nova permissão de navegação/execução ou acesso remoto.
- **Operação:** nada de instalação, reset de banco ou teste contra perfil real sem autorização. A sessão Plan não executou nenhum teste nem abriu aplicativo.
- **Recuperação:** não há migração a desfazer. Em implementação problemática, revisar/reverter apenas as alterações próprias, mediante autorização, preservando trabalho preexistente. Não usar reset/checkout destrutivo.

## 7. Estado de execução e retomada (somente Execute)

O executor registra progresso em `docs/plans/wt-7c84dcfa/execution-state.json` após marcos e no handoff. Não criar esse arquivo durante Plan. Calcular SHA256 real dos bytes de plan.md como planRevision; calcular revision de tarefas e verification por conteúdo factual documentado e determinístico dos arquivos pertinentes. Nunca usar nomes de branch, rótulos ou hashes inventados como revisão.

Contrato version 2 estrito, sem campos adicionais:

- Raiz: `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`. `updatedAt`, se presente, é inteiro positivo em milissegundos. Não usar schemaVersion, worktree, branch, head, planPath, status ou checks soltos na raiz.
- Tarefa: `{id, status, files, validations, revision?, nextAction?}`. `validations: []` é sempre obrigatório; usar lista vazia quando nenhuma validação estiver registrada, nunca omitir. IDs: T1 a T5. Não adicionar title ou evidence.
- Estados de tarefa: `pending`, `running`, `completed`, `blocked`, `manualPending`, `validationFailed`.
- Verification: `{revision: SHA256, checks: [{command, revision: SHA256, result, exitCode?}]}`. Checks só dentro de verification.checks, com exatamente essas chaves permitidas.
- Resultados de validações/checks: somente `passed`, `failed`, `pending`, nunca blocked. Passed exige exitCode 0; failed exige inteiro diferente de 0; pending não aceita exitCode. Não usar null como código. `validations` deve registrar resultados nesse mesmo vocabulário, sem inventar checks executados.
- Limites: command de até 4.096 caracteres; nextAction de até 512 caracteres. Não adicionar blocker, diagnostic, branchSync, durableSync ou followUp em nenhum nível; explicar impedimentos em nextAction.
- Registrar comandos realmente executados, inclusive todas as falhas da suíte completa. Não inferir êxito/exitCode de tool:end.ok. Se processo não forneceu código, manter pendência e próxima ação; não falsificar resultado failed com código inventado.
- Checks vazios, resultados ausentes/desatualizados ou falha vigente impedem validação integral. Version 1 pode ser lida por compatibilidade, mas não comprova aprovação automaticamente. O arquivo é autodeclarado, não prova execução nativa e não detecta verificações omitidas.

Na retomada: consultar trechos pertinentes do plano, conferir documentos/código e estado já existente, comparar hashes reais, identificar lacunas e seguir dependências. Não reabrir decisões sem nova evidência. Reusar checks aprovados só quando o conteúdo coberto permanecer idêntico; mudanças relevantes invalidam resultados correspondentes. Assíncronos aguardam conclusão/notificação com tempo limitado e recuo, sem consultas repetidas sem informação nova. Cancelamento preserva resultado parcial e pendência explícita. Bloqueio ou manualPending sempre traz próxima ação concreta.

## 8. Revisão deste planejamento

Pedido, escopo e preservações conferidos: não substituir barra já nativa, não confundir tema do conteúdo com cores do sistema, não transformar F5 em reinício de sessão. Dependências e testes estão mapeados. Limitação crítica de detecção do fallback está explícita como gate, não ocultada por um booleano de IPC. As validações previstas não foram executadas. A entrega completa dependerá da comprovação nativa, não só de testes simulados.
