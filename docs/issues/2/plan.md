# Plano de implementação — Issue #2

## Descrição e Causa Raiz

**Relato confirmado por evidência estática no commit `d4317e5279c4eda34514dc3183039c3503a4a1b4`.** A remoção de um alvo com `remove_managed=true` exclui um symlink substituído pelo usuário, mesmo quando ele aponta para uma pasta pessoal diferente da fonte sincronizada. O conteúdo referenciado não é apagado: perde-se o link personalizado. A exclusão ocorre sem backup nesse ramo.

**Causa Raiz Comprovada:** `src-tauri/crates/mt-agents/src/skills/sync.rs:L290-314`. O serviço carrega os registros gerenciados, consulta `symlink_metadata(entry.dest_path)` e, nas linhas 301–304, usa exclusivamente `is_symlink()` para autorizar `remove_file`. Não lê o destino atual do link nem compara com `entry.source_path`; também não exige que `entry.mode` seja `Symlink`. Assim, trata o registro histórico de um caminho como prova de que o objeto atual ainda é gerenciado. O comentário de contrato da linha 290 promete remover apenas entradas comprovadamente gerenciadas e não modificadas, mas a implementação não cumpre essa condição para links.

### Rastreamento dos dados e fluxo causal

1. **Origem do vínculo gerenciado:** `src-tauri/crates/mt-agents/src/skills/sync.rs:L648-691` instala o symlink para `skill.source_dir`. O caminho instalado, modo e fonte são persistidos por `record_managed_entry`, em `sync.rs:L855-899`, após a instalação em `sync.rs:L463-480`.
2. **DB:** `src-tauri/crates/mt-db/migrations/0008_13_agents_skills_memory.sql:L89-100` já armazena `dest_path`, `mode` e `source_path`. A leitura em `sync.rs:L817-852` disponibiliza esses campos ao serviço. A evidência necessária para conferir a origem existe; ela é ignorada no ramo destrutivo.
3. **UI → Store:** `src/components/skills/SkillsSyncPanel.vue:L249-254` chama `removeTarget(targetId, false)`. Portanto, o botão atual não dispara a exclusão física relatada. A store aceita explicitamente `true` em `src/stores/skills.ts:L196-212`, repassa o argumento e remove o alvo e seu plano do estado após sucesso.
4. **Store → IPC → comando nativo:** `src/ipc/skills.ts:L30-32` chama o binding em `src/types/bindings.ts:L343-344`. `src-tauri/src/commands/skills.rs:L44-56` encaminha `target_id` e `remove_managed` diretamente ao serviço. Neste fluxo não há rota HTTP ou controller separado: o comando Tauri é a fronteira da API.
5. **Service → filesystem → DB:** `sync.rs:L295-314` efetua a limpeza; `sync.rs:L317-330` exclui o alvo, invalida planos e retorna a quantidade de itens efetivamente removidos. A chave estrangeira em `0008_13_agents_skills_memory.sql:L91` remove os registros de entradas em cascata. Não há validação posterior que recupere o link perdido.

**Comparação com proteção preexistente:** a classificação de poda já lê o link e compara seu destino com a fonte registrada em `src-tauri/crates/mt-agents/src/skills/conflict.rs:L186-218`. A adoção também confere a origem em `sync.rs:L730-757`. Essas proteções não são chamadas por `remove_target`; logo, não corrigem o relato.

### Cenário de reprodução a automatizar

Em um diretório temporário, cadastrar um alvo `Symlink`, executar `plan`/`apply`, substituir o link de uma skill por um link para uma pasta pessoal temporária contendo um arquivo sentinela e chamar `remove_target(id, true)`. No código atual, `symlink_metadata` identifica o novo link e `remove_file` o exclui; o arquivo sentinela permanece. O resultado esperado após a correção é preservar o link com o mesmo destino, preservar o sentinela e retornar zero para essa entrada.

A confirmação nesta etapa é por inspeção do fluxo executável acima, não por reprodução dinâmica. Nenhum teste, build ou operação de remoção de dados foi executado durante o planejamento. Os testes existentes em `src-tauri/crates/mt-agents/tests/skills_sync.rs:L128-323` cobrem catálogo, sincronização, conflitos, backups e watcher, mas não chamam `remove_target`.

## Arquivos afetados

Arquivos a alterar na implementação:

- `src-tauri/crates/mt-agents/src/skills/sync.rs`: restringir a exclusão de symlinks à origem e ao modo registrados.
- `src-tauri/crates/mt-agents/tests/skills_sync.rs`: adicionar testes de integração de remoção usando o serviço real, SQLite temporário e filesystem temporário.

Artefato desta etapa: `docs/issues/2/plan.md`. Os arquivos de UI, store, IPC, bindings, classificação de conflitos, resolução de caminhos e migration citados são referências do fluxo, sem alteração prevista.

## Execuções propostas

1. Trabalhar em worktree dedicada, respeitando as diretrizes do projeto, e reaproveitar `setup_test_env` de `tests/skills_sync.rs:L53-126`. Criar todos os dados de teste sob diretório temporário contido na worktree; nunca utilizar o catálogo ou a pasta pessoal reais.
2. Implementar o bloco completo da correção e dos testes abaixo antes de executar testes, verificações de tipos ou build.
3. Dentro do ramo `is_symlink()` de `remove_target`, exigir `entry.mode == SyncMode::Symlink`. Um registro de cópia substituída por link não autoriza removê-lo, mesmo se apontar à fonte original.
4. Ler o destino com `std::fs::read_link(&p)`. Se a leitura falhar, preservar a entrada e não incrementar `removed_count`. Não usar `unwrap_or_default`, pois ausência de evidência não comprova propriedade.
5. Resolver destinos relativos em relação a `p.parent()`, nunca ao diretório de trabalho do processo. Preservar a entrada se não for possível determinar o pai necessário. Comparar o caminho resultante com `Path::new(&entry.source_path)`, utilizando `canonical_or_lexical_path`, já importado em `sync.rs:L22`, para ambos os lados, conforme o padrão existente de poda e adoção. A fonte de comparação deve ser a persistida na instalação, não a configuração atual de `source_dir` nem um catálogo reescaneado.
6. Apenas se os destinos normalizados coincidirem, executar `remove_file(&p)` e incrementar o contador em caso de sucesso. Em divergência, pular o link sem criar backup, sem alterá-lo e sem seguir seu destino para remover conteúdo. Links gerenciados cujo destino foi apagado podem ser removidos quando o fallback lexical confirma a mesma fonte registrada; links quebrados para outro caminho devem ser preservados.
7. Manter o contrato `MtResult<u32>`, a remoção cadastral do alvo com cascata, a invalidação de planos e o comportamento de `remove_managed=false`. Preservar a lógica atual de cópias nesta correção cirúrgica. Não adicionar confirmação de UI ou regenerar bindings, pois não há alteração de interface pública.
8. Adicionar os testes de integração e executar a validação em lote. Corrigir as falhas relacionadas ao bloco autorizado conjuntamente e repetir a validação após as correções. Revisar o diff para confirmar o escopo antes da integração.

## Testes de regressão e validação em lote

Usar nomes com prefixo `remove_target_`, fixtures independentes e chamadas reais a `upsert_target`, `plan`, `apply` e `remove_target`. Para cenários de um item, selecionar explicitamente uma skill; não depender da quantidade total do catálogo. Conferir `errors.is_empty()` no resultado da instalação para evitar falsos positivos.

| Cenário | Asserções obrigatórias |
| --- | --- |
| Link substituído por destino pessoal absoluto | Após `remove_target(id, true)`, `symlink_metadata` continua indicando link; `read_link` devolve exatamente o destino personalizado; sentinela intacto; contador zero. Este é o teste principal de regressão. |
| Link substituído por destino pessoal relativo | Resolver a fixture em relação ao pai do link; verificar preservação do conteúdo literal de `read_link`, do sentinela e contador zero. |
| Link original gerenciado | O link é removido, contador um, diretório fonte e `SKILL.md` permanecem intactos. |
| Link relativo equivalente à fonte registrada | A equivalência é reconhecida e somente o link é removido; incluir caminho com `.`/`..` sem ancestrais symlink na fixture. |
| Link quebrado para fonte registrada | Apagar apenas a fonte temporária antes da chamada; o link gerenciado é removido e contado. Não usar `Path::exists()` para verificar existência de link quebrado. |
| Link quebrado para destino estrangeiro | O link permanece detectável por `symlink_metadata`, seu destino literal permanece igual e o contador é zero. |
| Registro `Copy` substituído por symlink | Preservar o link mesmo se apontar para a fonte registrada; contador zero. |
| Configuração da fonte alterada após sincronização | O link que ainda aponta para a fonte persistida é removível; um link substituído para a nova fonte configurada é preservado. |
| `remove_managed=false` | Preservar tanto links originais quanto personalizados; contador zero; alvo removido do cadastro. |
| Cópias intacta e modificada | Cópia intacta segue sendo removida e tem backup íntegro; cópia modificada é preservada. |
| Entrada ausente, arquivo regular ou item sem registro | Não falhar por entrada ausente; preservar arquivo substituto e item não registrado; não incluí-los no contador. |
| Alvo com links mistos | Remover apenas o link gerenciado, preservar o personalizado e retornar a contagem exata. |

Nos testes de remoção concluída, consultar o DB temporário para confirmar ausência do alvo e de suas entradas após a cascata; verificar também `list_targets`. Criar um plano antes da remoção e confirmar que sua aplicação posterior retorna `skills.plan_stale`, preservando a invalidação do cache. Não escrever testes que apenas reproduzam as condicionais internas: as provas são os efeitos observáveis no disco, banco e retorno público.

Somente depois de implementar todo esse bloco, executar primeiro a suíte `skills_sync`, depois a suíte do pacote e as verificações estáticas indicadas abaixo. Registrar os resultados reais e corrigir falhas em lote. Falhas preexistentes ou de ambiente devem ser identificadas como tais, sem declarar validação bem-sucedida.

## Banco de dados

**Nenhuma migration.** `skill_sync_entries` já contém todos os campos necessários (`mode`, `source_path`, `dest_path`). Manter o schema e a cascata existentes. Ao remover o alvo, links personalizados preservados deixam de ter registro gerenciado, coerentemente com a exclusão cadastral solicitada. Os testes devem confirmar essa combinação de preservação física e remoção cadastral.

## Riscos de quebra e Não-Regressão

- **Comparação incorreta de relativos:** usar o pai do link; testes com destinos relativos próprios e estrangeiros impedem dependência acidental do diretório de execução.
- **Fonte ausente:** uma canonicalização obrigatória impediria limpar links gerenciados quebrados; validar o fallback lexical e distinguir fonte registrada de destino estrangeiro ausente.
- **Mudança de configuração ou modo:** comparar a origem e o modo persistidos por entrada, com testes de alteração de configuração e cópia substituída por link.
- **Contagem e contrato IPC:** `u32` continua significando itens físicos realmente removidos, não registros apagados. Exercitar zero, um e lote misto sem mudar parâmetros, tipos ou bindings.
- **Cópias, backup, cascata e cache:** manter as garantias existentes e verificá-las na suíte de integração para que o ajuste do ramo symlink não altere os demais caminhos.
- **Concorrência entre verificação e exclusão:** `read_link` seguido de `remove_file` não é uma operação atômica contra substituições externas. Este plano corrige a substituição feita antes da chamada, demonstrada na issue; não deve ser apresentado como eliminação de toda corrida TOCTOU. A lógica de normalização compartilhada também conserva suas limitações para combinações complexas de ancestrais symlink e `..`; não ampliar suas garantias sem testes e análise específicos.

## Validação

Executar a partir da raiz da worktree, com `TMPDIR` apontando para um diretório temporário criado dentro dela. Os comandos abaixo são previstos para a implementação; não foram executados no planejamento.

```bash
cargo test --locked --manifest-path src-tauri/Cargo.toml -p mt-agents --test skills_sync
cargo test --locked --manifest-path src-tauri/Cargo.toml -p mt-agents
cargo clippy --locked --manifest-path src-tauri/Cargo.toml -p mt-agents --all-targets -- -D warnings
git diff --check
```

**Prova conclusiva do relato:** o teste `remove_target_preserves_replaced_symlink` deve instalar pelo serviço real, substituir o link, chamar `remove_target(id, true)` e comprovar simultaneamente link personalizado preservado, destino inalterado, sentinela intacto, contador zero e cadastro removido. Pode ser executado isoladamente, quando necessário, com:

```bash
cargo test --locked --manifest-path src-tauri/Cargo.toml -p mt-agents --test skills_sync remove_target_preserves_replaced_symlink -- --exact
```

Esse teste distingue o código atual da correção: atualmente a asserção de existência do link falharia. A suíte completa acrescenta a prova de que links próprios continuam removíveis e os demais contratos permanecem íntegros.

## Skills Aplicáveis

- `security-and-hardening`: orientar a conferência de propriedade de caminhos e o tratamento conservador de entradas externas antes da exclusão no filesystem.
- `code-review`: revisar o diff da implementação e dos testes, a preservação dos contratos e o escopo cirúrgico antes da integração.

Aplicar apenas skills disponíveis e pertinentes à implementação Rust e à revisão. As skills Vue não são necessárias, pois não há mudança de UI ou reatividade. A sequência de implementação e validação deve respeitar o bloco completo e a validação em lote definidos neste plano.
