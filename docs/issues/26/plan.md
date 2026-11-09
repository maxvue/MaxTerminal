# Plano de implementação — Issue #26

### Descrição e Causa Raiz

**Veracidade: confirmada por inspeção estática.** A divergência documental continua presente nesta worktree. O problema é o contrato descrito no Rustdoc de `ShellPersister::update_session_runtime_blocking`, não a necessidade demonstrada de mudar sua execução. A reprodução consiste em comparar o comentário com o corpo do método e seguir a chamada até o repositório e a fachada do banco. Não foi necessário executar um PTY para comprovar a divergência textual; os cenários de runtime foram inspecionados no código, não executados nesta etapa.

**Causa raiz comprovada:**

- `src-tauri/crates/mt-shell/src/persist.rs:L124-139`: o comentário atribui a escrita a `Db::write_blocking`, mas o corpo clona `SessionsRepo`, consulta `tokio::runtime::Handle::try_current()` e, somente em caso de sucesso, agenda uma tarefa com `handle.spawn`. Essa tarefa aguarda `SessionsRepo::update_runtime_state`; o chamador não aguarda a tarefa nem recebe confirmação de persistência.
- `src-tauri/crates/mt-shell/src/persist.rs:L132-138`: quando não há runtime Tokio atual acessível, o `if let` não executa seu corpo. Nenhuma atualização é agendada, não há fallback e o método retorna `()` sem comunicar essa ausência ao chamador.
- `src-tauri/crates/mt-shell/src/persist.rs:L133-136`: o resultado da operação é descartado por `let _ =`. O wrapper não propaga o erro nem emite seu próprio log de falha. Isso não equivale a afirmar que nenhuma camada interna possa registrar logs.
- `src-tauri/crates/mt-db/src/repos/sessions.rs:L164-186`: `update_runtime_state` usa `Db::write_sized("sessions_update_runtime", 256, ...)`, atualiza `sessions.last_cwd` com `COALESCE`, atualiza `integration_mode` e sincroniza a projeção de busca na transação.
- `src-tauri/crates/mt-db/src/lib.rs:L425-438` e `L440-455`: `write_sized` delega ao escritor assíncrono por `execute_tx(...).await`; `write_blocking` é outro caminho, com aquisição de orçamento e execução síncrona. A referência documental aponta para o caminho errado.

**Fluxo causal e rastreamento reverso:** os campos em SQLite remontam a `SessionsRepo::update_runtime_state` ← tarefa Tokio do `ShellPersister` ← `ShellService::dispatch_tracker_events`. Neste último, `IntegrationModeChanged` passa `None` para o diretório e o novo modo (`src-tauri/crates/mt-shell/src/service.rs:L790-812`); `CwdChanged` passa o diretório sanitizado e o modo corrente (`src-tauri/crates/mt-shell/src/service.rs:L815-843`). O registro em memória e os eventos do sink são atualizados/emitidos sem aguardar confirmação dessa escrita. Portanto, eventos de estado não constituem confirmação de persistência. UI, store frontend e rotas HTTP não participam do contrato documental corrigido; a entrada relevante é o evento interno do tracker, e a saída persistida é a tabela `sessions`.

**Agravantes:** o sufixo `_blocking` pode induzir a supor escrita concluída no retorno; o comentário omite a dependência do contexto Tokio e o descarte do resultado. O contrato deve expressar uma tentativa assíncrona, sem garantia de conclusão no retorno, sem prometer entrega durante encerramento do runtime. Não há evidência aqui que justifique atribuir a origem histórica do comentário a uma alteração específica.

### Arquivos afetados

- `src-tauri/crates/mt-shell/src/persist.rs`: único arquivo de implementação a alterar, exclusivamente no Rustdoc de `update_session_runtime_blocking`.
- `docs/issues/26/plan.md`: artefato desta etapa de planejamento.

Referências de leitura para a execução: `src-tauri/crates/mt-shell/src/service.rs`, `src-tauri/crates/mt-db/src/repos/sessions.rs` e `src-tauri/crates/mt-db/src/lib.rs`.

### Execuções propostas

1. Trabalhar na worktree dedicada e conferir o estado inicial do Git, preservando alterações preexistentes. Reconfirmar os trechos acima, pois as linhas podem mudar até a execução.
2. Reescrever integralmente o Rustdoc do método em português, descrevendo que ele **agenda** a atualização de `last_cwd` e `integration_mode` quando existe um runtime Tokio atual acessível.
3. Documentar explicitamente o caminho `SessionsRepo::update_runtime_state` → `Db::write_sized`, usando links Rustdoc resolvíveis, por exemplo, [`SessionsRepo::update_runtime_state`] e [`Db::write_sized`]. Remover a atribuição a `Db::write_blocking`.
4. Incluir uma seção de comportamento/garantias: assinatura síncrona, tarefa assíncrona independente, retorno sem aguardar sua conclusão, nenhuma confirmação de escrita ao chamador e resultado da operação descartado pelo wrapper.
5. Registrar expressamente que, sem runtime Tokio atual acessível, nenhuma atualização é agendada e a função retorna sem informar falha. Esclarecer que o sufixo `_blocking` não significa espera pela persistência neste método.
6. Manter assinatura, nome, corpo, tratamento de erro, SQL e chamadores intactos. A mudança autorizada resolve a divergência documental; adicionar fallback, logging, propagação de erros ou renomear a API exigiria uma alteração funcional com escopo e testes próprios.
7. Concluir todo o bloco documental antes da validação. Executar as verificações abaixo em lote; corrigir eventuais falhas conjuntamente e repetir somente as verificações afetadas. Não executar testes, tipos ou build entre edições parciais.

### Testes de regressão e validação em lote

Esta correção é documental e reversível: não criar testes de comportamento que apenas repliquem o corpo existente. A validação combina integridade do código executável, geração de documentação e revisão do contrato contra a implementação.

Na raiz da worktree, executar o script abaixo após concluir o Rustdoc. Ele compara com `HEAD`, exige mudança documental e comprova que nenhuma linha não documental do arquivo foi alterada. Se houver alterações preexistentes nesse arquivo, usar como base o conteúdo inicial preservado antes da implementação, em vez de atribuí-las à correção.

```bash
python3 - <<'PY'
from pathlib import Path
import subprocess

path = 'src-tauri/crates/mt-shell/src/persist.rs'
before = subprocess.check_output(['git', 'show', f'HEAD:{path}'], text=True)
after = Path(path).read_text()

def executable_lines(text):
    return [line for line in text.splitlines()
            if line.strip() and not line.lstrip().startswith('///')]

assert before != after, 'A documentação ainda não foi alterada.'
assert executable_lines(before) == executable_lines(after), 'Houve alteração fora do Rustdoc.'
print('Validação concluída: somente comentários Rustdoc foram alterados.')
PY
git diff --check
RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links" cargo doc --manifest-path src-tauri/Cargo.toml -p mt-shell --no-deps
```

Critérios de revisão em lote do Rustdoc gerado: (a) nomeia o caminho real até `write_sized`; (b) informa a condição do runtime atual; (c) informa ausência de agendamento nesse contexto quando o runtime não existe; (d) distingue retorno do método de conclusão da tarefa; (e) informa descarte do resultado, sem inventar confirmação, retry ou fallback. Esses critérios cobrem os cenários com runtime, sem runtime e com erro de escrita por confronto estático com o corpo preservado. Não declarar que foram executados testes comportamentais desses cenários.

### Banco de dados

Nenhuma. Não há migration, alteração de schema, SQL, WAL, sanitização ou projeções de busca.

### Riscos de quebra e Não-Regressão

- **Contrato público:** renomear o método devido ao sufixo seria uma quebra desnecessária. Preservar nome, assinatura e chamadores; o script acima verifica que o código executável não mudou.
- **Garantias exageradas:** substituir apenas o nome da API deixaria as omissões de runtime e conclusão. Revisar os cinco critérios documentais em conjunto.
- **Semântica de erro:** documentar o descarte no wrapper sem afirmar ausência universal de logs em camadas internas.
- **Links Rustdoc:** caminhos incorretos podem produzir documentação inválida; validar os links com `cargo doc` e o lint específico.
- **Persistência e eventos:** a emissão de eventos, o tratamento de `None` em `last_cwd`, a sanitização e a escrita transacional permanecem respaldados pelo código preservado. Não acrescentar garantias de ordenação ou de conclusão no shutdown.
- **Ambiente:** se a geração de documentação for bloqueada por dependências ou configuração Rust, registrar o erro real e a validação pendente; não classificar geração interrompida como sucesso.

### Validação

Aceite: script de integridade com saída de sucesso, `git diff --check` sem erros, `cargo doc` concluído sem links quebrados e revisão do Rustdoc gerado satisfazendo os cinco critérios acima. A compilação de documentação prova a integridade dos links; o confronto entre comentário e código comprova a correção semântica, que o compilador sozinho não verifica. A prova de não-regressão funcional é a preservação das linhas executáveis, não um teste artificial de presença de frases.

Nesta etapa foi feita apenas inspeção estática e elaboração do plano; os comandos de validação da futura alteração documental ainda não foram executados.

### Skills Aplicáveis

- `code-review`: revisão final da precisão do contrato, do escopo estritamente documental e da preservação da API antes de integrar a correção.
- `git-worktree`: quando for necessário criar ou administrar uma worktree para a execução; a sessão de planejamento já está em `.max-code-worktrees/wt-implement-issue-26`.

Skills de Vue, banco de dados, TDD ou orquestração complexa não são necessárias para esta alteração localizada de Rustdoc.
