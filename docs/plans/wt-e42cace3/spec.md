# Configurações gerais e do projeto — especificação técnica

Data: 2026-11-03. Projeto: MaxTerminal. Worktree: `wt-e42cace3`.
Raiz autorizada: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-e42cace3`.
Estado: proposta revisável; implementação não iniciada.

## 1. Contexto & Objetivo

Adicionar acesso visível às configurações gerais na parte inferior da aplicação e reunir configurações do projeto em um painel acessível pelo cabeçalho e pelo menu do projeto.

O código já possui `PreferencesDialog.vue`, aberto por `useWindowStore.openPreferences`, com Sistema & SO, Aparência & Temas, Inteligência Artificial, Agentes CLI, MaxSkills & Memória e Integração MaxCode. Há edição de identidade em `ProjectEditModal.vue` e temas por projeto em `ProjectThemeOverrides.vue`, mas não um painel unificado do projeto.

**Decisão confirmada pelo usuário:** reunir opções existentes, sem criar novas configurações de domínio. O recorte proposto para o painel do projeto é identidade e aparência; os editores operacionais de templates, grupos de comandos e agentes permanecem em seus pontos atuais.

## 2. Escopo

### Incluído

- Botão **Configurações**, com texto e ícone, fixo no rodapé da coluna de projetos, fora da área rolável e disponível inclusive sem projetos.
- Reaproveitar o painel geral e suas seis seções; título visível **Configurações gerais**. Manter o acesso superior e os eventos nativos existentes.
- `MaxIconButton` na extremidade direita da linha do título em `ProjectWorkspace.vue`, com nome acessível **Configurações do projeto**; só existe com projeto selecionado.
- Item **Configurar** no menu de `ProjectItem.vue`, entre Editar e Re-detectar identidade; fechar o menu e encaminhar o ID correto.
- Um painel **Configurações do projeto — {título}**, com identidade (título, ícone, cor, restaurar identidade automática) e aparência (tema próprio ou herdar tema global).
- MaxComponentsUi para os controles novos e os contêineres de painéis alterados; stores da instância com MaxPinia; MaxUse para helpers genéricos necessários.
- Regressões de navegação, persistência, isolamento por projeto, teclado e foco.

### Fora do escopo

Novos campos de configuração; novas rotas HTTP; alterações em SQLite/Rust/bindings; migrações; atualização das bibliotecas; conversão completa das seções antigas para MaxComponentsUi; migração geral das stores para cache/autosave HTTP; mudanças no ciclo de vida de PTYs, abas, templates ou agentes.

## 3. Arquitetura & Mudanças Técnicas

### Fluxo

```text
ProjectList / rodapé ── openPreferences('system') ── PreferencesDialog / MaxModal
Cabeçalho / MaxIconButton ─┐
Menu / Configurar(id) ─────┴─ openProjectSettings(id)
                              ├─ validar projeto e selecionar alvo
                              ├─ fechar configurações gerais
                              └─ ProjectSettingsDialog / MaxModal
                                  ├─ ProjectIdentityForm → projectsStore.updateProject
                                  └─ ProjectThemeOverrides [projeto] → themeStore.setActiveTheme
```

**Escolha de navegação proposta:** Configurar seleciona o projeto alvo antes de abrir o painel. O ID do painel é explícito e capturado, não inferido novamente ao salvar. Isso mantém tema efetivo e projeto ativo coerentes: `themeStore.setActiveTheme` e o listener atual aplicam o tema retornado na interface. Não introduzir edição silenciosa do tema de um projeto em segundo plano.

### Contratos

- Em `window.ts`: `projectSettingsProjectId: ProjectId | null`, `openProjectSettings(projectId): void`, `closeProjectSettings(): void`. Projeto inexistente não abre painel nem altera seleção. A ação de abrir valida pelo catálogo, seleciona o alvo e fecha preferências gerais; abrir preferências gerais fecha o painel de projeto. O evento nativo `onOpenPreferences` deve usar a mesma ação de abertura, preservando controle de revisão.
- Host único `ProjectSettingsDialog.vue` em `App.vue`, junto de `PreferencesDialog`. Resolver projeto pelo ID na store; fechar ao remover o alvo ou mudar a seleção, sem reaproveitar rascunhos em outro projeto. Capturar ID antes de cada chamada assíncrona e não deixar conclusão antiga fechar/alterar um painel reaberto.
- Novo `ProjectIdentityForm.vue`: props `project: Project_Serialize`, `active: boolean`; eventos `saved(project)`, `cancel`, `busy-change(boolean)`. Rascunho local inicializado na abertura ou mudança real de ID; não sobrescrever digitação a cada resync do catálogo. Reutilizar validadores de `projectIdentity.ts`, `useIpcError` e `projectsStore.updateProject`.
- `ProjectEditModal.vue` mantém props `visible/project`, eventos `update:visible/saved` e fechamento após salvar. Passa a envolver o formulário compartilhado em MaxModal. No novo painel, salvar identidade mantém painel aberto e apresenta sucesso. Cancelar descarta apenas o rascunho de identidade; tema já aplicado permanece, com essa distinção visível.
- Preservar `identityEdited`, normalização de cor e os patches atuais: título validado, ícone/cor nulos quando não editados, `resetIdentity` somente na ação correspondente. Restaurar identidade automática continua persistindo imediatamente.
- Tema usa `ThemeScope` existente: `{ kind: 'project', projectId }` ou `{ kind: 'clear_project', projectId }`; herança não grava cópia do tema global. Preservar o ID de fallback válido usado pelo componente atual, pois o backend resolve o ID antes de processar `clear_project`.
- `ProjectThemeOverrides.vue` mantém props e evento `change(id, scope)`, mas usa MaxInputSelect. No painel do projeto recebe apenas `[project]`; na seção global continua recebendo todos os projetos. Loading, erro e escrita em andamento ficam visíveis; desabilitar mudanças concorrentes até concluir.
- Identidade salva explicitamente; tema salva imediatamente, como hoje. Não criar botão que prometa uma transação única para as duas operações.

### Bibliotecas e compatibilidade verificadas

- `@maxvue/max-components-ui` instalado: **1.1.36**. Fontes consultadas: `MaxModal.vue`, `MaxIconButton.vue`, `MaxInputText.vue`, `MaxInputSelect.vue`; exports consultados no package.json.
- MaxModal: `visible`/`update:visible`, `noButton`, `title`, `subTitle`, dimensões, `beforeClose`, Escape, foco e teleport. Usar fechamento controlado; bloquear fechamento enquanto a gravação do próprio painel estiver pendente. Não presumir que o slot footer declarado é renderizado: colocar ações no conteúdo.
- MaxIconButton: `icon`/`i`, nome acessível, `disabled/loading`, evento canônico `click`. MaxInputText: `modelValue`/`update:modelValue`, `label`, `error`, `disabled`. MaxInputSelect: `modelValue`, opções, `optionValue/optionLabel/optionName`, estado desabilitado. Confirmar os tipos publicados na execução, sem inventar `MaxSelect` ou `MaxInput` (esses caminhos não existem).
- Imports explícitos pelos subpaths exportados: Vite não configura resolver de componentes; somente MaxIcon está global em `app/plugins.ts`.
- `@maxvue/max-pinia` instalado: **0.2.0**, registrado no boot. Seu plugin retorna sem ativar cache/autosave quando não há `isCached/is_cached`. Stores examinadas usam IPC e cache próprio. Reutilizar essas stores na instância com MaxPinia; não alegar que o autosave HTTP está ativo nem adicionar rotas fictícias para ativá-lo.
- MaxUse exporta helpers e VueUse em `src/index.ts`. Usar, por exemplo, `_` para comparação/clonagem se necessárias, sem novo lodash, cache manual ou temporizador de autosave. Validadores de domínio existentes continuam canônicos.
- Vue instalado: **3.6.0-rc.13**. Documentação oficial consultada sobre modelos nomeados em `https://vuejs.org/guide/components/v-model` e `https://vuejs.org/api/sfc-script-setup`; a consulta não é documentação específica dessa RC.

### Arquivos

Existentes a modificar: `src/app/App.vue`; `src/stores/window.ts`; `src/components/projects/{ProjectList,ProjectItem,ProjectWorkspace,ProjectEditModal}.vue`; `src/components/preferences/PreferencesDialog.vue`; `src/components/preferences/appearance/ProjectThemeOverrides.vue`.

Novos propostos: `src/components/projects/ProjectSettingsDialog.vue`; `src/components/projects/ProjectIdentityForm.vue`; testes descritos no plano. Alterar arquivos adicionais somente se houver dependência comprovada, registrando justificativa.

## 4. Plano de Execução Faseado

1. Preflight somente por leitura: revalidar worktree, alterações locais, exports efetivos, scripts, harness e isolamento.
2. Estabilizar estado de navegação e contratos; integrar os três acessos e host único.
3. Adaptar o painel geral para MaxModal, preservando seções e entradas existentes.
4. Extrair formulário de identidade, manter Editar e compor painel do projeto com tema individual.
5. Completar testes e revisar contratos; então executar validações em lote e aceite visual/nativo isolado.

Detalhamento operacional e registro de retomada: `plan.md` no mesmo diretório.

## 5. Critérios de Aceite & Validação

- Todos os três acessos solicitados funcionam; o rodapé não rola com a lista.
- Os dois acessos do projeto abrem o mesmo painel e o ID correto, inclusive ao configurar um projeto antes não selecionado.
- Editar, Re-detectar identidade, Remover, indicadores, abas e atalhos existentes continuam funcionando.
- Persistência usa contratos existentes; falhas não são apresentadas como sucesso; nenhuma configuração de outro projeto é gravada.
- Remoção do projeto, troca de seleção, reabertura e respostas atrasadas não trocam o alvo da gravação.
- Geral mantém seis seções, fallback de seção desconhecida e entrada por evento nativo; apenas um dos dois painéis de configurações fica aberto.
- Teclado, Escape, foco confinado/restaurado, textos longos e coluna estreita funcionam com componentes reais.
- Validações futuras confirmadas em package.json: `npm test`, `npm run typecheck`, `npm run typecheck:vue`, `npm run lint`, `npm run lint:style`, `npm run build`, depois do bloco completo e do preflight aprovado.
- Nenhum teste, build ou execução nativa foi realizado nesta sessão de planejamento. Aceite nativo depende de ambiente isolado comprovado; manter pendente quando não disponível.
