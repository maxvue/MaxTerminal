# Especificação técnica — persistência de abas, templates SSH e gatilhos no viewport

Data: 03/11/2026. Projeto: MaxTerminal. Worktree/branch: `wt-292f24a6`.
Raiz autorizada: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-292f24a6`.
Estado: proposta para revisão; nenhuma implementação ou validação operacional realizada.

## 1. Contexto & Objetivo

Entregar três mudanças: restaurar as abas abertas ao reiniciar o aplicativo; configurar templates locais ou remotos por SSH; abrir os seletores existentes ao digitar seus gatilhos diretamente no viewport do terminal.

Decisões expressas do usuário durante o planejamento:
- Senhas SSH devem ser gravadas **criptografadas no SQLite do MaxTerminal**.
- A proteção será uma **senha mestra informada ao abrir**, não uma chave salva junto do banco nem um cofre do Linux.
- Trabalhar exclusivamente na worktree acima, sem criar outra, mudar branch, commitar, integrar ou publicar.

### Evidências do comportamento atual

- `src-tauri/src/workspace/restore.rs:62–95` enumera somente `list_workspace_templates()`; não restaura todas as abas manuais.
- `src-tauri/src/workspace/manager.rs:2494–2509` arquiva abas no shutdown pelo mesmo caminho de fechamento; `terminate_and_archive_tab_entry`, linha 1316, chama `close_tab_tx` inclusive para `AppExit`.
- O banco já guarda identidade, ordem, layout, painéis e `launch_json` em `tabs`/`panes`; `TabsRepo::list_all_open` existe. `reopen_workspace_tab_tx` recupera uma aba por ID, mas o chamador atual a escolhe pelo template e reconstrói o snapshot a partir dele.
- `TemplateEditorModal.vue` não tem origem ou conexão; `TemplateConfig`, `TabTemplateInput` e `LaunchSnapshot`, em `mt-core/src/workspace/template.rs`, só modelam execução local. `spawn_pane_pty` cria shell local, com integração e hooks locais.
- `RichInput.vue`/`useRichInput.ts` possuem a entrada e o popup; `TerminalInstance` encaminha `xterm.onData` diretamente ao `InputPipeline`. `useCompletion` cancela a sessão em passthrough.
- `triggerRegistry.ts` já aplica maior prefixo primeiro (`>>` antes de `>`) e escape literal. Reutilizar, não criar outro parser de gatilhos.
- `src/app/plugins.ts` instala MaxComponentsUi e MaxPinia. O plugin MaxPinia só ativa cache/get/save para stores com `isCached`/`is_cached`; a store de templates atual não tem esse contrato. Não confundir plugin instalado com cache efetivo.

## 2. Escopo

### Incluído

1. Persistência lógica das abas abertas, inclusive múltiplas abas do mesmo template, layouts divididos, títulos, posições, painel ativo, snapshots de lançamento e histórico vinculado.
2. Diferenciar fechar uma aba de encerrar o aplicativo. Recuperar após saída normal e crash, sem duplicação e sem perder candidatos quando uma restauração falhar.
3. Modal de criação **e edição** com `MaxInputSelect`, label `Origem`, opções `Local`/`Remoto`, padrão `Local`; remoto com `IP`, `Usuário`, `Senha` e opção `Utilizar Chave SSH`.
4. Conexão SSH funcional, supervisionada pelos PTYs existentes, autenticação por senha ou chave/agente SSH; seleção opcional de caminho de chave existente.
5. Cofre nativo: cadastro/confirmação inicial da senha mestra, desbloqueio no início, armazenamento cifrado de senhas SSH, troca de senha mestra autenticada e tratamento de cancelamento/erro.
6. Gatilhos `>`, `>>`, `@`, `?`, `/` no viewport, com os mesmos provedores, seleção, filtros e resultados do campo rico, quando houver contexto de shell elegível.
7. Migração aditiva, bindings gerados, testes, controle de acesso IPC e validação manual nativa isolada.

### Fora de escopo

- Retomar a memória de processos mortos, reproduzir scrollback antigo no PTY novo ou reexecutar o histórico de comandos.
- Criar servidor SSH, gerar/copiar chaves privadas, SFTP, túneis, sincronização remota de arquivos ou instalar hooks permanentemente no servidor.
- Implementar descoberta remota de Artisan, stack, grupos multiabas ou portas remotas nesta entrega. Não executar descobertas locais como se fossem remotas.
- Alterar provedores/modelos de IA, escolhas de agente, temas ou a política existente de bandeja.
- Criptografar o banco inteiro ou prometer proteção contra root, processo comprometido ou captura de memória com o cofre aberto.
- Recuperar senha mestra esquecida, apagamento criptográfico garantido de backups antigos ou compatibilidade de downgrade com schema novo.

## 3. Arquitetura & Mudanças Técnicas

### 3.1. Persistência de intenção, não de processo

Usar `closed_at IS NULL` como intenção durável de manter uma aba aberta. No encerramento do aplicativo, finalizar processos e arquivar sessões, **sem fechar** as linhas de aba/painéis. Fechamento explícito, último painel fechado e remoção de projeto continuam removendo a intenção de restauração. Não criar segundo catálogo de abas em localStorage.

No boot: reconciliar sessões/ledger → ler abas abertas → restaurar cada `TabId` pelo snapshot persistido → garantir templates de autoinício ainda não representados por uma aba de origem `workspace` → publicar snapshot/relatório. Preservar a distinção existente entre abas manuais e a aba automática de cada template. Uma manual do mesmo template não substitui a aba automática que já era prevista pelo produto.

Extrair uma primitiva de restauração por `StoredTabRow`, sem depender da existência ou conteúdo atual do template. Conservar layout, ordem e metadados; gerar novos PTYs, sessões e gerações, mantendo IDs de aba e painéis válidos. Preservar a recuperação atual de layout inválido para uma folha, com aviso e histórico intacto. Restaurar foco usando dados persistidos existentes, sem novo mecanismo de cache de runtime.

Falha de diretório, spawn, rede ou cofre bloqueado não deve marcar a aba como fechada nem remover sua intenção. Expor painel sem PTY, com erro recuperável, e permitir reconectar/fechar. Adaptar compensações hoje usadas para novas abas, que fecham a aba em erro de spawn: novas aberturas abortadas e restaurações de abas existentes têm semânticas diferentes.

Proposta de segurança: abas manuais restauradas não repetem automaticamente o comando inicial; quando houver, apresentá-lo para execução explícita (`Waiting`), sem apagar o snapshot. Templates com `addToWorkspace` conservam a política atual de autoinício e deduplicação do despacho. Nunca reproduzir comandos do histórico. Esta distinção é proposta de comportamento novo, não um fato do código atual.

Migração: não reabrir indiscriminadamente abas antigas com `closed_at` preenchido. O comportamento novo começa pelas abas ainda abertas e pelas abertas após atualização; os registros antigos não distinguem com certeza a intenção final do usuário após a saída. Não prometer recuperar retroativamente todas as abas já perdidas por versões anteriores.

### 3.2. Contratos de conexão e armazenamento

Adicionar origem discriminada separada de `environment.kind` (shell/agente):
- `connection: { kind: 'local' }`;
- `connection: { kind: 'ssh', host, user, auth: { kind: 'password', credentialId } | { kind: 'key', identityFile: string | null } }`.

Nomes acima são **novos contratos propostos**, sujeitos à tipagem Rust/Specta; valores ausentes em JSON legado devem desserializar como local. `host` aceita IPv4/IPv6 válidos; não aceitar flags, comandos, NUL ou quebras de linha. Porta 22, sem novo campo de porta obrigatório. Usuário é um identificador de login, nunca fragmento de shell. Chave é um caminho local para arquivo existente; vazio usa identidades/agente padrão do OpenSSH, sem importar material privado.

Senha nova/substituída é entrada write-only, separada do DTO público e do snapshot. Edição deve distinguir manter a credencial, substituir e remover, sem preencher o campo com a senha descriptografada. CRUD de metadados e credencial deve ser atômico, com revisão otimista e isolamento por projeto/template. Alterar destino não reutiliza silenciosamente uma senha sem confirmação no formulário. Snapshots já abertos continuam representando o destino original.

Propor tabelas próprias para metadados do cofre e registros de credenciais cifradas, além de coluna de conexão em templates. Preservar referências de credenciais utilizadas por abas abertas/snapshots se o template for excluído ou editado; remover órfãos somente sem referências e sem apagar dados por heurística. Nunca incluir credenciais no FTS ou `workspace_operations.result_json`.

### 3.3. Criptografia e senha mestra

No Rust, propor Argon2id para derivar chave de 32 bytes da senha mestra e salt aleatório de 16 bytes; parâmetros versionados e limitados antes de alocar memória. Ponto de partida proposto: 64 MiB, 3 iterações e paralelismo 1, medindo responsividade no gate nativo. Não rodar KDF na thread de UI nem dentro de transação SQLite longa.

Usar chave aleatória de dados (DEK), encapsulada pela chave derivada da senha mestra. Cifrar DEK e cada credencial com XChaCha20-Poly1305, nonce aleatório de 24 bytes novo por escrita e dados associados contendo versão, identidade do banco/cofre, projeto, ID da credencial e propósito. Não vincular a cifra ao template mutável se o snapshot precisar conservar a credencial. Persistir algoritmo/versão, salt/parâmetros KDF, nonces e ciphertext/tag; nunca a senha mestra, chave derivada, DEK aberta ou senha SSH em claro.

Trocar senha mestra exige senha atual e reencapsula a DEK atomicamente; falha não invalida o cofre anterior. Usar bibliotecas criptográficas mantidas, com versões/MSRV escolhidas e fixadas na execução; não implementar cifra própria. Novas dependências propostas: `argon2`, `chacha20poly1305`, `zeroize` e geração criptográfica de aleatoriedade compatível com essas versões. Não assumir que as versões mais recentes servem ao toolchain atual.

Estado nativo: não configurado/bloqueado/desbloqueado. Estado público não contém segredos e não pode ser restaurado como desbloqueado a partir de cache. Senhas transitórias só em refs locais do formulário pelo menor tempo possível; limpar ao concluir/cancelar/desmontar. Não passar por autosave, cache MaxPinia, telemetria, logs, inspeção ou IA. Memória Rust sensível sem `Debug` revelador e com descarte explícito; não alegar zeroização perfeita de strings JavaScript.

No primeiro uso por senha: criar senha mestra com confirmação. Em cada inicialização com cofre configurado: pedir desbloqueio uma vez e permitir adiar; abas locais permanecem utilizáveis, abas por senha ficam aguardando desbloqueio. Desbloquear uma vez libera as pendentes de modo idempotente, sem duplicar sessão/processo. Minimizar para bandeja não equivale a reiniciar o app nem encerra conexões. Cancelamento e senha errada preservam os registros; limitar tentativas concorrentes e aplicar recuo sem bloquear a UI. Informar que perder a senha mestra impede recuperar as senhas salvas, sem oferecer reset destrutivo automático.

Backups do SQLite levam ciphertext, salt e DEK encapsulada, nunca chaves abertas; restauração exige a senha mestra correspondente ao backup, inclusive após troca de senha. WAL mantém apenas valores cifrados para credenciais. Metadados de destino não serão cifrados por este escopo.

### 3.4. Transporte SSH e limites de confiança

Proposta: OpenSSH instalado no Linux como processo direto no PTY existente (`SpawnSpec.shell` aceita executável explícito e args), `login: false`, sem concatenar `ssh ...` no shell local. Preservar resize, sinais, encerramento supervisionado e geração por painel. Cada split/restart/nova sessão remota conserva a origem; nunca cair silenciosamente em shell local.

Criar serviço remoto e helper askpass no próprio executável, despachado **antes** de resolver diretórios, abrir DB, logs ou Tauri. Usar `SSH_ASKPASS_REQUIRE=force` e canal Unix privado para responder autenticação. O helper só recebe endpoint e identificador efêmero, nunca senha em argv/env/arquivo temporário. Vincular autorização a boot/aba/painel/geração/processo SSH, validar UID e descendência/identidade de processo, limitar tamanho, prazo, tentativas e concorrência; invalidar em cancelamento/saída. stdout privado do helper vai somente ao OpenSSH, não ao recorder do terminal.

Pergunta de confiança de host NÃO recebe senha salva como resposta. Prompt de confirmação deve apresentar identificação/fingerprint e exigir aceite; host alterado bloqueia. Passphrase de chave e desafios adicionais são interativos, não recebem automaticamente senha de login. Não desabilitar `StrictHostKeyChecking`; sem `sshpass -p`, senha em ambiente ou injeção de senha detectando texto do terminal. Usar known_hosts com arquivo isolado nos testes; só a execução explicitamente autorizada pode atualizar confiança persistida.

Preflight futuro deve verificar versão/recursos do OpenSSH local, modo helper no binário empacotado/AppImage, allowlist de env de `mt-pty`, supervisão e comportamento de askpass no PTY. Se inviável, bloquear a parte remota com próxima ação concreta; não trocar de transporte nem enfraquecer autenticação silenciosamente.

Desabilitar hooks de agentes e shell integration **locais** para o processo SSH. `cwd` remoto é campo do servidor, não passa por `Path::is_dir()` local; cwd local do cliente permanece a raiz válida do projeto. Nesta primeira versão, esconder/desabilitar envVars locais ao selecionar remoto, sem apagar os valores do template local até salvar a mudança; não encaminhar segredos/ambiente local ao servidor. Pasta e comando inicial remotos só são enviados com confirmação após conexão estabelecida quando não houver prompt confiável; não usar timer como prova de autenticação. Não presumir sintaxe de bootstrap remoto universal: usar comando explícito pelo usuário e escapar pasta com estratégia validada, sem interpolação insegura.

Recursos locais (Git, Artisan, comandos rápidos, grupos e links de editor) precisam de guarda de origem: não operar na máquina local supondo que o viewport representa o projeto remoto. Shell remoto básico é suportado; recursos dependentes de OSC/stack remoto não serão simulados. Sem integração remota confiável, preservar passthrough e expor a limitação, não interceptar senha ou terminal remoto desconhecido.

### 3.5. Entrada no viewport e popups

Criar ponte síncrona de intenção de teclado do viewport para a entrada rica **existente**, no handler público já instalado em `TerminalInstance`. Compor com copiar/colar, sem sobrescrever o handler em outro componente. Contrato proposto de registro descartável por instância: evento → consumido/não consumido. Ao consumir o primeiro caractere de gatilho, bloquear seu envio ao PTY, inicializar o draft rico, transferir foco e reutilizar `useRichInput`/`CompletionPopup`. `>` seguido rapidamente de `>` precisa chegar integralmente ao draft e mudar para grupos sem executar nem duplicar prefixo.

Gate antes de consumir: painel visível/ativo, entrada rica permitida, shell ocioso com prompt confiável na geração atual, linha vazia conhecida, sem execução em andamento, sem TUI/agent hint de passthrough, sem busca/modal/seleção/IME/modificadores de atalho. Modo direto escolhido pelo usuário e `richInput=false` continuam diretos. A ausência de altScreen não prova shell ocioso: prompts de senha e aplicações de linha também existem no buffer normal.

Usar estado OSC 133 correlacionado e um rastreador conservador de edição da linha, invalidando-o quando houver movimentação/histórico/entrada não reconhecida. Não extrair comando de pixels/buffer privado do xterm nem copiar texto de terminal potencialmente secreto para o frontend. Sem certeza de linha vazia, não consumir. Após retorno a prompt/linha vazia comprovada, reabilitar. Confirmar por testes reais que essa guarda cobre o fluxo local padrão; se ele não fornecer os sinais necessários, adicionar contrato mínimo de prompt vazio na integração de shell, sem liberar interceptação insegura.

No draft rico: manter maior prefixo, escape, Enter/Tab/setas/clique/Escape, ações de criação, flags e seletores de harness/modelo/raciocínio existentes. Escape fecha o popup sem executar ou enviar o prefixo; manter o draft editável. Comandos como `echo x > arquivo`, `>>` no meio da linha e `\>` continuam literais. Cola/IME não são equivalentes a keydown: preservar pipeline e bracketed paste, sem capturar respostas VT/binário ou automatismos como se fossem digitação. Nunca drenar todo `onData` para um editor virtual.

### 3.6. Frontend Max e integração

Usar MaxComponentsUi nos controles novos: `MaxInputSelect`, `MaxInputText`, `MaxInputPassword`, `MaxInputCheckbox`, `MaxModal`, `MaxButton` e feedback existente, confirmando exports efetivos. Consulta local comprovou select com `v-model`, `options`, `optionValue`, `optionLabel`, `optionName`; password com `v-model`, `label`, `required`, `error` e autocomplete desativado por padrão. Manter a entrada especializada existente do terminal; não reescrever textarea/xterm por causa deste pedido.

Manter estado público em stores com MaxPinia. Adaptar cache/get de metadados de templates via adapter IPC exclusivo para o Axios do plugin, com URLs internas allowlisted e sem criar servidor HTTP ou alterar bibliotecas compartilhadas. Preservar fachada `templatesFor/loadForProject/createTemplate/updateTemplate` e revisões. Listas com `isCached`, partição por projeto e `isList`/`block_save`, sem autosave da senha ou do formulário. CRUD explícito continua apropriado ao botão Salvar. Status do cofre é volátil e sempre lido do backend; PTYs, handlers e segredos nunca são conteúdo de cache. Helpers/composables genéricos vêm de MaxUse, não de nova dependência lodash/VueUse direta.

## 4. Plano de Execução Faseado

1. Preflight somente por leitura, isolamento operacional e estabilização dos contratos.
2. Domínio/migração aditiva/compatibilidade de snapshots e referências cifradas.
3. Cofre nativo e IPC mínimo; revisão de segurança antes da integração SSH.
4. Persistência e restauração por ID; distinguir shutdown/close e falhas recuperáveis.
5. Transporte SSH, helper e supervisão; gate de identidade do destino e autenticação.
6. Formulários/MaxPinia/MaxUse e fluxo de desbloqueio/retentativa.
7. Ponte de entrada do viewport e reutilização dos gatilhos, sem mudar passthrough.
8. Concluir todos os testes do escopo; depois validação agrupada, correções em lote e aceite manual nativo.

Responsabilidades, arquivos propostos, matriz de aceite, comandos comprovados e retomada estão em `plan.md`. Etapas dependentes não podem começar com contratos compartilhados instáveis.

## 5. Critérios de Aceite & Validação

- Abrir várias abas manuais, duas do mesmo template, dividir/reordenar e reiniciar: mesmas identidades/layout/títulos/ordem; sessões/PTYs novos; aba fechada explicitamente não retorna. Autoinício existente sem duplicações entre boots.
- Saída para bandeja mantém processos como hoje; saída real finaliza processos supervisionados, mas conserva intenção de reabrir. Crash não duplica sessões nem perde abas.
- Criar/editar local/remoto; campo Origem usa exatamente o componente e rótulos pedidos. IP/usuário obrigatórios no remoto; senha obrigatória para nova credencial por senha e dispensada no modo chave. Cancelar não salva segredo.
- Conexão por senha salva funciona após desbloqueio; sem senha mestra correta, ciphertext permanece inacessível. Senhas diferentes geram ciphertext diferente; adulteração e troca de credenciais entre projetos falham; nonce não se repete nos casos de teste.
- Senhas não aparecem em SQLite/WAL/backups/FTS/logs/snapshot/cache/argv/env/scrollback ou prompts de IA; testar com canários sintéticos, não credenciais reais.
- Conexão por chave, chave protegida, host novo/alterado, senha inválida, rede indisponível, timeout, fechar/restart/split e AppImage: evidência nativa, não somente mocks.
- Digitar cada gatilho dentro do viewport local elegível abre o mesmo popup do RichInput, sem bytes prematuros no PTY. Confirmar uma vez envia uma vez. TUI, senhas, comandos em execução, redirecionamentos, múltiplos painéis e colagem não sofrem interceptação indevida.
- Não há endpoint HTTP novo. Verificar wrappers IPC/Specta/permissões e proibir retorno público de segredo.
- Comandos existentes: `npm run bindings` quando contratos mudarem e `npm run check` para os dez gates canônicos. Só executar na sessão Execute após comprovar isolamento; nenhum foi executado em Plan. Verificações manuais externas sem ambiente autorizado permanecem `manualPending` e impedem conclusão integral.

### Fontes e limites

Fontes locais: arquivos citados; `package.json`, `Cargo.toml`, migração `0004_07_workspace_templates_tabs_sessions.sql`, registry de migrations, `scripts/check.mjs`, `vitest.config.ts`, `tests/setup.ts`, código local de MaxInputSelect/MaxInputPassword, MaxPinia/plugin.ts e MaxUse/index.ts. Symlinks resolvidos para os três repositórios MaxVue em `/home/johnattas/GitHub`; fonte consultada, mas dist/exports não validados operacionalmente.

Documentação consultada: xterm.js via Context7 (`/xtermjs/xterm.js`), RustCrypto (`/rustcrypto/password-hashes`, `/rustcrypto/aeads`), OpenSSH (`/openssh/openssh-portable`) e manual oficial `https://man.openbsd.org/ssh.1`. Confirmações: handler de teclado retorna false para suprimir entrada; colagem passa por outro caminho; Argon2 deriva bytes de chave; XChaCha usa nonce de 24 bytes; OpenSSH suporta askpass forçado. Docs não estavam fixadas exatamente às versões instaladas: conferir typings locais/versões escolhidas na execução; snippets não são implementação pronta.

Não foram iniciados aplicativo, SSH, testes, build, preflight operacional, migrations ou serviços. Não há destino SSH/credenciais de teste fornecidos. Todas as conexões/segredos usados no aceite devem ser sintéticos e autorizados. O novo cofre torna a mudança multietapas e de alto risco; revisão independente de criptografia, fronteira askpass e lifecycle é gate obrigatório da entrega futura.
