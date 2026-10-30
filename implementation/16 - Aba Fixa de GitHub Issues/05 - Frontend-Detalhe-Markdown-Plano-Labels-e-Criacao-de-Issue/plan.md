# Plano de Execução — Implementação 05: Frontend Detalhe da Issue, Markdown Seguro, Plano de Implementação, Editor de Labels e Criação de Issue

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `05 - Frontend-Detalhe-Markdown-Plano-Labels-e-Criacao-de-Issue`
> **Especificação Técnica:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Implementar `useMarkdownRenderer.ts` e `useIssuePlan.ts`
1. Verificar/instalar `markdown-it`, `dompurify`, `@types/markdown-it` e `@types/dompurify` em `package.json`.
2. Criar `src/composables/useMarkdownRenderer.ts`:
   - Configurar `MarkdownIt({ html: false, linkify: true, breaks: true })`.
   - Configurar `DOMPurify` removendo tags perigosas (`<script>`, `<iframe>`, `<style>`, `<object>`, `<embed>`, `<form>`), atributos de evento (`on*=`) e esquemas de URL inseguros (`javascript:`, `data:`, `vbscript:`, `file:`, `vscode:`, `maxterminal:`).
   - Garantir `rel="noopener noreferrer"` e `target="_blank"` em todos os links `http://` e `https://` e interceptar o clique para abrir no navegador externo do SO.
3. Criar `src/composables/useIssuePlan.ts` expondo metadados computados a partir de `IssueDetail.plan` e helpers de rolagem para `#issue-plan-section` e `#issue-comment-<id>`.

### Passo 2: Implementar `IssueLabelChip.vue` e `IssueLabelEditor.vue`
1. Criar `src/components/github/IssueLabelChip.vue`:
   - Renderizar a cor hexadecimal da label com cálculo de luminância para contraste legível, ícone discreto para `LabelKind` (`State`, `Closure`, `Category`) e botão opcional de remover com `aria-label` em `pt-BR`.
2. Criar `src/components/github/IssueLabelEditor.vue`:
   - Agrupar as 24 labels canônicas em `Estado (15)`, `Encerramento (3)` e `Categoria (6)`, seguidas de `Outras labels do repositório`.
   - Computar diff incremental `{ add, remove }` em relação às labels atuais da issue.
   - Exibir aviso não-bloqueante quando a combinação de estado selecionada divergir das transições sugeridas no catálogo.
   - Exigir confirmação adicional quando houver um ciclo de bugs ativo no repositório e o usuário alterar labels de workflow.
   - Chamar `store.patchLabels(...)` com `expectedUpdatedAtMs` e tratar resultados `Confirmed`, `Partial` e `Unknown`.

### Passo 3: Implementar `IssuePlanSection.vue`, `IssueComments.vue` e `IssueDetail.vue`
1. Criar `src/components/github/IssuePlanSection.vue`:
   - Renderizar o conteúdo Markdown de `IssuePlan` usando `useMarkdownRenderer`.
   - Exibir badge de origem (`"Extraído da descrição"` ou `"Extraído do comentário #..."`) e alerta quando `plan.provisional === true`.
2. Criar `src/components/github/IssueComments.vue`:
   - Renderizar a lista de `IssueComment` com autor, timestamp formatado em `pt-BR`, badge `"Plano ativo"` no comentário correspondente a `plan.sourceCommentId` e botão `"Carregar mais comentários"` quando `commentsComplete === false`.
3. Criar `src/components/github/IssueDetail.vue`:
   - Renderizar cabeçalho (`#number`, título sanitizado, badge `Aberta`/`Fechada`, botão `"Ir para o plano"` quando `detail.plan` existir, slot de ações `IssueActionsMenu` e `IssueLabelEditor`).
   - Renderizar corpo principal, `IssuePlanSection` e `IssueComments`.

### Passo 4: Implementar `IssueCreateModal.vue` e Integrar em `IssuesTab.vue`
1. Criar `src/components/github/IssueCreateModal.vue`:
   - Validar título (1..256 chars) e descrição (≤ 64 KiB) antes de habilitar envio.
   - Manter `requestId` estável por submissão para evitar duplicidade.
   - Tratar `MutationOutcome::Confirmed` (atualizando lista/contador e oferecendo atalho `"Ver issue criada"` caso filtrada), `MutationOutcome::Partial` (informando labels omitidas pelo servidor) e `MutationOutcome::Unknown` (orientando reconciliação sem reenviar POST cego).
2. Atualizar `src/components/github/IssuesTab.vue` para abrir `IssueCreateModal.vue` a partir do botão `"Nova Issue"` e exibir `IssueDetail.vue` no painel direito quando uma issue estiver selecionada.

### Passo 5: Escrever Testes Vitest (`detail-plan-and-mutations.spec.ts`)
1. Criar `tests/frontend/github/detail-plan-and-mutations.spec.ts`:
   - Comprovar que `useMarkdownRenderer` remove `<script>alert(1)</script>`, `<img src=x onerror=alert(1)>`, `[link](javascript:alert(1))` e `[link](data:text/html,...)`, mantendo `rel="noopener noreferrer"` em links `https://`.
   - Comprovar que `IssuePlanSection` destaca o último `## PLANO DE IMPLEMENTAÇÃO`, exibe âncora `"Ir para o plano"` e sinaliza `provisional: true` quando comentários estão incompletos.
   - Comprovar que `IssueLabelEditor` envia apenas `{ add: ['Waiting Execute'], remove: [] }` (preservando `Planned` e labels de categoria) e exibe aviso não-bloqueante em transição fora do padrão.
   - Comprovar que `IssueCreateModal` insere a nova issue sem aguardar o TTL de 120s e informa resultado parcial quando o servidor omite labels.

---

## 2. Comandos de Verificação e Validação

```bash
cd "$(git rev-parse --show-toplevel)"
# 1. Executar typecheck estrito e lint/stylelint
npm run typecheck
npm run lint

# 2. Executar testes unitários da Implementação 05 no Vitest
npx --no-install vitest run tests/frontend/github/detail-plan-and-mutations.spec.ts
```

---

## 3. Critérios de Aceite Atendidos nesta Implementação

| Critério (Fase 16) | Como é atendido e verificado |
|--------------------|------------------------------|
| **CA-5 (Typecheck e Lint)** | `tsgo --noEmit`, ESLint e Stylelint sem erros |
| **CA-6 (`useMarkdownRenderer` e `IssuePlanSection`)** | Testados em `detail-plan-and-mutations.spec.ts` contra vetores XSS (`<script>`, `on*=`, `javascript:`) e destaque do último plano |
| **CA-11 (Renderização Markdown e âncora do Plano)** | `IssueDetail.vue` e `IssuePlanSection.vue` renderizam Markdown completo com âncora `"Ir para o plano"` |
| **CA-12 (Edição manual de labels e aviso não-bloqueante)** | `IssueLabelEditor.vue` aplica diff incremental (`add`/`remove`) e exibe aviso não-bloqueante quando aplicável |
| **CA-13 (Criação de issue imediata e resultado parcial)** | `IssueCreateModal.vue` insere a issue criada sem esperar o TTL e notifica quando labels são omitidas pelo servidor |
