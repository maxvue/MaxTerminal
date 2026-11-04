# Especificação técnica — Temas e MaxApp

## 1. Contexto & Objetivo

- Projeto: MaxTerminal. Data: 2026-11-04. Worktree: `wt-91fbda02`.
- Raiz confirmada por `pwd` e `git rev-parse --show-toplevel`: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-91fbda02`.
- Objetivo solicitado: gerenciar temas em Configurações Gerais → Aparência & Temas, com adição, edição, duplicação e exclusão; substituir AppShell por MaxApp em `blank-mode`, integrado à prop `colors`.
- Fato: o CRUD já existe em componentes, IPC e Rust. `App.vue` já envolve AppShell em `<MaxApp blank-mode no-login controlled-theme :colors="maxAppColors">`. Portanto, a entrega deve completar a substituição, consolidar o uso de MaxPinia e comprovar o fluxo existente, sem reimplementar o backend.
- Não houve execução da aplicação ou testes nesta investigação; existência de código não comprova funcionamento em execução.

## 2. Escopo

### Incluído

1. Remover a dependência de AppShell na raiz, preservando ocupação da janela, estados de inicialização/erro/recuperação e hosts de modais.
2. Manter e validar os botões existentes e o editor de nome, autor, cores principais, seleção com alfa, cursor e 16 cores ANSI; preservar cores indexadas não editadas.
3. Centralizar leitura/atualização do catálogo em store MaxPinia e manter salvamento explícito do editor por MaxPinia sobre IPC.
4. Impedir solicitações repetidas durante operações e apresentar erros sem perder o rascunho.
5. Validar atualização reativa da paleta de MaxApp, tokens locais e terminais, incluindo preview e restauração.

### Fora de escopo

- Alterar bibliotecas compartilhadas, schema SQLite, comandos IPC, proteção dos temas nativos ou política de exclusão.
- Novo editor independente de escalas da UI, importador novo, redesign geral, autenticação web, migrações ou mudanças em PTYs.
- Commit, merge, push ou outra worktree. Esta sessão escreve apenas documentos autorizados.

## 3. Arquitetura & Mudanças Técnicas

### Contratos comprovados

- Biblioteca efetiva: link `node_modules/@maxvue/max-components-ui` → `/home/johnattas/GitHub/MaxComponentsUi`, versão declarada `1.1.36`. `MaxApp.vue` e declaração em `dist/components/MaxApp.vue.d.ts` oferecem `blankMode`, `noLogin`, `controlledTheme`, `colors: PartialColorPalette` e slot `blank`.
- MaxApp exige `route.name`; monta internamente MaxToast, MaxPopoverConfirm e MaxLoadScreen. Possui múltiplas raízes: não depender de encaminhamento automático de `class`/`style` na instância.
- `controlledTheme` impede o gerenciamento próprio da classe `dark`; não desativa o watcher de `colors`. `useColorsStore.setColors` gera CSS e cacheia a paleta.
- O MaxApp fornece altura mínima de viewport, não o mesmo contrato de altura fixa de AppShell. Usar um único contêiner de conteúdo no slot `blank` com altura de viewport, flex, `min-width/min-height: 0`, `overflow: hidden`, fundo/texto semânticos e `data-loading`. Não recriar outro componente shell com os slots antigos.
- Preservar `registerAppPlugins`, configuração offline e rotas nomeadas. Manter todos os hosts atuais no slot `blank`, inclusive os modais do cofre e Horizon; não transferi-los para `extras`, condicionado à autenticação.
- `resolveMaxAppColors` já prioriza tema efetivo, snapshot e fallback. Manter `warn` na paleta da biblioteca e `--max-warning-*` nos tokens canônicos; não renomear indiscriminadamente.

### Fluxo proposto

```text
Aparência → store de catálogo MaxPinia → adapter virtual → IPC → ThemeService → SQLite
Editor → store themeEditor/MaxPinia → adapter virtual → theme_create/theme_update
Resposta + eventos → catálogo/cache resolvido → effectiveResolved
effectiveResolved → MaxApp.colors + tokens CSS canônicos + terminais
```

- Novo arquivo proposto `src/stores/themeCatalog.ts`: store cacheada MaxPinia, `data: ThemeSummary[]`, rota GET existente `/app/themes/catalog`, sem gravação automática; métodos públicos `refreshCatalog(): Promise<ThemeSummary[]>` e `replaceCatalog(themes): void`. Checar erro/status real do plugin após `reload`; não tratar Promise resolvida como sucesso automático.
- `src/stores/theme.ts`: manter sua API pública e contexto de runtime; delegar catálogo à nova store, usando uma única coleção reativa. Preservar Map de temas resolvidos como cache de runtime e sua invalidação por eventos, distinto do cache persistente do catálogo.
- `AppearancePreferences.vue`: retirar GET direto e atribuição externa a `themeStore.themes`; atualizar por ação da store. Adicionar estados de operação, confirmação consistente e mensagens de erro. Na exclusão de override, explicar que o projeto passa a seguir o tema global: o SQL remove o override, não atribui sempre Navy/Teal ao projeto.
- `themeEditor.ts`: manter rascunho isolado, MaxUse `_.cloneDeep`/`_.isEqual`, autosave bloqueado e envio apenas no botão. Usar MaxPinia efetivo com Axios configurado; ausência do plugin deve bloquear a operação com erro, não disparar POST alternativo. Confirmar resultado da operação atual e status de erro, sem usar `lastSavedResolved` de leitura anterior como recibo de gravação.
- Não adicionar retentativa automática de create/duplicate/delete: repetir mutações pode produzir cópias extras. Nova tentativa é explícita após erro.

### Semântica preservada

- Criar/duplicar adiciona ao catálogo sem ativação automática. Duplicação permite qualquer origem e gera nome/ID únicos no backend.
- Editar mantém ID e metadados de origem; atualizar tema efetivo deve refletir na UI e no terminal sem reiniciar PTY.
- Nativos permanecem somente leitura; edição desabilitada com explicação, exclusão indisponível e duplicação habilitada.
- Excluir requer confirmação. Backend remove o tema, troca ativo global para fallback quando necessário, restaura preferências claro/escuro e remove overrides associados numa transação.
- Cancelar editor não persiste; erro mantém modal e rascunho; ações do cartão não selecionam o tema inadvertidamente.

## 4. Plano de Execução Faseado

1. Preflight somente-leitura e estabilização dos contratos de bibliotecas/harness.
2. Consolidar catálogo/editor em MaxPinia e ajustar ações de aparência sem recriar CRUD.
3. Remover AppShell e preservar layout/hosts no slot blank de MaxApp.
4. Completar testes de comportamento, integração e paleta; revisar contratos e diff.
5. Executar validações agrupadas e aceite nativo isolado; registrar pendências reais.

## 5. Critérios de Aceite & Validação

- Todas as quatro operações acessíveis para origens permitidas, persistência comprovada após reinício em perfil isolado e nenhuma mutação ao cancelar.
- Exatamente uma mutação por acionamento; erros e bloqueio de operação concorrente visíveis, sem perda de dados.
- Nenhuma importação/renderização de AppShell; workspace ocupa a janela, divisórias continuam redimensionáveis e terminal não perde área útil.
- MaxApp recebe paleta reativa, um único host de toast, nenhuma chamada HTTP de usuário/menu/login e preservação de claro/escuro, preview, bootstrap e overrides.
- Comandos futuros confirmados: `npm run check` (orquestrador completo); detalhes, matriz de aceite e protocolo de retomada em `plan.md`.
- Testes automatizados com mocks não comprovam Tauri, persistência real ou aparência nativa. Sem ambiente isolado demonstrado, marcar aceite nativo como `manualPending`.
