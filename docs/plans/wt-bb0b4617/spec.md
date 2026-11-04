# Persistência da seleção do workspace

## 1. Contexto & Objetivo

- Projeto: MaxTerminal. Data: 04/11/2026.
- Worktree exclusiva: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-bb0b4617`, branch `wt-bb0b4617`.
- Pedido: ao fechar e abrir o aplicativo, voltar ao projeto selecionado e à aba que estava aberta.
- Este documento especifica implementação futura. Nesta sessão foram autorizadas apenas investigação e documentação.

### Fatos encontrados

1. `src/stores/projects.ts:211–219,333–387` já salva `projects.selected_id`, com atraso de 200 ms, e lê a preferência antes de aguardar o catálogo. Não aguarda a hidratação das configurações.
2. `src/app/App.vue:101–106` inicia `settingsStore.hydrate()` depois de `appStore.bootstrap()`. `AppLayout` já pode montar e iniciar os listeners nesse intervalo. A leitura prematura é possível; não foi reproduzida em runtime.
3. `projects.activeTabByProject` existe somente em memória. `setActiveTab()` não persiste o mapa.
4. `src/stores/tabs.ts:220–250` hidrata abas e escolhe a primeira quando não há seleção local. `AppLayout.vue:59–71` faz reconciliação complementar quando o catálogo chega depois do workspace.
5. `src-tauri/src/workspace/restore.rs:66–127` já restaura abas abertas preservando sua identidade e complementa templates automáticos. Não é necessário criar novas abas para restaurar a seleção.
6. A aba fixa Issues usa outro estado: `githubIssues.pinnedIssuesTabActiveByProject`, também volátil. Ela faz parte da aba visível e será abrangida.
7. O fechamento nativo é tratado em Rust (`src-tauri/src/lib.rs:573–589`), incluindo bandeja/minimização. Um debounce ou uma promessa em `beforeunload` não garante a gravação da última seleção.

## 2. Escopo

### Incluído

- Restaurar projeto e aba visível; lembrar a última aba de cada projeto sem misturar seus IDs.
- Abranger terminal e aba fixa Issues, preservando a seleção terminal de retorno.
- Corrigir a ordem de hidratação e reconciliar somente após catálogo e workspace válidos.
- Preservar mudanças feitas pelo usuário ou por foco externo enquanto o boot está em andamento.
- Tratar ausência de preferência, formato inválido, projeto removido, aba fechada/não restaurada e falhas de armazenamento.
- Garantir captura local síncrona da seleção antes de depender do auto-save assíncrono.
- Testes automatizados e aceite manual em aplicativo nativo isolado.

### Fora de escopo

- Alterar restauração de PTYs, comandos iniciais, sessões, templates, split panes, política de bandeja ou desligamento.
- Persistir modais, painel de grupos, filtros/detalhes de Issues ou histórico aberto.
- Corrigir regras de marcação de resultados como vistos; há notificação separada.
- Mudar bibliotecas externas, adicionar dependências, migrações ou novos comandos Tauri.
- Garantir recuperação após perda de disco, limpeza do armazenamento ou falha simultânea de todos os meios de persistência.
- Sincronizar preferências entre computadores ou múltiplas instâncias concorrentes do aplicativo.

## 3. Arquitetura & Mudanças Técnicas

### Contrato proposto

Adicionar `src/types/workspaceSelection.ts`, com contrato versionado:

```ts
interface WorkspaceSelection {
    version: 1;
    mutationId: string;
    selectedProjectId: ProjectId | null;
    activeViewByProject: Record<ProjectId, {
        kind: 'terminal' | 'issues';
        terminalTabId: TabId | null;
    }>;
}
```

- `mutationId`: identificador opaco novo por alteração lógica, usado para correlacionar gravação/confirmação; não é relógio nem revisão do SQLite.
- Somente identificadores e tipo da aba. Não armazenar caminhos, comandos, saídas, credenciais ou detalhes das issues.
- Validar em runtime o objeto, versão, strings e entradas. Associação projeto/aba é validada contra os snapshots, nunca por formato apenas.
- Nova configuração proposta: `ui.workspace.selection`, padrão `null`, adicionada ao esquema/defaults frontend. O registro Rust aceita chaves livres `ui.*` (`mt-db/src/settings/registry.rs:572–585`); ausência retorna `null` (`settings/repo.rs`). Não requer migração.
- Chave local de recuperação proposta: `maxterminal.workspace-selection.pending.v1`. Contém o mesmo snapshot completo enquanto sua gravação SQLite não foi confirmada.

### Store e transporte

Criar `src/stores/workspaceSelection.ts` como Setup Store com `isCached = true`, `data`, `default_value` e `options.get/options.save`. MaxPinia continua responsável pelo cache e auto-save de 300 ms. Uma nova rota interna, proposta como `/app/workspace/selection`, será interceptada por adapter Axios local e convertida em IPC existente de settings; não é endpoint HTTP nem novo comando Rust.

Adicionar `src/ipc/workspaceSelectionMaxPiniaAdapter.ts` e compor o adapter em `src/app/plugins.ts` antes do adapter Horizon. Interceptação deve conferir caminho e método exatos. Rotas Horizon e demais rotas preservam o comportamento atual.

- GET: ler a preferência via settings store/IPC existente, aguardar hidratação bem-sucedida e validar. Precedência: snapshot local pendente válido, SQLite válido, seleção legada `projects.selected_id`, ausência.
- POST: validar snapshot, serializar gravações, persistir `ui.workspace.selection` pela settings store e devolver o objeto confirmado. Converter `false` em erro; resolução de `saveInServer()` isoladamente não comprova sucesso.
- Cancelamento Axios não desfaz IPC já enviado. Uma escrita antiga deve terminar antes da próxima, e sua confirmação não pode apagar o snapshot pendente mais recente.
- Remover a chave pendente apenas após sucesso SQLite e correspondência de `mutationId` com o snapshot ainda pendente.
- Reutilizar retry de conflito do settings store; nada de sobrescrita concorrente sem controle ou retries ilimitados.

### Captura antes do fechamento

Usar `useStorage`, exportado por `@maxvue/max-use` via VueUse, com escrita síncrona (`flush: 'sync'`), serialização/validação explícitas e `writeDefaults: false`. A ação que muda a seleção registra primeiro o snapshot completo pendente e depois atualiza os dados destinados ao MaxPinia. Sem segundo debounce de backend, polling ou dependência de eventos de unload.

O armazenamento síncrono é uma pequena proteção de recuperação, não uma segunda store de domínio. Não guardar dados de terminal nele. Caso esteja indisponível, manter a interface utilizável, registrar erro sanitizado e tentar o caminho SQLite; não prometer captura imediata sem confirmação. Em boot com recuperação de banco pendente, não iniciar escrita nem apagar preferência.

### Hidratação e reconciliação

```text
Boot → configurações prontas → carregar seleção via MaxPinia
                         ↘ catálogo + workspace/listeners
Todos prontos → validar projeto e pertencimento das abas → aplicar seleção uma vez
Interação/foco externo → registrar snapshot local síncrono → MaxPinia → settings/SQLite
```

- Coordenar no nível de `AppLayout`/composable de navegação. Remover o caminho independente que lê `projects.selected_id` cedo demais.
- A inicialização continua idempotente, sem perder o protocolo de buffer/bootId/eventSeq do workspace.
- Não persistir as seleções provisórias da primeira aba durante o boot. Não podar IDs enquanto um snapshot ainda não foi carregado com sucesso.
- A aplicação da preferência não chama `tabsStore.focusTab()` só para restaurar destaque: o backend desse comando marca conclusões como vistas. Usar seleção local e os mecanismos de apresentação já existentes.
- Consumir a preferência no boot uma vez. Resyncs posteriores mantêm a seleção atual, salvo invalidação real. Interação/foco explícito mais novo vence a preferência de boot e respostas assíncronas atrasadas.
- Falhas de boot devem deixar estados de erro/retry existentes acessíveis, sem espera infinita. Reidratação após recuperação pode tentar novamente sem reverter interação mais nova.

### Compatibilidade e fallbacks

- Ler `projects.selected_id` como migração apenas quando não existir snapshot novo válido. Não apagar a chave antiga; retirar seu writer de debounce da navegação para não haver dois donos.
- Projeto persistido ausente: manter o fallback atual do boot (nenhum projeto), sem escolher/criar um projeto arbitrário. Remoção durante execução mantém seleção do vizinho atual.
- Aba terminal ausente/fechada ou de outro projeto: primeira aba válida pela ordenação atual; sem abas, manter estado vazio, sem abrir terminal adicional.
- Issues: aplicar visualmente quando a resolução confirmar GitHub. Durante resolução preservar intenção; erro transitório não apaga a preferência. Confirmação de não GitHub usa fallback terminal e atualiza a preferência.
- Projetos ocultos continuam cadastrados: não desocultar, remover ou impedir restauração só por estarem ocultos.
- Fechamento da aba ativa preserva o fallback vizinho; fechamento da última registra `terminalTabId: null`.

### Arquivos previstos

- Existentes: `src/app/plugins.ts`, `src/components/layout/AppLayout.vue`, `src/stores/projects.ts`, `src/stores/tabs.ts`, `src/stores/githubIssues.ts`, `src/types/projectSettings.ts` e, se necessário para gate de montagem, `src/app/App.vue`.
- Novos: tipos, store e adapter acima; `src/composables/workspace/useWorkspaceSelection.ts` para orquestração/cleanup, sem ciclo de importação entre stores.
- Testes existentes a ampliar: `tests/frontend/projects/projects.store.spec.ts`, `tests/frontend/workspace/tabs.store.spec.ts`, `tests/frontend/workspace/workspace-components.spec.ts`, `tests/app/plugins.spec.ts`.
- Testes novos: `tests/stores/workspaceSelection.spec.ts`, `tests/ipc/workspaceSelectionMaxPiniaAdapter.spec.ts`, `tests/frontend/workspace/useWorkspaceSelection.spec.ts`.

### Bibliotecas verificadas e limitações

- MaxPinia instalado: 0.2.0; declara `saveInServer`, `saveInCache`, `pauseSave/resumeSave` e status. Fonte instalada oferece `onServerData`; cache pode atribuir `data` diretamente. A store coordenadora deve proteger a seleção atual de cache/GET tardios, não somente interceptar o GET.
- MaxUse instalado: 2.0.1, reexporta VueUse; `useStorage` e `UseStorageOptions` foram localizados no pacote efetivo. Não foi encontrado helper ausente necessário à proposta.
- MaxComponentsUi instalado: 1.1.36; plugin já registrado e `MaxToast` montado. Não há mudança visual nova. Eventual feedback usa a infraestrutura existente, sem componente artesanal.
- Pinia declarado: 3.0.1. Context7 consultado para plugins/hidratação; o material retornado referia-se ao ramo v4, portanto não prova comportamento da versão instalada. Contratos decisivos foram lidos localmente.
- Não foram executados testes, aplicação nativa, preflight ou checks de integridade; nenhuma garantia de runtime é declarada.

## 4. Plano de Execução Faseado

1. Preflight por leitura, contratos e isolamento; bloquear validação dependente se falhar integridade das bibliotecas.
2. Implementar contrato, store MaxPinia, recuperação local síncrona e adapter de settings, com todos os testes desse bloco.
3. Integrar captura/reconciliação em projetos, abas e Issues; eliminar writer legado e escolhas provisórias persistidas; completar testes de boot e concorrência.
4. Revisar contratos, executar validações em lote e corrigir falhas agrupadas.
5. Aceite nativo em perfil comprovadamente isolado e entrega com pendências explícitas.

## 5. Critérios de Aceite & Validação

- Reabrir no mesmo projeto e na mesma aba terminal, inclusive quando não são os primeiros da lista.
- Reabrir na aba Issues quando ela era a visível e o projeto permanece GitHub.
- Trocar de seleção e fechar antes de 200/300 ms: restauração pela captura síncrona; resultado nativo obrigatório.
- Catálogo, settings e workspace em diferentes ordens de resposta produzem o mesmo resultado.
- Respostas de foco/cache/GET antigas não desfazem seleção mais nova; gravações serializadas e confirmação antiga não removem recuperação nova.
- Referências inválidas, dados corrompidos e armazenamento indisponível não quebram boot nem criam abas/processos extras.
- Bandeja, foco externo, seleção por projeto, aba vizinha e política de vistos não ganham efeitos extras.
- Comandos confirmados no projeto, para Execute após isolamento: `npm run check` agrega integridade, lint, tipos, cobertura, build e checks Rust. Não repetir automaticamente seus componentes sobre a mesma revisão.
- Aceite nativo pendente não é substituído por testes com mocks. Detalhamento e matriz em `plan.md`.
