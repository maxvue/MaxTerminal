# MaxComponentsUi — divergência de peers e dependências compartilhadas

- Data: 03/11/2026. Origem: investigação por leitura na worktree `wt-897572b7` do MaxTerminal.
- Estado: incompatibilidade declarada confirmada; efeito em runtime não testado. Nenhuma instalação ou atualização realizada.

## Localização e evidência

- `MaxTerminal/package.json:46-50`: aplicativo declara Axios `1.7.9`, Pinia `3.0.1` e Vue Router `4.5.0`; os manifestos resolvidos em node_modules confirmaram essas versões.
- `MaxComponentsUi/package.json:976-981`, versão `1.1.36`: peers obrigatórios incluem Pinia `^4.0.0` e Vue Router `^5.2.0`. As versões do aplicativo estão fora dessas faixas.
- O manifesto da biblioteca também declara Axios `^1.20.0` em dependencies. A diferença em relação ao aplicativo não prova, isoladamente, uma segunda instância carregada ou uma falha; a identidade efetiva deve ser conferida no harness.
- `node_modules` desta worktree é link para o node_modules da árvore principal do MaxTerminal. Os pacotes MaxComponentsUi, MaxUse e MaxPinia são links para repositórios irmãos, compartilhados.
- `scripts/sync-local-libs.mjs:235-259` verifica presença dos peers obrigatórios, não se suas versões satisfazem as faixas. Um resultado positivo nesse check não comprova compatibilidade de versões.
- `vite.config.ts:45-50` e `vitest.config.ts:11-17` deduplicam Vue/Pinia; essa configuração não substitui a verificação do contrato das versões.

## Impacto e limites

Migração ampla de componentes depende de confirmar compatibilidade da biblioteca efetiva e identidade de estado/HTTP nos testes. Uma instalação, sincronização, rebuild ou limpeza no destino compartilhado pode afetar outras sessões. A divergência é risco concreto, mas não permite afirmar que o aplicativo já falha em execução.

## Encaminhamento sem execução

1. Preservar o gate por leitura/integração previsto em T0 do plano. Se não for possível comprovar compatibilidade e isolamento, bloquear a validação dependente com próxima ação concreta.
2. Não instalar/atualizar pacotes nem modificar libs ou links compartilhados como parte automática da auditoria de componentes.
3. Resolver alinhamento de versões e eventual melhoria de verificação semântica em demanda própria, autorizada e isolada; não contornar o gate usando comandos diretos ou desativando configuração/hooks.

Referência: `docs/plans/wt-897572b7/plan.md`, seções 3, T0 e riscos. Nenhum segredo ou dado pessoal foi reproduzido.
