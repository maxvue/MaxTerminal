# Plano de implementação — Issue #3

### Descrição e Causa Raiz

**Veracidade: bug confirmado por inspeção estática do fluxo completo**, na revisão `d4317e5279c4eda34514dc3183039c3503a4a1b4`. A implementação atual permite que um registro criado para `A/skill` autorize atualização ou remoção de `B/skill`. O hash comprova igualdade de conteúdo, não propriedade sobre o caminho. Não foi executada reprodução dinâmica nesta etapa; as evidências abaixo são do código atual. Este documento é o único arquivo alterado no planejamento.

**Causa raiz comprovada e fluxo causal:**

1. `src-tauri/crates/mt-agents/src/skills/sync.rs:L233-255`: `upsert_target` atualiza `dest_canonical` e `dest_override` mantendo o mesmo ID, sem modificar os registros de `skill_sync_entries`. A invalidação de planos em `L282-287` impede reutilizar o plano antigo, mas não elimina o erro em planos novos.
2. `src-tauri/crates/mt-agents/src/skills/sync.rs:L817-849`: `load_managed_entries` consulta exclusivamente por `target_id`, carrega `dest_path` e indexa os registros pelo nome da skill. `plan`, em `L342-357`, combina esses registros antigos com o novo destino resolvido.
3. `src-tauri/crates/mt-agents/src/skills/conflict.rs:L64-68` e `L128-159`: a classificação recupera o registro pelo nome e usa `synced_hash` sem comparar `ManagedEntryRecord.dest_path` com o caminho avaliado. Se `hash(B/skill) == hash_sincronizado_em_A` e a fonte mudou, a ação é `Update`, mesmo sem manifesto em B.
4. `src-tauri/crates/mt-agents/src/skills/sync.rs:L413-435`: `apply` repete o mesmo classificador com os registros em cache. O fingerprint permanece válido porque o erro de identidade é repetido, não porque B tenha sido legitimamente gerenciado.
5. `src-tauri/crates/mt-agents/src/skills/sync.rs:L457-485` e `L693-725`: `Update` chama a instalação diretamente; `remove_existing_path(final_dest)` remove B antes do rename. Esse ramo não faz backup. O backup exigido para sobrescrita de conflitos existe em `L513-550`, mas não é alcançado.
6. `src-tauri/crates/mt-agents/src/skills/sync.rs:L874-883`: após a instalação, o upsert de `(target_id, skill_name)` troca `dest_path` de A para B. Isso ocorre depois da perda do conteúdo anterior de B.

**Rastreamento reverso dos dados, incluindo a apresentação:**

- Disco/DB → serviço: o esquema `src-tauri/crates/mt-db/migrations/0008_13_agents_skills_memory.sql:L71-100` distingue o destino do alvo e o `dest_path` de cada entrada. A chave primária da entrada é `(target_id, skill_name)`; portanto, cabe ao classificador verificar o caminho antes de usar esse registro como prova de gerenciamento.
- Serviço → IPC: `src-tauri/src/commands/skills.rs:L34-42` e `L59-79` expõem upsert, planejamento e aplicação. O transporte é IPC Tauri, sem controller ou rota HTTP neste fluxo.
- IPC → store: `src/ipc/skills.ts:L26-49` encaminha as chamadas; `src/stores/skills.ts:L166-187` aceita um alvo com ID existente e `L215-251` armazena planos/resultados sem reclassificar as ações.
- Store → UI: `src/components/skills/SkillsSyncPanel.vue:L43-55`, `L164-168` e `L222-243` apresenta somente ações `Conflict` como conflitos e envia as resoluções ao backend. Um `Update` incorreto não oferece a proteção de conflito/backup ao usuário.
- Limite da reprodução via interface: o formulário atual cria alvos com `id: null` (`SkillsSyncPanel.vue:L201-215`) e só mostra o override para `agy` (`L23`). A alteração do mesmo ID está comprovadamente disponível no serviço/IPC/store; não se afirma existir hoje um botão de edição desse destino na tela. A reprodução deve usar `SkillsService::upsert_target` com o ID original.

**Agravantes comprovados:**

- O ramo de symlink também aceita o registro apenas pelo nome (`conflict.rs:L78-118`), permitindo tratar um link independente em B como gerenciado.
- O ramo de poda constrói o caminho a partir do novo `dest_dir` e compara o hash antigo, sem verificar `dest_path` (`conflict.rs:L186-225`). Uma skill retirada da seleção pode causar remoção indevida em B; para diretórios esse ramo faz backup, mas a remoção continua indevida.
- O auto-sync aceita `Update` e `Prune` e chama `apply` sem resoluções (`sync.rs:L614-644`), ampliando o impacto para execução automática.
- O fallback de manifesto em `conflict.rs:L129-136` não pode anular uma divergência de caminho comprovada pelo registro. Apenas filtrar o registro e cair nesse fallback deixaria escapar uma cópia de B contendo o manifesto de A. O hash exclui o manifesto (`src-tauri/crates/mt-agents/src/skills/catalog.rs:L31-35` e `L88-95`).

### Arquivos afetados

Na implementação:

1. `src-tauri/crates/mt-agents/src/skills/conflict.rs` — vincular a confiança no registro ao caminho da entrada, incluindo cópias, symlinks, manifesto e poda.
2. `src-tauri/crates/mt-agents/tests/skills_sync.rs` — regressões de classificação e integração com SQLite, aplicação, backup e auto-sync, reutilizando `setup_test_env`.

Artefato desta etapa: `docs/issues/3/plan.md`. Os arquivos de serviço, resolução de caminhos, UI, store, IPC e migration citados são referências de investigação; os contratos existentes permitem implementar a correção nos dois arquivos acima.

### Execuções propostas

1. Trabalhar na worktree dedicada. Implementar integralmente o bloco abaixo e seus testes antes da primeira execução de testes, verificações de tipos ou build; depois corrigir eventuais falhas em lote e validar novamente.
2. Em `conflict.rs`, introduzir uma função privada para comparar a identidade do caminho instalado: comparar o nome final e o diretório pai normalizado com `canonical_or_lexical_path`. Não canonicalizar o componente final da skill: dois symlinks distintos em A e B podem apontar para a mesma fonte e continuar sendo entradas independentes. Rejeitar correspondência quando não houver pai/nome válidos. Reutilizar o helper de normalização existente sem alterar sua política global.
3. Na seleção de cada skill, distinguir três situações: registro ausente; registro do mesmo caminho; registro de caminho divergente. Só o segundo autoriza reutilizar `synced_hash`, `mode` e demais evidências do registro como propriedade da entrada atual.
4. Manter `Create` quando a entrada de destino não existir. Quando existir um diretório com registro divergente, classificá-lo como `Conflict::Unmanaged` antes de consultar o manifesto ou comparar hashes. Aplicar isso mesmo quando B tenha conteúdo idêntico à fonte atual, ao hash antigo ou um manifesto copiado de A. O fallback de manifesto existente permanece para casos sem registro; a divergência explícita de caminho não pode ser apagada pelo fallback.
5. Para symlinks, usar apenas registros do mesmo caminho. Um link em B sem registro correspondente deve seguir o comportamento existente de `Conflict::ForeignSymlink`, inclusive se apontar para a fonte correta; a resolução `Adopt` já cobre essa situação.
6. No laço de poda, ignorar registros cujo caminho não corresponda à entrada no destino atual, antes de inspecionar conteúdo ou emitir ações. Não planejar remoção de A durante o planejamento de B, nem conflito/poda de B com base exclusivamente no registro de A.
7. Conservar os registros antigos no DB até uma instalação/adoção legítima no destino atual. Não apagar diretórios antigos nem realizar limpeza automática na mudança de destino. Isso evita perder a evidência da divergência e protege também bases que já possuem registros antigos. O upsert existente após `Create`, `Overwrite` ou `Adopt` registra B; `Keep` mantém o registro anterior.
8. Reutilizar as proteções existentes: conflito sem resolução resulta em `Keep`; `Overwrite` exige backup bem-sucedido antes da instalação; `Adopt` exige identidade com a fonte; alteração de alvo invalida planos antigos. A correção central do classificador deve atender tanto `plan` quanto a revalidação em `apply` e o auto-sync, sem novos DTOs ou modos de resolução.

### Testes de regressão e validação em lote

Adicionar testes com nomes estáveis prefixados por `destination_change_` em `skills_sync.rs`. Usar diretórios temporários e DB isolado, sem tocar em destinos reais. A suíte atual (`L156-259`) cobre conflito por alteração local e backup, mas não troca de destino mantendo o ID.

| Caso | Preparação e asserções obrigatórias |
| --- | --- |
| `destination_change_copy_requires_conflict` | Sincronizar apenas uma skill em A/Copy; criar B/skill com os mesmos bytes, sem manifesto; atualizar a fonte e salvar o mesmo alvo com override B. Confirmar ID preservado, registro ainda apontando para A e plano com `Conflict::Unmanaged` em B, nunca `Update`/`Unchanged`. |
| `destination_change_keep_preserves_both_paths` | Aplicar esse plano com mapa vazio e, em fixture independente, com `Keep` explícito. Comparar os bytes e hashes de A e B antes/depois; zero aplicações para a skill, zero backups e registro de A preservado. |
| `destination_change_overwrite_backs_up_before_install` | Replanejar e resolver `Overwrite`. Exigir ausência de erros, backup existente contendo os bytes antigos de B, B contendo a fonte nova, A intacto e `dest_path` persistido em B. Em fixture separada, impedir a criação do diretório de backups colocando um arquivo em seu caminho; exigir erro e B intacto. |
| `destination_change_copied_manifest_does_not_grant_ownership` | Copiar também o manifesto de A para B. Mesmo com manifesto parseável e hash correspondente, exigir conflito por caminho divergente e preservação com mapa vazio. |
| `destination_change_symlink_requires_adoption` | Criar links distintos em A e B apontando para a mesma fonte. Exigir conflito em B; aplicar `Adopt`, conferir o registro em B e verificar que o link A permanece intacto. |
| `destination_change_does_not_prune_unmanaged_entries` | Após sincronizar A, colocar cópia idêntica em B e retirar a skill da seleção ao alterar o destino. Exigir ausência de ação para essa skill, preservação de A/B e registro antigo intacto. Repetir com symlink em B. |
| `destination_change_auto_sync_preserves_conflicts` | Habilitar auto-sync e mudar para B com skill conflitante e outra skill selecionada ainda inexistente. A segunda força `apply` no auto-sync: ela deve ser criada enquanto a primeira permanece intacta e não é atualizada/podada. |
| `destination_change_empty_destination_creates` | Mudar de A para B inexistente. Exigir `Create`, instalação em B e atualização legítima do registro, sem modificar A. |
| `destination_change_adopt_requires_current_source` | B idêntico à fonte atual continua conflitante até `Adopt`; após adoção, plano seguinte é `Unchanged`. Se B só corresponder ao hash antigo, adoção deve retornar erro e preservar conteúdo/registro. |
| `destination_change_invalidates_previous_plan` | Planejar em A e salvar o alvo com destino B. Aplicar o plano de A deve retornar `skills.plan_stale` sem modificar A/B. |

Adicionar também controles de não regressão: mesmo destino continua produzindo `Unchanged`, `Update`, `LocallyModified` e `Prune` conforme conteúdo/seleção; mudança Copy↔Symlink no mesmo caminho continua funcionando; aliases do diretório pai e segmentos `.` equivalentes não geram conflito falso; dois links finais distintos para a mesma fonte nunca são confundidos. Preservar a recuperação por manifesto quando não existe registro e cobrir o registro divergente cujo caminho antigo já não existe.

Implementar todo esse bloco antes de validar. Na primeira rodada executar a suíte focada e as verificações abaixo, registrar todas as falhas, corrigi-las em conjunto e repetir os comandos afetados. Não executar build/frontend para uma alteração exclusivamente interna em Rust sem mudança de bindings.

### Banco de dados

**Nenhuma migration.** `skill_sync_entries.dest_path` já contém a evidência necessária. Manter esquema, chave primária, FK e configuração WAL existentes. Não executar limpeza ou atualização em massa de registros antigos: o novo classificador precisa lidar com eles em leitura. Os testes devem consultar o registro antes/depois de `Keep`, falha de backup, `Overwrite`, `Create` e `Adopt` para comprovar os efeitos esperados.

### Riscos de quebra e Não-Regressão

- **Identidade de symlink:** canonicalizar a folha confundiria A e B pela fonte comum. Cobrir links finais distintos e aliases apenas do pai.
- **Manifesto como recuperação:** remover indiscriminadamente o fallback mudaria comportamento existente. Preservar o caso sem registro e bloquear expressamente o caso com registro divergente, com testes separados.
- **Poda e dados antigos:** filtrar somente as skills selecionadas deixaria a remoção indevida ativa. Aplicar a mesma verificação no laço de poda e comprovar preservação física e no DB.
- **Backup:** exigir bytes anteriores no backup, não apenas um caminho retornado. Falha de backup deve impedir instalação e troca de registro. O manifesto é excluído pelo mecanismo de cópia atual; validar a árvore de conteúdo da skill sem prometer recuperação desse metadado.
- **Compatibilidade:** DTOs, nomes IPC, enums de conflito e schema permanecem iguais. O aumento de conflitos após mudança real de destino é a mudança de comportamento pretendida e utiliza a UI de resolução já existente.
- **Execução automática e planos:** testar conflito junto de uma ação válida, além de invalidação de planos, para evitar sucesso aparente causado por auto-sync que simplesmente não executou nada.
- **Limite do escopo:** preservar diretórios antigos não implementa migração física de instalações nem resolve corridas gerais entre processos externos e o filesystem. O objetivo verificável é impedir que evidência de A autorize mutação de B.

### Validação

Executar a partir da raiz da worktree, após implementar código e testes:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p mt-agents --test skills_sync destination_change_
cargo test --manifest-path src-tauri/Cargo.toml -p mt-agents
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-agents --all-targets -- -D warnings
git diff --check
```

A prova conclusiva é a combinação: plano de B contém conflito, aplicação sem resolução mantém os bytes de B e A, sobrescrita explícita preserva os bytes antigos de B em backup, e poda/auto-sync não contornam essa regra. Exigir que o comando focado execute os testes adicionados (não aceitar zero testes), que todas as asserções passem e que o diff final se restrinja aos arquivos planejados. Nesta etapa foram realizadas apenas inspeções de código e elaboração do plano; os comandos Rust acima são validações futuras, não resultados já obtidos.

### Skills Aplicáveis

- `git-worktree`: isolamento da implementação na worktree dedicada e integração/limpeza conforme autorização.
- `security-and-hardening`: revisão da confiança em metadados de gerenciamento, caminhos e operações de filesystem sobre dados locais.
- `code-review`: revisão final da vinculação entre caminho e registro, preservação de dados e cobertura antes de integrar a correção.

Seguir `AGENTS.md`, comentários em pt-BR e identificadores em inglês. O ciclo de validação deve respeitar a exigência desta tarefa: concluir o bloco autorizado de implementação e testes antes de executar testes, tipos ou build, corrigindo falhas em lote.
