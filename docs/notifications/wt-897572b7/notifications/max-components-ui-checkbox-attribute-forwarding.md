# MaxComponentsUi — id encaminhado a dois elementos de MaxInputCheckbox

- Data: 03/11/2026. Biblioteca observada: MaxComponentsUi `1.1.36`.
- Estado: achado estático de contrato; sem reprodução em runtime ou alteração da biblioteca.

## Localização e evidência

Em `MaxComponentsUi/src/components/MaxInputCheckbox.vue:2-14`, o wrapper recebe `rootAttrs` e o input recebe `inputAttrs`. As duas projeções de `$attrs` (`:40-47`) mantêm o atributo `id` fornecido pelo consumidor. O input também declara `:id="id"`, sendo esse identificador gerado internamente (`:25`); o label interno aponta para o identificador gerado (`:14`).

## Impacto e limites

Quando um consumidor fornece `id`, o contrato estático encaminha esse valor tanto ao wrapper quanto ao input, criando risco de identificador duplicado; também pode divergir da associação do label interno. Não foi afirmada ocorrência em uma tela atual nem executado teste de DOM. A migração planejada de checkboxes deve evitar presumir que passar id tenha a mesma semântica do elemento nativo.

## Encaminhamento sem execução

Tratar em demanda própria da biblioteca: definir um identificador efetivo único do controle, excluir id do wrapper e alinhar o for do label, com testes usando o componente distribuído. No MaxTerminal, preferir label da biblioteca ou nome acessível explícito sem introduzir id duplicado; testar associação e ativação por rótulo. Não editar a biblioteca compartilhada nesta tarefa.
