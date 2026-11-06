# Issue #1 — Impedir remoções causadas por leitura incompleta do catálogo

### Descrição e Causa Raiz

**Veracidade: confirmada por evidência estática no código atual.** O tratamento que causa o problema continua presente. Esta etapa não executou sincronização destrutiva, não reproduziu a falha em runtime e não modificou código-fonte. A comprovação é o encadeamento explícito entre erro de leitura, catálogo retornado como sucesso, classificação `Prune` e remoção física/persistida.

Uma fonte existente, sem `index.json`, pode conter `grupo/skill/SKILL.md`. Se a listagem de `grupo` falhar por permissão ou I/O, a descoberta termina esse ramo com sucesso. As skills do ramo desaparecem do catálogo retornado, embora permaneçam na fonte. Para um alvo habilitado, em modo `Copy`, com `auto_sync = true` e seleção que anteriormente incluía essas skills, a próxima sincronização remove as cópias gerenciadas intactas e seus registros. O backup permite recuperação, mas não mantém as skills instaladas e disponíveis aos agentes.

**Causa Raiz Comprovada:**

1. `src-tauri/crates/mt-agents/src/skills/catalog.rs:L337-367`: `collect_skill_md_dirs` converte falha de `read_dir` em `Ok(())` nas linhas 345–347; `rd.flatten()` descarta erros de entradas na linha 348; falha de `symlink_metadata` vira `continue` nas linhas 360–362. São três pontos de omissão silenciosa na descoberta.
2. `src-tauri/crates/mt-agents/src/skills/catalog.rs:L281-294` e `L328-334`: o chamador recebe sucesso e publica um `SkillCatalog` parcial ou vazio. A leitura de `SKILL.md` também usa `unwrap_or_default`, podendo perder nome/relevância do frontmatter. Falhas permanentes de leitura de uma skill já descoberta geralmente são barradas pelo hash (`L309`, `L173-175`); isso não protege subárvores que sequer foram descobertas nem uma falha transitória anterior ao hash.
3. `src-tauri/crates/mt-agents/src/skills/sync.rs:L352-390`: `plan` considera o catálogo bem-sucedido como base válida, carrega o estado gerenciado, seleciona skills, classifica ações e armazena o plano. Seleção vazia produz somente um aviso (`L359-362`).
4. `src-tauri/crates/mt-agents/src/skills/conflict.rs:L186-225`: uma entrada gerenciada ausente da seleção é candidata a `Prune`; no modo `Copy`, basta que seu hash local corresponda ao último hash sincronizado. Não há distinção entre remoção real e omissão por falha de leitura.
5. `src-tauri/crates/mt-agents/src/skills/sync.rs:L614-645`: `run_auto_sync_all` aceita `Prune` como trabalho automático e chama `apply` com resoluções vazias. Não é necessário conflito nem aprovação específica para essa ação.
6. `src-tauri/crates/mt-agents/src/skills/sync.rs:L413-435`: a revalidação usa as skills já presentes no plano em cache; não redescobre as omitidas. Portanto, o fingerprint não corrige a classificação originada de uma descoberta incompleta.
7. `src-tauri/crates/mt-agents/src/skills/sync.rs:L486-511` e `L902-914`: `apply` faz backup da cópia, executa `remove_dir_all` e apaga a linha de `skill_sync_entries`. `L590-602` ainda atualiza `last_synced_at` se não houver erros.

**Rastreamento de dados e fluxo reverso:**

- UI: `src/components/skills/SkillsSyncPanel.vue:L222-246` → `planSync`/`applySync` em `src/stores/skills.ts:L215-255` → `src/ipc/skills.ts:L34-49` → comandos Tauri `skills_plan`/`skills_apply` em `src-tauri/src/commands/skills.rs:L59-79` → `SkillsService::plan/apply` → filesystem e SQLite.
- Caminho automático: eventos de filesystem → `src-tauri/crates/mt-agents/src/skills/watch.rs:L136-149` → invalidação de planos → `run_auto_sync_all` → o mesmo serviço. Esse caminho não depende da UI/store e não passa por uma rota HTTP.
- DB → decisão: `skill_sync_targets` fornece `mode`, `enabled`, `auto_sync` e seleção (`sync.rs:L112-155`); `skill_sync_entries` fornece nomes, caminhos e hashes anteriormente sincronizados (`sync.rs:L817-852`). Fonte → catálogo → seleção é comparada a esses registros. A omissão da fonte termina em exclusão da cópia e do registro, e o resultado volta por IPC/eventos.
- No fluxo manual, erros já são registrados e relançados pela store (`src/stores/skills.ts:L226-228`); no automático, erro de `plan` já impede `apply` (`sync.rs:L627-629`). Logo, propagar corretamente o erro na descoberta interrompe o fluxo antes da remoção.

**Cenário de regressão a automatizar:** instalar pelo serviço duas skills, uma em pasta intermediária e outra em ramo legível; retirar temporariamente a leitura da pasta intermediária, mantendo-a existente; confirmar `PermissionDenied` no mesmo processo; executar `scan`, `plan` e `run_auto_sync_all`. Atualmente, a skill omitida pode gerar `Prune`. Após a correção, `scan`/`plan` devem falhar e a tentativa automática deve preservar integralmente destino e registros. Repetir com todas as skills sob a pasta bloqueada para cobrir catálogo falsamente vazio.

### Arquivos afetados

Arquivos a editar na implementação:

1. `src-tauri/crates/mt-agents/src/skills/catalog.rs`: descoberta estrita, propagação contextual de erros e testes unitários internos para falhas difíceis de provocar pelo filesystem real.
2. `src-tauri/crates/mt-agents/tests/skills_sync.rs`: regressões de leitura, preservação do destino/SQLite no auto-sync, recuperação e controles de remoção legítima.

Artefato desta etapa: `docs/issues/1/plan.md`.

### Execuções propostas

1. **Estabelecer a invariável:** `scan_catalog` só retorna `Ok` quando todas as leituras exigidas pelo caminho escolhido de descoberta terminarem com sucesso. Catálogo vazio é válido somente quando efetivamente observado, dentro das regras atuais de descoberta; erro não é ausência.
2. **Corrigir a coleta recursiva:** substituir o `let Ok(read_dir) ... else { Ok(()) }`, o `flatten()` e o descarte de erro de metadados por iteração explícita e `map_err(...)?`, usando `error::skills_io`. Incluir operação, caminho do diretório/entrada quando disponível e erro original, com mensagem em pt-BR. Uma falha após já coletar skills deve abortar o catálogo inteiro, sem publicar o vetor parcial.
3. **Separar ausência de falha nas sondagens:** revisar os usos de `exists`/`is_dir`/`is_file` em `scan_catalog` e `collect_skill_md_dirs` (`catalog.rs:L215-246`, `L341`). Consultar metadados com resultado explícito. `NotFound` de um marcador opcional (`index.json` ou `SKILL.md` de uma pasta intermediária) permite a descoberta normal; `PermissionDenied` e outros erros de I/O devem abortar. Se uma entrada já enumerada desaparecer antes da inspeção, tratar a varredura como instável e abortar essa tentativa. Manter `skills.source_missing` para raiz realmente inexistente ou que não seja diretório; preservar o erro de I/O quando a resolução da raiz falhar por outro motivo.
4. **Tornar a leitura do frontmatter estrita:** substituir `read_to_string(...).unwrap_or_default()` por propagação de `skills.io`. Se um `SKILL.md` já identificado desaparecer ou não puder ser lido, abortar, em vez de inventar conteúdo vazio e alterar nome/relevância. Conteúdo legível sem frontmatter continua válido e usa os fallbacks atuais.
5. **Tratar o índice coerentemente:** separar aquisição do arquivo de sua validação semântica. Índice ausente continua permitindo fallback; índice legível com JSON/schema/versão inválidos continua seguindo a política atual de fallback. Falha de leitura/metadados/canonicalização por I/O não deve parecer índice inválido. Caminho ausente de item indexado pode invalidar o índice e iniciar fallback, mas o fallback também deve obedecer à propagação estrita. O hash já propaga erros e deve continuar assim.
6. **Preservar as regras de descoberta:** manter exclusões de diretórios, limite de profundidade de descoberta, não travessia de symlinks de diretórios e limites do hash. A completude exigida é relativa a essa política, não a todas as pastas do repositório. Evitar que sondagens novas passem a seguir diretórios simbólicos antes ignorados. Não confundir uma exclusão intencional com erro de leitura.
7. **Usar o bloqueio existente no serviço:** o `?` de `plan` deve impedir a criação/cache de um novo plano; o auto-sync deve pular o alvo quando o planejamento falhar. Verificar esse comportamento por testes com DB e filesystem reais. Uma checagem apenas de `catalog.skills.is_empty()` seria insuficiente, pois a falha pode omitir somente parte das skills e fontes realmente vazias são legítimas.
8. **Criar o bloco completo de testes abaixo**, reutilizando `setup_test_env`, `MockWorkspacePort` e DB temporário existentes. Se necessário, extrair auxiliares privados mínimos dentro de `catalog.rs` para testar consumo de entradas e tratamento de erros de forma determinística; evitar criar uma abstração pública de filesystem só para testes.
9. **Somente após concluir implementação e testes**, executar a validação em lote. Agrupar as falhas encontradas, corrigi-las em conjunto e repetir os comandos afetados. Revisar o diff e os contratos antes de integrar mediante autorização.

### Testes de regressão e validação em lote

Os nomes abaixo são propostos, não testes já existentes ou executados nesta análise.

| Teste / grupo | Preparação e prova exigida |
| --- | --- |
| `catalog_read_failure_rejects_partial_and_empty_scan` | Fonte sem índice, skill aninhada sob pasta sem leitura, com e sem uma segunda skill legível. Exigir `codes::SKILLS_IO` e caminho contextual; nunca aceitar `Ok` parcial/vazio. |
| `catalog_read_failure_propagates_entry_and_metadata_errors` | Testes internos determinísticos: sucesso seguido de erro de enumeração, erro de metadados e entrada desaparecida após enumeração. Exigir erro global, inclusive se o acumulador já tiver entradas. |
| `catalog_read_failure_rejects_unreadable_markers_and_index` | Índice sem leitura, erro ao consultar marcador e `SKILL.md` sem leitura. Incluir bytes UTF-8 inválidos em `SKILL.md` cujo nome de frontmatter difira da pasta: `read_to_string` deve falhar em vez de derivar outra identidade. |
| `catalog_fallback_preserves_valid_discovery_rules` | Índice válido; índice ausente; índice legível com JSON/versão inválidos; markdown sem frontmatter; diretórios intermediários sem marcador; raiz genuinamente vazia. Conferir nomes, relevância, hashes, ordenação e `from_index`. |
| `catalog_read_failure_auto_sync_preserves_copy_and_database` | Instalar duas cópias intactas via `plan`/`apply`, com alvo habilitado e auto-sync. Guardar bytes, hashes, manifesto, linhas completas de `skill_sync_entries`, `last_synced_at` e lista de backups. Bloquear ramo intermediário. `plan` deve falhar; `run_auto_sync_all` não deve produzir aplicação para esse alvo. Comparar os snapshots, incluindo a skill do ramo legível: nenhuma mutação nessa tentativa. Repetir com seleção `All`, `Recommended` e `Explicit` que incluía a skill. |
| `catalog_read_failure_recovery_keeps_installed_skills` | Restaurar permissões com garantia de limpeza mesmo se uma asserção falhar. Confirmar fonte existente, catálogo completo, novo plano `Unchanged` e auto-sync sem remoções nem backups extras. |
| `catalog_complete_scan_still_prunes_removed_skills` | Controle positivo: remover de verdade a skill da fonte em fixture legível; executar scan completo, planejar e aplicar. Deve haver `Prune`, backup íntegro em `Copy` e exclusão do registro correspondente. Cobrir fonte completamente vazia e mudança intencional de seleção. |
| `catalog_read_failure_preserves_symlinks_and_conflicts` | Verificar preservação também em `Symlink`; com fonte legível, confirmar que cópia local modificada permanece `Conflict` e não é podada automaticamente, e que arquivos não gerenciados permanecem intactos. |

**Confiabilidade dos testes:**

- Rodar os cenários de permissão como usuário Linux sem privilégios de bypass. Salvar permissões originais, restringir somente fixtures e restaurá-las por guarda RAII antes da limpeza. Usar uma pasta intermediária que não contenha `SKILL.md`, para atingir exatamente a falha de listagem reportada.
- Antes de validar o comportamento do serviço, exigir que `std::fs::read_dir` da fixture retorne `ErrorKind::PermissionDenied`. Se o ambiente ignorar o bloqueio (por exemplo, root), o caso não é prova válida: executar em ambiente não privilegiado e registrar a limitação, sem produzir aprovação silenciosa.
- Cobrir erros de enumeração/metadados com injeção privada determinística quando não houver reprodução estável por permissões. Não usar concorrência com remoção de arquivos nem temporizações para tentar acertar uma corrida.
- Testar a rotina automática chamando `run_auto_sync_all` diretamente; isso verifica o caminho chamado pelo watcher sem depender do debounce ou da entrega de eventos do sistema. Manter o teste de watcher já existente na suíte completa.
- Construir fixtures e artefatos dentro da worktree durante a execução; usar `TMPDIR` apontando para uma pasta ignorada `.tmp` da própria worktree.

### Banco de dados

**Nenhuma** migration. Reutilizar `skill_sync_targets` e `skill_sync_entries`. Na falha de leitura, todas as linhas gerenciadas e `last_synced_at` devem permanecer idênticos. Após remoção legítima, manter a exclusão de registro já implementada.

### Riscos de quebra e Não-Regressão

- **Severidade do defeito: alta, com recuperação disponível.** Há remoção automática de cópias instaladas e registros; o backup reduz o dano, mas não evita a interrupção. O fluxo comprovado não indica exclusão da fonte nem perda irreversível das cópias protegidas por backup.
- **Maior rigor pode suspender todo um alvo por uma pasta ilegível:** esse é o comportamento necessário para não usar uma observação incompleta como autorização de remoção. Validar recuperação após restaurar acesso e ausência de efeitos no ramo ainda legível.
- **Compatibilidade do fallback:** distinguir índice inválido/ausente de erro de acesso. Testes devem preservar os fallbacks semânticos existentes e impedir regressão para catálogo vazio em falha de I/O.
- **Remoções legítimas:** não desabilitar `Prune` globalmente. Testar ausência real, fonte vazia legível e deseleção intencional.
- **Identidade e relevância:** erro de leitura de frontmatter não pode renomear skill pelo nome da pasta nem removê-la da seleção `Recommended`. Conferir os três tipos de seleção e markdown sem frontmatter.
- **Contratos IPC:** conservar `MtResult<SkillCatalog>`, DTOs, comandos e códigos de erro já existentes. A store já recebe erros; os testes focam a propagação do backend. O retorno automático atual continua omitindo resultados quando o planejamento falha; não alegar que uma nova notificação de erro será emitida.
- **Symlinks, limites e exclusões:** manter as políticas existentes e seus testes, sem expandir o escopo de travessia ao tornar consultas falíveis.
- **Janela entre plano e aplicação:** a correção impede novos planos baseados em leituras falhas. A revalidação atual usa o catálogo em cache e não constitui snapshot transacional da fonte; reformular esse contrato é uma questão separada. Manter a cobertura existente de `skills.plan_stale` e não apresentar esta correção como eliminação de todas as corridas de filesystem.

### Validação

Executar a partir da worktree dedicada, após todo o bloco autorizado de implementação e testes estar concluído:

```bash
mkdir -p .tmp
export TMPDIR="$PWD/.tmp"
cargo test --manifest-path src-tauri/Cargo.toml -p mt-agents catalog_read_failure
cargo test --manifest-path src-tauri/Cargo.toml -p mt-agents --test skills_sync
cargo test --manifest-path src-tauri/Cargo.toml -p mt-agents
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-agents --all-targets -- -D warnings
git diff --check
```

**Prova conclusiva de aceite:** em ambiente que comprovadamente negue a leitura da fixture, o teste `catalog_read_failure_auto_sync_preserves_copy_and_database` deve observar erro em `plan` e igualdade integral dos snapshots de destino/manifestos/DB/backups depois do auto-sync; após restaurar acesso, a skill continua descoberta e instalada. O controle `catalog_complete_scan_still_prunes_removed_skills` deve demonstrar que a remoção legítima permanece funcional. Verificar que o filtro do primeiro comando executou casos efetivos, não zero testes. Falhas preexistentes de infraestrutura ou lint devem ser registradas com seus logs, sem declarar aprovação integral.

Nesta etapa de planejamento, esses comandos não foram executados. A validação realizada foi leitura do código e rastreamento causal; a execução das regressões pertence à implementação.

### Skills Aplicáveis

- `security-and-hardening`: tratamento de falhas de entrada/filesystem antes de ações destrutivas e proteção de dados persistidos.
- `code-review`: revisão final da propagação de erros, invariantes de remoção e cobertura de regressão antes de integração.
- `linux-troubleshooting`: aplicável ao reproduzir a negação de acesso e verificar permissões efetivas do processo Linux.

Seguir também o `AGENTS.md` canônico: trabalho isolado em worktree, comunicação/mensagens em pt-BR e autorização explícita antes de commit/merge.
