# Cobertura incompleta de tokens de tema para consumidores externos

Data: 04/11/2026. Biblioteca lida: `@maxvue/max-components-ui` 1.1.36, symlink para `/home/johnattas/GitHub/MaxComponentsUi`.

## Evidência

`src/themes/tokens.scss:92–128,145–179` define cores literais para borda/fundo de campos, float labels, texto secundário/help e seleção. `src/themes/tokens.scss:130–141,181–187` usa também aliases legados para tabelas. O mapa do consumidor MaxTerminal em `src/theme/cssTokens.ts:77–115` atualiza escalas básicas e aliases locais, não todos esses papéis.

## Impacto

Trocar apenas a paleta semântica central não garante que controles, labels e tabelas acompanhem um esquema arbitrário. Alguns papéis podem continuar na paleta padrão ou perder contraste. A leitura não comprova todos os efeitos no artefato distribuído ou no navegador.

## Encaminhamento

O plano local prevê adaptação dos tokens públicos usados, preservando contratos da biblioteca. Revalidar `dist` e cascata na execução. Eventual correção de literais internos não substituíveis por contrato público exige tarefa/autorização própria na biblioteca; não editar symlink ou reintroduzir PrimeVue. Nenhuma biblioteca foi alterada nesta sessão.
