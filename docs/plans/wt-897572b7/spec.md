# Especificação técnica — auditoria e padronização MaxComponentsUi

## 1. Contexto & Objetivo

- Projeto: MaxTerminal. Data: 03/11/2026.
- Worktree exclusiva: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-897572b7`, branch `wt-897572b7`.
- Base investigada: `c76d16f2ef3ecc4ef87d4c6bdb7d45d40203dd1b`; árvore inicialmente sem alterações.
- Solicitação: localizar elementos nativos/componentes locais que tenham substitutos adequados em MaxComponentsUi e corrigir usos incorretos dos componentes Max existentes. Esta sessão entrega somente documentação; a implementação pertence a outra sessão.
- A auditoria cobriu 117 SFCs: 47 com controles nativos, 26 com oportunidades de composição e 44 sem nova substituição adequada identificada. O plano detalha dez grupos de correção de contratos Max e as exceções funcionais.
- Valor: uniformidade visual e contratos corretos sem perder acessibilidade, atalhos, comportamento do terminal, salvamentos ou isolamento por projeto.

## 2. Escopo

### Incluído

1. Inventário de todos os SFCs em `src`, controles nativos, ícones e primitivas locais, distinguindo substituição adequada de exceção funcional.
2. Conferência dos usos Max com exports, props, eventos, slots, modelos e métodos realmente disponíveis na biblioteca local efetiva.
3. Migração futura dos candidatos confirmados e correção dos contratos inválidos; ajustes de estilos e testes relacionados.
4. Testes de regressão de eventos reais, atributos no elemento interativo, submit único, bloqueio de ações, foco e navegação por teclado.
5. Proteção arquitetural contra reintrodução dos problemas, com exceções específicas e justificadas.

### Fora do escopo

- Implementar nesta sessão; modificar Rust, SQLite, esquemas, IPC, serviços externos ou dados reais.
- Atualizar dependências, reconstruir/editar bibliotecas locais compartilhadas ou instalar pacotes automaticamente.
- Migrar globalmente stores IPC para transporte HTTP, introduzir autosave em formulários com confirmação explícita ou duplicar cache de dados sensíveis.
- Redesenhar a experiência, substituir xterm/splitpanes ou eliminar componentes locais que representam regras de negócio.
- Criar outra worktree, trocar branch, commitar, integrar, publicar ou remover esta worktree.

## 3. Arquitetura & Mudanças Técnicas

### Fontes e restrições

- Aplicativo: Vue `3.6.0-rc.13`, Pinia `3.0.1`, Vue Router `4.5.0`, Axios `1.7.9`; scripts e dependências em `package.json`.
- Bibliotecas efetivas: MaxComponentsUi `1.1.36`, MaxUse `2.0.1`, MaxPinia `0.2.0`, ligadas a `/home/johnattas/GitHub/{MaxComponentsUi,MaxUse,MaxPinia}`.
- `node_modules` aponta para a árvore principal compartilhada. Escrever nele pode atingir outras sessões.
- A biblioteca MaxComponentsUi declara peers Pinia `^4.0.0` e Vue Router `^5.2.0`, diferentes dos efetivamente presentes no aplicativo. Isso é incompatibilidade declarada, não prova de falha observada. Gate de compatibilidade precede validações dependentes; eventual atualização exige decisão separada.
- A divergência de peers, a lacuna de atributos acessíveis de botões e o risco de encaminhamento duplicado de id em checkbox foram registrados, sem alterar bibliotecas, em `docs/notifications/wt-897572b7/notifications/max-components-ui-peer-compatibility.md`, `max-components-ui-button-accessible-attributes.md` e `max-components-ui-checkbox-attribute-forwarding.md` no mesmo diretório. Os limites de evidência estática e o encaminhamento fora desta demanda constam de cada notificação.
- `vite.config.ts` não configura auto-import; preservar importações explícitas e o registro existente. Não presumir auto-resolução a partir de exemplos genéricos de skills.
- Usar MaxComponentsUi nas primitivas substituíveis; MaxPinia na integração de estado e MaxUse para helpers novos/alterados. Manter stores, ações e transporte existentes; não inventar rotas HTTP para operações Tauri. APIs devem ser conferidas no código e no `dist` efetivo, não apenas no catálogo de referência.

### Fluxo preservado

```text
Interação → componente Max / composição local de domínio
          → eventos/modelo existentes → store existente + integração MaxPinia
          → IPC Tauri → backend e persistência existentes

Helpers de interface novos/alterados → MaxUse
```

Não há novos contratos backend. Conversões de valores devem ficar nas fronteiras da interface e preservar tipos, nulabilidade, payloads e identidade do projeto. A troca de elemento não autoriza duplicar handlers ou chamadas de salvamento.

### Áreas afetadas

Os caminhos e achados específicos estão no inventário de `plan.md`. Mudanças de produção ficam restritas aos SFCs ali identificados e a seus estilos; bootstrap/stores somente se necessário para corrigir um contrato confirmado da integração. Testes existentes em `tests/components`, `tests/frontend`, `tests/terminal`, `tests/app` e `tests/lint` devem acompanhar seus respectivos fluxos. Arquivos de teste novos serão explicitamente propostos no plano.

Elementos estruturais sem equivalente funcional, superfícies especializadas do terminal e composições de domínio permanecem locais. Uma exceção é válida por requisitos demonstráveis, não apenas porque já existe em um teste antigo.

## 4. Plano de Execução Faseado

1. Revalidar revisão, alterações locais, bibliotecas efetivas e limites de escrita. Realizar preflight por leitura antes de alterar código.
2. Estabilizar contratos compartilhados de botões, inputs, seletores, ícones e modais; classificar cada achado como correção, substituição ou preservação.
3. Corrigir contratos inválidos e substituir primitivas por grupos de arquivos sem sobreposição de escrita. Implementar os testes junto de todo o escopo antes de rodar a bateria completa.
4. Atualizar auditoria arquitetural e testes de integração com componentes reais, sem afrouxar expectativas para ocultar falhas.
5. Fazer revisão independente nos marcos de risco e entrega; executar validações agrupadas, corrigir falhas em lote e revalidar a revisão final.
6. Realizar aceite manual seguro no ambiente desktop, quando disponível e autorizado. Pendências permanecem explícitas.

## 5. Critérios de Aceite & Validação

- Todo candidato inventariado tem destino ou justificativa funcional; nenhuma substituição é puramente nominal.
- Nenhum uso incorreto confirmado permanece; props, modelos, eventos, slots e métodos respeitam a biblioteca efetiva.
- Ícones, rótulos acessíveis, foco, estados disabled/loading, submit e ações por teclado funcionam no elemento real; uma interação gera no máximo uma ação esperada.
- Os indicadores AiStatusIndicator e BridgeStatusIndicator mantêm nome acessível e tooltip nos modos normal e compacto; não depender apenas de props aria-label/title que MaxButton consome sem encaminhar. Os testes específicos e o aceite correspondente estão no plano.
- Terminal mantém IME, seleção, colagem, atalhos, histórico e envio ao PTY; abas mantêm seleção, fechamento e arraste quando aplicável.
- Dados continuam passando pelas stores e contratos existentes, com MaxPinia integrado e helpers por MaxUse; nenhuma persistência paralela é introduzida.
- A bateria canônica identificada é `npm run check`: bibliotecas locais, ESLint, Stylelint, tipos TS/SFC, cobertura, build e verificações Rust. Executar somente em Execute, após comprovar isolamento e segurança de todos os passos. Gate falhado impede continuar por comandos alternativos que o contornem.
- Testes isolados não comprovam comportamento nativo: aceite desktop pendente significa `manualPending`, não conclusão integral.
- Nesta sessão não foram executados testes, builds ou scripts de ambiente. Validações descritas são futuras, salvo leituras e conferências documentadas no plano.
