# Issue #20 — Atomicidade da atualização de templates e senhas SSH

### Descrição e Causa Raiz

**Veracidade: bug confirmado por inspeção do fluxo executável atual.** A revisão otimista protege a linha de `tab_templates`, mas a senha é gravada e confirmada em outra transação antes dessa proteção. Não foi executada reprodução dinâmica nesta etapa; as evidências abaixo comprovam a ordem das operações. Este documento é somente planejamento.

**Causa Raiz Comprovada:** em `src-tauri/src/workspace/manager.rs:L732-757`, `template_update` reutiliza o identificador da credencial do template e aguarda `update_or_store_credential`. Somente depois chama `templates().update(template_id, expected_revision, ...)`, em `L770-777`. Não existe compensação nem transação compartilhada entre essas operações.

Rastreamento completo, da origem ao efeito e de volta ao usuário:

1. **UI:** `src/components/workspace/TemplateEditorModal.vue:L272-283` produz `PasswordCredentialOperation::Set` quando há uma nova senha. `src/components/workspace/TabWorkspaceView.vue:L377-399` mantém o template em edição e envia seu `id` e sua `revision`; em erro, mantém o modal aberto.
2. **Store:** `src/stores/templates.ts:L104-121` repassa revisão/input, recarrega templates apenas no sucesso e registra/repropaga o erro.
3. **API IPC:** `src/ipc/templates.ts:L32-41` chama o binding Tauri. `src-tauri/src/commands/templates.rs:L41-52` declara o contrato de `expected_revision` e delega ao manager. Este fluxo não possui rota HTTP/controller web.
4. **Coordenação:** `src-tauri/src/workspace/manager.rs:L719-757` valida o input, lê o template atual e altera a credencial antes da atualização protegida por revisão.
5. **Serviço do cofre:** `src-tauri/src/credentials/service.rs:L296-327` conserva o mesmo ID, cifra a senha recebida e persiste a substituição. Para conexão anteriormente sem senha, `L328-333` insere uma credencial antes de saber se o template será aceito.
6. **DB da credencial:** `src-tauri/crates/mt-db/src/repos/credentials.rs:L119-125` abre `credentials_update`; `L267-290` modifica nonce, ciphertext, versão criptográfica e timestamp por ID. O writer confirma cada closure individualmente em `src-tauri/crates/mt-db/src/writer.rs:L527-539`.
7. **DB do template:** `src-tauri/crates/mt-db/src/repos/templates.rs:L195-217` abre outra operação, `templates_update`. `L425-435` rejeita a revisão desatualizada; `L461-500` também mantém o predicado SQL de revisão. O rollback dessa segunda transação não desfaz a primeira.
8. **Retorno:** `src-tauri/src/workspace/errors.rs:L28-31` converte a rejeição em `template.conflict`, que retorna pelo IPC à store/UI. A emissão de evento em `manager.rs:L778` não ocorre quando há erro.
9. **Efeito efetivo, rastreamento reverso:** a abertura SSH lê a senha pelo ID em `manager.rs:L1387-1407`; `service.rs:L353-381` busca e decifra a linha de `credentials`. Essa linha já foi sobrescrita pelo passo 6, embora o template permaneça na revisão aceita anteriormente.

**Cenário determinístico derivado do código:** criar template SSH com senha A e revisão R; atualizar metadados para R+1 conservando a referência da credencial; enviar atualização com `expected_revision=R` e senha B. O código atual grava B, depois devolve `template.conflict`. O template continua R+1, mas `retrieve_credential` retorna B. O mesmo problema pode atingir rejeições posteriores por validação de segredos/constraints. Na transição local/chave → senha, uma tentativa rejeitada pode deixar credencial órfã.

### Arquivos afetados

Implementação prevista:

- `src-tauri/src/workspace/manager.rs`: coordenação de preparação e persistência atômica, mantendo o evento após sucesso.
- `src-tauri/src/credentials/service.rs`: preparação de `StoredCredentialRow` cifrada sem gravação, reutilizável pelas operações existentes.
- `src-tauri/crates/mt-db/src/repos/templates.rs`: operação transacional combinada para template e credencial.
- `src-tauri/crates/mt-db/tests/template_credentials.rs` **(novo)**: regressões transacionais, concorrência e rollback.
- `src-tauri/tests/template_credentials.rs` **(novo)**: regressão pelo `WorkspaceManager`, com cofre real e senha decifrada.

Os helpers públicos já existentes em `repos/credentials.rs` serão reutilizados sem exigir alteração nesse arquivo. UI, store, comandos IPC e `workspace/errors.rs` são referências de contrato/validação. Nesta etapa, o único arquivo preenchido é `docs/issues/20/plan.md`.

### Execuções propostas

1. Trabalhar na worktree dedicada e manter o escopo em `template_update`. Usar DB temporário e segredos sintéticos nos testes. Preservar os dados existentes do cofre.
2. Separar a preparação criptográfica da persistência no `VaultService`: criar helper interno que recebe projeto, ID opcional e senha; verifica a DEK desbloqueada; conserva o ID existente ou gera UUID; cifra com a mesma versão/AAD e retorna `StoredCredentialRow`. Não executar leitura/gravação de credencial nesse helper. Manter proteção `Zeroizing` da DEK e não transportar senha em claro ao writer, logs, eventos ou DTOs de saída.
3. Reutilizar a preparação nos métodos atuais do cofre para evitar duplicação da criptografia, preservando suas assinaturas e comportamentos. O fluxo corrigido de atualização de template deixará de chamar o método que grava independentemente. Na atualização de uma linha existente, preservar `created_at`, como faz o helper SQL atual.
4. Acrescentar em `TemplatesRepo` uma operação interna ao backend, por exemplo `update_with_credential`, que receba ID, revisão esperada, input normalizado, timestamp e linha cifrada preparada. Manter `update` disponível para os fluxos sem nova senha. Não alterar o contrato IPC nem gerar bindings novos.
5. A operação combinada deve validar/sanitizar o input e executar **uma única** `db.write_sized`. Dentro da closure: carregar o template; rejeitar ausência/revisão divergente antes de qualquer escrita; conferir que o projeto da credencial coincide com o do template e que seu ID coincide com a referência normalizada do input. Para substituir senha existente, conferir também a referência persistida do template e a propriedade da linha existente; para uma nova credencial, exigir ID novo, sem sobrescrever outro registro. Toda conferência dependente do estado persistido deve ocorrer nessa mesma transação.
6. Reutilizar `update_template_tx` para atualizar template, revisão, padrão e projeção de busca; usar `get_credential_conn`, `update_credential_tx` ou `insert_credential_tx` com a **mesma** `Transaction` para a credencial. Propagar qualquer erro ao writer, permitindo rollback de ambas as tabelas e da projeção. Não chamar métodos assíncronos de repositório dentro da closure, pois abririam outro trabalho/transação. Ajustar o orçamento de `write_sized` para incluir a linha cifrada.
7. No manager, para `Set`, preparar a linha sem persistir, normalizar a conexão para `KeepExisting { credential_id }` e chamar a operação combinada. Para os demais tipos de conexão/operação, preservar o caminho atual. Manter validações de ambiente/cwd, códigos de erro e `emit_template_changed` somente depois da confirmação bem-sucedida.
8. Não considerar uma comparação antecipada de revisão no manager como solução: entre leitura e escrita outra atualização pode vencer. Não substituir a atomicidade por restauração compensatória da senha, mutex apenas local ou simples inversão de duas gravações independentes.
9. Implementar todo esse bloco e seus testes antes de executar testes, checagens de tipos ou build. Em seguida, validar em lote, corrigir conjuntamente as falhas e repetir os alvos afetados.

### Testes de regressão e validação em lote

Criar fixtures com `Db::open(DbConfig::for_path(...))`, projeto válido e diretório temporário, seguindo o padrão de `src-tauri/crates/mt-db/tests/search_sources.rs:L29-62`. Na suíte nativa, instanciar `WorkspaceManager::new` com suas dependências e `VaultService` compartilhando o mesmo DB; configurar o cofre com senha sintética. Não abrir janela, conexão SSH externa ou PTY para testar atualização de templates.

Casos obrigatórios:

- **`stale_revision_preserves_ssh_password`:** reproduzir R → R+1 → tentativa com R/senha B pelo manager. Asserir exatamente `template.conflict`; comparar template, revisão, referência, linha cifrada completa e quantidade de credenciais com o estado imediatamente anterior à tentativa. Decifrar e comprovar que a senha continua A.
- **`accepted_revision_updates_ssh_password`:** revisão válida muda senha para B, incrementa revisão uma vez e mantém ID/`created_at` da credencial; leitura posterior pelo cofre confirma B.
- **`concurrent_updates_keep_winner_password`:** preparar duas solicitações sobre a mesma revisão antes de disparar a persistência; executar concorrentemente com sincronização determinística, sem sleeps. Exigir um sucesso e um conflito; o conteúdo final do template e da credencial deve pertencer ao mesmo vencedor. Cobrir a janela após a preparação, para impedir que uma checagem antecipada isolada satisfaça o teste.
- **`rejected_new_password_creates_no_credential`:** template local ou SSH com chave; tentativa de mudar para senha usando revisão obsoleta. Asserir nenhuma linha adicional e conexão original intacta. Testar também a transição aceita, que cria exatamente uma linha referenciada.
- **`credential_write_failure_rolls_back_template`:** na fixture do mt-db, usar trigger SQLite temporário de teste que aborte INSERT/UPDATE de `credentials`. Com revisão válida, forçar a falha depois da atualização do template dentro da closure; comprovar rollback da revisão, configuração, promoção de padrão e projeção de busca. Não adicionar mecanismo de falha ao código de produção.
- **`template_rejection_preserves_credentials`:** input com segredo literal rejeitado pelo sanitizador e colisão de nome/constraint; comprovar igualdade das credenciais antes/depois. Verificar ausência de órfãs para a variante de nova senha.
- **`credential_scope_mismatch_is_rejected`:** linha preparada com projeto/ID incompatível é recusada sem alterar nenhum dos projetos; testar tanto substituição quanto inserção.
- **Não-regressão:** atualizações locais e SSH com chave; manutenção de senha existente usando o payload interno já suportado; cofre bloqueado com `Set`; template removido antes da persistência; manutenção de um único padrão por projeto. Não ampliar esta issue para corrigir outros comportamentos do formulário.
- Conferir que senhas sintéticas não aparecem no template serializado nem na projeção de busca e que erro não percorre o ramo de emissão de sucesso. Preservar `template.conflict` no caminho público.

Executar os comandos da seção Validação somente após implementar o bloco completo. Não há alegação de testes aprovados nesta etapa de planejamento.

### Banco de dados

**Nenhuma migration.** `credentials` e `tab_templates` já estão no mesmo SQLite e os helpers aceitam `Transaction`. A mudança reúne operações na transação existente do writer (`TransactionBehavior::Immediate`), preservando WAL, esquema, criptografia e projeções. Não é possível reconstruir automaticamente uma senha anterior já sobrescrita; o plano previne novas ocorrências.

### Riscos de quebra e Não-Regressão

- **Concorrência/TOCTOU:** revisão e propriedade conferidas somente fora da transação continuariam vulneráveis. Cobertura: teste concorrente com payloads preparados antes da persistência e validação transacional.
- **Commit parcial em falha SQL:** salvar template antes da credencial em transações distintas apenas inverte o problema. Cobertura: trigger de falha e igualdade das duas tabelas/projeção após rollback.
- **Contrato e referências em sessões:** trocar IDs indiscriminadamente pode afetar referências já capturadas. Preservar o ID ao substituir uma senha existente; gerar ID somente na transição sem credencial anterior. Cobertura: ID estável, decifragem e transição aceita/rejeitada.
- **Isolamento por projeto e AAD:** projeto/ID divergentes podem gravar na credencial errada ou impedir decifragem. Cobertura: rejeição de incompatibilidades e leitura real pelo `VaultService`.
- **Cofre e memória:** preservar erro de cofre bloqueado e tratamento da DEK; somente bytes cifrados seguem para a closure. Cobertura: cofre bloqueado e ausência de senha em persistência pública.
- **Padrão, busca e validação de segredos:** não duplicar o SQL do template; reutilizar `update_template_tx` e `validate_and_check_secrets`. Cobertura: promoção/rollback de padrão, projeção e rejeições por segredos/constraints.
- **Eventos e UI:** manter retorno/códigos e emissão somente após commit; a store continuará apresentando conflito sem sinalizar sucesso. Sem mudança de DTO, assinatura Tauri ou frontend.

### Validação

Comandos a executar a partir da raiz da worktree após a implementação:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db --test template_credentials
cargo test --manifest-path src-tauri/Cargo.toml -p maxterminal --test template_credentials
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db
cargo check --manifest-path src-tauri/Cargo.toml -p maxterminal --all-targets
git diff --check
```

**Prova conclusiva:** os testes novos devem ser descobertos e executados, incluindo `stale_revision_preserves_ssh_password`: retorno `template.conflict` combinado com senha A decifrada, linha cifrada inalterada e template em R+1. O teste concorrente deve demonstrar que a senha final é a do único vencedor, e o teste de falha injetada deve demonstrar rollback integral. Registrar qualquer bloqueio ambiental separadamente de falha do produto; não declarar sucesso com testes filtrados para zero casos.

### Skills Aplicáveis

- `security-and-hardening`: fronteira IPC → cofre → DB, propriedade de credenciais, proteção de segredos e evidências com dados sintéticos.
- `code-review`: revisão da unidade transacional, tratamento de erros, contratos e testes antes da integração.
- `test-driven-development`: orientar os cenários de regressão e asserções observáveis; nesta tarefa prevalece a ordem explícita de implementar o bloco e testes antes da validação em lote.

Não há necessidade de skills de UI/Vue para a correção proposta. A integração e eventual limpeza da worktree devem respeitar a confirmação de commit/merge exigida pelo projeto.
