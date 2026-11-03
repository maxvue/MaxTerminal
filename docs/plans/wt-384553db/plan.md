# Correção pontual de sintaxe SCSS em InlineDiagnosis

## Identificação e objetivo

- Data: 03/11/2026.
- Projeto: MaxTerminal.
- Worktree e branch: `wt-384553db`.
- Raiz obrigatória: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-384553db`.
- Solicitação: corrigir o erro `[plugin:vite:css] [sass] Error: unmatched "}"` reportado em `InlineDiagnosis.vue`.
- Entrega esperada: componente novamente compilável, sem alterar aparência, interações ou contratos.
- Esta sessão somente investigou e documentou; não implementou nem executou testes, build, preflight operacional ou serviços.
- Sem spec separada: correção atômica em um arquivo existente, sem mudança de arquitetura, contrato, persistência ou regra de negócio. As etapas abaixo são preparação, correção e validação da mesma alteração.

## Evidências e diagnóstico

1. `git status --short` estava vazio antes da criação deste documento.
2. `src/components/ai/InlineDiagnosis.vue` foi lido integralmente. No bloco `<style lang="scss" scoped>`:
   - `.inline-diagnosis__dismiss` abre na linha 225;
   - `&:hover` abre em 228 e fecha em 231;
   - o seletor externo fecha corretamente em 232;
   - as linhas 233 e 234 contêm duas chaves de fechamento sem abertura correspondente;
   - `.inline-diagnosis__caution` e `.inline-diagnosis__caution-actions` são regras independentes, válidas, que devem permanecer.
3. O erro informado aponta para a árvore principal `/home/johnattas/GitHub/MaxTerminal/src/...`; o mesmo defeito existe na worktree autorizada. Corrigir somente a worktree. Não presumir que a instância em execução usa esta revisão.
4. `vite.config.ts:58–63` injeta `@use '@/styles/params' as *;` no SCSS. `src/styles/params.scss` contém somente um mixin fechado corretamente. Não há evidência para alterar essa configuração.
5. `package.json` declara Vite `8.0.0`, `sass-embedded` `1.86.3`, Vue `3.6.0-rc.13` e Vitest `5.0.5`. São versões declaradas; a integridade efetiva será conferida antes da execução.
6. `.stylelintrc.json` usa os presets SCSS e Vue, sem necessidade de alteração. `.gitignore` já ignora `.worktrees/` e `.max-code-worktrees/`.
7. `src/components/ai/__tests__/InlineDiagnosis.spec.ts` já importa e monta o SFC: verifica resumo, cache, comando sugerido, inserção, aplicação segura e dispensa. Não é preciso criar um teste artificial que conte chaves. A compilação de produção é o gate principal do SCSS.
8. `src/composables/__tests__/useInlineDiagnosis.spec.ts` cobre elegibilidade, códigos ignorados, alt-screen, diagnóstico, inserção, execução e confirmação destrutiva, com IPC/adaptadores simulados.
9. `src/composables/ai/useInlineDiagnosis.ts` e o início de `src/stores/ai.ts` confirmam que dados e ações estão fora do SCSS. O store atual usa `defineStore` de Pinia; esta correção não autoriza migrá-lo.
10. `tests/setup.ts` configura stubs de teleport/tooltip e limpeza de mocks Tauri. `vitest.config.ts` usa happy-dom, alias `@`, deduplicação de Vue/Pinia e inclui testes em `src/**/__tests__/**/*.spec.ts`.
11. A biblioteca acessível `node_modules/@maxvue/max-components-ui/package.json` informa versão `1.1.36` e exporta `MaxButton`; o contrato distribuído `dist/components/MaxIconButton.vue.d.ts` confirma o componente e seu evento de clique. O SFC já usa MaxIcon, MaxButton e MaxIconButton: preservá-los.
12. Referência oficial consultada: https://github.com/sass/sass/blob/main/README.md e especificação de sintaxe no mesmo repositório, via Context7. Confirmam delimitação e aninhamento SCSS com chaves. A referência é da documentação atual, não uma reprodução da instalação local de Sass 1.86.3; a conclusão principal decorre do arquivo local.

## Escopo, contratos e decisões

### Incluído

- Remover exclusivamente as duas chaves excedentes após `.inline-diagnosis__dismiss`.
- Preservar o fechamento do `&:hover`, o fechamento de `.inline-diagnosis__dismiss` e todas as declarações seguintes.
- Validar sintaxe, regressões existentes e compilação na revisão corrigida.

### Fora de escopo

- Alterar template, script, props `block`/`autoTrigger`, eventos `inserted`/`applied`/`dismissed`, stores, composables, IPC, backend, configurações ou dependências.
- Redesenhar botões, cores, espaçamento, confirmações, cache ou elegibilidade.
- Migrar estado para MaxPinia ou helpers para MaxUse: são os padrões obrigatórios para novas necessidades dessas camadas, mas esta mudança não cria nenhuma. Preservar a integração atual e não introduzir alternativas às bibliotecas Max.
- Executar comandos sugeridos pelo diagnóstico, chamar fornecedor de IA real, instalar pacotes, sincronizar bibliotecas, copiar `.env`, modificar dados ou iniciar processos de terminal.
- Criar outra worktree, mudar de branch, editar a árvore principal ou a biblioteca compartilhada, fazer commit, merge, push ou remover esta worktree. Integração e limpeza são do MaxCode/usuário.

### Justificativa

O erro é estrutural e localizado; remover somente as duas chaves mantém os seletores no nível original. Inserir aberturas artificiais, retirar o hover ou mover regras mudaria o CSS sem necessidade. Atualizar Sass/Vite não corrige a fonte inválida e amplia o risco.

## Execução ordenada

### T1 — Admissão e preflight por leitura

1. Ler este plano integralmente na primeira admissão; nas retomadas, consultar apenas tarefas e decisões pertinentes.
2. Conferir diretório, branch e alterações locais sem tocar em outras worktrees. Reabrir o trecho afetado e verificar se a correção já foi realizada; números de linha são referências da investigação, não instrução de remoção cega.
3. Inspecionar scripts e eventuais pre/posthooks em `package.json`, configuração do harness, resoluções/symlinks efetivos das três bibliotecas Max e disponibilidade dos artefatos distribuídos. Conferir identidade única de Vue/Pinia e resolução de Axios se envolvida no bootstrap.
4. Conferir isolamento dos testes e se inicializações podem alcançar banco, cache, filas, storage ou serviços reais. Worktree, mocks parciais e transações não bastam como prova. Não executar suíte compartilhada sem isolamento comprovado.
5. Se biblioteca ou harness falhar, interromper validação dependente e registrar próxima ação. Não contornar gates com comandos diretos, configuração ignorada, remoção de hooks ou instalações/sincronizações não autorizadas.

Conclusão: fonte ainda apresenta o mesmo defeito e ambiente está apto para a alteração/validação, ou bloqueio concreto está registrado. Arquivos de produção: nenhum modificado nesta tarefa.

### T2 — Correção atômica

1. Em `src/components/ai/InlineDiagnosis.vue`, apagar apenas as duas linhas excedentes entre o fechamento válido de `.inline-diagnosis__dismiss` e `.inline-diagnosis__caution`.
2. Revisar o diff: duas remoções de `}`, sem alterações em template, script, tokens, seletores ou declarações.
3. Manter os testes existentes e suas asserções. Não é necessária alteração de testes para este defeito de parser: os testes do componente e o build já exercitam o caminho relevante.

Conclusão: a estrutura SCSS está balanceada por inspeção. Executar todo o bloco antes das validações; não rodar testes entre remoções de linhas.

### T3 — Validação em lote e revisão de entrega

Após T1 e T2, executar os scripts confirmados em `package.json`, a partir desta worktree:

```bash
npm run test
npm run typecheck
npm run typecheck:vue
npm run lint
npm run lint:style
npm run build
```

- A suíte completa inclui os dois arquivos de regressão identificados; não repetir testes direcionados sobre a mesma revisão aprovada.
- Nenhum desses comandos foi executado no Plan. `lint` e `lint:style` são verificadores; não usar variantes com `--fix` para esta correção.
- Registrar saídas e códigos reais, inclusive falhas da suíte completa. Classificar falhas em lote por causa. Não corrigir problemas preexistentes ou de outros arquivos sem demonstrar relação e obter autorização para ampliação.
- Falhas recorrentes de bootstrap pedem diagnóstico agrupado antes de repetir comandos equivalentes. Reaproveitar verificações válidas quando seus insumos não mudaram; revalidar as afetadas pelas correções.
- Revisar o diff final e a segurança das confirmações por leitura. Nenhum teste deve executar os comandos shell presentes nas fixtures.
- Verificação manual: em instância autorizada que comprovadamente sirva esta worktree, conferir ausência do overlay Sass, renderização do diagnóstico e hover da dispensa, sem disparar comandos sugeridos. Usar diagnóstico já disponível ou fixture isolada; não provocar falha real nem chamar IA só para esta inspeção.
- Se a instância ainda servir a árvore principal, não editar essa árvore nem reiniciar serviços nesta autorização. Registrar a checagem visual como `manualPending`, com ação concreta para o responsável disponibilizar a revisão correta.

Conclusão: todos os gates aplicáveis aprovados e inspeção manual concluída, ou pendências/falhas explicitadas sem conclusão integral.

## Matriz de aceite

| Tarefa / requisito | Evidência esperada | Testes / verificações | Revisão pelo risco | Manual |
| --- | --- | --- | --- | --- |
| T1: execução isolada | Diretório correto, hooks e bibliotecas conferidos, harness sem efeitos reais | Leitura dos scripts, resoluções e bootstrap | Bloquear dependentes se integridade/isolamento falhar | Confirmar origem da instância usada |
| T2: eliminar erro Sass | Diff limitado às duas chaves excedentes | `npm run build`; parser SCSS em `npm run lint:style` | Garantir fechamentos válidos e regras caution independentes | Ausência do overlay na revisão corrigida |
| T2/T3: preservar contratos | Template/script e camadas de dados inalterados | Suíte `npm run test`, incluindo componente e composable existentes | Conferir confirmações caution/destrutiva sem executar comandos reais | Diagnóstico e hover sem mudança visual |
| T3: compatibilidade | Saídas e códigos de todos os gates registrados | Tipos TS/Vue, ESLint, Stylelint e build listados acima | Falha preexistente não autoriza ampliação | Pendência deve permanecer explícita |

## Riscos, reversão e limites

- Risco baixo: remoção de dois tokens inválidos, sem impacto esperado em dados, cache, processos ou contratos. Sem migração ou implantação especial.
- Build e lint podem revelar outros defeitos não investigados. Isso não é prova de causalidade com a correção; comunicar separadamente e não declarar entrega integral verde.
- Não foi reproduzido o erro com compilador nesta sessão; o diagnóstico está fundamentado na leitura e no log recebido.
- Não há teste de navegador real executado nem garantia sobre a árvore servida atualmente.
- Para desfazer a implementação futura, revisar e reverter somente o próprio hunk com autorização, preservando trabalho alheio. Não usar reset, checkout destrutivo ou remoção de arquivos. Reintroduzir as chaves devolve o defeito conhecido.
- Não delegar esta mudança mínima: as tarefas são sequenciais e compartilham um único arquivo de escrita.

## Progresso e retomada durante Execute

Criar/atualizar `docs/plans/wt-384553db/execution-state.json` somente durante Execute, com a autorização dessa fase; é proibido escrevê-lo no Plan. Registrar após os marcos T1, T2 e T3 e no handoff. Revalidar alterações locais antes de continuar, executar lacunas e não reabrir decisões sem evidência nova.

Contrato version 2 estrito, sem chaves extras:

- Raiz: `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`.
- `planRevision`: SHA256 real dos bytes de `plan.md` vigente.
- `tasks`: lista de `{id, status, files, validations, revision?, nextAction?}`. Usar IDs T1/T2/T3; `files` lista caminhos envolvidos e `validations: []` é sempre obrigatório, podendo permanecer vazio; detalhar comandos e resultados em `verification.checks`.
- Status de tarefa: `pending`, `running`, `completed`, `blocked`, `manualPending` ou `validationFailed`.
- `revision`, quando presente, é SHA256 real do conteúdo pertinente à tarefa; registrar a revisão factual dos arquivos, nunca rótulos. Para múltiplos arquivos, usar uma serialização determinística de caminhos ordenados e conteúdo e manter o mesmo cálculo nas retomadas.
- `verification`: `{revision, checks: [{command, revision, result, exitCode?}]}`. Revisions são hashes SHA256 reais dos insumos da validação, não nomes de branch ou estados.
- Check só aceita `{command, revision, result, exitCode?}` dentro de `verification.checks`. `command` tem no máximo 4.096 caracteres e corresponde ao comando realmente executado, ou ao comando planejado explicitamente pendente.
- Resultado de check/validação: apenas `passed`, `failed` ou `pending`; nunca `blocked`. `passed` exige `exitCode: 0`; `failed` exige inteiro diferente de zero; `pending` proíbe `exitCode`. Nunca gravar `exitCode: null`.
- `updatedAt`, se usado, é inteiro positivo em milissegundos, nunca data ISO. `nextAction`, na raiz ou tarefa, tem até 512 caracteres e contém motivo/próxima ação concreta de bloqueio ou pendência.
- Proibidos na raiz: `schemaVersion`, `worktree`, `branch`, `head`, `planPath`, `status`, `checks`. Proibidos na tarefa: `title`, `evidence`. Não usar `blocker`, `diagnostic`, `branchSync`, `durableSync` ou `followUp` em nenhum nível.
- Registrar todos os comandos efetivamente executados e todas as falhas, inclusive da suíte completa. Não inferir aprovação/exit code de eventos `tool:end.ok`. Ausência de checks, resultado vigente ausente ou falha vigente impede validação integral. Version 1 existente continua compatível, mas não prova aprovação integral.
- Estado autodeclarado não comprova execução nativa nem detecta verificações omitidas. Saídas reais e revisão de conteúdo continuam necessárias. Pendência manual ou bloqueio exige próxima ação, sem relato de conclusão total.
