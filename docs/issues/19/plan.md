# Plano de implementação — Issue #19

### Descrição e Causa Raiz

**Veracidade: bug confirmado por inspeção do fluxo executável atual.** A edição de um template SSH autenticado por senha, sem informar outra senha, salva um identificador vazio no lugar do identificador existente. Não há tratamento de `Keep` em `WorkspaceManager::template_update`. A análise é estática; o cenário não foi executado contra um servidor SSH nesta etapa.

**Causa Raiz Comprovada:** `src-tauri/src/workspace/manager.rs:L719-777` carrega o template existente, mas resolve somente `PasswordCredentialOperation::Set` (`L732-768`). A operação `Keep` segue intacta para o repositório. Em `src-tauri/crates/mt-core/src/workspace/template.rs:L645-653`, somente `KeepExisting` mantém um ID; as demais operações resultam em `credential_id: String::new()`. Esse resultado intermediário é efetivamente persistido, contrariando o contrato de `Keep` documentado em `template.rs:L132-150`.

Rastreamento UI ⇄ Store ⇄ IPC ⇄ serviço ⇄ DB e consumo posterior:

1. `src/components/workspace/TemplateEditorModal.vue:L192-214` identifica a autenticação por senha e inicializa o campo vazio; `L42-44` promete preservar a senha; `L285-296` emite `action: 'keep'` quando a senha permanece vazia.
2. `src/components/workspace/TabWorkspaceView.vue:L382-394` encaminha ID, revisão e input para `src/stores/templates.ts:L104-117`. A store passa o payload sem resolver credenciais e recarrega os templates após sucesso.
3. `src/ipc/templates.ts:L32-41` chama o comando IPC; `src-tauri/src/commands/templates.rs:L41-52` delega a `WorkspaceManager::template_update`. Não existe rota HTTP nesse fluxo.
4. `src-tauri/src/workspace/manager.rs:L722-768` já possui a configuração anterior, porém apenas substitui `Set` por `KeepExisting`. A informação necessária para resolver `Keep` está disponível e não é utilizada.
5. `src-tauri/crates/mt-db/src/repos/templates.rs:L195-217` chama `validate_and_check_secrets`; `L248-252` reutiliza o validador do core, produzindo o ID vazio. `L455-492` serializa a conexão e substitui `tab_templates.connection_json`, incrementando a revisão. A transação verifica a revisão em `L432-436`, mas não recupera a associação da credencial.
6. No caminho inverso, o retorno/reload entrega a conexão já corrompida à store. Na abertura posterior, `src-tauri/src/workspace/manager.rs:L1387-1400` passa o ID persistido a `VaultService::retrieve_credential`. `src-tauri/src/credentials/service.rs:L353-364` procura literalmente esse ID e retorna `VAULT_CREDENTIAL_NOT_FOUND` quando o cofre está desbloqueado. Com o cofre bloqueado, `L345-350` retorna primeiro `CREDENTIAL_LOCKED`, mascarando temporariamente a perda da referência.

**Agravantes:** salvar aparenta sucesso; simples renomeação basta; novas edições continuam oferecendo manutenção de senha pela presença de `auth.kind === 'password'`, mesmo com ID vazio. A credencial cifrada não é apagada nesse fluxo: perde-se a associação do template. O teste de conexão já resolve `Keep` em `manager.rs:L602-629`, mas esse caminho não é usado ao salvar, portanto um teste bem-sucedido antes da edição não garante persistência correta.

**Cenário verificável:** configurar um cofre de teste, criar template SSH com `Set`, guardar seu ID de credencial e revisão; atualizar apenas o nome enviando `Keep`; reler o template e recuperar a senha com o ID relido. Atualmente a atualização persiste `"credentialId":""`; após a correção deve preservar exatamente o ID anterior e permitir recuperar a mesma senha.

### Arquivos afetados

- `src-tauri/src/workspace/manager.rs`: resolver `Keep` a partir da conexão persistida antes de atualizar o repositório; rejeitar manutenção sem referência válida.
- `src-tauri/tests/template_credentials.rs` (novo): testes de integração do manager com SQLite temporário e cofre real de teste, cobrindo persistência e recuperação da credencial.
- `tests/components/workspace/TemplateEditorModalSsh.spec.ts`: regressão do contrato de edição com senha vazia e da substituição explícita.

O artefato desta etapa é `docs/issues/19/plan.md`. Os demais caminhos citados são evidências e pontos de validação do fluxo.

### Execuções propostas

1. Executar a implementação em worktree isolada e conferir o estado inicial. Preparar fixtures com projeto, banco temporário e `WorkspaceManager`, usando os construtores reais e dependências já declaradas; não iniciar PTY ou conexão externa para testar atualização de template.
2. Em `template_update`, após carregar `existing` e antes de `templates().update`, tratar explicitamente a conexão SSH com senha e operação `Keep`. Ler o ID exclusivamente de `existing.config.connection`, exigindo que ela seja SSH com `SshAuth::Password` e ID não vazio após inspeção de espaços. Clonar o ID original integralmente, sem normalizá-lo, pois ele participa da autenticação criptográfica.
3. Converter essa operação para `KeepExisting { credential_id }`, conservando o host e o usuário do input e todos os demais campos enviados. A resolução deve copiar somente a referência: não descriptografar, recriar, atualizar ou remover a credencial e não exigir desbloqueio do cofre para uma edição de metadados.
4. Quando a configuração anterior for local, usar chave SSH ou possuir ID vazio/branco, retornar `codes::TEMPLATE_INVALID` com mensagem em português solicitando informar uma senha SSH. Fazer isso antes da gravação, sem alterar nome, revisão ou credenciais. Não tentar adivinhar a associação de templates anteriormente corrompidos.
5. Preservar o processamento existente de `Set` com `update_or_store_credential` e sua conversão para `KeepExisting`, bem como o comportamento de `KeepExisting`, conexão local e autenticação por chave. Manter a validação estrutural inicial, a checagem transacional de `expected_revision` e a emissão de `template://changed` somente depois do sucesso da atualização.
6. Manter o contrato público `action: 'keep'`: a resolução contextual pertence ao manager, que dispõe do template anterior. Não transferir o ID ou a senha para lógica adicional do formulário. Não rejeitar indiscriminadamente `Keep`/`Set` no validador genérico: ele também é chamado antes de o manager resolver a credencial.
7. Implementar integralmente a correção e os testes abaixo antes de executar testes, tipos ou build. Depois executar a bateria definida, corrigir as falhas pertinentes em lote e repetir os comandos afetados; registrar separadamente eventuais impedimentos de ambiente.

### Testes de regressão e validação em lote

**Integração Rust — novo alvo `template_credentials`:** usar fixtures descartáveis, senhas exclusivamente sintéticas, SQLite em WAL e `VaultService` real. Exercitar o método público `WorkspaceManager::template_update`, não apenas reproduzir sua lógica em um helper de teste.

- `template_update_keep_preserves_credential`: criar via manager com `Set`, renomear com `Keep`, verificar nome novo e revisão anterior + 1; comparar ID retornado e relido com o original, verificar que não é vazio e recuperar a senha original por esse ID. Repetir a edição para comprovar manutenção sucessiva.
- Conferir o JSON persistido e a linha da credencial: ausência de senha em texto puro na configuração e preservação de ID, ciphertext e nonce após `Keep`. Reabrir o banco/cofre na fixture e confirmar a mesma associação persistente.
- Bloquear o cofre antes de renomear com `Keep`: a edição deve funcionar; desbloquear depois e recuperar a senha original. Isso impede uma correção que passe a descriptografar durante a manutenção da referência.
- Exercitar `Keep` sobre template local, SSH por chave e SSH com ID vazio/branco (fixture legada): esperar `TEMPLATE_INVALID` e igualdade do registro antes/depois, incluindo revisão.
- Exercitar revisão desatualizada com `Keep`: esperar o conflito existente e garantir que o estado vencedor e a credencial permanecem intactos. Incluir atualização concorrente previamente concluída, sem depender de temporização.
- Exercitar `Set` após `Keep`: a nova senha deve ser recuperável pelo ID persistido. Exercitar também o caminho já aceito de `KeepExisting`, atualização local e SSH por chave para evitar regressão do despacho.

**Componente Vue — arquivo existente:** montar um template SSH com referência sintética, editar somente o nome, deixar a senha vazia e submeter. Exigir `operation` exatamente `{ action: 'keep' }`, sem senha nem ID fornecido pelo formulário; verificar o texto de manutenção. Preencher outra senha e exigir `Set`. Asserções apenas sobre emissão do modal não substituem o teste integrado Rust, pois esse contrato já funciona atualmente.

Executar o bloco completo de implementação/testes antes da primeira validação. Não executar comandos que exportem bindings ou alterem arquivos gerados sem mudança de contrato que os justifique.

### Banco de dados

**Nenhuma migration.** O schema já armazena a referência em `tab_templates.connection_json` e a credencial cifrada separadamente. Corrigir a resolução antes da escrita é suficiente. Manter WAL e as transações existentes. Para registros que já perderam o ID, a recuperação segura é informar novamente a senha (`Set`); não há evidência de uma associação unívoca que permita migration automática de reparo.

### Riscos de quebra e Não-Regressão

| Risco | Garantia / teste |
| --- | --- |
| Copiar a conexão antiga inteira e descartar campos editados | Comparar nome, host e usuário enviados com o retorno; copiar apenas o ID existente. |
| Passar a exigir cofre desbloqueado para renomear | Teste de `Keep` com cofre bloqueado, seguido de recuperação após desbloqueio. |
| Aceitar `Keep` sem senha anterior e persistir ID vazio novamente | Matriz de conexão local, chave e ID inválido, com ausência de mutações. |
| Perder controle de concorrência ou sobrescrever a edição vencedora | Teste com `expected_revision` obsoleta e igualdade do estado persistido. |
| Alterar a substituição de senha ou autenticação por chave/local | Testes de `Set`, `KeepExisting` e atualizações dos outros modos. |
| Expor senha ou trocar a credencial de outro template/projeto | Copiar somente a referência do template carregado pelo ID; fixtures com dois templates/projetos devem preservar suas referências independentes e a recuperação deve respeitar o projeto. |
| Testes exigirem servidor SSH, display gráfico ou banco pessoal | Criar fixtures temporárias sem spawn de sessão; validar o consumidor nativo pelo serviço real de recuperação. A compilação do alvo Tauri ainda requer as dependências nativas do projeto. |

### Validação

Comandos a executar **na implementação**, a partir da raiz da worktree, após concluir todo o bloco autorizado:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked -p maxterminal --test template_credentials
cargo test --manifest-path src-tauri/Cargo.toml --locked -p mt-core -p mt-db
npm run test -- tests/components/workspace/TemplateEditorModalSsh.spec.ts tests/frontend/workspace/templates.store.spec.ts
npm run typecheck
npm run typecheck:vue
npm exec eslint -- --max-warnings 0 tests/components/workspace/TemplateEditorModalSsh.spec.ts
cargo check --manifest-path src-tauri/Cargo.toml --locked -p maxterminal
```

**Prova conclusiva do defeito corrigido:** o teste `template_update_keep_preserves_credential` precisa chamar o manager real, reler a persistência e recuperar a senha com o ID relido. A igualdade exata dos IDs antes/depois e a recuperação da senha original comprovam o reparo da associação usada na abertura de abas. Registrar resultados e quantidade de testes executados; comando com zero testes não constitui aprovação. Esta etapa de planejamento não executa essa bateria nem afirma sucesso de conexão SSH real.

### Skills Aplicáveis

- `security-and-hardening`: fronteira IPC, credenciais sintéticas nos testes e preservação de referência sem exposição de senha; aplicar os controles pertinentes ao cofre local.
- `vitest-skill` e `vue-vitest-testing-best-practices`: testes do formulário com Vue Test Utils, mocks do IPC e sincronização de eventos.
- `vue-typescript-best-practices`: fixtures e payloads tipados conforme os bindings existentes.
- `code-review`: revisão do diff e dos contratos de credenciais, concorrência e persistência antes de integrar a implementação.

Seguir também o `AGENTS.md` do projeto. A cadência de implementação e validação em lote definida nesta solicitação prevalece sobre rotinas genéricas das skills.
