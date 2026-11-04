# F12 e persistência da posição do inspetor

## 1. Contexto & Objetivo

Projeto: MaxTerminal. Data: 04/11/2026. Worktree ativa: `wt-d489f358`, em `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-d489f358`.

O usuário solicita F12 para inspecionar elementos e que a localização escolhida para o painel seja lembrada, em vez de voltar sempre à parte inferior. O painel é o Web Inspector nativo do WebKitGTK. A investigação foi somente por leitura; nenhum comportamento foi reproduzido executando o aplicativo.

## 2. Escopo

Incluído:
- F12 simples abre ou traz à frente o inspetor da janela principal, inclusive com foco no terminal, em campos ou em modais.
- Manter o menu de contexto “Inspecionar elemento”, inclusive a seleção do elemento clicado.
- Persistir localmente o último modo escolhido pelo usuário e restaurá-lo nas aberturas por qualquer entrada e após reiniciar o aplicativo.
- Distinguir painel acoplado de janela separada, modos comprovados pela API disponível.
- Não deixar o fechamento do inspetor substituir uma preferência acoplada por separada.
- Falhas na preferência não impedem o aplicativo ou o inspetor de funcionar.

Fora do escopo:
- Novo painel Vue, tela de configuração, IPC, serviço remoto ou migração SQLite.
- Atalho global do sistema operacional, alternância de abrir/fechar pelo F12, ativação automática do seletor de elementos por APIs privadas.
- Habilitar DevTools em builds que hoje não o disponibilizam; preservar a política atual de desenvolvimento.
- Persistência de posição em pixels, monitor, geometria ou tamanho da janela separada; criação de acoplamento lateral inexistente no WebKitGTK público.

Interpretação proposta: F12 abre as ferramentas de inspeção, conforme o uso habitual dessa tecla; a ação contextual continua selecionando o elemento. “Local do painel” significa o modo de apresentação oferecido pelo inspetor nativo. Não prometer posições laterais: se o produto em execução oferecer outros modos relevantes, revalidar esse contrato antes da implementação dependente.

## 3. Arquitetura & Mudanças Técnicas

### Evidências

- `src-tauri/Cargo.lock`: Tauri 2.11.6, runtime Wry 2.11.4, Wry 0.55.1, GTK 0.18.2, webkit2gtk 2.0.2.
- `src-tauri/src/lib.rs`: composição e `.setup` da janela; nenhuma integração própria de inspetor encontrada.
- `src-tauri/Cargo.toml`: Tauri sem feature `devtools` explícita; não expandir sua disponibilidade em release.
- `src-tauri/tauri.conf.json`: janela `main`; nenhuma configuração própria de DevTools.
- Fontes locais do Tauri 2.11.6: `with_webview` recebe callback `Send + 'static`; `PlatformWebview::inner()` dá acesso ao WebView GTK. Objetos GTK devem ser obtidos e usados na thread de UI, não capturados de fora no callback.
- Fontes locais de GTK 0.18.2: `connect_key_press_event` recebe `gdk::EventKey` e retorna `glib::Propagation`.
- Fontes locais de webkit2gtk 2.0.2: `WebInspectorExt` oferece `show`, `attach`, `detach`, `is_attached`, `connect_attach`, `connect_detach`, `connect_open_window`, `connect_closed`, `connect_bring_to_front`.
- Fontes locais de Wry 0.55.1, `src/webkitgtk/mod.rs`: abertura usa `inspector.show()`; Wry já acompanha abertura/fechamento. Preservar seus handlers.
- `src-tauri/crates/mt-core/src/paths.rs`: `AppPaths.config_dir` respeita `MAXTERMINAL_HOME`; usar esse contrato, não caminho HOME montado manualmente.
- `src/composables/usePageReloadShortcut.ts`, `src/app/App.vue` e `src/terminal/engine/terminalInstance.ts`: atalhos frontend e encaminhamento de teclas ao PTY. A mudança proposta é nativa para não duplicar esses handlers.
- `.gitignore` já ignora as pastas de worktrees.

### Solução proposta

Arquivos existentes a alterar: `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml` e, se necessário para registrar dependências diretas, `src-tauri/Cargo.lock`. Novo módulo proposto: `src-tauri/src/devtools.rs`, com testes unitários internos. Não alterar fontes de dependências nem arquivos gerados.

Implementar adaptador Linux, instalado uma única vez na janela `main`, protegido por `cfg` coerente com a disponibilidade de DevTools atual. Declarar dependências GTK/WebKitGTK diretas específicas de Linux e compatíveis com as versões já resolvidas; usar reexports seguros para GDK/GLib quando disponíveis. Não atualizar versões transitivas por conveniência. Nenhum `unsafe` é necessário no código da aplicação.

Instalar o handler nativo de F12 na janela GTK, antes de a tecla alcançar o WebView/PTY. Consumir somente F12 sem Ctrl/Alt/Shift/Meta, considerando máscaras de modificadores relevantes, sem rejeitar a tecla por NumLock/CapsLock. Suprimir autorrepetição por estado de tecla pressionada e liberação, limpando o estado ao perder foco. F12 já aberto apenas traz à frente, sem fechar ou mover o painel. Validar a ordem real de entrega GTK antes de considerar esse caminho comprovado.

Preferência proposta: arquivo `devtools.json` em `AppPaths.config_dir`, com contrato fechado `{"version":1,"placement":"attached"}` ou `{"version":1,"placement":"detached"}`. A versão deste arquivo não é a versão do registro de execução. Sem arquivo, manter o padrão nativo. Persistir somente enumeração, nunca HTML, URLs, texto selecionado, histórico ou dados do inspetor. Limitar leitura a 4 KiB, validar versão e valor; arquivo inválido volta ao padrão com aviso sanitizado. Gravação atômica por temporário no mesmo diretório e rename, permissões privadas Unix, sem seguir links simbólicos no destino. Em erro, manter escolha em memória e não interromper o aplicativo. Não criar esse arquivo nesta sessão Plan.

O adaptador deve observar o ciclo de vida compartilhado por F12 e menu contextual. Separar intenção persistida, modo efetivo, restauração programática e transição de fechamento:
1. Carregar preferência antes da primeira abertura, sem abrir o inspetor no startup.
2. Ao abrir, restaurar preferência uma vez por ciclo, na thread GTK, sem recursão de sinais e preservando seleção contextual.
3. Acoplar escolhido pelo usuário confirma `attached`; abertura real de janela separada após desacoplar confirma `detached`.
4. Não salvar `detached` apenas por receber `detach`: a documentação informa que esse sinal também ocorre antes de `closed`.
5. `closed` e encerramento do app preservam a última intenção, sem inferir preferência de `is_attached()` depois de fechar.
6. Restauração e fallback por indisponibilidade de acoplamento não podem sobrescrever a preferência do usuário. Usar `can_attach()` se disponível com as features efetivas; validar sua disponibilidade antes de escrever código dependente.

A ordem exata dos sinais e a restauração sem cintilação são gates de integração nativa, não fatos comprovados nesta sessão. Se restaurar via sinais perder o elemento clicado ou gerar loops, interromper e ajustar o adaptador sem substituir o inspetor nem recorrer a protocolos privados.

Não há frontend novo; MaxComponentsUi, MaxPinia e MaxUse permanecem intactos. Não adicionar store Vue artificial para estado pertencente ao toolkit nativo. Caso surja necessidade real de frontend, replanejar e verificar as três bibliotecas antes de incluí-lo.

## 4. Plano de Execução Faseado

1. Revalidar worktree, alterações locais, disponibilidade de bibliotecas, `cfg`, resolução das dependências e APIs nativas; conferir em ambiente isolado os modos disponíveis.
2. Implementar contrato e persistência privada com testes puros e arquivos temporários isolados.
3. Implementar adaptador GTK, F12, observação/restauração de modo e integração única no setup; completar testes de transições antes da validação agrupada.
4. Revisar segurança, ciclo de sinais e encaminhamento de teclas; executar validações automatizadas agrupadas após todo o escopo e testes estarem prontos.
5. Validar nativamente em perfil descartável, registrar evidências e pendências reais.

Detalhamento operacional e matriz de aceite: `plan.md` deste diretório.

## 5. Critérios de Aceite & Validação

- F12 simples abre/traz à frente uma única instância, não envia dados ao PTY nem executa comandos; combinações com modificadores e F5 seguem intactas.
- Acoplado e separado permanecem após fechar/reabrir por F12 e pelo menu contextual, recarregar a página e reiniciar o app com o mesmo perfil.
- Fechar acoplado não grava separado; fechar separado mantém separado; restauração não altera a preferência por efeitos colaterais.
- Menu contextual continua inspecionando o nó clicado; não há abertura automática no startup, loops ou handlers duplicados.
- Preferência ausente, corrompida, de versão desconhecida, inacessível ou sem permissão não quebra startup ou DevTools.
- Build release padrão continua compilando sem ativar DevTools indevidamente.
- Comandos existentes para execução futura, após preflight: `npm run check` (orquestrador canônico com libs, lint, tipos, cobertura, build, fmt check, clippy e testes Rust); testes novos são cobertos pelo estágio Rust. Aceite nativo é separado e não pode ser substituído por testes de estado.

Fontes oficiais consultadas: https://webkitgtk.org/reference/webkit2gtk/stable/class.WebInspector.html (página declara WebKitGTK 2.42.5/API 4.1; cruzada com bindings locais 2.0.2); documentação Tauri via Context7 e fontes locais fixadas, que prevalecem para assinaturas e `cfg`. Nenhum teste/build foi executado.
