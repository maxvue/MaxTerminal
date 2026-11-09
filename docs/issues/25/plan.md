# Plano de implementação — Issue #25

### Descrição e Causa Raiz

**Veracidade: confirmada por inspeção do código atual.** A divergência é documental: o JSDoc de `flush()` promete um teto de bytes que a implementação deliberadamente flexibiliza para consumir um frame completo. O comentário permanece incorreto em `src/terminal/engine/writeBatcher.ts:L90`; não há evidência de correção prévia no estado inspecionado.

**Causa Raiz Comprovada:**

- `src/terminal/engine/writeBatcher.ts:L90-99`: o texto afirma “Drena imediatamente até `frameBudgetBytes`”, enquanto o orçamento efetivo é normalizado com `Math.max(1, ...)` e o lote começa vazio.
- `src/terminal/engine/writeBatcher.ts:L101-109`: a guarda de orçamento exige `batchFrames.length > 0`. Logo, o primeiro frame de **cada lote**, e não apenas o primeiro da sessão, entra integralmente mesmo acima do orçamento. Frames seguintes só entram se couberem no orçamento restante.
- `src/terminal/engine/writeBatcher.ts:L113-135`: os bytes desse primeiro frame são encaminhados integralmente a `target.write`; não ocorre divisão para satisfazer o orçamento.
- `src/terminal/engine/writeBatcher.ts:L93-95`: fila vazia, escrita em andamento ou descarte também impedem a drenagem imediata, devendo a redação evitar uma promessa incondicional.

**Cenário verificável por rastreamento:** com orçamento 4 e frame de 8 bytes aceito por `push()`, `budget = 4`, `batchFrames.length = 0` e a guarda da linha 103 é falsa. O lote recebe os 8 bytes e `target.write` recebe o frame completo. Havendo outro frame não vazio na fila, ele fica para outro lote. Este cenário foi comprovado estaticamente; nenhum teste foi executado nesta etapa de planejamento.

**Fluxo causal e rastreamento reverso dos dados:**

1. Saída do PTY chega ao consumidor `ptyStreamHub.subscribe(...).onData` em `src/terminal/engine/terminalInstance.ts:L562-577`; `seq`, `byteOffset` e `bytes` são repassados a `WriteBatcher.push`.
2. `push()` verifica o teto agregado de 1 MiB e enfileira o frame inteiro em `src/terminal/engine/writeBatcher.ts:L64-87`.
3. `flush()` seleciona frames completos, aplicando a exceção identificada, e escreve no alvo. A ligação do alvo à UI do terminal (`this.xterm`) e da configuração a `opts.flow` está em `src/terminal/engine/terminalInstance.ts:L245-252`.
4. Somente o callback de parse confirma cursor e consumo, em `src/terminal/engine/writeBatcher.ts:L125-156`; o consumo volta ao `FlowController` em `src/terminal/engine/terminalInstance.ts:L251-252`, cujo ACK é encaminhado à assinatura em `L227-235`.
5. Store, rotas HTTP, controller e banco de dados não participam da seleção desse lote nem da divergência do comentário. O recorte causal é o consumidor do stream → fila → escrita na UI → confirmação/ACK.

**Agravantes:** confundir o orçamento de agrupamento com um teto rígido pode induzir alterações que deixem frames grandes presos na fila ou fragmentem fronteiras usadas na confirmação de cursor. A exceção continua subordinada ao limite agregado de 1 MiB de `push()` (`L75-82`).

### Arquivos afetados

- `src/terminal/engine/writeBatcher.ts`: alteração exclusivamente do JSDoc imediatamente anterior a `flush()`.
- `docs/issues/25/plan.md`: artefato de planejamento desta etapa.

Referências de validação, sem alteração prevista: `tests/terminal/writeBatcher.spec.ts`, `src/terminal/engine/terminalInstance.ts` e `package.json`.

### Execuções propostas

1. Executar a implementação na worktree dedicada e confirmar que o comentário e a guarda ainda correspondem às evidências acima.
2. Substituir exclusivamente o JSDoc de `flush()` pelo texto abaixo, mantendo a indentação de quatro espaços:

   ```ts
   /**
    * Drena a fila em uma única chamada `target.write` quando não há escrita em andamento
    * e a instância está ativa. Preserva frames inteiros e usa `frameBudgetBytes`
    * (mínimo de 1 byte) como orçamento de agrupamento.
    * O primeiro frame de cada lote é consumido integralmente mesmo quando excede
    * o orçamento, garantindo progresso da fila; frames seguintes só entram se couberem.
    * A exceção ao orçamento não altera o limite agregado de 1 MiB imposto por `push()`.
    */
   ```

3. Conferir que “primeiro frame” se refere a cada lote, que a exceção é explícita e que o texto explica preservação de frames e progresso da fila.
4. Concluir toda a edição autorizada antes das verificações. Esta correção documental utiliza os testes existentes; não requer novos testes permanentes nem alterações na lógica de execução.
5. Validar em lote pelos comandos abaixo; se houver falhas, analisar todas antes de corrigir o bloco afetado e repetir somente as verificações pertinentes.

### Testes de regressão e validação em lote

Após concluir a edição, executar na worktree:

```sh
git diff --check
npm run test -- tests/terminal/writeBatcher.spec.ts
```

A suíte existente cobre agrupamento abaixo do orçamento e fronteiras de frames (`tests/terminal/writeBatcher.spec.ts:L14-84`), escrita única em voo e cursor pós-parse (`L54-81`), preservação de bytes UTF-8/ANSI (`L86-114`), agendamento oculto e rejeição acima de 1 MiB (`L116-149`). Ela não contém um caso específico de primeiro frame acima do orçamento; essa exceção é comprovada pela guarda existente e pela revisão do comentário contra o fluxo descrito neste plano.

Executar também a verificação automatizada da seção “Validação”, que prova que a única mudança no arquivo é a substituição documental especificada. Por se tratar de comentário, sem alteração de tokens executáveis, tipos e build não são necessários para este escopo. Se as dependências locais impedirem a suíte de rodar, registrar o impedimento sem declarar testes aprovados.

### Banco de dados

Nenhuma.

### Riscos de quebra e Não-Regressão

- **Teto rígido indevido:** remover a condição de lote não vazio poderia impedir progresso de um frame acima do orçamento. A comparação automatizada deve rejeitar qualquer mudança executável.
- **Fragmentação/ACK:** dividir frames pode alterar cursores e callbacks. Preservar integralmente a implementação e executar a suíte existente, especialmente as asserções de cursor e bytes.
- **Ambiguidade temporal:** documentar apenas “primeiro frame” pode sugerir uma exceção única na sessão. Exigir “primeiro frame de cada lote”.
- **Confusão entre limites:** a exceção de agrupamento não autoriza exceder o teto de memória de 1 MiB. Conferir o texto e o teste de overflow existente.
- **Contratos:** assinaturas públicas, configuração, callbacks, persistência e consumidores permanecem com os mesmos contratos; a alteração autorizada é apenas documental.

### Validação

Antes do commit de implementação, executar o script abaixo a partir da raiz da worktree. Ele compara com `HEAD`, exige o JSDoc proposto e rejeita qualquer outra mudança no arquivo. Caso a base já tenha mudado, revalidar as evidências antes de adaptar o comparador.

```sh
python3 - <<'PY'
from pathlib import Path
import subprocess

path = 'src/terminal/engine/writeBatcher.ts'
before = subprocess.check_output(['git', 'show', f'HEAD:{path}'], text=True)
after = Path(path).read_text()
old = '    /** Drena imediatamente até `frameBudgetBytes` em uma única chamada `target.write`. */'
new = '''    /**
     * Drena a fila em uma única chamada `target.write` quando não há escrita em andamento
     * e a instância está ativa. Preserva frames inteiros e usa `frameBudgetBytes`
     * (mínimo de 1 byte) como orçamento de agrupamento.
     * O primeiro frame de cada lote é consumido integralmente mesmo quando excede
     * o orçamento, garantindo progresso da fila; frames seguintes só entram se couberem.
     * A exceção ao orçamento não altera o limite agregado de 1 MiB imposto por `push()`.
     */'''
assert before.count(old) == 1, 'A base documental mudou; revalidar o plano.'
assert after == before.replace(old, new, 1), 'Alteração divergente do escopo documental aprovado.'
print('Documentação corrigida; implementação executável preservada integralmente.')
PY
```

**Critério de aceite:** comparador aprovado, `git diff --check` sem erros, suíte direcionada aprovada e conferência do texto contra `flush()`. O comparador prova a correção documental especificada; a suíte verifica a preservação dos comportamentos já cobertos. Nenhum desses comandos foi executado durante este planejamento.

### Skills Aplicáveis

- `code-review`: revisar a alteração documental e sua fidelidade à implementação antes de integração.
- `vitest-skill`: seguir as orientações ao executar e interpretar a suíte existente, caso seja necessário diagnosticar sua execução.

As demais skills de UI, banco de dados e implementação multifile não são necessárias para esta correção documental localizada.
