# Divergência declarada de versões dos peers da biblioteca

Data: 04/11/2026. Achado fora do escopo de correção visual.

## Localização e evidência

- `MaxTerminal/package.json`: dependências `pinia: 3.0.1` e `vue-router: 4.5.0`.
- Pacote efetivo `node_modules/@maxvue/max-components-ui/package.json`, versão 1.1.36, resolve para `/home/johnattas/GitHub/MaxComponentsUi`: `peerDependencies.pinia: ^4.0.0` e `peerDependencies.vue-router: ^5.2.0`.

## Impacto e limites

Os intervalos declarados não incluem as versões da aplicação. Isso pode afetar instalação, resolução de tipos, identidade das instâncias e validação dos componentes. Não foram executadas instalação, checagem de integridade ou testes; não se afirma falha de runtime nem duplicação comprovada.

## Próxima ação

No preflight da execução, conferir manifesto, exports/dist e resolução efetiva de Pinia/router/Axios. Se bloquear os gates, interromper validações dependentes e solicitar tratamento separado da compatibilidade. Não atualizar pacotes, modificar bibliotecas compartilhadas ou contornar gates automaticamente. Nenhuma alteração de dependência foi realizada.
