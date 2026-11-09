# Plano de implementação — Issue #4

## Descrição e Causa Raiz

**Veracidade: confirmada por inspeção estática do código atual.** Um arquivo regular `arquivo-importado.db` dentro de `backup_dir` é listado como `daily` e pode ser removido quando ultrapassa a retenção. Não houve execução destrutiva nem reprodução dinâmica nesta etapa; a evidência é o caminho determinístico até `fs::remove_file`. O plano é o único arquivo alterado, na worktree dedicada `wt-implement-issue-4`.

### Causa Raiz Comprovada

- `src-tauri/crates/mt-db/src/maintenance/backup.rs:L212-247`: a listagem aceita arquivos regulares não ocultos com extensão `.db`, inclusive `.DB`, sem exigir identidade de backup da aplicação. Links simbólicos e diretórios já são ignorados.
- `src-tauri/crates/mt-db/src/maintenance/backup.rs:L258-271`: `classify_backup_filename` usa substrings para categorias especiais e retorna `daily` para qualquer nome restante. Logo, `arquivo-importado.db` é classificado como diário. Um nome externo contendo `-premigration-` também pode entrar na retenção pré-migração.
- `src-tauri/crates/mt-db/src/maintenance/backup.rs:L250-254,L274-297`: a listagem é ordenada por `mtime` decrescente e nome como desempate; a rotação conta os itens classificados como diários ou pré-migração e remove os excedentes. Não há conferência independente do nome antes da exclusão.
- `src-tauri/crates/mt-db/src/maintenance/backup.rs:L89-95,L181-191`: backups reais recebem `maxterminal-{YYYYMMDDTHHMMSSZ}-{seq:03}-{tag}.db`, e toda criação concluída chama a rotação, independentemente da categoria criada.

**Cenário derivado do código:** um importado com `mtime` antigo e dois backups diários mais recentes, com retenção 1, entra na contagem diária e é apagado ao rotacionar. Se o importado for recente, pode ocupar a vaga de um backup legítimo. Além disso, pode adiar a criação automática diária: `src-tauri/crates/mt-db/src/maintenance/scheduler.rs:L178-185` usa a mesma classificação e considera suficiente um suposto diário modificado nas últimas 20 horas.

### Fluxo causal e rastreamento reverso

1. **Filesystem ← serviço de backup:** `remove_file` em `backup.rs:L285,L291` recebe o caminho de `list_backups`; a classificação incorreta transforma um arquivo externo em candidato à retenção. A exclusão ocorre no filesystem, não por SQL.
2. **Serviço ← configuração persistida:** `MaintenanceHandle::create_backup`, em `src-tauri/crates/mt-db/src/maintenance/scheduler.rs:L46-65`, lê `db.retention.backup_daily_keep` e chama `create_backup_blocking`. A configuração é lida da tabela `settings` em `src-tauri/crates/mt-db/src/settings/repo.rs:L81,L117`; seu valor padrão é 7 e a faixa permitida é 1–60 em `src-tauri/crates/mt-db/src/settings/registry.rs:L80-96`.
3. **Serviço ← execução automática:** `scheduler.rs:L178-185,L205-221` executa manutenção e gera diários; `src-tauri/crates/mt-db/src/migrations/runner.rs:L159-171` gera pré-migração. A restauração agendada cria `PreRestore` em `backup.rs:L680-687`, também acionando rotação antes de copiar o candidato em `backup.rs:L723-728`.
4. **UI ⇄ estado local ⇄ IPC ⇄ serviço:** `src/components/preferences/system/PreferencesSystemSection.vue:L54-71,L104-106,L240-258` mantém backups em `ref`, lista e cria backups via `src/ipc/backup.ts:L5-17`; não existe store intermediária de backups neste fluxo. Os comandos Tauri em `src-tauri/src/commands/backup.rs:L15-34` chamam diretamente o serviço. O botão manual usa retenção fixa 7, portanto o cenário com retenção 1 deve exercitar `MaintenanceHandle`, não esse botão.
5. **Retorno à UI:** `BackupEntryDto.kind` é `String` em `src-tauri/crates/mt-db/src/models.rs:L32-45` e `string` em `src/types/bindings.ts:L2426-2436`. A UI exibe nome e tamanho, permitindo restauração pelo nome. A validação de restauração aceita nomes externos seguros e valida o conteúdo SQLite em `backup.rs:L339-381,L517-571`; essa capacidade deve ser preservada.

## Arquivos afetados

| Arquivo | Alteração prevista na implementação |
| --- | --- |
| `src-tauri/crates/mt-db/src/maintenance/backup.rs` | Reconhecimento estrito dos nomes gerenciados, categoria desconhecida na listagem, elegibilidade explícita para rotação e testes unitários do reconhecimento. |
| `src-tauri/crates/mt-db/src/models.rs` | Documentar `unknown` em `BackupEntryDto.kind` e alinhar o exemplo de nome ao formato efetivamente gerado. |
| `src-tauri/crates/mt-db/tests/backup.rs` | Regressões de preservação, retenção, listagem e restauração de importados. |
| `src-tauri/crates/mt-db/tests/retention.rs` | Regressão garantindo que arquivo externo recente não adie o backup diário. |
| `src/types/bindings.ts` | Atualização gerada pelo Specta dos comentários do DTO; manter `kind: string`. |

O artefato desta etapa é `docs/issues/4/plan.md`. Os demais arquivos citados são evidências ou suítes existentes de validação.

## Execuções propostas

1. Implementar um reconhecedor privado que retorne `Option<BackupKind>`, compartilhado pela classificação e pela elegibilidade da rotação. Reconhecer o nome completo, com prefixo `maxterminal-`, timestamp UTC compacto válido, sequência decimal e tag exata antes de `.db`; remover classificações por `contains`.
2. Aceitar as tags emitidas por `BackupKind::file_tag`: `daily`, `manual`, `premigration-v{from_version}-to-v{to_version}`, `pre_reset`, `pre_restore` e `corrupted`. Exigir versões inteiras válidas para pré-migração. Respeitar o formato da sequência: preenchimento mínimo de três dígitos, sem limitar a três, pois o contador pode ultrapassar 999. Validar a estrutura e os componentes do timestamp conforme `src-tauri/crates/mt-db/src/time.rs:L14-38`, sem dependência nova.
3. Qualquer nome não reconhecido deve permanecer na listagem como `kind: "unknown"`. Manter os filtros de arquivo regular, symlink, ocultos, extensão e a ordenação existentes. Nomes com `.DB`, prefixos parecidos, tags incompletas, sufixos extras ou formatos antigos não comprovados ficam preservados como desconhecidos. O exemplo sem sequência hoje documentado no DTO não comprova um gerador histórico; não torná-lo automaticamente removível.
4. Em `rotate_backups`, selecionar candidatos pelo reconhecedor aplicado a `item.file_name`, em vez de confiar exclusivamente no `kind` público. Somente nomes reconhecidos como `Daily` ou `PreMigration` participam das respectivas contagens e exclusões. Desconhecidos nunca consomem cotas. Preservar retenção mínima diária 1, máximo pré-migração 3 e retenção ilimitada das demais categorias.
5. Manter a possibilidade de selecionar e restaurar um SQLite importado usando os validadores existentes. Atualizar a documentação do DTO e, após concluir código e testes do bloco, regenerar os bindings pelo comando existente. Não adicionar `Unknown` a `BackupKind`, pois esse enum representa categorias que a aplicação pode criar.
6. Adicionar todos os testes abaixo antes da primeira rodada de testes, tipos ou build. Usar diretórios temporários isolados e timestamps de arquivo controlados; evitar esperas temporizadas como requisito de ordenação. Não alterar diretórios reais de backups.

## Testes de regressão e validação em lote

- **`rotation_preserves_unknown_backups`:** configurar retenção 1 via settings; inserir `arquivo-importado.db` antigo com conteúdo conhecido e criar dois diários reais por `MaintenanceHandle`. Assertar bytes intactos do importado, apenas um diário gerenciado e exclusão do diário antigo. Repetir com o externo mais recente para provar que não ocupa a cota.
- **Matriz de nomes:** testar nome externo contendo `-daily`, `-manual`, `-premigration-`, prefixo parcial, timestamp inválido, sequência ausente/inválida, versão ausente/inválida, extensão maiúscula e sufixo extra. Não basta corrigir somente o fallback diário: falsos pré-migração também precisam ser preservados.
- **Reconhecimento positivo:** cobrir todas as tags emitidas, sequências `001`, `999` e `1000`, e versões com múltiplos dígitos. Exercitar nomes realmente produzidos por `create_backup_blocking` para evitar um parser que rejeite todos os backups e desative a rotação.
- **Retenção por categoria:** criar mais de três pré-migração válidos e diários acima da cota; confirmar manutenção dos mais recentes em cada grupo, inclusive desempate por nome. Manuais, pré-reset, pré-restauração e corrompidos devem permanecer íntegros. Repetir a rotação acionada por criação manual para cobrir o caminho usado pela UI.
- **Listagem e limites de filesystem:** desconhecidos continuam visíveis com `kind == "unknown"`; ocultos, diretórios e symlinks não entram nas contagens nem são removidos. Validar conteúdo do alvo do symlink e permissões `0o600` dos backups criados.
- **`unknown_backup_does_not_delay_daily_creation`:** em `tests/retention.rs`, colocar somente um externo recente no diretório de backups e executar `run_now`; deve surgir um diário real, preservando o externo. Confirmar que um diário reconhecido recente ainda impede criação redundante.
- **`imported_backup_remains_restorable_after_rotation`:** preparar um SQLite válido com dado verificável, sob nome externo; gerar diários acima da retenção, agendar sua restauração e aplicar no boot. Verificar o dado restaurado e a preservação do arquivo importado. A preparação deve produzir mais de sete diários antes do boot, com retenção maior, para também exercitar a rotação com limite 7 disparada por `PreRestore`.
- Executar as suítes existentes de backup/recuperação, retenção e `upgrade_from_fixtures`, incluindo rollback de restauração. Agrupar falhas por causa, corrigir o conjunto afetado e repetir a validação pertinente após concluir as correções.

## Banco de dados

**Nenhuma migration.** A correção altera classificação e seleção de arquivos no filesystem. Não modifica schema, dados persistidos, WAL, FTS ou formato do conteúdo dos backups.

## Riscos de quebra e Não-Regressão

- **Parser excessivamente restritivo:** pode impedir retenção de backups legítimos. Cobrir nomes do gerador, datas válidas, sequência acima de 999 e versões de migração; nomes não comprovados permanecem preservados e listados.
- **Novo valor de `kind`:** `unknown` é uma mudança semântica observável, embora preserve o contrato `string`. Validar serialização, listagem, agendador e restauração; a UI atual não apresenta ramificações por categoria.
- **Retenção desativada acidentalmente:** testes devem comprovar tanto a preservação de externos quanto a remoção dos excedentes gerenciados; somente contar sobreviventes externos é insuficiente.
- **Restauração e manutenção:** um externo não deve adiar backup diário nem desaparecer durante o backup pré-restauração. Cobrir ambos os caminhos e manter as verificações de integridade, versão e rejeição de symlinks.
- **Limite de identidade:** o nome canônico reconhece o namespace gerenciado, não comprova autoria física. Um arquivo externo renomeado exatamente para esse padrão é indistinguível sem metadados de proveniência; este plano corrige nomes externos/importados não reconhecidos, que são o caso comprovado na issue. Formatos históricos desconhecidos devem falhar de modo conservador, permanecendo em disco.

## Validação

Após implementar integralmente código e testes, executar na raiz da worktree:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked -p mt-db --test backup --test retention --test upgrade_from_fixtures
cargo test --manifest-path src-tauri/Cargo.toml --locked -p mt-db
cargo fmt --manifest-path src-tauri/Cargo.toml -p mt-db -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked -p mt-db --all-targets -- -D warnings
npm run bindings
npm run typecheck
git diff --check
```

**Prova conclusiva:** os testes `rotation_preserves_unknown_backups`, `unknown_backup_does_not_delay_daily_creation` e `imported_backup_remains_restorable_after_rotation` passam, com assertivas de bytes/conteúdo e retenção efetiva dos gerenciados; a matriz também prova preservação dos falsos pré-migração. Os comandos acima são validações futuras, não resultados executados durante este planejamento.

## Skills Aplicáveis

- `security-and-hardening`: tratamento conservador de nomes não confiáveis e limitação da seleção destrutiva.
- `code-review`: revisão da correção e dos contratos antes de integrar, com foco em preservação de dados e cobertura de regressão.
- `vue-typescript-best-practices`: conferir o contrato DTO Rust/TypeScript ao regenerar os bindings, sem remodelar a UI.

Seguir também o `AGENTS.md`: trabalho isolado em worktree, comunicação e comentários em pt-BR, commit/merge somente após autorização. Este plano não requer delegação.
