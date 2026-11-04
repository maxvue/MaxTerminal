# MaxComponentsUi — atributos acessíveis de botões não chegam ao alvo esperado

- Data: 03/11/2026. Origem: auditoria estática do MaxTerminal na worktree `wt-897572b7`.
- Biblioteca local observada: `@maxvue/max-components-ui` versão `1.1.36`.
- Estado: achado documentado; biblioteca não modificada; reprodução visual/leitor de tela pendente.

## Localização e evidência

- `MaxComponentsUi/src/types/index.ts:66-78` declara `aria-label`, `ariaLabel`, `title` e `tooltip` como props do contrato de botões.
- `MaxComponentsUi/src/components/MaxButton.vue:2-7`: no ramo com label ou slot padrão, o `<button>` recebe type/classes/disabled/click, mas não vincula essas props ao elemento. O ramo alternativo delega para MaxIconButton.
- `MaxComponentsUi/src/components/MaxIconButton.vue:84-91` usa title/tooltip como alternativas para calcular o nome acessível. Seu template (`:2-22`) não vincula title explicitamente; como é prop declarada, não se deve presumir tooltip HTML nativo por fallthrough.
- Consumidores no MaxTerminal que dependem de nome via prop e usam slot de ícone incluem `src/components/shell/AppTitleBar.vue:7`, `src/components/projects/ProjectList.vue:6,60` e `src/components/projects/ProjectWorkspace.vue:17,30`.

## Impacto

No ramo textual/com slot de MaxButton, um controle só de ícone pode ficar sem o nome acessível solicitado. A declaração tipada pode induzir consumidores a acreditar que o atributo chega ao botão. `title` de MaxIconButton não comprova tooltip nativo, embora possa fornecer nome acessível.

É uma lacuna na implementação da biblioteca, não uma prop inventada no aplicativo. A observação é estática; não se afirma resultado de teste de acessibilidade que não foi executado.

## Encaminhamento sem execução

1. Manter a correção da biblioteca fora desta demanda; tratar em trabalho próprio no repositório MaxComponentsUi, após autorização.
2. Nesse trabalho, revisar encaminhamento das props declaradas em ambos os ramos e adicionar testes de atributos/nome acessível no DOM real, preservando a API existente.
3. No plano atual do MaxTerminal, ações só de ícone podem usar MaxIconButton com aria-label; slots complexos podem fornecer texto acessível real. Validar o nome final sem depender apenas de stubs.

Referência: `docs/plans/wt-897572b7/plan.md`, achado M10 e contratos C1/C4. Nenhum dado sensível foi reproduzido.
