# Especificação técnica — Horizon no cabeçalho do projeto

## 1. Contexto & Objetivo

- Projeto: **MaxTerminal**. Data: **03/11/2026**.
- Worktree e branch existentes: `wt-1b4b2671`.
- Raiz exclusiva: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-1b4b2671`.
- Solicitação: adicionar `MaxBadgeButton` **HORIZON** em `project-workspace__header`, disponível em projetos com Laravel Horizon, com três contadores; abrir modal com terminal, iniciar/parar Horizon e limpar tarefas pendentes.
- Decisão confirmada pelo usuário: **limpar somente pendentes, inclusive agendadas, preservando tarefas reservadas/em execução**. Não substituir por `horizon:clear`.
- Esta sessão produz apenas esta especificação e `plan.md`. Nenhuma implementação, comando PHP, teste, build ou alteração de serviço foi executada.

## 2. Escopo

### Incluído

1. Detectar Horizon instalado e executável na raiz Laravel do projeto selecionado, sem depender apenas do rótulo da stack.
2. Mostrar três overlays sempre na ordem: pendentes (azul), em execução (verde), erros (vermelho). Zero é valor válido; ausência de medição não é zero.
3. Modal `MaxModal` com estado do Horizon, escopo monitorado, saída real do processo gerenciado e ações `MaxButton`.
4. Processo PHP supervisionado pelo PTY existente, independente das abas comuns. Fechar modal não para o processo.
5. Limpeza específica e confirmada das filas configuradas, sem remover reservas, falhas ou outras filas.
6. Estado/cache por contexto usando **MaxPinia**, IPC tipado e helpers **MaxUse**.
7. Testes automatizados e aceite real em fixture isolada, além das permissões Tauri e ciclo de vida no tray/shutdown.

### Fora do escopo

- Instalar/configurar Horizon, Redis, PHP ou dependências no projeto monitorado; gerar comandos, rotas ou arquivos Laravel nele.
- Abrir dashboard HTTP, autenticar no dashboard, listar/reexecutar tarefas ou apagar histórico de falhas.
- Executar migrations/resets; alterar SQLite; persistir payloads de jobs ou credenciais.
- Assumir controle ou capturar retrospectivamente stdout de processos Horizon externos.
- Habilitar Sail/Docker sem supervisão, operar servidores remotos ou reformular abas/terminal/tema global.
- Commit, merge, push, criação/remoção de worktree. A worktree fornecida pertence ao fluxo do MaxCode.

## 3. Arquitetura & Mudanças Técnicas

### 3.1 Evidências e limites

| Evidência existente | Consequência para a solução |
| --- | --- |
| `src/components/projects/ProjectWorkspace.vue:3–26` contém cabeçalho e ações; bloco sem projeto é separado | Inserção apenas no cabeçalho com projeto; preservar busca e indicadores atuais |
| `src-tauri/crates/mt-commands/src/artisan/root.rs` resolve raiz e fronteiras Git; `commands/quick_commands.rs` oferece `DbProjectLookup` | Reutilizar conceitos, mas autorizar por ID/caminho exato do projeto no novo fluxo; não confiar no fallback amplo de worktrees irmãs |
| `artisan/runner.rs` executa apenas `list_json`; `run_bounded` limita processos finitos; Sail está desabilitado | Criar runner Horizon específico; não fingir que existe API genérica no ArtisanRunner; daemon não usa timeout de descoberta |
| `src/app/plugins.ts` instala `createMaxPinia()`; nenhuma store em `src` encontrada com `isCached` | Apenas criar outra store Pinia simples não satisfaz o pedido de MaxPinia/cache |
| MaxPinia local: `src/plugin.ts`, `src/types.ts` | GET usa Axios injetável, `options.get.route`, `reload`, `onServerData`; plugin é ativado por `isCached` e chave inclui `$id` e `id` |
| MaxComponentsUi local: `MaxBadgeButton.vue`, `MaxBadge.vue` | `overlay` aceita `OverlayData[]`, cada item `{value, color}`; não existe prop `badges` nem tooltip individual no item; zero não é filtrado |
| `TerminalView.vue`, `useTerminal.ts`, `instanceRegistry.ts` | Terminal reutilizável por `paneId`/`ptyId`; desmontar apenas desanexa; nova geração deve usar novo paneId ou descartar a anterior |
| `mt-pty/src/types.rs` e `spawn/shell.rs` | `SpawnSpec.shell` aceita executável PHP com argumentos explícitos, sem shell intermediário; owner pode ter painel sem aba |
| `mt-pty/src/kill.rs` | `KillMode::Graceful` escala sinais no cgroup inteiro; não equivale a aguardar jobs do Horizon |
| `src-tauri/src/adapters/os.rs:134–151` | Atividade atual vem de abas/shell; PTY sem aba exige inclusão explícita na decisão de minimizar para tray |

Bibliotecas locais consultadas em `/home/johnattas/GitHub/{MaxComponentsUi,MaxPinia,MaxUse}`; o link de MaxComponentsUi foi resolvido por leitura. Não foi executado o gate de integridade. Não existe aplicação Laravel/Horizon-alvo indicada na solicitação; sua versão, Redis, permissões e isolamento real continuam gates do Execute.

Fontes oficiais consultadas:

- Context7 `/laravel/horizon` e catálogo com referência `v5.48.3`; consulta específica nessa versão não retornou conteúdo. A consulta geral retornou documentação da branch 5.x.
- Código oficial fixado em `https://github.com/laravel/horizon/tree/v5.48.3`: `src/Console/ClearCommand.php`, `src/Repositories/RedisJobRepository.php`, `src/MasterSupervisor.php`.
- `https://github.com/laravel/framework/blob/13.x/src/Illuminate/Queue/LuaScripts.php`.
- Fontes mostram: `horizon:clear` chama `purge` (inclui pending/reserved) e limpeza da fila; `clear()` do framework exclui ready/delayed/reserved/notify; `size()` soma os três estados. `countPending()` do Horizon inclui reservas; não serve diretamente para o overlay azul. Resumos da documentação não substituem as assinaturas do código instalado.

### 3.2 Componentes e fluxo propostos

```text
ProjectWorkspace -> HorizonBadgeButton -> store Horizon (MaxPinia)
                                           |
                                  adapter Axios restrito
                                           |
                                    IPC horizon_*
                                           |
                 HorizonService + probe PHP + runner limitado
                       |                         |
                  PtyManager              Laravel/Redis do alvo
                       |
                TerminalView no MaxModal
```

- Backend de métricas/helper PHP em novo módulo `mt-commands/src/horizon/`; composição PTY e lifecycle em novo `src-tauri/src/horizon/` e comandos em `commands/horizon.rs`.
- Helper PHP é código constante embarcado no binário, executado via PHP com argumentos estruturados e cwd autorizado; não gravar scripts dentro da aplicação monitorada. Bootstrap pelo autoload e kernel de console da aplicação instalada, sem Tinker ou dashboard. Capturar warnings/ruído separadamente de um envelope JSON identificado e limitado. Não aceitar PHP arbitrário do frontend.
- Novos componentes `src/components/horizon/HorizonBadgeButton.vue` e `HorizonModal.vue`; nova store `src/stores/horizon.ts`; nova fronteira `src/ipc/horizon.ts`; novo adapter `src/ipc/horizonMaxPiniaAdapter.ts`.
- Ajustes nos registros Rust, Specta, ACL e boot Vue; testes novos em diretórios existentes de teste. Relação completa e responsabilidades em `plan.md`.

### 3.3 Contexto, descoberta e medição

- O contexto é **projeto selecionado + raiz canônica cadastrada**, não cwd da aba ativa. Não mudar escopo ao alternar abas/splits.
- Detectar `laravel/horizon` instalado pelo autoload/metadata local e comandos realmente registrados, após evidência estática barata; mera declaração em composer.json ou lock não comprova instalação executável. Não executar catálogo em todo render; a descoberta existente pode gravar cache no alvo, portanto o novo probe não deve invocá-la implicitamente.
- Estados de disponibilidade: `checking`, `unavailable`, `available`, `error`. Indisponível oculta botão; erro transitório após disponibilidade mantém botão e explica falha, sem fabricar zeros.
- Resolver ambiente ativo, defaults/overrides dos supervisores e pares únicos `(connection, queue)`. Não somar duas vezes a mesma fila. Rejeitar driver incompatível e configuração curinga/não resolvida para mutações; explicar limitação.
- Escopo de filas deve aparecer no modal e confirmação. Identidade inclui fingerprint opaco do destino efetivo, ambiente, prefixo Horizon e conjunto de filas; não retornar URL autenticada, senha, .env ou configuração completa ao frontend.
- **Pendentes**: quantidade em ready + delayed das filas resolvidas; **em execução**: reservas Redis dessas filas. Explicar no modal que a reserva é o indicador do Laravel e pode permanecer até retry_after após queda do worker; não prometer inspeção instantânea de CPU.
- **Erros**: `JobRepository::countFailed()` do namespace Horizon ativo, respeitando retenção de falhas do Horizon; não apenas falhas da última execução do terminal. Explicitar que é o total do namespace, que pode ser compartilhado. Não usar `totalFailed()`/ZCARD sem a semântica de retenção desejada.
- Consultas limitadas, timeout, stdout/stderr limitados, sanitização e deduplicação. Cache de descoberta: proposta 30 s; métricas: proposta 5 s em projeto visível, 2 s com modal aberto, uma consulta em voo; sem sondagem de todos os projetos. Backoff 5/10/20/30 s e pausa com janela oculta; retomar uma atualização ao voltar.
- Snapshot carrega revisão monotônica por contexto e data; cache histórico aparece como desatualizado até leitura viva. Falha parcial permite contadores `null` individuais; não computar total a partir de parte das filas sem avisar.

### 3.4 Contratos propostos, ainda não existentes

Definir DTOs Rust com serde camelCase e gerar bindings; nomes abaixo são decisões desta proposta, não APIs já disponíveis.

- `HorizonContext`: `projectId`, `root`, `contextRevision` (hash opaco dos insumos efetivos) e `scopeId` (fingerprint opaco do destino).
- `HorizonSnapshot`: contexto, disponibilidade, versão Horizon detectada, pares connection/queue, ambiente, estado `stopped|starting|running|stopping|external|error`, `pending/running/failed: number|null`, `sampledAtMs`, `revision`, diagnóstico sanitizado, capacidades `canStart/canStop/canClearPending` e sessão opcional `{ptyId,paneId,generation}`.
- Capacidades retornadas não substituem autorização no backend. Respostas atrasadas não podem atravessar projeto/raiz/revisão; sessão de runtime nunca é recuperada como viva a partir do cache persistente.
- `horizon_snapshot(projectId)`: descobre/mede e reconcilia runtime; operação não inicia Horizon nem limpa tarefas.
- `horizon_start(projectId, expectedContextRevision, requestId)`: valida novamente o contexto, evita duplicação por raiz/destino e devolve snapshot/sessão.
- `horizon_stop(projectId, expectedGeneration, requestId)`: age somente sobre processo gerenciado confirmado; devolve estado em parada ou já parado. Eventos PTY/snapshot comprovam conclusão.
- `horizon_clear_pending_preview(projectId)`: devolve token opaco, revisão, escopo, estimativa e validade curta (proposta 30 s).
- `horizon_clear_pending(projectId, token, requestId)`: consome confirmação uma única vez, revalida destino e devolve removidas por fila, parcial/limite e aviso de reconciliação quando aplicável.
- Usar envelope `MtError` e `src/ipc/core.ts`; mapear ausência de PHP, dependência ausente, Redis inacessível, contexto alterado, versão/driver incompatível, stop pendente, conflito externo e resultado parcial sem expor segredos. Não exigir endpoint HTTP Laravel novo.

### 3.5 Runtime e terminal

- Iniciar apenas por clique, com `php artisan horizon` como processo principal de PTY supervisionado, argumentos separados e cwd autorizado; sem `sh -c`, sem injetar comando na aba do usuário e sem autostart.
- Um runtime por raiz/destino efetivo; lock backend, idempotência por requestId, geração e identidade de processo. Duas worktrees com mesmo Redis/prefixo exigem conflito explícito, não dois daemons silenciosos.
- Reservar `paneId` real e exclusivo por geração e `owner.tabId = None`; correlacionar root/projeto/sessão no serviço. UI reutiliza `TerminalView`, sem dois consumidores simultâneos para um mesmo PTY. Preservar buffer limitado e saída entre fechar/reabrir modal enquanto a sessão existe.
- Início do PTY não prova Horizon pronto: reconciliar registro/heartbeat do master e falhas de bootstrap. Não marcar `running` só por exit code de spawn.
- Parar normalmente por `SIGTERM` **somente ao master gerenciado**, com identidade validada por `PtyManager::signal_process`, e aguardar término de jobs/descendentes. Não executar `horizon:terminate` global nem `ptyKill(graceful)` como equivalente silencioso. Prazo excedido mantém `stopping`/erro com ação concreta; não forçar nem apagar referências ao processo vivo. Revalidar o comportamento da versão instalada, incluindo fast_termination e término do líder antes dos descendentes.
- Fechar modal/trocar projeto só desanexa UI. Remover projeto e sair do aplicativo executam política supervisionada existente; shutdown explícito pode encerrar processos segundo política do app, com aviso se houver jobs. Incluir runtime e spawns pendentes no `ActivitySnapshot` para fechamento normal minimizar ao tray.
- Processo externo: mostrar `external`, impedir início duplicado e bloquear parada indiscriminada; explicar que saída anterior não está acessível. Não fingir anexação ao terminal externo. Não há controle remoto nem adoção automática nesta entrega.

### 3.6 Limpeza somente de pendentes — requisito crítico

- Confirmação `MaxModal` mostra projeto, ambiente, filas, estimativa, agendadas incluídas, irreversibilidade e possível compartilhamento. Cancelar não altera nada. Confirmação backend vinculada ao contexto, com token temporário e consumo idempotente.
- Proibidos `horizon:clear`, `queue:clear`, `FLUSHDB`, `FLUSHALL` e `JobRepository::purge`: todos têm escopo/efeito incompatível.
- Implementar adapter Redis por versão suportada dentro do helper PHP, usando conexões/prefixos resolvidos pelo Laravel. Nunca montar credenciais no Rust nem hardcode de prefixo `queues:` sem validar contrato instalado.
- Retirar **apenas** entradas ready e delayed em lotes atômicos pequenos (proposta 100), removendo apenas notificações correspondentes aos ready retirados. Nunca incluir `:reserved` nas chaves mutadas. A reserva concorrente e a retirada devem ser serializadas no Redis; job já reservado fica intacto.
- Impor orçamento total por operação (proposta 10 s/10.000 entradas), finalizar com resultado parcial explícito se ultrapassado; não repetir automaticamente limpeza após timeout incerto. Filas novas/produtores concorrentes podem gerar pendentes depois; não prometer que o contador permanece em zero.
- Reconciliar metadados Horizon somente dos IDs realmente retirados; preservar registros reserved/failed e retries concorrentes. Não varrer nem purgar pending_jobs globalmente. Como metadados Horizon e filas podem usar conexões diferentes, não prometer transação única: falha após remoção registra resultado parcial e aviso de metadados, sem reaplicar exclusão. Não exportar payloads para Rust/UI/logs.
- Gate obrigatório: provar contratos de chaves, cliente Redis/cluster, atomicidade, notificação e metadados na versão instalada antes de habilitar. Em versão sem garantia verificável, desabilitar ação com motivo; **não reduzir o escopo nem usar limpeza ampla como fallback**. Limitação não equivale a aceite total.

### 3.7 Frontend e MaxPinia

- `MaxBadgeButton label="HORIZON"`, `overlay` com três `{value,color}`. Cores: azul `var(--blue-b-500)`, verde `var(--max-success-500)`, vermelho `var(--max-danger-500)`. A biblioteca define `--blue-b-500: #8FBCF5` na paleta clara (`src/themes/colors.scss:517`); `--blue-500` é ciano (`#56C2D7`), assim como a família info do projeto tende ao teal. Usar a paleta azul existente, sem redefinir tema global, e revalidar contraste na versão efetiva.
- Overlays sem dado usam `—` nas três posições; título e aria-label explicam cada número/estado; legenda no modal e tooltip geral, pois OverlayData não tem campo tooltip. Controlar modelValue pela abertura do modal, para o toggle interno não ficar divergente ao fechar por Escape.
- Modal com altura limitada, terminal redimensionado ao abrir, loading/erro/retry, controles bloqueados enquanto ação conflitante ocorre e foco restaurado no badge. Preservar legibilidade light/dark e cabeçalho estreito.
- Store de leitura com `isCached`, `options.get.route`, `id` por projeto/raiz/contexto, `reload` e status do MaxPinia. Persistir somente snapshot sanitizado, nunca sessão PTY viva, token, payload, saída terminal ou flags de mutação. Não configurar auto-save remoto para ações destrutivas.
- MaxPinia suporta Axios injetável, não callback IPC em `options.get.route`. Injetar instância Axios com adapter de rota sintética restrita ao snapshot Horizon em `registerAppPlugins`; devolver AxiosResponse com `data` do snapshot e respeitar AbortSignal. Rotas não Horizon mantêm transporte padrão intacto. Ações explícitas usam IPC tipado na store, sem auto-save/replay; sucesso provoca reload.
- Helpers de polling, visibilidade e temporização vêm de `@maxvue/max-use`; não criar loop em componente nem importar VueUse paralelo. Confirmar exports efetivos antes de uso.

## 4. Plano de Execução Faseado

1. **T1 — Contratos/preflight por leitura:** revalidar worktree, bibliotecas, ambiente isolado de fixture e contratos Laravel/Horizon instalados. Congelar DTOs, versões suportadas e política Redis antes dos dependentes.
2. **T2 — Métricas e limpeza:** implementar módulo Horizon/testes de domínio, PHP embarcado, validações de raiz/contexto, limites e limpeza exclusiva com concorrência e falha parcial.
3. **T3 — Runtime nativo:** PTY, identidade/geração, start/stop, reconciliação, remoção de projeto, tray e shutdown; testes de lifecycle.
4. **T4 — Integração IPC/MaxPinia:** registrar ACL/Specta, gerar bindings, adaptar leitura para plugin real e testar cache/cancelamento/identidades.
5. **T5 — UI:** badge/modal/terminal e testes completos de integração visual e interações.
6. **T6 — Validação e entrega:** revisão de risco, suíte canônica completa depois de todo código/testes, correções em lote e aceite real isolado. Manter `manualPending` se fixture/ambiente não estiver disponível.

Não executar testes/builds entre microedições. Apenas tarefas com arquivos de escrita disjuntos podem ser paralelas após contratos estabilizados; a presente sessão não delegou trabalho.

## 5. Critérios de Aceite & Validação

- Botão aparece somente com disponibilidade comprovada; não depende de Horizon já estar rodando.
- Três overlays corretos e ordenados, inclusive zeros; dado desconhecido/obsoleto claramente distinguido.
- Clique abre modal com saída real; fechar/reabrir não reinicia processo; restart cria geração/painel sem conflito.
- Iniciar não toca terminal do usuário; cliques concorrentes não duplicam; parar age só no master proprietário e informa estado até terminar.
- Limpeza confirmada retira ready/delayed das filas autorizadas e preserva bytes/membros de reserved, failed e filas fora do alvo, inclusive sob corrida. Sem fallback amplo e sem mentir sobre resultado parcial.
- Troca de projeto/configuração/worktree não aplica resposta ou confirmação antiga; cache nunca autoriza mutação ou restaura PTY vivo.
- Fechar janela com Horizon gerenciado mantém processo no tray; saída explícita/remoção não deixa órfãos.
- Sem alterações Laravel, .env copiado, migrações ou logs com segredos.
- Validações futuras confirmadas no projeto: `npm run bindings` para geração deliberada; `npm run check` executa integridade das libs, lint, styles, tipos TS/Vue, cobertura, build, fmt check, clippy e testes Rust. Não executadas nesta sessão. A matriz e procedimento seguro constam em `plan.md`.
