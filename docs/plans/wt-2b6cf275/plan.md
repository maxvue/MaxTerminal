# Ocultar projetos na lista lateral

- Projeto: MaxTerminal
- Data: 03/11/2026
- Worktree/branch: `wt-2b6cf275`
- Raiz autorizada: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-2b6cf275`
- Situação: plano revisado; implementação e validações ainda não executadas.

## 1. Objetivo e escopo

Adicionar um botão **Ocultar** por projeto na lista esquerda. Projetos ocultos saem da lista principal e ficam em um accordion **Ocultos (N)**, imediatamente acima do botão Configurações. Expandir o accordion mostra os projetos ocultos.

Decisões propostas para completar o comportamento sem perguntas bloqueantes:
- Botão compacto e diretamente acessível em cada item, com texto Ocultar, sem depender de abrir o menu de opções. Nos itens ocultos, substituí-lo por **Mostrar**, que restaura o projeto.
- Accordion presente somente quando houver ocultos; inicialmente recolhido, aberto/fechado pelo usuário. Não persistir sua expansão.
- Persistir quais projetos estão ocultos entre reinicializações, usando a infraestrutura de configurações existente.
- Ocultar é somente organização visual: não remover cadastro, pastas, abas, sessões, processos, contadores ou notificações. Manter `selectedId` e o workspace atual, inclusive ao ocultar o projeto selecionado. Selecionar um oculto pelo accordion não o restaura automaticamente.
- Manter título, caminho, identidade, badges e ações existentes nos dois grupos. Sem ocultos, o comportamento atual deve permanecer equivalente.
- Se todos estiverem ocultos, informar “Todos os projetos estão ocultos” na área principal; não mostrar o convite de primeiro cadastro. Catálogo realmente vazio continua usando ProjectEmptyState.

Fora do escopo: ocultar pastas de descoberta ou abas, encerrar processos, refatorar outros botões, mudar o modelo Rust de projetos, criar endpoints, alterar bibliotecas compartilhadas, migrações ou integração Git.

## 2. Evidências e viabilidade

Arquivos inspecionados e fatos relevantes:
- `src/components/projects/ProjectList.vue`: lista completa em `projectsStore.projects`; rodapé de Configurações separado da região flexível com scroll; drag-and-drop por índice; setas selecionam e Alt+setas reordenam todos os projetos.
- `src/components/projects/ProjectItem.vue`: item selecionável com badges, menu e eventos `select/edit/configure/redetect/remove`; ainda não existe ocultação.
- `src/stores/projects.ts`: catálogo ordenado por `sortOrder`, snapshots revisionados, seleção persistida e vínculos de abas; `reorder` envia todos os IDs ao backend. Não filtrar o catálogo canônico.
- `src/stores/settings.ts`, `src/types/settings.ts`, `src/types/projectSettings.ts` e `src/ipc/db.ts`: defaults tipados, hidratação, escrita SQLite via IPC, eventos de alteração, revisão otimista e retorno booleano em `set`. Erros podem também rejeitar a Promise.
- `src-tauri/crates/mt-db/src/settings/registry.rs`: namespace livre `ui.*` aceito. `src-tauri/crates/mt-db/src/settings/repo.rs`, `get_many(None)`: devolve defaults registrados mais valores persistidos, inclusive `ui.*`. Portanto uma preferência visual nova não exige backend novo. Limite de payload existente: 64 KiB.
- `src-tauri/crates/mt-db/tests/settings.rs`: cobre escrita em chave livre `ui.*`, limite de payload e validação de segredos.
- `src/app/plugins.ts`: `createMaxPinia()` já registrado. As stores atuais não habilitam `isCached`; o plugin retorna antes das funcionalidades de cache/GET/autosave nesses casos. Não alegar autosave automático nem ativá-lo cegamente sobre IPC.
- `src/app/App.vue`, `src/components/layout/AppLayout.vue` e `src/composables/useProjectTabFocus.ts`: hidratação, lista/workspace e foco por seleção. Preservar esses fluxos sem remontar terminais para ocultar um item.
- Biblioteca instalada: `node_modules/@maxvue/max-components-ui/src/components/MaxAccordion.vue` usa `v-model:value`, `multiple`, `lazy` e slot default; `MaxAccordionItem.vue` recebe `value/title`, oferece slots `header/content`, ARIA e teclado. `MaxButton.vue` recebe `label/icon/loading/disabled` e emite `click`.
- `node_modules/@maxvue/max-use/src/index.ts`: helpers próprios e VueUse exportados; usar MaxUse para helpers necessários, sem lodash/VueUse direto. `vite.config.ts` não configura auto-resolução: importar novos componentes explicitamente pelos exports públicos, como nos componentes existentes.
- Testes existentes: `tests/frontend/projects/ProjectItem.spec.ts`, `projects.store.spec.ts`, `ProjectSettingsNavigation.spec.ts`; `vitest.config.ts` usa happy-dom, `tests/setup.ts` e globs para novos testes `.spec.ts`.
- `package.json` e `scripts/check.mjs`: comandos locais e gate agregado confirmados. `.gitignore` já ignora `.max-code-worktrees/` e `.worktrees/`. Estado Git inicial sem alterações.

Consulta complementar: documentação Pinia via Context7 (`/vuejs/pinia`, plugins e testes). Plugins só se aplicam após instalação da instância na aplicação; em testes que exercitem o plugin, instalar a instância em uma aplicação de teste. A versão instalada e os contratos locais prevalecem sobre exemplos de versões mais novas.

Limitações: nenhuma aplicação, teste, banco, build ou serviço foi executado. Há bibliotecas acessíveis em node_modules; links locais não são arquivos autorizados para edição. Compatibilidade visual e persistência nativa precisam de validação posterior.

## 3. Solução e contratos

### Estado, persistência e falhas
1. Acrescentar em `src/types/projectSettings.ts` a chave `ui.projects.hidden_ids: string[]`, default `[]`, herdada pelo SettingsSchema. Usar apenas IDs, nunca caminhos ou objetos completos.
2. Reutilizar as stores sob a integração MaxPinia existente: getters `hiddenProjectIds`, `visibleProjects`, `hiddenProjects` e `isHidden(id)` em `src/stores/projects.ts`, derivados das configurações reativas e do catálogo completo. Validar na leitura que o valor é array de strings não vazias e deduplicar; valor inválido equivale a `[]`. IDs ausentes do catálogo não geram linhas nem contagem. Não limpar a preferência automaticamente durante snapshots: catálogo parcial/vazio não prova remoção definitiva.
3. Adicionar `setProjectHidden(id, hidden): Promise<boolean>` e estado de salvamento na store. Aceitar só IDs cadastrados; operação idempotente. Desabilitar novas alterações de visibilidade enquanto há escrita em andamento, usando sempre o estado atual ao formar a próxima lista. Persistir por `settingsStore.set`; não adicionar HTTP, Axios, localStorage, IndexedDB ou watcher de autosave paralelo.
4. A UI deve consolidar a mudança apenas com sucesso da persistência; capturar retorno false e rejeições, manter o item recuperável e exibir erro em pt-BR na lista, mesmo com catálogo não vazio. Limpar estado pendente em finally. Eventos de settings e reidratação continuam sendo a fonte canônica; não duplicar estado persistido em refs locais.
5. Reutilizar a política de conflito da store de settings. Mesclagem de edições simultâneas entre várias janelas não faz parte desta entrega; não alterar a política global. Não ativar `isCached/options.get/options.save`: hoje tais contratos são HTTP, não adaptadores de Tauri; SQLite já fornece persistência e as stores mantêm o estado compartilhado em memória.

### Componentes, posição e acessibilidade
- `ProjectItem.vue`: prop opcional `hidden` com default false e estado de salvamento/desabilitação; eventos tipados `hide` e `show` com ProjectId. Usar MaxButton nas novas ações. Clique não pode emitir `select`, abrir menu ou iniciar drag acidentalmente; manter menu atual.
- `ProjectList.vue`: lista principal usa `visibleProjects`. Reutilizar ProjectItem no accordion com `hidden=true` e os mesmos handlers/badges. Accordion ancorado entre a região flexível e o rodapé; conteúdo com altura limitada e scroll próprio, preservando Configurações visível em janela baixa e sidebar estreita. Usar MaxAccordion/MaxAccordionItem reais, com value estável `hidden-projects`, título e contador em pt-BR. Estilizar localmente com tokens existentes, sem editar o design system.
- Foco: após Ocultar/Mostrar, levar foco ao item vizinho sobrevivente do grupo; sem vizinho, ao cabeçalho do accordion, ou a Configurações quando o último oculto for restaurado. Preservar foco visível; não deslocar seleção automaticamente. Enter/Espaço devem acionar accordion e botões sem trocar projeto.
- Restringir setas de seleção/reordenação ao grupo de projetos de origem do evento. O cabeçalho do accordion, menus e botões de ação não devem disparar o handler global da lista. Conteúdo recolhido não participa da navegação. No grupo de ocultos, setas selecionam somente ocultos; Alt+setas e drag ficam desabilitados ali nesta entrega.

### Reordenação sem perda de IDs
Manter drag e Alt+setas para projetos visíveis. Guardar o ID arrastado, não índice que pode mudar durante uma atualização. Ao reordenar o subconjunto visível, preencher os slots originalmente visíveis no array completo com a nova sequência; os slots dos ocultos permanecem intactos. Enviar a permutação completa a `reorder`.

Exemplo: catálogo `[A, H, B, C]`, H oculto; mover C antes de A resulta em `[C, H, A, B]`. Se IDs/visibilidade mudarem durante drag, cancelar em vez de mandar permutação incompleta. Restaurar não modifica sortOrder. Não alterar o contrato IPC nem o reorder genérico usado por outros consumidores.

## 4. Etapas sequenciais e matriz de aceite

Implementar o escopo inteiro e seus testes antes da validação em lote. Não há necessidade de delegação.

| ID | Requisito / trabalho | Arquivos | Testes e evidência de conclusão |
| --- | --- | --- | --- |
| T1 | Preferência tipada, partições reativas e ação persistente com tratamento de erro/concorrência | `src/types/projectSettings.ts`, `src/stores/projects.ts` | Estender `tests/frontend/projects/projects.store.spec.ts`: default, hidratação, dados inválidos, duplicados, ID desconhecido, ocultar/mostrar idempotente, persistência falsa/rejeitada, serialização de cliques, eventos e snapshot; catálogo, seleção, donos de abas e contadores preservados. |
| T2 | Botões Ocultar/Mostrar com contrato tipado e MaxButton | `src/components/projects/ProjectItem.vue`, `tests/frontend/projects/ProjectItem.spec.ts` | Ambos os eventos carregam ID; não emitem select/remove; estados pending/disabled funcionam; menu, identidade, badges e configuração mantidos. |
| T3 | Accordion acima de Configurações, estados vazios, erros, foco e ordenação correta | `src/components/projects/ProjectList.vue`; novo `tests/frontend/projects/ProjectListVisibility.spec.ts`; ajustar `ProjectSettingsNavigation.spec.ts` se necessário | Partição sem duplicatas, contador, abrir/fechar, restauração, todos ocultos, último restaurado, erro visível, seleção mantida, foco, teclas sem interferência, drag e Alt+setas com ocultos intercalados enviando todos os IDs. |
| T4 | Fechamento e regressão | Arquivos anteriores; novo `tests/frontend/projects/projectVisibility.persistence.spec.ts` se necessário para separar cenários de settings | Mock fiel de DbCallResult e SettingSnapshot: escrita e nova hidratação recompõem visibilidade, evento externo atualiza grupos, falha não perde acesso. Gates em lote e inspeção nativa descritos abaixo; registrar pendências reais. |

Dependências: T1 antes da integração T3; T2 antes de T3; T4 após todas as implementações e testes. Se um helper de ordenação for extraído, mantê-lo no domínio de projetos e documentar seu arquivo no registro de execução, sem criar abstração genérica desnecessária.

## 5. Preflight e validação futura

Na admissão do Execute, somente por leitura: conferir status/diff da worktree, versões e scripts, resolução dos três pacotes Max, contratos exportados e configuração Vitest. Não instalar ou sincronizar bibliotecas automaticamente; não copiar `.env`. Não configurar rotas Axios: a preferência usa IPC existente.

Testes automatizados devem usar Pinia novo por caso, mocks de IPC/eventos e settings isolados; não tocar SQLite/cache do usuário nem iniciar PTYs reais. Usar o accordion real nos cenários de interação, evitando stubs que tornem ARIA/teclado uma falsa evidência. Helpers de coleção ou foco, se necessários, vêm do MaxUse; computed/ref/nextTick continuam APIs do Vue.

Após completar T1–T4, executar na raiz da worktree, em lote: `npm run lint`, `npm run lint:style`, `npm run typecheck`, `npm run typecheck:vue`, `npm run test:coverage`, `npm run build`. Scripts confirmados no package.json. São gates complementares: build não inclui tipos neste projeto. Alternativamente, usar `npm run check` se o ambiente disponibilizar os gates Rust; ele já agrega esses comandos e não deve ser seguido de repetição dos seus passos sobre a mesma revisão. Não usar lint:fix, instalações, bindings ou migrações como validação automática.

Inspeção manual posterior em ambiente nativo comprovadamente isolado: ocultar selecionado com comando ativo e verificar que saída/abas continuam; expandir, selecionar e restaurar; reiniciar com perfil de teste e verificar persistência; exercitar janela baixa, muitos ocultos, teclado e Configurações. Antes de iniciar qualquer app, confirmar por leitura seu mecanismo real de isolamento de banco/storage. Na ausência dele, não lançar contra perfil real: registrar `manualPending` e solicitar ambiente seguro. Este plano não confirma um mecanismo de perfil isolado nem autoriza criá-lo.

Resultados devem registrar exit codes reais; falhas de baseline não podem ser escondidas nem corrigidas ampliando o escopo sem autorização. Corrigir causas do bloco em lote e repetir apenas gates afetados. Ausência de validação nativa não equivale a aceite nativo.

## 6. Riscos e operação

- Ocultação não é controle de acesso nem pausa de processo. Notificações e foco externo podem continuar apontando a projetos ocultos.
- Filtrar diretamente `projects` quebraria seleção, vínculos, snapshots e reorder; usar apenas projeções da UI.
- O settings livre não valida domínio no Rust: normalização defensiva no consumidor é obrigatória. Preferência acima de 64 KiB deve falhar de modo recuperável, sem esconder item apenas em memória.
- Não remover IDs órfãos automaticamente; são inofensivos na projeção e não justificam escrita durante leitura. Novos IDs aparecem visíveis.
- Bibliotecas vinculadas podem estar compartilhadas com outras sessões: consultar sem modificar, instalar ou reconstruir. Ausência de API pública necessária é bloqueio a relatar, não autorização para alterar a biblioteca.
- Recuperação de uma ocultação é Mostrar; não requer apagar dados. Sem mudanças de esquema, permissões ou serviços externos.

## 7. Retomada e registro de execução

Executor: ler este plano na admissão, respeitar AGENTS.md, conferir alterações já presentes e executar somente lacunas. Usar exclusivamente a worktree existente; não criar outra, trocar branch, commitar, integrar, publicar ou remover a worktree. Limpeza e integração são do MaxCode/usuário.

Somente em Execute, registrar marcos em `docs/plans/wt-2b6cf275/execution-state.json`. Não criar esse arquivo em Plan Mode. Seguir o schema version 2 estrito encontrado nos planos existentes do projeto:
- Raiz: `{ version: 2, planRevision, tasks, verification, updatedAt?, nextAction? }`, sem campos extras.
- `planRevision`: SHA256 real de plan.md; `tasks`: array de `{ id, status, files, validations, revision?, nextAction? }`. Usar IDs T1–T4 e `validations: []` obrigatório, com checks no local canônico.
- Status: `pending | running | completed | blocked | manualPending | validationFailed`.
- `verification`: `{ revision, checks }`; cada check: `{ command, revision, result, exitCode? }`.
- `result`: `passed | failed | pending`; passed exige exitCode inteiro 0, failed exige inteiro diferente de 0, pending omite exitCode. Nunca null; blocked não é resultado de check.
- `command`: comando realmente executado, até 4096 caracteres; não inventar comandos/resultados. Usar os scripts confirmados acima e registrar também falhas.
- Revisões de tarefa/check/verificação: SHA256 real de manifesto determinístico de caminhos relativos ordenados e hashes dos conteúdos correspondentes. Excluir o próprio execution-state.json do manifesto para evitar autorreferência; descrever o método no handoff, sem chaves JSON novas. Alteração relevante invalida checks dependentes.
- `updatedAt`, se usado: inteiro positivo em milissegundos; `nextAction`: até 512 caracteres. Proibidos schemaVersion, campos soltos de status/checks/worktree/branch/head/planPath e title/evidence em tarefas.
- Verificações vazias, falhas ou pendentes não provam conclusão integral. Preservar resultados parciais e pendências manuais, sem declarar aceite completo por falta de ferramentas.

## 8. Revisão do plano

Pedido coberto por botão direto, agrupamento inferior expansível e restauração mínima reversível. Escolhas adicionais foram explicitadas como propostas, não como requisitos já confirmados pelo usuário. Contratos de persistência, componentes e reordenação foram investigados. Escopo limitado a frontend e testes, sem novos serviços ou dependências. Nenhuma implementação ou validação foi executada nesta sessão.
