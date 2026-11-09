# Plano de execução — Issue #24

### Descrição e Causa Raiz

**Veracidade: confirmada.** O exemplo do contrato compartilhado em `implementation/README.md:L157-161` declara `TabStatus` com `finished` e `error`, enquanto o contrato implementado usa `completed_unseen` e `error_unseen`. A falha é documental: consumidores que copiem a união do roadmap rejeitarão valores válidos ou deixarão de reconhecer conclusões reais. Não foi constatada falha de execução na aplicação atual.

**Causa Raiz Comprovada:** a declaração manual em `implementation/README.md:L159` está desatualizada em relação ao enum `TabStatus` de `src-tauri/crates/mt-core/src/shell.rs:L397-412`. O enum contém `CompletedUnseen` e `ErrorUnseen` e possui `#[serde(rename_all = "snake_case")]`. O binding gerado confirma os cinco literais em `src/types/bindings.ts:L12987-12998`. O comentário em `implementation/README.md:L160` agrava a ambiguidade ao tratar `finished/error` como estados do contrato e mencionar `seen` sem identificar seu proprietário.

Há uma distinção importante: `finished` e `error` são valores legítimos de **TabBadgeBucket**, definido em `src/stores/tabStatus.ts:L41-63`. A store converte `completed_unseen → finished` e `error_unseen → error` para apresentação. Isso não os torna valores serializados de `TabStatus`; uma substituição global quebraria essa separação. A semelhança evidencia a ambiguidade, mas não comprova a origem histórica do erro editorial.

**Rastreamento reverso UI ⇄ Store ⇄ IPC ⇄ Service ⇄ estado/DB:**

1. UI: `src/components/shell/TabStatusIndicator.vue:L12-53` importa o binding, reconhece os literais `*_unseen`, exibe “Finalizado”/“Com Erro” e projeta conclusões vistas como “Parado”.
2. Store: `src/stores/tabStatus.ts:L127-129` devolve o status recebido; `L218-245` armazena snapshots por revisão; `L49-63` faz a conversão explícita para buckets visuais.
3. IPC: `src/ipc/tabState.ts:L14-27` usa os comandos tipados de consulta e confirmação de visualização; `L43-49` recebe `tab://state-changed` sem renomear o status.
4. Comandos nativos: `src-tauri/src/commands/tab_state.rs:L13-42` delega consultas e `tab_mark_seen` ao `ShellService`. Não há controller HTTP ou rota REST nesse fluxo.
5. Service/evento: `src-tauri/crates/mt-shell/src/service.rs:L270-299` consulta o registro e publica a confirmação; `src-tauri/src/events/shell.rs:L15-19,L74-86` transmite o snapshot transparentemente.
6. Origem do valor: `src-tauri/crates/mt-shell/src/state/machine.rs:L325-361` calcula conclusão e visibilidade, produzindo `CompletedUnseen`/`ErrorUnseen` quando a conclusão não foi vista; `L366-380` marca a conclusão vista e retorna o painel a `Idle` quando aplicável. O DTO carrega `status: TabStatus` e `last_completion` em `src-tauri/crates/mt-core/src/shell.rs:L506-525`; `seen` pertence a `CompletionSnapshot` (`L440-464`) e também a `AttentionSnapshot` (`L466-478`), não ao enum.
7. Limite com DB: o estado consultado nesse caminho reside no registro em memória `TabStateRegistry`, comprovado em `src-tauri/crates/mt-shell/src/state/registry.rs:L1-39` e nas consultas do service acima. Não há transformação SQL responsável pelos literais divergentes nem necessidade de alterar persistência para esta correção documental.

**Reprodução realizada nesta etapa:** comparação estática automatizada dos conjuntos de literais do README, do binding e das variantes Rust convertidas para snake_case. Binding e enum coincidiram. O comando terminou com código 1 na comparação do README, com `exclusivos do README=['error', 'finished']; ausentes no README=['completed_unseen', 'error_unseen']`. Essa falha esperada comprova a divergência sem depender de iniciar Tauri ou executar comandos em PTY.

### Arquivos afetados

- `implementation/README.md`: único arquivo a modificar na implementação; corrigir a seção 2.4 e sua explicação.
- `docs/issues/24/plan.md`: artefato desta etapa de planejamento, preenchido na worktree já dedicada à issue.

Os arquivos de código, bindings e testes citados são referências para verificação. A correção planejada não exige modificá-los nem regenerar os bindings.

### Execuções propostas

1. Na worktree dedicada, confirmar que a seção 2.4 e o contrato canônico ainda correspondem às evidências acima; preservar alterações de outras tarefas.
2. Corrigir somente a união documental para:

   ```ts
   type TabStatus = 'running' | 'continuous' | 'completed_unseen' | 'error_unseen' | 'idle';
   ```

3. Substituir o comentário antigo por uma explicação em pt-BR: `completed_unseen` representa conclusão bem-sucedida ainda não visualizada; `error_unseen` representa erro ou atenção pendente ainda não visualizada. Referenciar `lastCompletion.seen` e `attention.seen` nos snapshots, sem inventar uma propriedade `seen` no enum.
4. Explicar que a confirmação de visualização ocorre pelo fluxo `tab_mark_seen`, com recálculo do estado. Evitar prometer que a aba agregada sempre se torna `idle`: outros painéis em execução ou alertas pendentes podem determinar seu estado.
5. Acrescentar links relativos para a fonte Rust (`../src-tauri/crates/mt-core/src/shell.rs`) e o binding gerado (`../src/types/bindings.ts`), reforçando que o trecho é uma representação documental e que consumidores devem importar o contrato gerado. Distinguir brevemente os buckets visuais `finished/error`, com referência a `../src/stores/tabStatus.ts`.
6. Concluir todo esse bloco documental antes de validar. Não alterar os nomes do enum, adicionar aliases de serialização ou substituir ocorrências globais de `finished/error`.
7. Executar a validação em lote indicada abaixo, corrigir em conjunto eventuais problemas da documentação e repetir apenas as verificações afetadas. Registrar resultados reais antes de solicitar integração conforme as regras do projeto.

### Testes de regressão e validação em lote

- Verificação determinística principal: executar o script da seção **Validação** após a edição. Ele compara todos os cinco valores entre README, binding e enum e falha se o trecho estiver ausente, divergir ou mantiver literais antigos.
- Validar os três links relativos adicionados e executar `git diff --check`.
- Cobertura existente de referência: `src/components/shell/__tests__/TabStatusIndicator.spec.ts:L7-36` verifica os cinco estados e a projeção de conclusões vistas; `src/stores/__tests__/tabStatus.spec.ts:L120-149,L198-224` cobre contadores/reconciliação e confirmação de visualização. Como a alteração é exclusivamente documental e reversível, não criar testes permanentes que apenas espelhem o exemplo e não exigir build/typecheck ou suíte de runtime para provar a edição.
- Se a execução exceder o escopo documental, reavaliar o plano antes de mudar contratos; os testes existentes podem ser executados conjuntamente por `npm test -- src/stores/__tests__/tabStatus.spec.ts src/components/shell/__tests__/TabStatusIndicator.spec.ts`, mas não substituem a comparação documental.
- A prevenção de recorrência combina fonte canônica vinculada, importação dos tipos gerados pelos consumidores e o script repetível abaixo. Não se propõe nova infraestrutura de CI para esta correção pontual.

### Banco de dados

Nenhuma migration. A mudança corrige a descrição de valores já implementados e não altera schema, dados históricos ou persistência.

### Riscos de quebra e Não-Regressão

- **Confundir contrato e apresentação:** renomear `TabBadgeBucket` quebraria contadores e consumidores legítimos. Limitar o diff à seção documental e manter a distinção explícita.
- **Alterar compatibilidade IPC por engano:** renomear variantes ou editar bindings manualmente mudaria o protocolo. Comparar os conjuntos e revisar o diff para garantir que a correção siga o contrato existente.
- **Simplificar incorretamente a visualização:** a agregação de múltiplos painéis pode manter a aba ativa após marcar uma conclusão vista. Descrever recálculo, não uma transição universal para `idle`.
- **Confundir enums diferentes:** `CompletionOutcome::Error` é válido e independente de `TabStatus`; o script restringe a comparação ao tipo correto.
- **Links inválidos ou exemplos novamente divergentes:** validar os destinos e repetir a comparação durante futuras alterações de `TabStatus`.

### Validação

Executar na raiz da worktree após implementar o bloco autorizado. O script é somente leitura e exige Python 3; não instala dependências nem altera arquivos.

```bash
python3 - <<'PY'
import re
from pathlib import Path

def values(text):
    return set(re.findall(r"['\"]([a-z_]+)['\"]", text))

readme = Path('implementation/README.md').read_text()
bindings = Path('src/types/bindings.ts').read_text()
rust = Path('src-tauri/crates/mt-core/src/shell.rs').read_text()
section = re.search(r'### 2\.4\.[\s\S]*?(?=### 2\.5\.)', readme)
assert section, 'Seção documental de estados não encontrada'
doc_match = re.search(r'type TabStatus\s*=\s*([^;]+);', section.group())
ts_match = re.search(r'export type TabStatus\s*=\s*([^;]+);', bindings)
enum_match = re.search(r'#\[serde\(rename_all = "snake_case"\)\]\s*pub enum TabStatus\s*\{([^}]+)\}', rust)
assert doc_match and ts_match and enum_match, 'Contrato não encontrado'
documented = values(doc_match.group(1))
generated = values(re.sub(r'/\*.*?\*/', '', ts_match.group(1), flags=re.S))
variants = re.findall(r'^\s*([A-Z][A-Za-z0-9]*)\s*,', enum_match.group(1), re.M)
serialized = {re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower() for name in variants}
assert len(documented) == 5, documented
assert documented == generated == serialized, (documented, generated, serialized)
assert not ({'finished', 'error'} & documented), documented
for target in (
    '../src-tauri/crates/mt-core/src/shell.rs',
    '../src/types/bindings.ts',
    '../src/stores/tabStatus.ts',
):
    assert f']({target})' in section.group(), f'Link ausente: {target}'
    assert (Path('implementation') / target).is_file(), f'Destino ausente: {target}'
print('Contrato documental TabStatus alinhado ao Rust e aos bindings; links válidos.')
PY
git diff --check
```

**Critério de aceite:** ambos os comandos retornam código zero; a seção documenta exatamente os cinco estados canônicos, esclarece visualização e buckets, e o diff de implementação fica restrito ao README. Nesta etapa foi executada somente a reprodução estática da divergência; a correção e suas validações ficam para a execução do plano.

### Skills Aplicáveis

- `code-review`: revisar o diff documental e sua aderência ao contrato antes da integração.
- `vue-pinia-state-management-best-practices`: referência caso seja necessário aprofundar a distinção entre os valores recebidos pela store e os buckets visuais; não autoriza refatorar a store nesta issue.
- `vue-debugging-best-practices`: referência condicional caso surja dúvida sobre a projeção visual de conclusões vistas; o defeito comprovado aqui é documental.

Não há necessidade de orquestração de agentes, criação de novos tipos ou alteração de componentes para implementar este plano.
