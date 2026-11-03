# Plano de execução — configurações gerais e do projeto

## 1. Identificação e retomada

- Data: 2026-11-03; projeto: MaxTerminal; worktree/branch: `wt-e42cace3`.
- Raiz única: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-e42cace3`.
- Especificação: `docs/plans/wt-e42cace3/spec.md`.
- Estado: planejamento; nenhuma implementação ou validação de execução realizada.
- Não criar worktree, trocar branch, instalar dependências, integrar Git ou remover a worktree por iniciativa própria. Commit/merge/push/limpeza pertencem ao usuário/MaxCode.
- Na primeira admissão, ler este plano e a spec. Nas retomadas, conferir progresso e consultar apenas trechos pertinentes. Revalidar arquivos e alterações locais, executar lacunas e não reabrir decisões sem evidência nova ou mudança de requisito.

## 2. Objetivo, requisitos e decisões

Entregar acesso inferior às configurações gerais, MaxIconButton à direita do título do projeto selecionado e ação Configurar no menu do projeto. O usuário confirmou **reunir opções existentes**, sem novas configurações.

| ID | Requisito/decisão |
| --- | --- |
| R1 | Botão Configurações no rodapé fixo da lista de projetos, inclusive lista vazia. |
| R2 | Painel geral reaproveita Sistema, Aparência, IA, Agentes, MaxSkills e MaxCode, com título Configurações gerais. |
| R3 | MaxIconButton à direita da linha do título abre configurações do projeto; ausente quando não há seleção. |
| R4 | Configurar entre Editar e Re-detectar identidade fecha menu e abre o mesmo painel pelo ID do item. |
| R5 | Painel do projeto reúne identidade e tema individual/herança global com persistência existente. |
| R6 | MaxComponentsUi para novos controles e painéis adaptados, stores com MaxPinia e helpers MaxUse. |
| R7 | Preservar fluxos atuais, garantir isolamento de alvo, erros visíveis, acessibilidade e responsividade. |

**Decisões propostas, executáveis após aprovação do plano:** rodapé na coluna esquerda; painéis modais; selecionar o projeto ao acionar Configurar; manter acesso superior geral; novo painel contém Identidade e Aparência. Não incluir gerenciadores de templates, grupos, execução de agentes ou novos campos. Manter esses recursos nos acessos existentes. Não mudar autosave, cache, persistência ou rotas de domínio.

**Motivo para selecionar o alvo:** `themeStore.setActiveTheme` e seu listener aplicam o tema resolvido na interface; abrir o painel de um projeto não selecionado e editar seu tema introduziria incoerência visual. Selecionar antes de abrir usa o fluxo já existente de `useThemeApplier`, sem reformar o subsistema de temas.

**Salvamento:** identidade com ação explícita; tema aplicado imediatamente. Cancelar/fechar não desfaz alterações de tema já persistidas. Rótulos deixam isso claro. Restaurar identidade mantém sua ação imediata existente. Não há transação única entre identidade e aparência.

## 3. Evidências da investigação

| Arquivo existente | Evidência e consequência |
| --- | --- |
| `src/components/projects/ProjectList.vue` | Lista flex vertical com `.project-list__scroll` flexível; local adequado para rodapé irmão da área rolável. Eventos de Editar/Remover/Re-detectar já encaminhados aqui. Teclado no aside exige evitar captura de setas do novo rodapé/painel. |
| `src/components/projects/ProjectItem.vue` | Menu manual com três ações e eventos tipados por ProjectId; inserir configure sem emitir select acidental pelo clique do item. |
| `src/components/projects/ProjectWorkspace.vue` | `.project-workspace__title-row` contém título/stack/missing, sem ações. Inserir botão com margem automática e sem comprimir o ícone. |
| `src/app/App.vue` | Host PreferencesDialog já global; acesso superior abre system. Adicionar host do projeto no mesmo nível, sem remontar terminais. |
| `src/components/preferences/{PreferencesDialog.vue,registry.ts}` | Seis seções; fallback para primeira se ID desconhecido; contêiner atual é modal manual. Preservar IDs, ordem, componentes e fallback. |
| `src/stores/window.ts` | Estado de preferências e eventos OS; abertura nativa hoje atribui flags diretamente. Centralizar transição sem perder filtro de revisão e reset. |
| `src/components/projects/ProjectEditModal.vue` | Rascunho local, identityEdited, validadores, patches e eventos de sucesso; extrair sem duplicar regras. |
| `src/stores/projects.ts:327-369` | select valida catálogo; updateProject usa IPC e resync. Falha de resync após salvar pode retornar sucesso com aviso 'Alteração salva; atualização da lista pendente'. Preservar essa distinção. |
| `src/components/preferences/appearance/{AppearancePreferences,ProjectThemeOverrides}.vue` | Tela global já edita overrides; componente de lista emite change(id, scope), incluindo clear_project. Reaproveitá-lo filtrado para um projeto. |
| `src/stores/theme.ts`, `src/composables/useThemeApplier.ts`, `src/ipc/themes.ts` | Hidratação/cache, aplicação de tema e persistência existentes. Não criar IPC direto em formulário. init pode absorver falhas e resultar em catálogo vazio: tratar ausência de opções sem fingir carga bem-sucedida. |
| `src-tauri/crates/mt-themes/src/service.rs:412-449` | Resolve ID antes de clear_project; remove override e retorna tema global. Não substituir herança por gravação explícita do tema global. |
| `src/components/workspace/TabWorkspaceView.vue`, `src/stores/templates.ts` | Templates já têm fluxo de configuração e CRUD independente; não transportar esses fluxos nesta mudança de acesso às configurações. |
| `src/app/plugins.ts`, `vite.config.ts` | MaxPinia instalado na mesma instância Pinia; só MaxIcon global; Vue/Pinia deduplicados; não há auto-resolver de MaxComponentsUi. |
| `node_modules/@maxvue/max-components-ui/{package.json,src/components/MaxModal.vue,src/components/MaxIconButton.vue,src/components/MaxInputText.vue,src/components/MaxInputSelect.vue}` | Componentes e APIs reais consultados, versão 1.1.36; usar imports por exports públicos, nunca importar fonte absoluta da biblioteca. |
| `node_modules/@maxvue/max-pinia/{package.json,src/plugin.ts}` | Versão 0.2.0; gate isCached, HTTP via Axios e cache localforage. Registro do plugin não ativa automaticamente essas funções nas stores IPC. |
| `node_modules/@maxvue/max-use/src/index.ts` | Helpers nomeados, `_` e reexports VueUse disponíveis. |
| `tests/frontend/projects/ProjectItem.spec.ts` | Teste existente cobre seleção, identidade visual e três eventos do menu; estender com configure e preservação. |
| `vitest.config.ts`, `tests/setup.ts`, `package.json` | Vitest/happy-dom, include tests e src/**/__tests__, limpeza de mocks Tauri; scripts de validação listados abaixo. |
| `.gitignore` e `git status --short` | Worktrees ignoradas; working tree estava limpa antes da criação dos documentos. Diretório destes documentos não existia. |

Documentação oficial Vue consultada sobre v-model nomeado: `https://vuejs.org/guide/components/v-model` e `https://vuejs.org/api/sfc-script-setup`. Não foi possível atribuir essa consulta especificamente à versão RC instalada. APIs locais prevalecem para componentes Max.

**Limites:** inspeção estática, sem executar aplicativo, testes, preflight executável ou mutações IPC. Disponibilidade de fontes não comprova integridade dos bundles/dist e symlinks; essa verificação continua obrigatória antes dos testes na execução. Não houve revisão de segredos nem necessidade de lê-los.

## 4. Arquitetura e contratos compartilhados

### Estado e entradas

Ampliar `useWindowStore`, preservando ciclo de vida de janela: `projectSettingsProjectId: ProjectId | null`; `openProjectSettings(id)` valida e seleciona alvo no catálogo, fecha geral e abre projeto; `closeProjectSettings()` limpa ID. `openPreferences(section)` fecha projeto e abre geral; evento nativo usa esse método. Incluir estado novo em `$reset`.

`ProjectItem` emite `configure: [id: ProjectId]`; ProjectList encaminha para a ação comum. ProjectWorkspace chama a mesma ação com props.project.id. Host em App resolve alvo por ID e não por snapshot de objeto. Fechar ao remover alvo ou trocar seleção. Capturar ID e geração de abertura em operações assíncronas; resposta antiga não controla a abertura nova. Não persistir abertura/rascunho no banco ou no localStorage.

Não criar segunda instância Pinia. Stores de dados continuam na instância com `createMaxPinia`; getters, cache, gravação e eventos IPC atuais são a autoridade. Não habilitar isCached indiscriminadamente: o plugin altera `data` e espera contratos HTTP. Não implementar cache ou debounce paralelo. Eventual migração para autosave MaxPinia seria outro escopo, não requisito implícito desta entrega.

### Componentes

- **Existentes:** modificar App, window store, ProjectList, ProjectItem, ProjectWorkspace, PreferencesDialog, ProjectEditModal e ProjectThemeOverrides.
- **Novo ProjectIdentityForm:** contrato exato da spec; reutilizável por Editar e Configurações. MaxInputText para título/identificador de ícone/cor hexadecimal; manter seletor visual de cor usando MaxInputText com type color, verificando suporte de atributos no input real; sugestões com MaxButton/MaxIconButton. Se houver incompatibilidade comprovada, usar componente de cor exportado da biblioteca após consultar sua API, sem alterar a biblioteca.
- **Novo ProjectSettingsDialog:** MaxModal controlado, nome do projeto e caminho somente leitura, áreas Identidade e Aparência no mesmo painel com rolagem. Não precisa criar registry, tabs ou store de dados adicional.
- **PreferencesDialog:** trocar overlay/header/fechar por MaxModal e navegação por MaxButton; manter registro e conteúdos existentes. Computed setter ou eventos explícitos atualizam windowStore, sem dois estados de visibilidade.
- **ProjectEditModal:** wrapper MaxModal sobre formulário compartilhado; preservar eventos/fechamento e reabertura limpa.
- **ProjectThemeOverrides:** trocar select por MaxInputSelect; manter contrato para consumidor global e novo consumidor filtrado. Model de seleção representa ThemeId ou string vazia de herança, mapeando explicitamente para o evento atual.

MaxModal usa `noButton`, `visible/update:visible`, título e dimensões responsivas. Foco/Escape/restauração vêm do componente real; testar, não duplicar armadilha de foco. Seu slot footer não aparece no template inspecionado: ações entram no conteúdo. MaxIconButton usa evento click e nome acessível. Imports explícitos por `@maxvue/max-components-ui/components/...` e helpers por `@maxvue/max-use`.

SFC: template/script setup/style scoped; quatro espaços; tags de abertura em linha única; strings simples e ponto e vírgula; tokens semânticos. Sem PrimeVue. Não migrar controles internos de todas as seções antigas como efeito colateral.

### Falhas e concorrência

- Invalidar abertura quando ID não existe; sem projeto selecionado, ocultar botão do cabeçalho.
- Durante gravação, desabilitar repetição e fechamento pelo usuário; garantir finally e erro localizado, permitindo tentar novamente.
- Cancelar rascunho não salva; reabrir usa dados atuais. Atualização em background do mesmo projeto não apaga edição em curso.
- Se projeto for removido externamente, fechar painel e não redirecionar gravação a outro ID. Operação em andamento pode terminar/errar no ID original, nunca em selectedId atualizado.
- Erro de tema mantém seleção persistida visível e mensagem; catálogo vazio/indisponível não permite selecionar tema fictício. Tratamento de falha silenciosa da store deve ser honesto, sem reformar a inicialização global.
- Eventos gerais recebidos enquanto painel do projeto está aberto fecham o painel do projeto pelo estado comum; operações já enviadas permanecem no ID original. Não prometer cancelamento nativo que IPC não oferece.
- Preservar aviso de gravação concluída com resync pendente; não emitir falso rollback.

## 5. Etapas executáveis e matriz de aceite

Executar sequencialmente. Não há delegação autorizada nesta sessão; se autorizada futuramente, somente testes/arquivos de escrita distintos após estabilizar contratos. App/window e formulário compartilhado têm um único responsável. Passar referências, contratos e revisão factual mínima, não a conversa inteira.

| Tarefa | Dependência e arquivos | Resultado/critério | Testes e revisão |
| --- | --- | --- | --- |
| T1 — preflight | Nenhuma; leitura de Git, package/lock, scripts, configs e libs efetivas | Raiz/branch corretas, trabalho prévio reconhecido, exports/dist coerentes, harness isolado | Revisão documental do ambiente; gate falhado bloqueia validação dependente |
| T2 — navegação e acessos | T1; window.ts, App.vue, ProjectList.vue, ProjectItem.vue, ProjectWorkspace.vue, novo host do projeto | R1/R3/R4: três acessos, alvo correto, exclusão mútua, reset e evento nativo preservados | Eventos, projeto não selecionado, ID inválido, seleção/remoção e reabertura; revisar ciclo de estado |
| T3 — geral | T2; PreferencesDialog.vue | R2/R6: seis seções, fallback, título e MaxModal; acesso superior permanece | Abertura pelo rodapé/superior/evento, mudança de seção e fechamento; revisar visibilidade controlada |
| T4 — conteúdo do projeto | T2; novo ProjectIdentityForm.vue, ProjectSettingsDialog.vue, ProjectEditModal.vue, ProjectThemeOverrides.vue | R5/R6/R7: identidade e tema úteis, persistência por stores, Editar preservado | Patches corretos, validação, erro, herança, resync pendente, respostas atrasadas; revisar isolamento e contratos |
| T5 — testes completos | T2–T4; testes existentes e novos abaixo | Todas as regressões implementadas antes de executar suíte/tipos/build | Revisão de risco do conjunto; pelo menos uma integração com componentes Max reais |
| T6 — validação e aceite | T5; sem mudanças de escopo | Checks vigentes aprovados; visual e nativo comprovados ou explicitamente pendentes | Lote automatizado, revisão final do diff, checklist manual e registro de falhas |

### Testes planejados

Existente a estender: `tests/frontend/projects/ProjectItem.spec.ts` (configure emite ID, fecha menu, preserva ações e não emite seleção acidental).

Novos propostos:

- `tests/frontend/projects/ProjectSettingsNavigation.spec.ts`: stores reais com mocks IPC, painel aberto pelos dois acessos, seleção do alvo, reset, exclusão mútua e evento geral nativo; botão ausente sem seleção.
- `tests/frontend/projects/ProjectSettingsDialog.spec.ts`: persistência A versus B, identidade, erro/repetição, reset de identidade, tema/herança, alteração externa/remoção, pendência e resposta atrasada. Testar payloads IPC e resultado exibido, não apenas existência de botão.
- `tests/frontend/projects/ProjectIdentityForm.spec.ts`: validadores existentes e payloads, preservação do rascunho em resync, cancelamento, reabertura e compatibilidade de Editar.
- `src/components/preferences/__tests__/PreferencesDialog.spec.ts`: seis seções/fallback, abertura geral, MaxModal real para foco/Escape e restauração ao acionador.

Evitar mocks que escondam props/events incorretos de MaxModal/MaxInputSelect. Harness deve instalar Pinia com MaxPinia como o boot, limpar listeners/mocks/teleports após cada teste e impedir qualquer IPC/rede real. Casos visuais que happy-dom não comprovar ficam no aceite manual, não são declarados aprovados por dublês.

## 6. Preflight e validação em lote

Na execução, **antes de alterar código dependente**, inspecionar scripts/pre/post-hooks, targets reais dos symlinks, package/exports/dist das três bibliotecas e identidade resolvida de Vue, Pinia e Axios entre app/teste/libs. Confirmar consistência de aliases/dedupe e que nenhum teste chama bootstrap nativo real. Não rodar sync:local-libs, instalações ou builds de bibliotecas como reparo implícito.

Gate de integridade falhado bloqueia os testes dependentes; sem comando direto para contornar hook, ignoreConfig ou remoção de verificação. Agrupar diagnóstico de harness/bootstrap antes de repetir testes equivalentes. Worktree não isola banco/cache/filas/storage. Não copiar .env, resetar dados, migrar ou usar perfis reais. Para teste nativo, comprovar diretórios/DB/cache descartáveis e autorização operacional antes de iniciar; se não houver mecanismo comprovado, marcar manualPending com próxima ação concreta.

Depois de implementar **todo o escopo e seus testes**, executar os scripts existentes, a partir da raiz autorizada:

```bash
npm test
npm run typecheck
npm run typecheck:vue
npm run lint
npm run lint:style
npm run build
```

Não executar entre microalterações. Capturar todas as falhas e códigos de saída; corrigir em lote e reexecutar somente verificações invalidadas/falhas ou justificadas por nova preocupação. Se um comando agregado futuramente for escolhido, ler sua composição e evitar repetir etapas na mesma revisão. Falha repetida sem progresso exige diagnóstico causal, não tentativas equivalentes. Nenhum desses comandos foi executado em Plan.

### Aceite manual por requisito

1. R1: lista vazia e lista longa; Configurações continua no rodapé e abre geral; setas no novo controle não selecionam projeto por engano.
2. R2: navegar pelas seis seções, abrir pelo acesso superior e entrada nativa; nenhuma preferência antiga desaparece.
3. R3/R4: título longo, coluna estreita e aviso de pasta ausente; botão permanece utilizável. Configurar B estando em A seleciona B e abre B. Ações antigas permanecem.
4. R5: salvar identidade, reabrir e confirmar persistência; restaurar identidade automática; selecionar tema e herdar global; confirmar que outro projeto não foi alterado e que tema global não foi sobrescrito.
5. R6/R7: Tab/Shift+Tab, Enter/Espaço, Escape, retorno de foco, dropdown de tema acima do modal e rolagem interna; sem dois modais gerais/projeto abertos simultaneamente.
6. R7: manter terminal com conteúdo ao abrir/fechar painéis; não recriar PTY, perder abas ou reiniciar comandos. Em ambiente nativo isolado, reiniciar aplicativo e conferir dados salvos.

Pendência visual ou nativa impede declarar aceite integral. Não confundir sucesso em mocks com persistência nativa comprovada.

## 7. Riscos, recuperação e preservação

- **Componentes locais:** fonte e dist podem divergir. Conferir contratos publicados antes de codificar. Não modificar MaxComponentsUi/MaxUse/MaxPinia nesta entrega.
- **Foco/teleport:** testar componente real, empilhamento e fechamento; não manter overlays manuais em paralelo no wrapper convertido.
- **Dados/race:** capturar alvo, bloquear duplicidade, preservar mensagens e distinguir escrita concluída de refresh falho. Não implementar rollback fictício de IPC.
- **Escopo:** manter templates, grupos e agentes nos acessos atuais; nenhuma mudança de autenticação, endpoints ou segredos é necessária.
- **Desempenho:** reutilizar catálogo e cache de stores; não carregar dados a cada render, nem remontar terminais para abrir painéis.
- **Recuperação:** sem migração; reverter apenas os arquivos desta entrega por revisão autorizada, preservando alterações de terceiros. Configurações já salvas continuam nos contratos existentes. Não executar reversão destrutiva automática.

## 8. Registro de execução e handoff

Somente na sessão **Execute**, registrar marcos e handoff em `docs/plans/wt-e42cace3/execution-state.json`. Não criar esse arquivo em Plan. SHA256s são calculados do conteúdo real, nunca rótulos ou valores inventados.

Contrato **version 2 estrito**, sem chaves adicionais:

- Raiz: `{ version: 2, planRevision, tasks, verification, updatedAt?, nextAction? }`.
- `planRevision`: SHA256 real de `plan.md`.
- `tasks`: array de `{ id, status, files, validations, revision?, nextAction? }`; `validations: []` obrigatório em cada tarefa. Neste plano, manter esse array vazio e registrar checks no local canônico `verification.checks`, sem inventar estrutura interna de validações.
- Status de tarefa: `pending | running | completed | blocked | manualPending | validationFailed`.
- `verification`: `{ revision, checks }`; revisão SHA256 real da revisão factual de conteúdo validada.
- Cada check, exclusivamente em `verification.checks`: `{ command, revision, result, exitCode? }`.
- `result`: somente `passed | failed | pending`. `passed` exige exitCode inteiro 0; `failed` exige inteiro diferente de 0; `pending` não contém exitCode. Nunca null. `blocked` é status de tarefa, nunca resultado de check/validação.
- `command`: comando realmente executado, máximo 4.096 caracteres. Incluir falhas, inclusive da suíte completa; não inferir resultado ou código pelo sucesso da chamada da ferramenta.
- `revision` de tarefa/check/verificação: SHA256 real do conjunto factual correspondente. Calcular sobre manifesto determinístico de caminhos relativos ordenados e hashes de seus conteúdos; registrar o método na descrição do handoff sem adicionar campos JSON.
- `updatedAt`, se presente: inteiro positivo de milissegundos; nunca ISO. `nextAction`, se presente: até 512 caracteres com a próxima ação e o motivo de bloqueio/pendência.
- Proibidos: schemaVersion, blocker, diagnostic, branchSync, durableSync, followUp, worktree, branch, head, planPath, status/checks soltos na raiz; title/evidence em tarefa.

Checks vazios, sem resultado vigente ou com falha vigente impedem validação integral. Version 1 pode ser lida por compatibilidade, mas não prova aceite. Registro autodeclarado não comprova execução nativa nem detecta comandos omitidos. Não marcar tarefa manual concluída por ausência de ferramenta; usar manualPending e próxima ação segura.

Em retomada, comparar hashes reais, verificar trabalho existente e executar somente lacunas. Reaproveitar aprovação vinculada a conteúdo igual; alterações relevantes invalidam checks dependentes. Para trabalho assíncrono, aguardar notificações; se necessário, espera limitada com recuo, sem polling repetitivo. Cancelamento mantém resultados parciais e pendências explícitos.

## 9. Revisão do plano

Pedido coberto pelos três acessos e dois painéis; opção confirmada de reutilizar configurações existentes respeitada. Persistência e componentes reais identificados. Distinção entre MaxPinia registrado e autosave efetivamente habilitado documentada. Contratos de identidade/tema e herança investigados; risco de tema de projeto não selecionado resolvido pela navegação proposta. Alterações limitadas ao frontend e testes, com validações futuras claramente separadas das leituras realizadas. Execução integral depende dos gates automatizados e manuais, sem integração Git implícita.
