# Plano de implementação — Issue #17

## Descrição e Causa Raiz

**Veracidade: confirmada por inspeção estática reexecutada nesta worktree.** Base analisada: `feb8f520ff5c7a8f9cace41e393a088c24c5fe9c`. Os seis arquivos desktop existem, mas nenhum registra testes ou executa asserções. A correção necessária é transformar essa especificação textual em uma suíte desktop efetivamente executada e obrigatória no CI.

### Causa Raiz Comprovada

| Evidência | Consequência |
| --- | --- |
| `e2e/specs/no-orphans.e2e.ts:L4-10`, `sanitizer.e2e.ts:L4-10`, `deeplink.e2e.ts:L4-10`, `tray.e2e.ts:L4-10` e `tabs.e2e.ts:L4-11`, `search.e2e.ts:L4-11` — os últimos cinco caminhos também pertencem a `e2e/specs/` | Exportam objetos com `suite` e `steps`; importar esses módulos não registra casos Mocha. |
| `e2e/wdio.conf.ts:L7-33` | Há configuração declarada para Mocha e um binário release, mas não há preparação do binário, gerenciamento de tauri-driver ou isolamento de dados. |
| `package.json:L15-35,L53-77` | Não há comando desktop nem dependências do runner/framework/reporters WebdriverIO. |
| `vitest.config.ts:L26-35` | O Vitest seleciona apenas `tests/**/*.spec.ts` e `src/**/__tests__/**/*.spec.ts`, não os arquivos desktop. |
| `tsconfig.test.json:L1-12` | A checagem de tipos dos testes também não inclui `e2e/`. |
| `scripts/check.mjs:L28-88` e `.github/workflows/ci.yml:L32-44` | O gate executa Vitest, build frontend e testes Rust; não existe etapa de execução desktop. |
| `.github/workflows/ci.yml:L16-19` | Xvfb já é instalado, mas não WebKitWebDriver/tauri-driver nem uma sessão desktop de teste. |

**Fluxo causal do defeito:** CI → `npm run check` → Vitest com glob restrito + testes Rust + build Vite → nenhuma execução de `e2e/wdio.conf.ts` → nenhuma ação na janela Tauri → nenhuma asserção de resultado desktop. Mesmo executar a configuração isoladamente não resolveria os objetos sem `describe`/`it` e sem runner instalado. O glob do Vitest não deve ser ampliado para simular E2E com happy-dom.

**Reprodução realizada:** leitura integral dos seis arquivos e da configuração; script Python somente de leitura contou, para cada arquivo, zero chamadas de registro `describe`/`it`/`test`, zero `expect`/`assert` e um export declarativo. O manifesto retornou listas vazias de scripts e pacotes WebdriverIO; `check.mjs` e CI não contêm chamada ao runner. A contagem textual foi corroborada pela inspeção integral, não utilizada como prova isolada. Não foi executado binário desktop, build ou teste funcional nesta etapa de planejamento.

### Rastreamento dos fluxos que os E2E devem observar

1. **Fechamento e persistência:** `src/components/workspace/TabWorkspaceView.vue:L3,L338-344,L444-449` → `src/stores/tabs.ts:L336-352,L439-460` → `src/ipc/tabs.ts:L30-33,L58-61` → `src-tauri/src/commands/tabs.rs:L30-39` → `src-tauri/src/workspace/manager.rs:L1457-1527,L1530-1598`. O serviço encerra PTYs (`L1538-1548`), descarrega o recorder (`L1553-1557`), fecha sessões/aba no SQLite (`L1564-1593`) e emite eventos (`L1595-1598`); a store remove a aba após sucesso (`src/stores/tabs.ts:L340-346`). Rastreando no sentido inverso: ausência de PIDs e sessão arquivada devem decorrer desse clique, não de limpeza feita pelo próprio teste.
2. **Inspecionar:** `src/components/bridge/InspectButton.vue:L2,L40-49` → `src/composables/useMaxCodeInspect.ts:L35-38` → `src/stores/maxCodeBridge.ts:L273-286` → `src/ipc/bridge.ts:L38-43` → `src-tauri/src/commands/bridge.rs:L41-50` → `src-tauri/crates/mt-bridge/src/client.rs:L302-334,L368-378`. O contexto vem de `src-tauri/src/bridge/host.rs:L337-380`; a redação do payload é aplicada antes do transporte. A persistência de scrollback passa por `StreamSanitizer` em `src-tauri/src/workspace/recorder.rs:L145-168`. Os oráculos precisam verificar dados efetivamente gravados e o payload efetivamente despachado.
3. **Busca:** `src/components/search/GlobalSearchPalette.vue:L3-8,L26-47` → `src/stores/search.ts:L233-251` → `src/ipc/search.ts:L16-19` → `src-tauri/src/commands/search.rs:L14-27` → `SearchEngine`/SQLite FTS5 → seções exibidas na paleta. O nome real do comando é `search_query`; `search_global` na lista textual não é o contrato IPC a implementar.
4. **Abas/sessões:** UI de workspace → `src/stores/tabs.ts:L292-311,L399-437,L520-530` → IPC → WorkspaceManager; a troca de sessão começa em `src-tauri/src/workspace/manager.rs:L2249` e encerra os PTYs anteriores em `L2313` antes de criar o novo conjunto persistido. Testar IDs e histórico, além da aparência.
5. **Tray:** fechamento nativo → `src-tauri/src/lib.rs:L578-597` → `OsIntegration.close_requested` → `quake.hide`/`minimize` ou shutdown. Validar o ramo de ocultação mantendo PTY vivo e a restauração; minimizar por ausência de integração não equivale a validar bandeja.

Os testes Rust existentes cobrem partes importantes, mas não substituem esses caminhos: `src-tauri/crates/mt-pty/tests/orphans.rs:L66-108` chama diretamente PtyManager; `src-tauri/crates/mt-db/tests/sanitizer_sinks.rs:L164-186` cria seu próprio banco/sink. Além disso, `orphans.rs:L32-43` considera zumbis como ausentes; o E2E solicitado exige um oráculo que também detecte zumbis. A ausência de E2E está comprovada; vazamento de processos, falha de sanitização ou defeito de deep link no produto não foram demonstrados nesta análise.

## Arquivos afetados

### Alterar na execução

- `e2e/specs/no-orphans.e2e.ts`, `e2e/specs/sanitizer.e2e.ts`, `e2e/specs/deeplink.e2e.ts`, `e2e/specs/tray.e2e.ts`, `e2e/specs/tabs.e2e.ts`, `e2e/specs/search.e2e.ts`: substituir objetos declarativos por suítes executáveis.
- `e2e/wdio.conf.ts`: tipos, capabilities, hooks, limites, imports explícitos e relatórios.
- `package.json` e `package-lock.json`: dependências reproduzíveis, scripts de execução e inclusão do typecheck E2E.
- `scripts/check.mjs`: acrescentar a etapa desktop obrigatória e atualizar nomes/contagem das etapas.
- `.github/workflows/ci.yml`: preparar drivers, bibliotecas locais, sessão gráfica e publicar artefatos.

### Criar na execução

- `tsconfig.e2e.json`: configuração isolada para WDIO/Mocha, sem colisão com globais Vitest.
- `scripts/run-desktop-e2e.mjs`: build nativo, preflight, ambiente isolado e gerenciamento do ciclo completo.
- `scripts/check-desktop-e2e-report.mjs`: validar resultados por suíte, ausência de skips e propagação de falhas.
- `e2e/support/desktop-session.mjs`: X11/gerenciador de janelas, D-Bus e watcher StatusNotifier de teste, com encerramento dos recursos criados.
- `e2e/support/desktop.ts`: navegação da UI, espera de prontidão e operações comuns em abas/painéis.
- `e2e/support/processes.ts`: acompanhamento de identidades PID/starttime e verificação de encerramento.
- `e2e/support/database.ts`: consultas somente de leitura e inspeção dos bytes persistidos, via processo Python/sqlite3 com argumentos estruturados.
- `e2e/support/external-open.ts`: captura local do destino de abertura externa e leitura do payload real de inspeção.
- `e2e/fixtures/process-tree.py`: árvore controlada com nohup, setsid e double-fork, registro de PIDs e heartbeat.
- `e2e/fixtures/xdg-open`: substituto de teste do lançador externo que registra argumentos sem abrir aplicativos pessoais.
- `src-tauri/crates/mt-db/examples/desktop_fixture.rs`: gerador de banco descartável com migrations/repos reais e dados das nove fontes de busca; executar antes de abrir o aplicativo, com encerramento limpo do writer.
- `tests/scripts/desktop-e2e-gate.spec.ts`: contratos do relatório, seleção de suítes e códigos de saída do orquestrador.
- `e2e/README.md`: execução, dependências, evidências e diagnóstico de falhas.

Os módulos de produção citados no rastreamento são referências para exercitar o fluxo. O plano usa controles acessíveis existentes e os caminhos configuráveis já disponíveis. Se uma asserção revelar defeito funcional adicional, registrar sua evidência e tratar a correção específica antes de aprovar a entrega, sem trocar o oráculo por mocks ou skips. Nesta etapa foi preenchido apenas este plano, inicialmente vazio.

## Execuções propostas

1. **Preparar o runner reproduzível.** Instalar versões compatíveis e fixadas de `@wdio/cli`, `@wdio/local-runner`, `@wdio/mocha-framework`, `@wdio/spec-reporter`, `@wdio/junit-reporter`, `@wdio/globals`, `@wdio/types`, tipos Mocha e o carregador TypeScript exigido pela versão escolhida. Se o watcher D-Bus usar `dbus-next`, fixá-lo também. Importar APIs de teste explicitamente; manter as convenções de lint existentes. Configurar `hostname`/porta e capability Tauri de acordo com os tipos/documentação da versão instalada.
2. **Unificar o comando de execução.** Criar `npm run test:e2e:desktop` apontando para o orquestrador. Ele resolve caminhos relativos ao próprio arquivo, executa `npm run tauri -- build --no-bundle`, verifica sucesso e usa o binário produzido. Resolver `CARGO_TARGET_DIR` quando definido em vez de assumir sempre `src-tauri/target`; manter perfil release alinhado à configuração. Não depender de binário antigo, servidor Vite ou `npx` com instalação implícita.
3. **Isolar o aplicativo e os fixtures.** Criar diretório único sob `.tmp/e2e-desktop/` na worktree. Definir HOME/XDG de teste, diretório runtime com permissão 0700, shell bash controlado, diretórios do projeto e `MAXTERMINAL_DB_PATH` absoluto. `src-tauri/crates/mt-core/src/paths.rs:L49-84` já oferece `MAXTERMINAL_HOME`, mas torna `runtime_dir = None`; preferir XDG explícito para preservar o canal runtime/single-instance. Manter Cargo/Rustup fora do HOME descartável durante o build. Gerar o banco somente antes do app, usando migrations e repositórios reais; nunca semear diretamente os resultados da ação que será testada. Separar dados por spec e preservar o mesmo perfil apenas nos casos de reabertura.
4. **Preparar sessão Linux real.** O CI instala WebKitGTK 4.1, `webkit2gtk-driver`, Xvfb, D-Bus, um gerenciador X11, Python/sqlite3 e as dependências nativas já exigidas pelo projeto; instala uma versão fixada de tauri-driver com `--locked`. O helper inicia sessão gráfica e watcher StatusNotifier, verifica prontidão e disponibiliza observação do registro da bandeja. A suíte de tray exige suporte real detectado; ausência de requisito falha no preflight. Para contenção PTY, verificar as capacidades cgroup/systemd e o mecanismo efetivo disponível; não considerar um cenário setsid/double-fork validado se a árvore nem chegou a iniciar.
5. **Gerenciar lifecycle.** Iniciar tauri-driver com ambiente isolado, aguardar prontidão por condição e prazo, executar as specs sequencialmente (`maxInstances: 1`). Encerrar driver, aplicativo, fixtures e display em sucesso, falha, timeout e SIGTERM/SIGINT. Identificar apenas processos próprios. O teste de órfãos faz suas asserções antes da limpeza defensiva. Propagar o código de falha original mesmo quando o teardown também falhar. Separar timeout de preparação/build do timeout de cada cenário.
6. **Implementar as seis suítes com asserções observáveis:**

| Suíte | Estímulo real e provas obrigatórias |
| --- | --- |
| `tabs` | Abrir aba pelo Template Padrão via UI; comprovar aba ativa e PTY funcional; dividir horizontal e vertical; comprovar painéis e comandos independentes. Iniciar nova sessão pela UI; comparar IDs persistidos, verificar arquivamento da sessão anterior, substituição de PTY e histórico consultável. Reabrir o app no mesmo perfil para validar persistência. |
| `no-orphans` | Rodar fixture pelo terminal visível e aguardar PIDs/heartbeat de todas as variantes, incluindo sessão separada e double-fork real. Fechar aba e, em outro caso, painel pela UI. Registrar PID + starttime antes do fechamento; aguardar desaparecimento da mesma identidade de `/proc`, contando estados Z/X como ainda presentes até remoção. Verificar que outra aba continua executando e produzindo heartbeat. A fixture não deve encerrar sozinha durante a janela de observação. |
| `sanitizer` | Executar pelo terminal comando que emite Bearer, APP_KEY, cookie e senha em URL sintéticos, dividindo também um segredo entre chunks, e termina com erro. Aguardar bloco persistido e marcador não secreto. Confirmar redação nos registros de comando/scrollback, consultas FTS5 e bytes de `.db`/`.db-wal`. Fixar uma leitura/snapshot de teste quando necessário para manter WAL observável, sem alterar PRAGMAs de produção; provar existência e conteúdo positivo do WAL. Inspecionar antes e depois do checkpoint/encerramento. Clicar em Inspecionar e capturar o URI/payload real produzido pelo backend usando o lançador isolado. Exigir contexto correto, modo plan e marcadores redigidos; ausência de arquivo/payload não é sucesso. |
| `search` | Preparar nove fontes com IDs válidos, relações e termo compartilhado usando repositórios reais, tomando `src-tauri/crates/mt-db/tests/search_sources.rs:L29-130` como referência. Abrir Ctrl+Shift+F na janela Tauri, buscar e conferir as nove seções e seus resultados específicos. Selecionar bloco e comprovar projeto/aba/sessão/bloco de destino; verificar também termo inexistente. Não substituir IPC/FTS por respostas fabricadas. |
| `tray` | Iniciar comando contínuo e confirmar PID/heartbeat. Solicitar fechamento nativo pelo gerenciador de janelas, sem destruir a sessão WebDriver. Comprovar ocultação, registro da bandeja e continuidade do mesmo processo. Restaurar pelo Quake/toggle real e verificar janela e terminal utilizáveis. Validar o fallback de minimização separadamente se exercitado, sem contá-lo como cobertura de bandeja. |
| `deeplink` | Com instância ativa e projeto fixture autorizado, entregar `maxterminal://open-project` e `maxterminal://open-terminal` pelo caminho nativo de ativação, incluindo lançamento de segunda instância com URI. Confirmar projeto/aba na UI, identidade da instância principal e encerramento da secundária. Para `cmd`, `shell` e `initial_command`, exigir rejeição observável e ausência de execução de canário/efeitos laterais; aguardar processamento efetivo para não aprovar um link que nunca foi recebido. |

7. **Escolher oráculos independentes.** Usar roles, rótulos acessíveis e atributos já existentes, como `data-block-id`; não coordenadas da WebView nem classes cosméticas como contrato principal. Ações do cenário passam pela UI/nativo; consultas SQLite e `/proc` ficam no processo controlador. Não usar `window.__TAURI__`: `src-tauri/tauri.conf.json:L12-14` desabilita esse global. Não expor comandos privilegiados de teste no binário de produção. Para integração externa, substituir somente o destino externo, preservando a geração/sanitização interna do payload.
8. **Impedir sucesso vazio.** Emitir JUnit por spec em pasta única da execução. O verificador exige as seis suítes esperadas, pelo menos um caso aprovado em cada, zero failures/errors/skipped e execução de todos os casos obrigatórios da matriz. Rejeitar relatório ausente, vazio, antigo, incompleto ou XML inválido; habilitar falha do runner quando nenhum teste for encontrado. Não usar retries de cenário para esconder instabilidade. A seleção reduzida de `--spec`, se oferecida ao desenvolvedor, não satisfaz o gate completo.
9. **Integrar o CI.** Acrescentar a etapa desktop a `npm run check`, preservando as validações atuais e atualizando a contagem. Provisionar os três pacotes MaxVue com refs fixadas e build/exportações disponíveis, chamar a sincronização antes do `--check` e validar `MAX_LIBS_ROOT`; o CI atual faz apenas `npm ci`, enquanto `scripts/sync-local-libs.mjs:L47-88` exige as bibliotecas locais. Configurar timeout e coleta `if: always()` de relatórios, logs e screenshots da execução isolada, sem `continue-on-error`, `|| true` ou skips por falta de ambiente. O status do job deve bloquear integração; documentar a exigência de branch protection, cuja configuração é externa ao YAML.

Referência consultada para Tauri v2: documentação oficial de WebDriver/CI e exemplo WebdriverIO (`https://v2.tauri.app/develop/tests/webdriver/ci/` e `https://v2.tauri.app/develop/tests/webdriver/example/webdriverio/`), via Context7. Ela confirma WebKitWebDriver + tauri-driver + display virtual e o lifecycle do driver. Validar APIs exatas contra as versões fixadas durante a implementação.

## Testes de regressão e validação em lote

Implementar todo o bloco autorizado (infraestrutura, seis suítes, integração CI e testes do gate) antes de executar testes, tipos ou build. Depois validar em lote, agrupar falhas por causa, corrigir e repetir o lote afetado. A implementação não deve parar apenas com o primeiro cenário convertido.

- Casos desktop: toda a matriz acima, com positivos e negativos que comprovem que o estímulo ocorreu. Não basta conferir inexistência de segredos/PIDs em ambiente vazio.
- Testes do gate em `tests/scripts/desktop-e2e-gate.spec.ts`: relatório válido com seis suítes passa; zero casos, uma suíte ausente, skipped, failure/error, relatório antigo e runner que retorna erro falham. Exercitar subprocessos falsos controlados para verificar propagação de erro e teardown mesmo quando o driver não inicia; não espelhar apenas o array de etapas.
- Prova de sensibilidade: em worktree de execução, introduzir temporariamente uma asserção falsa em um caso desktop, rodar o comando completo e confirmar retorno não zero; restaurar e rodar novamente. Remover temporariamente os casos de uma suíte e confirmar rejeição por ausência de cobertura, sem manter essas mutações no diff final.
- Regressões existentes: manter Vitest e testes Rust do workspace, especialmente `mt-pty` (órfãos), `mt-db` (sanitizer/FTS) e `mt-bridge` (payload). Não relaxar cobertura, timeout de invariantes ou política de sanitização para obter verde.
- Estabilidade: uma segunda execução desktop com perfil novo deve passar sem depender de banco, driver ou processos remanescentes da primeira. Nos casos de reabertura, verificar que a persistência decorre somente do perfil explicitamente reutilizado.

## Banco de dados

**Nenhuma migration.** O esquema de produção não precisa mudar. Os fixtures devem usar as migrations/repositórios atuais, SQLite em WAL e diretórios descartáveis. O gerador Rust termina e libera locks antes do aplicativo; o controlador usa leituras após confirmação de commit. Consultas FTS5 precisam verificar ausência do segredo e presença do marcador/contexto esperado. Não apagar WAL nem forçar checkpoint antes da primeira inspeção dos bytes. O banco pessoal nunca é alvo dos scripts.

## Riscos de quebra e Não-Regressão

- **Ambiente desktop incompleto:** Xvfb sozinho não garante bandeja, foco, ativação ou contenção. Preflight explícito e asserções nativas devem falhar quando faltar capacidade; documentar versão do runner e dependências.
- **Falhas funcionais hoje ocultas:** executar os cenários pode revelar defeitos no fechamento, ativação ou sanitização. Preservar evidências e asserções; a entrega só pode ser considerada completa com os seis cenários aprovados, não pela existência dos arquivos.
- **Tipagem/conflito de runners:** manter tsconfig E2E separado e imports explícitos, sem adicionar globais Mocha a testes Vitest; validar os dois conjuntos.
- **Corridas de persistência/indexação:** esperar blocos, commits, readiness e resultados por condições com prazo; evitar sleeps fixos e leituras que aprovam banco vazio.
- **PID reutilizado e zumbis:** comparar identidade e comprovar nascimento antes do fechamento; testar isolamento entre abas. Cleanup só depois da observação, nunca como implementação disfarçada do encerramento.
- **Dados e serviços pessoais:** HOME/XDG/runtime/DB e lançador externo isolados; matar apenas identidades gerenciadas pelo harness. Gerar segredos sintéticos e não conectar MaxCode/GitHub/IA reais para obter fixtures.
- **Bibliotecas locais/tempo de build:** provisionamento MaxVue com refs reproduzíveis e cache por lockfile/toolchain. Não aceitar binário stale como otimização; não retirar validações atuais do gate.
- **Falso verde por relatório:** validar identidade da execução, seis suítes, casos obrigatórios, códigos de saída e zero skips. Artefatos de falha são evidências e não substituem o resultado do job.

## Validação

Com dependências nativas e bibliotecas MaxVue provisionadas, os seguintes comandos serão executados na worktree de implementação:

```bash
npm ci
npm run sync:local-libs
npm run test -- tests/scripts/desktop-e2e-gate.spec.ts
npm run typecheck
npm run check
```

O script `typecheck` deverá incluir `tsconfig.e2e.json`; `check` deverá executar `test:e2e:desktop`, que constrói o binário, sobe a sessão isolada, roda WDIO, verifica os relatórios e encerra os recursos. Execução focada após correções: `npm run test:e2e:desktop`.

**Critério conclusivo:** CI com exit code 0, seis suítes executadas sem skips e todos os casos da matriz aprovados; evidências positivas de UI/SQLite/FTS5/processos/ativação; segunda execução isolada aprovada; teste propositalmente falho e suíte vazia produzem exit code diferente de zero. A mera passagem do Vitest/cargo ou presença de texto nos seis arquivos não satisfaz a issue.

## Skills Aplicáveis

- `javascript-testing-patterns`: desenho da suíte, fixtures, asserções e integração CI; consultada neste planejamento.
- `superpowers`: organizar a execução multi-arquivo e os ciclos de validação, respeitando o lote exigido pela solicitação.
- `vitest-skill`: testes automatizados do gate/orquestrador no Vitest já adotado.
- `typescript-advanced-types-best-practices`: contratos tipados de WDIO e separação de ambientes de teste.
- `linux-troubleshooting`: diagnóstico de X11, D-Bus, WebKitWebDriver, `/proc` e capacidades de contenção.
- `security-and-hardening`: isolamento dos fixtures, segredos sintéticos e captura de payload externo.
- `code-review`: revisão do diff de implementação antes de integração, focada em cobertura real, cleanup e ausência de sucesso vazio.

Aplicar somente as skills pertinentes à etapa; os exemplos de nomes da solicitação não implicam que skills adicionais existam no projeto.
