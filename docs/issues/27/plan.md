# Plano de implementação — Issue #27

### Descrição e Causa Raiz

**Veracidade: confirmada por inspeção estática.** A omissão documental permanece no componente atual: `src/components/terminal/HybridTerminal.vue:L49-68` declara cinco props e quatro eventos sem comentários semânticos; `L12-14` encaminha o slot `block-actions` sem explicar seu payload. O início do script (`L23-49`) também não apresenta a responsabilidade do componente. A reprodução consiste em ler essas declarações e confrontá-las com os fluxos abaixo. Não se trata de falha de execução comprovada, mas de ausência de documentação pública comprovada.

**Causa raiz comprovada:** a superfície pública informa apenas tipos, nomes e alguns valores padrão; as regras que dão significado a esses contratos estão distribuídas entre handlers, stores e componentes filhos. Isso obriga consumidores a reconstruírem a implementação para descobrir o comportamento. O tipo `CommandBlockModel` já existe e possui descrição em `src/types/terminal.ts:L330-347`, mas não está referenciado na documentação do slot de `HybridTerminal`.

Rastreamento reverso dos contratos e evidências:

| Contrato | Fluxo e evidência concreta |
| --- | --- |
| Identidade do painel | `src/components/workspace/PaneView.vue:L12` fornece `tabId`, `paneId` e `ptyId`; `HybridTerminal.vue:L10` encaminha painel e PTY a `TerminalView`. `src/components/terminal/TerminalView.vue:L29-46` chama `useTerminal`; `src/composables/terminal/useTerminal.ts:L56-106` registra o estado na store e vincula uma instância do terminal ao host. O componente recebe a identidade de um PTY, não uma configuração para criar um processo shell. |
| `tabId` opcional | `HybridTerminal.vue:L126-150` prioriza `windowStore.linkContextForPane(paneId)`; a consulta retorna o contexto rastreado ou `null` em `src/stores/window.ts:L232-234`. Só na ausência desse contexto usa `tabId` para formar o fallback `{ tabId, paneId, ptyId, cwdEpoch: 1 }`. Sem ambos, retorna `null`. Em `src/composables/useEditorLinks.ts:L87-93`, isso impede a resolução de links locais; havendo contexto, a resolução segue para `osIpc.resolveLinks` (`L119-124`). O playground já omite `tabId` em `src/components/terminal/__dev__/TerminalPlayground.vue:L49-50`. |
| `visible` | O padrão é `true` (`HybridTerminal.vue:L57-61`). A prop segue por `TerminalView.vue:L32-34` até `useTerminal.ts:L106-110,L117-125`, atualizando instância e store. `src/terminal/engine/terminalInstance.ts:L403-411` encaminha visibilidade ao loteador de escrita, renderizador e controlador de ajuste. Não equivale a esconder o wrapper por CSS, desmontar o componente ou encerrar o PTY. |
| `richInput` | O padrão é `true`. `HybridTerminal.vue:L107-124` condiciona a interface rica a `richInput && inputMode === 'rich'`; ao desabilitar a prop, solicita override `passthrough`; ao habilitar, limpa o override se ele for `passthrough`. A solicitação ao desabilitar é repetida no `ready` (`L142-146`). `src/stores/terminal.ts:L195-213` aplica e recalcula o modo; `src/terminal/input/inputModeResolver.ts:L54-77,L103-108` dá precedência às TUIs e considera preferências. Logo, `true` permite entrada rica, mas não garante sua exibição. `HybridTerminal.vue:L18-19` condiciona também os resultados de IA e desabilita o input quando há saída do processo. |
| Eventos | `exit` reencaminha `ExitInfo` sem transformação (`HybridTerminal.vue:L188-190`); `title-change` reencaminha a string de `TerminalView` (`L10`); `restart-requested` vem do banner, sem argumentos (`L17`); `focus` é emitido sem argumentos após marcar o painel ativo em cada `focusin` do contêiner (`L242-245`). No consumidor, `PaneView.vue:L12` associa foco/reinício ao identificador do painel. Emitir reinício não reinicia o PTY por si só. |
| `block-actions` | `src/composables/terminal/useCommandBlocks.ts:L76-79` lê os blocos da store por painel e calcula a projeção visual. `src/components/terminal/CommandBlockLayer.vue:L155-177` mantém `block: CommandBlockModel` separado de `entry`; `L5-14` encaminha o bloco através de `CommandBlock`. `src/components/terminal/CommandBlock.vue:L14-17` insere o slot após as ações internas e o botão de inspeção. `HybridTerminal.vue:L11-15` apenas repassa `{ block }`. A camada só é montada após `ready`, omite a tela alternativa (`CommandBlockLayer.vue:L2,L135-140`) e filtra blocos pela janela de viewport (`L162-169`). |

**Limite do rastreamento:** a causa está na documentação da UI. Stores, instância do terminal e IPC acima explicam a semântica; não há evidência de defeito em rotas, controller/service nativo ou banco relacionado a esta omissão. Não é necessário atribuir artificialmente a causa a essas camadas.

### Arquivos afetados

- **Implementação:** `src/components/terminal/HybridTerminal.vue` — somente comentários explicativos em português no script existente.
- **Artefato desta etapa:** `docs/issues/27/plan.md` — este plano.
- **Validação sem edição:** `tests/terminal/HybridTerminal.spec.ts`, tipos, stores e componentes citados acima. Não criar testes permanentes que apenas procurem palavras em comentários.

### Execuções propostas

1. Trabalhar em worktree dedicada. Registrar o commit-base antes de implementar, para comparação automatizada posterior, e conferir se as referências deste plano continuam atuais.
2. Adicionar um cabeçalho JSDoc no início de `<script setup lang="ts">`, antes dos imports. Explicar que o componente compõe viewport xterm, camada de blocos, busca, avisos e entrada rica por painel, integrando foco, modos e contexto para links.
3. Nesse cabeçalho, incluir um inventário explícito com `@prop` para cada uma das cinco props, `@event` para os quatro eventos e `@slot block-actions`. Usar as mesmas grafias da API; manter as descrições e explicações em pt-BR. Complementar as declarações com JSDoc junto a cada campo de `HybridTerminalProps` e cada membro de `defineEmits`.
4. Documentar cada prop:
   - `paneId: PaneId`: obrigatória; identidade do painel usada para estado, registry, foco e blocos.
   - `ptyId: PtyId`: obrigatória; identidade do PTY ao qual o terminal se conecta. Não descrever como comando ou pedido de criação de processo.
   - `tabId?: TabId | undefined`: padrão `undefined`; fallback de contexto para links locais, com precedência do contexto rastreado, `cwdEpoch: 1` e consequência de ausência de ambos. Não afirmar que omitir `tabId` desativa todos os links ou inviabiliza o terminal.
   - `visible?: boolean`: padrão `true`; sinaliza visibilidade à instância e à store para gerenciamento de renderização/escrita/ajuste. Não promete ocultação visual do contêiner nem interrupção do processo.
   - `richInput?: boolean`: padrão `true`; descreve a condição real de exibição, resultados de IA associados, override solicitado ao desabilitar e limpeza condicional ao reabilitar. Explicar que TUIs, preferências e o modo efetivo continuam participando da decisão; não prometer um bloqueio permanente de `inputMode` decorrente apenas da prop.
5. Documentar os quatro eventos com origem, momento e payload: `exit(ExitInfo)`, `title-change(string)`, `restart-requested()` e `focus()`. Referenciar o tipo canônico `ExitInfo` de `@/types/bindings`, evitando duplicar DTOs. Explicitar a responsabilidade do consumidor por reiniciar a sessão e que `focus` deriva de `focusin`, inclusive de descendentes.
6. Documentar `block-actions` como slot opcional de ações adicionais, com escopo `{ block: CommandBlockModel }`, remetendo a `@/types/terminal`:
   - `id`, `paneId` e `command` identificam bloco, painel e comando; `cwd` admite `null`.
   - `startedAt` e `finishedAt` representam os tempos do bloco; `finishedAt` e `exitCode` admitem `null`.
   - `status` é `running | success | error | aborted`; `promptLine`, `outputStartLine` e `outputEndLine` são referências de linhas, com as duas últimas anuláveis.
   - `meta` é `Readonly<Record<string, unknown>>`; nenhum campo arbitrário é garantido sem validação.
   - O objeto vem da store, não é cópia isolada nem `VueWebTerminalProjectedEntry`; orientar consumo sem mutação direta, sem alegar imutabilidade profunda em runtime.
   - O slot complementa as ações existentes e aparece apenas quando a camada e o bloco são renderizados. O escopo não oferece callbacks adicionais de execução.
   - Incluir exemplo curto de consumo de `#block-actions="{ block }"` dentro do comentário, com botão que encaminhe `block` a um handler do consumidor.
7. Concluir todo o bloco documental antes de rodar verificações. Preservar template, estilos, imports, tipos, defaults, handlers e runtime; não adicionar `defineSlots`, novos exports ou dependências para resolver esta omissão.

### Testes de regressão e validação em lote

- Reutilizar `tests/terminal/HybridTerminal.spec.ts:L39-201`: a suíte já cobre visibilidade/WebGL, temas por painel, renderização de blocos, atalhos de entrada/busca, título, foco e pedido de reinício. Não afirmar que ela cobre integralmente o fallback de `tabId` ou o contrato documental do slot.
- Para uma alteração apenas em comentários, a prova principal de não-regressão será a identidade do script sem comentários e a identidade exata de template/estilos frente ao commit-base, conforme comando abaixo. Isso preserva inclusive comportamentos não cobertos pela suíte atual.
- Finalizar os comentários e o exemplo antes de executar a validação. Rodar o comparador, ESLint direcionado, suíte existente e checagem Vue em um lote; reunir falhas, corrigir todas as introduzidas pela alteração e repetir somente os comandos afetados. Distinguir falhas preexistentes/dependências indisponíveis de regressões, sem ampliar o escopo.
- Não executar build completo nem criar testes comportamentais novos para esta alteração documental de baixo impacto. Se a implementação exigir alteração funcional, ela deve ser tratada em escopo próprio.

### Banco de dados

Nenhuma.

### Riscos de quebra e Não-Regressão

| Risco | Controle |
| --- | --- |
| Documentar `visible` como ocultação ou parada do PTY | Conferir o texto com `useTerminal` e `terminalInstance.setVisible`; manter o teste de visibilidade/WebGL existente. |
| Tratar `richInput=true` como garantia de modo rico | Explicar modo efetivo, TUIs e preferências; preservar a suíte de alternância e identidade do script. |
| Tornar `tabId` aparentemente obrigatório ou ocultar a precedência do contexto | Documentar fallback e caso sem contexto; não alterar tipo/default. |
| Confundir bloco com projeção visual, omitir nulabilidade ou incentivar escrita na store | Referenciar `CommandBlockModel`, separar `block` de `entry` e orientar uso sem mutação. |
| Prometer reinício automático, payload de foco ou propagação de eventos inexistentes | Documentar exatamente as quatro declarações e seus pontos de emissão. |
| Introduzir alteração funcional ou quebrar sintaxe SFC em exemplos | Comparação automatizada com a base, ESLint e `typecheck:vue`; manter o exemplo dentro de comentário válido. |
| Documentação divergir futuramente dos tipos | Manter JSDoc junto aos contratos e referenciar os tipos canônicos; revisar documentação em mudanças futuras dessa API. |

### Validação

Na execução futura, registrar a base antes de editar: `export BASE_REV=$(git rev-parse HEAD)`. Após concluir o bloco, executar na mesma worktree e shell:

```bash
node --input-type=module <<'NODE'
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { parse } from '@vue/compiler-sfc';
import ts from 'typescript';

const file = 'src/components/terminal/HybridTerminal.vue';
assert.ok(process.env.BASE_REV, 'Informe o commit-base da implementação.');
const before = execFileSync('git', ['show', `${process.env.BASE_REV}:${file}`], { encoding: 'utf8' });
const after = readFileSync(file, 'utf8');
const oldSfc = parse(before);
const newSfc = parse(after);
assert.deepEqual(newSfc.errors, [], 'O componente deve continuar válido.');
assert.equal(newSfc.descriptor.template.content, oldSfc.descriptor.template.content);
assert.deepEqual(newSfc.descriptor.styles.map(s => s.content), oldSfc.descriptor.styles.map(s => s.content));
const tokenize = (text) => {
    const scanner = ts.createScanner(ts.ScriptTarget.Latest, true, ts.LanguageVariant.Standard, text);
    const tokens = [];
    for (let kind = scanner.scan(); kind !== ts.SyntaxKind.EndOfFileToken; kind = scanner.scan()) {
        tokens.push([kind, scanner.getTokenText()]);
    }
    return tokens;
};
assert.deepEqual(tokenize(newSfc.descriptor.scriptSetup.content), tokenize(oldSfc.descriptor.scriptSetup.content), 'Somente comentários e espaços podem mudar no script.');
const comments = newSfc.descriptor.scriptSetup.content.match(/\/\*\*[\s\S]*?\*\//g)?.join('\n') ?? '';
for (const [tag, names] of Object.entries({ prop: ['tabId', 'paneId', 'ptyId', 'visible', 'richInput'], event: ['exit', 'restart-requested', 'title-change', 'focus'], slot: ['block-actions'] })) {
    for (const name of names) assert.ok(comments.includes(`@${tag} ${name}`), `Falta documentar ${tag} ${name}.`);
}
assert.ok(comments.includes('CommandBlockModel'), 'O slot deve referenciar o tipo canônico.');
assert.notEqual(after, before, 'A documentação precisa ter sido adicionada.');
console.log('Inventário documental presente e código executável preservado.');
NODE
npx eslint --max-warnings 0 src/components/terminal/HybridTerminal.vue
npm test -- tests/terminal/HybridTerminal.spec.ts
npm run typecheck:vue
git diff --check
```

**Critério conclusivo de aceite:** os comandos aplicáveis passam, o diff de implementação contém somente comentários no componente, e a revisão semântica confronta cada prop/evento/slot com a tabela de evidências. O comparador comprova preservação de tokens executáveis e presença do inventário; não substitui a leitura técnica do conteúdo documental. Verificar também no diff que atributos dos blocos SFC continuam iguais. Nesta etapa de planejamento, esses testes de implementação não foram executados.

### Skills Aplicáveis

- `vue-components`: preservar a estrutura Composition API/SFC e descrever corretamente props, eventos e slots durante a implementação documental.
- `vue-typescript-best-practices`: conferir opcionalidade, payloads e referências aos tipos canônicos sem alterar contratos.
- `vue-eslint-stylelint-quality-standards`: manter formato e ordem dos blocos, recuo e comentários compatíveis com o projeto.
- `code-review`: revisar precisão semântica e ausência de alterações funcionais antes de integrar a mudança.

Usar somente orientações dessas skills compatíveis com este repositório; não aplicar convenções específicas de outros produtos nem criar infraestrutura adicional para uma correção documental.
