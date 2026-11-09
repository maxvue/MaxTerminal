# Plano de implementação — Issue #14

### Descrição e Causa Raiz

**Veracidade: bug confirmado no código e em reprodução isolada.** Base analisada: `feb8f520ff5c7a8f9cace41e393a088c24c5fe9c`. O painel mantém o adaptador durante sua vida útil; cada execução adiciona chaves e uma função de cancelamento, embora a store descarte blocos antigos. A retenção cresce com o total histórico de comandos, não com os blocos visíveis. O custo inclui strings, funções e referências capturadas; esta análise não quantifica bytes de heap nem atribui todo crescimento de memória do aplicativo a este defeito.

**Causa raiz comprovada:**

- `src/composables/useShellCommandBlocks.ts:L68-70`: dois `Set<string>` e um array de cancelamentos são criados por instância, sem limite.
- `src/composables/useShellCommandBlocks.ts:L107-111` e `L163-167`: cada início/fim distinto adiciona uma chave `ptyId:blockId:commandSeq`; nenhum caminho remove essas chaves.
- `src/composables/useShellCommandBlocks.ts:L146-151`: o retorno de `runAfterFlush` é sempre acumulado, inclusive quando o callback já foi executado sincronamente. A conclusão apenas atualiza metadados.
- `src/composables/useShellMarkers.ts:L55-89`: há conclusão imediata sem renderer, por evento de flush ou por timeout de 24 ms. A rotina libera seu listener/timer, mas não remove a função guardada pelo adaptador. `src/terminal/engine/terminalInstance.ts:L487-495` confirma o registro e a remoção dos listeners reais.
- `src/stores/commandBlocks.ts:L113-117` e `L349-357`: a store retém, por padrão, 500 blocos **por painel**, descartando os mais antigos e seus marcadores. Essa remoção não alcança os acumuladores do composable. Também existem remoções por marcador/scrollback (`L267-286`) e limpeza completa (`L450-463`).
- `src/composables/useShellCommandBlocks.ts:L310-328`: `stop()` esvazia somente os cancelamentos. Os dois conjuntos continuam preenchidos enquanto a instância do adaptador permanecer referenciada; após perder todas as referências, tornam-se elegíveis à coleta. Portanto, o problema principal ocorre com o painel ainda ativo, não exige uma falha de desmontagem.

**Rastreamento reverso UI ⇄ stores ⇄ IPC ⇄ service ⇄ DB:**

1. `src/components/terminal/HybridTerminal.vue:L80-87,L142-151,L179-186` cria/inicia o adaptador quando o terminal fica pronto e o encerra ao trocar a vinculação ou desmontar.
2. O adaptador assina eventos e solicita uma página com `limit: 50` em `src/composables/useShellCommandBlocks.ts:L285-306`; `applyStarted`, `applyFinished` e `applyDto` projetam dados nas stores. `src/stores/shellHistory.ts:L139-185` já limita seu cache por sessão; essa proteção é independente dos acumuladores e da retenção visual por painel.
3. `src/ipc/shell.ts:L25-30,L49-65` liga a consulta `command_block_list` e os eventos Tauri aos consumidores. O transporte é IPC/eventos, sem rotas HTTP ou controller web neste fluxo.
4. `src-tauri/src/events/shell.rs:L103-128` emite os eventos. Para hidratação, `src-tauri/src/commands/shell.rs:L27-35` delega a `ShellService::command_block_list`, em `src-tauri/crates/mt-shell/src/service.rs:L398-434`, que consulta o repositório e pode complementar com cache.
5. `src-tauri/crates/mt-shell/src/persist.rs:L86-120` persiste início/fim e emite o término; `src-tauri/crates/mt-db/src/repos/command_blocks.rs:L295-300,L611` implementa a leitura da tabela `command_blocks`. A persistência não é responsável pela vida útil das coleções JavaScript.

**Reprodução executada nesta etapa, sem escrever código-fonte:** script Node via stdin transpila em memória o composable, as duas stores reais e `useShellMarkers`; usa Pinia real, substitui registry/terminal e temporizadores por controladores determinísticos e acrescenta apenas um leitor dos três tamanhos à cópia em memória. Processa 3.000 pares started/finished distintos no mesmo painel, disparando um flush após cada started. Resultado bruto:

```json
{"phase":"after-3000-flushes","blocks":500,"history":500,"started":3000,"finished":3000,"cancellations":3000,"listeners":0,"timers":0}
{"phase":"after-stop","started":3000,"finished":3000,"cancellations":0,"listeners":0,"timers":0}
```

Isso comprova a divergência mesmo com todos os flushes concluídos e sem listeners/timers pendentes. Não foi executado um ensaio desktop de longa duração. Os testes existentes em `src/composables/__tests__/useShellCommandBlocks.spec.ts:L39-125,L128-288` cobrem projeção/hidratação, mas não essa retenção.

### Arquivos afetados

Na implementação:

- `src/composables/useShellCommandBlocks.ts`: integrar retenção por bloco/painel, conclusão de flush e descarte.
- `src/composables/shellCommandBlockRetention.ts` **(novo)**: pequeno gerenciador interno, não reativo, de registros de deduplicação e cancelamentos pendentes; contagens consultáveis nos testes unitários, sem ampliar a API pública do adaptador.
- `src/composables/__tests__/shellCommandBlockRetention.spec.ts` **(novo)**: limites, identidade, remoção e ciclo de vida do gerenciador.
- `src/composables/__tests__/useShellCommandBlocks.spec.ts`: regressão integrada com stores reais e flushes controlados.

Artefato desta etapa: `docs/issues/14/plan.md`. Os demais arquivos citados são evidências/dependências para validação, sem mudança prevista de contrato.

### Execuções propostas

1. **Substituir os acumuladores por registros vinculados aos blocos retidos.** No gerenciador interno, usar mapas por painel e por `blockId`, com identidade completa (`ptyId`, `blockId`, `commandSeq`), flags separadas de início/fim e, opcionalmente, um cancelamento pendente. Não guardar payloads completos. Cada bloco retido admite no máximo um registro da sua execução e um flush pendente; identidade diferente substitui o registro e cancela eventual pendência anterior. Comparar campos, sem interpretar IDs por divisão de strings.
2. **Sincronizar com a store autoritativa.** Implementar reconciliação que remove registros cujo bloco saiu de `commandBlocksStore.byPane[paneId].blocks`, cancela suas pendências e elimina buckets internos vazios. Executá-la antes da deduplicação e após alterações de `applyStarted`, `applyFinished` e `applyDto`. Registrar uma execução nova somente depois de `feed.begin` e da retenção da store, se o bloco ainda existir. Preservar o filtro de painel/PTY/sessão antes de criar estado. A quantidade de registros deve ser menor ou igual à quantidade de blocos efetivamente retidos, respeitando `maxBlocksPerPane`, sem duplicar o literal 500.
3. **Cobrir remoções externas.** Observar apenas a estrutura de IDs retidos por painel, com `watch` batelado, sem observação profunda de metadados/output. Assim, `clearPaneBlocks`, `feed.clear`, descarte de marcador, `pruneEvicted` e substituição de bucket liberam os registros mesmo sem novos eventos. Antes de aceitar outro evento, a reconciliação síncrona evita uma janela de deduplicação obsoleta. Limpeza externa deve estar refletida até o próximo ciclo reativo; não usar watcher síncrono sobre arrays mutados repetidamente. Guardar e encerrar explicitamente o watcher em `stop()`, pois o adaptador também é criado em callback fora de um scope ativo.
4. **Concluir flush sem reter seu cancelador.** Criar um token de pendência antes de chamar `runAfterFlush`; o callback valida identidade, existência do bloco e estado ativo, aplica `positionQuality` e, em `finally`, remove a pendência. Após o retorno de `runAfterFlush`, armazenar o cancelador somente se o token ainda estiver pendente. Isso cobre callback síncrono sem acessar variável ainda não inicializada e impede recolocar uma função já concluída. A remoção por retenção ou `stop()` invalida o token antes de cancelar; callbacks atrasados não podem alterar blocos recriados com o mesmo ID. Manter o agendamento e o timeout existentes de `useShellMarkers`.
5. **Preservar as projeções e explicitar a janela de deduplicação.** Início e fim da mesma execução continuam distintos e idempotentes enquanto seu registro estiver retido. `finished` sem `started` ainda deve criar/finalizar o bloco; `applyDto` continua permitindo hidratação, não devendo marcar artificialmente eventos como recebidos. Um bloco terminal não deve regredir para execução ativa ao chegar um início atrasado. A garantia de deduplicação passa a abranger blocos retidos: após expulsão, um evento antigo poderá ser processado novamente pelo fluxo existente de criação/hidratação. Documentar e testar essa fronteira deliberada; não introduzir um histórico ilimitado de tombstones nem descartar eventos apenas por sequência menor, pois o código aceita eventos fora de ordem.
6. **Finalização completa e idempotente.** Em `stop()`, marcar a instância como encerrada, parar o watcher, invalidar/cancelar somente operações ainda pendentes, limpar registros/buckets e remover listeners existentes. Uma segunda chamada deve ser inofensiva. `onScopeDispose` continua delegando para `stop()`. O helper deve garantir limpeza dos registros mesmo se um cancelador lançar, sem impedir a liberação dos demais.
7. **Implementar o bloco inteiro e seus testes antes de validar.** Concluir helper, integração e casos abaixo antes de executar testes, tipos ou build. Depois executar as validações em lote, corrigir conjuntamente as falhas relacionadas e repetir os comandos afetados. Preservar identificadores em inglês, comentários em pt-BR e estilo local de TypeScript.

### Testes de regressão e validação em lote

Usar Vitest, Pinia real e controle determinístico de flush/timers; restaurar spies, timers, registry e scopes ao final. Não depender de GC, tempo real ou variação de heap para aprovar o teste.

- **Carga sustentada:** 3.000 e depois 6.000 execuções distintas, com flush por evento e retenção padrão. A store mantém 500 blocos, os registros do helper permanecem limitados aos blocos retidos e cancelamentos pendentes são zero após drenagem. Repetir com `maxBlocksPerPane = 2`; comparar identidades retidas, não apenas tamanhos.
- **Integração da retenção:** comprovar o descarte dos registros antigos pelo comportamento ao reaplicar evento expulso, enquanto uma duplicata retida não chama novamente history/feed nem agenda novo flush. O teste do helper mede cardinalidade diretamente; o teste do adaptador comprova que ele utiliza a reconciliação e só cancela pendências reais em `stop()`.
- **Flush síncrono:** ausência de renderer executa o callback antes do retorno; nenhuma pendência residual, nenhum erro de inicialização e nenhuma função concluída é cancelada em `stop()`.
- **Flush assíncrono e fallback:** conclusão por `onAfterFlush` e por timeout de 24 ms, isoladamente. Verificar metadados, remoção de listener/timer e ausência de pendência. Conclusão que lança na atualização de metadados também libera o token.
- **Rajada sem flush:** criar mais blocos que a retenção no mesmo turno. Os expulsos cancelam suas operações; no fim de cada handler há no máximo uma pendência por bloco retido. Ao drenar, zeram as pendências.
- **Remoções externas:** `feed.clear`, `clearPaneBlocks`, `pruneEvictedBlock`, descarte do marcador, substituição de bucket e redução do limite seguida de inserção. Aguardar `nextTick` quando aplicável; validar cancelamento e remoção das identidades expulsas, inclusive sem novo evento.
- **Concorrência lógica:** dois painéis, PTYs diferentes, filtros, mesma sequência com IDs diferentes, início/fim duplicados, término sem início, início atrasado após término, hidratação intercalada com eventos e callback antigo após remoção/recriação do bloco. Nenhuma alteração em outro painel ou na nova execução.
- **Descarte:** `stop()` antes/depois do flush, duas chamadas, scope encerrado, criação sem scope e cancelador que lança. Registros e pendências zerados; callbacks posteriores não atualizam a store e alterações futuras da store não reativam a observação encerrada.
- **Contratos atuais:** manter os casos de sucesso/erro/aborted, comando indisponível, saída truncada, qualidade de posição, modo de input e histórico já cobertos pelas suites existentes.

### Banco de dados

Nenhuma. Trata-se de retenção de objetos no frontend. Não há migration nem alteração de schema, consultas, paginação, bindings ou persistência de comandos.

### Riscos de quebra e Não-Regressão

| Risco | Proteção e teste |
| --- | --- |
| Deduplicação deixa de ser histórica ilimitada | Janela vinculada a blocos retidos explicitamente documentada; testar duplicata retida e reaplicação após expulsão. Nenhum filtro global por maior sequência. |
| Limpeza remove a execução recém-criada ou afeta outro painel | Registrar após `begin`, usar identidade completa e reconciliar por painel; testar retenção 2, múltiplos painéis e hidratação intercalada. |
| Registro de cancelamento ocorre após callback síncrono | Token criado antes do agendamento e checado após retorno; teste sem renderer. |
| Callback atrasado altera bloco recriado | Invalidar token na remoção e comparar identidade; testar recriação do mesmo ID. |
| Novo watcher cria outra retenção ou custo por output | Observar somente IDs, batelar callbacks e parar explicitamente; testar limpeza sem evento e após `stop()`. |
| Início atrasado sobrescreve término ou dedup do fim é perdida | Flags de fases separadas e preservação de estado terminal; testar ambas as ordens. |
| Teste confunde cache do histórico com cache visual | Verificar cada store e o helper separadamente; histórico por sessão e retenção visual por painel permanecem independentes. |

### Validação

Executar na worktree dedicada, após implementar todo o bloco e seus testes:

```bash
npm test -- src/composables/__tests__/shellCommandBlockRetention.spec.ts src/composables/__tests__/useShellCommandBlocks.spec.ts tests/terminal/commandBlocksStore.spec.ts src/stores/__tests__/shellHistory.spec.ts
npm run typecheck
npm run lint
npm run build
git diff --check
```

**Prova conclusiva:** a suite de carga deve falhar para o comportamento atual (3.000 registros/canceladores retidos para 500 blocos) e passar com registros limitados aos blocos, zero pendências após flush e zero registros após `stop()`. Checar os dois patamares de carga impede aprovar apenas uma redução parcial da taxa de crescimento. Os testes integrados devem também preservar as projeções e o descarte externo. Build ou tipagem isoladamente não comprovam correção de retenção.

Nesta etapa de planejamento foi executada somente a reprodução isolada descrita acima e a inspeção do código; os comandos de validação da implementação são futuros. O arquivo de plano encontrado estava vazio e foi preenchido nesta worktree; nenhum código-fonte foi alterado.

### Skills Aplicáveis

- `vue-debugging-best-practices`: ciclo de vida, descarte e ordenação dos efeitos reativos. Consultada na análise; o guia referenciado `reference/cleanup-side-effects.md` não está presente na instalação local.
- `javascript-testing-patterns`: testes determinísticos de callbacks, cancelamento e cenários assíncronos.
- `code-review`: revisar escopo cirúrgico, contratos e evidências de não-regressão antes da integração.

Documentação Vue consultada via Context7: `https://vuejs.org/guide/essentials/watchers` e `https://vuejs.org/api/reactivity-core` — watchers síncronos não são batelados; o handle retornado permite parada explícita. Aplicar essas restrições ao observador de retenção.
