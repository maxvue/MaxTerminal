# Especificação técnica — restauração da abertura do MaxTerminal

## 1. Contexto & Objetivo

- Projeto: **MaxTerminal**. Data: **03/11/2026**.
- Worktree e branch: `wt-c7cb59fa`.
- Raiz autorizada: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-c7cb59fa`.
- Revisão investigada: `ed45a0f514e09544d9c90cb0ca06856c80b1a703`; não havia alterações locais no início da investigação.
- Relato: ao iniciar o aplicativo, aparece um ícone na barra inferior, mas nenhuma janela utilizável é apresentada.

**Objetivo:** fazer a abertura normal apresentar a janela principal com conteúdo Vue utilizável, sem mascarar falhas nativas e sem alterar preferências de ocultação, sessões ou supervisão de processos.

### Evidências e limites

1. `src/app/plugins.ts:13-17` cria o Pinia, chama `pinia.use(createMaxPinia())` e registra `MaxIcon`, mas **não chama `app.use(pinia)`**. `src/app/main.ts:11-14` monta o app imediatamente depois. `App.vue:67-78` acessa stores durante o setup. É um defeito confirmado por leitura, suficiente para comprometer a montagem; não comprova sozinho a causa de uma janela nativa ausente.
2. `tests/stores/app.spec.ts:25-28` ativa um Pinia próprio com `setActivePinia`; não verifica a instalação feita pelo bootstrap de produção. Não existe teste de plugins em `tests/app/` na revisão investigada.
3. `src-tauri/tauri.conf.json:14-26` já define a janela `main` centralizada e `visible: true`. Mudar esse valor não corrige a omissão do frontend.
4. O log local `~/.local/state/maxterminal/logs/maxterminal.log.2026-11-03` continha 14 registros `app_startup` e 7 registros `backend_panic`. O hook em `src-tauri/src/logging.rs:426-435` descarta intencionalmente payload e stack. Não há horário por linha nem identificação de execução; não é possível relacionar cada pânico ao incidente relatado.
5. O lançador local usa `~/.local/bin/maxterminal %u`, sem `--hide` ou `--start-hidden`. Não havia processo com nome exato `maxterminal`/`maxterminal-bin` na consulta. A sessão consultada é KDE/Wayland; isso não prova a configuração de toda ocorrência anterior.
6. A origem/revisão do executável instalado não foi comprovada. Nesta sessão não se iniciou o app, não se executaram testes/builds nem se abriu o SQLite.

## 2. Escopo

### Incluído

- Instalar na aplicação Vue a mesma instância Pinia que recebe o plugin MaxPinia, antes de qualquer montagem que consuma stores.
- Criar regressões que atravessem o registro real de plugins e a montagem inicial, sem um Pinia previamente ativo que esconda o defeito.
- Acrescentar diagnóstico nativo mínimo por etapa de inicialização, mantendo a política de não registrar payload/stack de pânico.
- Validar o binário desktop real em ambiente isolado. Se persistir uma falha nativa de abertura, localizar e corrigir apenas a causa comprovada no fluxo de startup, com sua regressão. Sem evidência suficiente, manter a entrega pendente, não aplicar uma correção especulativa.

### Fora de escopo

- Redesenho de telas, migração de todas as stores para cache, alterações das bibliotecas irmãs ou troca de versões.
- Implementação de tray nativo, single-instance ou backends de atalhos ainda não acoplados; mudanças na semântica de CLI/Quake.
- Migrações, reset/recuperação do banco do usuário, apagamento de caches ou alteração de projetos/sessões reais.
- Desabilitar GPU/WebKit, forçar X11, ampliar CSP ou permissões para tentar contornar o problema sem causa demonstrada.
- Instalação do binário em `~/.local/bin`, modificação do `.desktop`, publicação, deploy, commit, merge, push ou remoção da worktree.

## 3. Arquitetura & Mudanças Técnicas

### 3.1. Fluxo de frontend

```text
main.ts
  → tema e handlers globais existentes
  → registerAppPlugins(app)
      → createPinia()
      → pinia.use(createMaxPinia())
      → app.use(pinia)                 [correção]
      → registro existente de MaxIcon/estilos
  → app.mount('#app')
      → App.vue → stores → bootstrap IPC → settings → windowStore.init()
      → os_ui_ready → política existente de visibilidade
```

- Preservar a assinatura `registerAppPlugins(app: App): void`; não criar singleton global nem usar `setActivePinia` como correção de produção.
- O plugin MaxPinia `0.2.0` atua somente em stores com `isCached`/`is_cached`. Não habilitar cache nas stores de runtime, não substituir `appStore.status` pelo status de cache e não introduzir HTTP no lugar do IPC Tauri.
- Manter MaxComponentsUi: componente `MaxIcon` e stylesheet já registrados. Sua implementação utiliza `useIconStore` e helpers do MaxUse. Não substituir por componentes nativos ou outra biblioteca.
- Usar MaxUse para qualquer helper/composable adicional; nenhuma nova abstração é necessária para a instalação do Pinia.
- Bibliotecas locais consultadas: MaxComponentsUi `1.1.36`, MaxUse `2.0.1`, MaxPinia `0.2.0`. Revalidar artefatos e resolução antes de executar testes dependentes; não editar os alvos dos symlinks.

### 3.2. Diagnóstico nativo mínimo

- Em `src-tauri/src/lib.rs`, marcar início/conclusão das etapas existentes de setup com identificadores estáticos: banco, temas/workspace, integração SO, bridge/IA/agentes, demais serviços e construção do runtime. Não reorganizar os serviços somente para instrumentá-los.
- Utilizar `target: "maxterminal"`, já aceito por `build_env_filter`, e os campos existentes `event`, `code` e `message`. Os nomes exatos dos novos eventos devem ser definidos uma vez no lote e cobertos por testes; são internos, não um contrato IPC.
- Registrar falhas de construção/setup com etapa e categoria sanitizada quando disponíveis, em vez de descartar completamente `Err(_err)`. Não persistir argumentos CLI, ambiente, prompts, tokens, caminhos pessoais, stack ou payload de pânico.
- Preservar `install_safe_panic_hook`, seu encadeamento pelo guard de PTY, `specta_builder.mount_events(app)` e o contexto Tokio já instalado em setup. `block_on` em setup não é, isoladamente, prova de deadlock.
- Não modificar schema, DTOs, comandos IPC, permissões ou bindings para acrescentar diagnóstico.

### 3.3. Arquivos

| Situação | Arquivo | Mudança prevista |
| --- | --- | --- |
| Existente | `src/app/plugins.ts` | Instalar o Pinia configurado com MaxPinia |
| Novo proposto | `tests/app/plugins.spec.ts` | Regressão do registro e da aplicação real do plugin |
| Novo proposto | `tests/app/bootstrap.spec.ts` | Montagem inicial com plugins reais e fronteiras externas isoladas |
| Existente | `src-tauri/src/lib.rs` | Marcos e tratamento seguro de falhas no startup |
| Existente | `src-tauri/src/logging.rs` | Testes do diagnóstico e, somente se necessário, suporte ao catálogo estático |
| Condicional | Arquivo exato do startup nativo identificado na reprodução e teste correspondente | Correção mínima da causa demonstrada, sem escopo aberto para refatoração |

`main.ts`, `App.vue`, `stores/window.ts`, `mt-os`, configuração Tauri e bibliotecas são referências a preservar; só alterá-los se evidência nova demonstrar necessidade diretamente ligada à falha e a revisão registrar a justificativa.

## 4. Plano de Execução Faseado

1. **T1 — Preflight por leitura:** revalidar revisão/alterações, scripts/hooks, resolução das bibliotecas e isolamento de storage, runtime PTY, bridge e rede. Bloquear validação dependente se a integridade não estiver demonstrada.
2. **T2 — Bootstrap e regressões:** instalar Pinia e implementar os dois testes propostos, incluindo uma store cacheada de teste para demonstrar aplicação do MaxPinia sem rede/storage reais.
3. **T3 — Diagnóstico nativo e testes:** implementar os marcos e as regressões de formatação/sanitização, preservando guards e contratos.
4. **T4 — Validação agrupada e aceite desktop:** após todo o lote e seus testes, executar os gates existentes e construir o app pelo fluxo Tauri. Verificar janela e conteúdo em KDE/Wayland isolado; investigar eventuais falhas em lote. Corrigir causa nativa apenas com evidência e revalidar a revisão afetada.
5. **T5 — Revisão/handoff:** conferir matriz de aceite, registrar progresso no contrato v2 do plano durante Execute e manter `manualPending`/`blocked` quando faltar validação real ou autorização operacional.

Detalhamento operacional, matriz de aceite, comandos e contrato de retomada: [plan.md](./plan.md).

## 5. Critérios de Aceite & Validação

- Abrir normalmente o binário da revisão validada apresenta `main` e conteúdo do shell inicial; ícone na barra ou processo vivo, isoladamente, não é sucesso.
- O registro real de plugins fornece Pinia aos componentes e aplica MaxPinia a uma store elegível; stores de runtime preservam seus estados e não ganham persistência por acidente.
- Testes não pré-ativam Pinia nem injetam uma instância alternativa no caminho que verifica o bootstrap.
- Falha de IPC continua exibindo o estado de erro existente e permite nova tentativa, quando o backend permanece operacional.
- Marcos nativos permitem delimitar a etapa de falha e não vazam sentinelas de segredos; o hook de pânico continua sem payload/stack.
- Preservar `--show`, `--toggle`, `--hide`, `--start-hidden`, handshake idempotente e fallback sem tray. O foco é solicitado, não garantido contra políticas do compositor Wayland.
- `npm run check` é o gate agregado identificado no projeto. O build desktop usa `npm run tauri -- build --no-bundle`, condicionado à conferência da CLI instalada. Nenhum desses comandos foi executado em Plan Mode.
- O aceite nativo é obrigatório e manual quando não houver harness executável. Os arquivos de tray/tabs em `e2e/specs` inspecionados apenas exportam roteiros; não comprovam testes desktop executados.
- Nenhum banco/cache real, processo de outra sessão, lançador instalado ou serviço externo pode ser alterado para validar a correção. Se o isolamento não puder ser comprovado, parar e registrar próxima ação concreta.
