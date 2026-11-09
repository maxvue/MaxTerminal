# Plano de implementação — Issue #23

## Descrição e Causa Raiz

**Veracidade: bug confirmado por evidência estática no commit `0a769a40c4bfe80abc2f4ffc68a44a49019d5a0b`.** A reprodução na aplicação desktop não foi executada nesta etapa. O código demonstra uma substituição incondicional do ambiente na submissão, suficiente para confirmar o relato. Este documento é o único arquivo alterado nesta etapa de planejamento.

Ao editar um template com `environment: { kind: 'agent', agentId: 'codex' }`, mesmo alterando somente nome ou ícone, o editor emite `environment: { kind: 'shell', agentId: null }`. Uma atualização válida persiste esses valores e perde a identificação do agente. A falha afeta também os demais identificadores aceitos: `claude-code`, `claude`, `agy` e `opencode`.

### Causa Raiz Comprovada

- `src/components/workspace/TemplateEditorModal.vue:L171-234`: a inicialização lê os campos editáveis do template, mas não carrega `config.environment`; a interface não oferece edição desse parâmetro.
- `src/components/workspace/TemplateEditorModal.vue:L372-386`: `handleSubmit` monta todo o payload; especificamente **L379-382** fixa `kind: 'shell'` e `agentId: null`, independentemente de `props.template`. Este é o ponto de perda da informação.
- `src-tauri/src/workspace/environment.rs:L35-50`: o backend aceita os agentes citados e também aceita Shell sem agente. Portanto, o payload incorreto é válido e não provoca erro de validação.

### Fluxo causal e rastreamento reverso UI ⇄ Store ⇄ IPC ⇄ Serviço ⇄ DB

1. **DB → objeto de domínio:** `src-tauri/crates/mt-db/src/repos/templates.rs:L746-782` lê `environment_kind` e `agent_id` e reconstrói `config.environment` corretamente. A listagem passa por `src-tauri/src/commands/templates.rs:L10-28` e `src/ipc/templates.ts:L14-21`.
2. **Store → UI:** `src/stores/templates.ts:L49-72` carrega a listagem; `src/components/workspace/TabWorkspaceView.vue:L377-379` seleciona o template existente e a ligação em **L47** entrega esse objeto ao editor. O ambiente original chega nas props.
3. **UI → Store:** a submissão descarta o ambiente em `TemplateEditorModal.vue:L379-382`; `TabWorkspaceView.vue:L382-393` encaminha o input com ID e revisão, e `src/stores/templates.ts:L104-117` o repassa sem reconstruir o ambiente.
4. **Store → IPC → serviço:** `src/ipc/templates.ts:L32-42`, `src/types/bindings.ts:L117` e `src-tauri/src/commands/templates.rs:L41-53` encaminham a atualização. Não há rota HTTP/controller neste fluxo: o comando Tauri chama `WorkspaceManager`.
5. **Serviço → DB:** `src-tauri/src/workspace/manager.rs:L713-729` valida o input; **L770-779** envia a atualização ao repositório. `src-tauri/crates/mt-core/src/workspace/template.rs:L601-632` aceita Shell sem agente, enquanto `src-tauri/crates/mt-db/src/repos/templates.rs:L195-217` prepara a atualização. Em **L461-493**, o SQL escreve diretamente `config.environment.kind` e `config.environment.agent_id` nas colunas, com controle de revisão.
6. **DB → retorno → Store:** `src-tauri/crates/mt-db/src/repos/templates.rs:L503-508` retorna o registro atualizado; `src/stores/templates.ts:L111-117` recarrega o projeto. Assim, o editor não apenas exibe Shell: a informação foi substituída na persistência.
7. **Consequência na próxima abertura:** `src-tauri/src/workspace/manager.rs:L940-952` copia o ambiente persistido para o `LaunchSnapshot`; `src-tauri/src/workspace/environment.rs:L138-148` só injeta automaticamente `MAXTERMINAL_TEMPLATE_AGENT` quando o ambiente é agente. A conversão elimina essa identificação automática, salvo valor explicitamente fornecido em `envVars`. Não há evidência de que salvar o template reconfigure PTYs já abertos.

**Cenário determinístico para regressão:** fornecer um template local válido de Codex ao modal aberto, alterar o nome por um evento do input e submeter o formulário. Atualmente, o evento `save` contém Shell/null; o esperado é preservar exatamente agente/Codex. Repetir mudando apenas o ícone. O defeito continua presente nesta revisão.

## Arquivos afetados

Arquivos a alterar/criar durante a implementação:

1. `src/components/workspace/TemplateEditorModal.vue` — corrigir a origem do campo `environment` no payload.
2. `tests/components/workspace/TemplateEditorModalEnvironment.spec.ts` — novo conjunto de regressão comportamental do modal e integração com a store real, usando IPC simulado.

Artefato desta etapa: `docs/issues/23/plan.md`. Os arquivos de store, IPC, bindings, Rust e SQLite citados acima são evidências e dependências de validação; não precisam de alteração para esta correção.

## Execuções propostas

1. Trabalhar na worktree dedicada da tarefa e conferir o estado inicial. Preservar alterações preexistentes e seguir o `AGENTS.md`.
2. No novo arquivo de teste, construir fixtures tipadas de `TabTemplate_Serialize`, com projeto, ID, revisão, configuração local completa e valores não vazios nos campos relevantes. Usar Pinia e a montagem do modal conforme o padrão de `tests/components/workspace/TemplateEditorModalSsh.spec.ts`.
3. Corrigir exclusivamente a composição de `environment` em `handleSubmit`: quando `props.template` existir, emitir uma **cópia** de `props.template.config.environment`; quando não existir, emitir um novo objeto `{ kind: 'shell', agentId: null }`. Usar o contrato existente de `TabTemplateInput_Deserialize`, sem casts que ocultem incompatibilidades.
4. Preferir derivar o ambiente das props no momento da submissão, pois não há controle editável correspondente. Isso evita criar estado duplicado ou um watcher adicional e preserva o ambiente do template atualmente selecionado. Não normalizar os identificadores, inclusive o alias `claude`.
5. Manter ambiente de execução independente de `connection`: um template de agente com SSH deve conservar o agente; origem local/remota não decide `environment.kind`. Preservar montagem de `envVars`, validações, autenticação SSH e campos já editáveis.
6. Completar todos os cenários de teste abaixo, incluindo integração modal → store → IPC simulado. Reaproveitar a configuração atual de Vitest e o setup do projeto, sem novas dependências ou mudanças de bindings.
7. **Implementar todo o bloco autorizado e seus testes antes de executar testes, tipos ou build.** Depois executar a validação em lote, registrar falhas, corrigir conjuntamente as relacionadas ao escopo e repetir os comandos afetados. Falhas preexistentes devem ser identificadas separadamente, sem ampliar a correção arbitrariamente.

## Testes de regressão e validação em lote

No novo `TemplateEditorModalEnvironment.spec.ts`:

- **Nome e ícone isolados:** parametrizar os cinco IDs aceitos pelo backend e as duas alterações; montar o componente, editar pelo input e submeter por `form.trigger('submit')`. Exigir exatamente um `save`, o campo alterado correto e `payload.environment` igual ao original.
- **Preservação e ausência de mutação:** conferir que a fixture original continua intacta e que o objeto `environment` emitido não é a mesma referência das props. Conferir os demais campos de configuração com valores válidos, evitando que os testes passem se a submissão descartar outros dados.
- **Salvar sem alterações:** template agente continua agente mesmo quando nenhum campo é modificado.
- **Shell existente e criação:** template Shell editado permanece Shell/null; um modal sem template cria Shell/null.
- **Reutilização do modal:** com `setProps` e atualização reativa aguardada, passar de agente Codex para outro agente, depois Shell, depois `template: null`, simulando fechar/reabrir. Cada submissão deve usar o ambiente do contexto atual, sem carregar agente anterior para a criação.
- **SSH:** editar template agente remoto com chave e com senha existente; preservar o ambiente e, no caso de senha, exigir `operation: { action: 'keep' }`, sem reapresentar segredo. Verificar conexão e `envVars` separadamente do ambiente.
- **Validação existente:** nome vazio impede `save`, inclusive para template de agente.
- **Integração com store real:** usar `useTemplatesStore` com Pinia real e simular apenas `@/ipc/templates`. Encaminhar o payload emitido pelo modal para `updateTemplate(id, revision, payload)`; verificar argumentos exatos de `templateUpdate`, manutenção do agente no retorno e na listagem recarregada, além de atualização da revisão. O retorno simulado deve refletir o payload recebido, evitando mascarar uma conversão feita pela UI. Este teste cobre o frontend e não equivale a um teste de SQLite/IPC nativo.

Executar também as suítes existentes de SSH e da store, que protegem os fluxos adjacentes. Os testes novos devem detectar a versão defeituosa porque a expectativa agente/Codex contradiz diretamente o literal Shell/null atual.

## Banco de dados

**Nenhuma** migration. As colunas `tab_templates.environment_kind` e `agent_id` já representam o ambiente corretamente. O contrato IPC continua enviando a configuração completa.

Templates já sobrescritos perderam sua identificação; esta correção previne novas conversões, mas não permite inferir com segurança o agente anterior a partir do nome ou comando. Não incluir reparo automático de dados neste escopo.

## Riscos de quebra e Não-Regressão

| Risco | Garantia e teste |
| --- | --- |
| Novo template herdar agente de edição anterior | Ler as props atuais e testar agente → Shell → criação na mesma instância. |
| Mutação indireta da configuração mantida na store | Copiar `environment`; afirmar igualdade de valor, independência de referência e fixture intacta. |
| Normalização indevida de IDs ou perda do alias `claude` | Casos parametrizados com todos os identificadores aceitos. |
| Confundir conexão SSH com ambiente de execução | Cobrir agente local, agente SSH/chave e agente SSH/senha existente. |
| Perder `envVars`, comando inicial ou opções do template | Comparar os demais campos do payload com fixtures completas. |
| Quebra do contrato de atualização/revisão e recarga | Integração com store real, argumentos de IPC, resposta e cache recarregado; executar suíte atual da store. |
| Teste apenas de função auxiliar deixar passar defeito no formulário | Montar o SFC e submeter o formulário real; conferir o evento `save`. |

O backend deve continuar aceitando uma mudança explícita de agente para Shell por clientes IPC. A correção pertence ao formulário que atualmente sobrescreve um parâmetro não editado; não bloquear transições válidas no serviço ou no banco.

## Validação

Executar a partir da worktree, após concluir código e testes, com as dependências locais do projeto disponíveis:

```bash
npm run test -- tests/components/workspace/TemplateEditorModalEnvironment.spec.ts tests/components/workspace/TemplateEditorModalSsh.spec.ts tests/frontend/workspace/templates.store.spec.ts
npm run typecheck
npm run typecheck:vue
npx --no-install eslint --max-warnings 0 src/components/workspace/TemplateEditorModal.vue tests/components/workspace/TemplateEditorModalEnvironment.spec.ts
npm run build
git diff --check
```

**Prova automatizada principal:** a primeira linha deve concluir com todos os testes passando, especialmente os cenários de editar nome/ícone de agente e conferir o payload recebido pelo IPC simulado. Tipos e build são verificações complementares, não substitutos da assertiva de preservação.

**Validação integrada complementar no Tauri:** em projeto de teste, criar via `template_create` um template local válido com `environment: { kind: 'agent', agentId: 'codex' }`, registrar ID e revisão, editar somente nome no modal e consultar via `template_get`. Exigir nome atualizado, revisão incrementada e ambiente agente/Codex; repetir para ícone. Abrir uma aba desse template e verificar a identificação automática do agente, sem sobrescrever `MAXTERMINAL_TEMPLATE_AGENT` nas variáveis. Usar os objetos completos exigidos pelos bindings e não alterar templates de produção.

Nenhum teste de implementação, build ou execução desktop foi realizado durante este planejamento; a confirmação do defeito se baseia nos trechos e no fluxo de dados documentados.

## Skills Aplicáveis

- `vue-components` — preservar os padrões do SFC, props e evento tipado do formulário.
- `vue-typescript-best-practices` — manter compatibilidade com os DTOs gerados e tipar fixtures/payloads.
- `vitest-skill` — estruturar testes de regressão com Vitest e simular apenas a fronteira IPC.
- `vue-eslint-stylelint-quality-standards` — respeitar ordem template/script/style, quatro espaços e formatação do projeto, usando a configuração local.
- `code-review` — revisar preservação dos contratos, cobertura de regressão e escopo antes da integração.

Aplicar essas orientações subordinadas ao `AGENTS.md` do MaxTerminal e ao fluxo de validação em lote solicitado nesta issue. Skills específicas de APIs MaxPinia/Engeapp não devem substituir os contratos reais deste repositório.
