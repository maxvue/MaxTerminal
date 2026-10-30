# Especificação Técnica — Implementação 05: Frontend Detalhe da Issue, Markdown Seguro, Plano de Implementação, Editor de Labels e Criação de Issue

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `05 - Frontend-Detalhe-Markdown-Plano-Labels-e-Criacao-de-Issue`
> **Referências Canônicas:** [`../../AGENTS.md`](../../../AGENTS.md) §3 e §7, [`../instructions.md`](../instructions.md) §2.1 e §5.1–§5.2, [`../spec.md`](../spec.md) §10.3, §10.4 e §11.

---

## 1. Contexto, Objetivo e Escopo

Esta quinta implementação entrega toda a experiência de inspeção detalhada e gestão local de issues (visualização segura de Markdown, destaque do `## PLANO DE IMPLEMENTAÇÃO`, comentários paginados, edição manual de labels e criação de novas issues):

1. **Renderização Segura de Markdown (`src/composables/useMarkdownRenderer.ts`):** Pipeline baseado em `markdown-it` (`html: false`, `linkify: true`) combinado com `DOMPurify`, bloqueando estritamente `<script>`, atributos `on*=`, URLs `javascript:`, `data:`, `file:`, `vscode://` ou `maxterminal://` oriundas de conteúdo de terceiros, reescrevendo links externos `http:`/`https:` com `rel="noopener noreferrer"` para abertura no navegador do sistema e bloqueando carregamento automático de imagens remotas rastreadoras por padrão.
2. **Localização e Destaque do `## PLANO DE IMPLEMENTAÇÃO` (`src/composables/useIssuePlan.ts`, `src/components/github/IssuePlanSection.vue`):** Consumo direto do DTO `IssuePlan` extraído pelo backend Rust (`plan.rs`), sem reimplementar heurística divergente no frontend; renderização em card de destaque com origem (corpo da issue ou comentário específico + timestamp), botão de âncora `"Ir para o plano"` e badge de aviso quando `provisional === true` (páginas de comentários ainda não carregadas integralmente).
3. **Detalhe da Issue e Comentários Paginados (`src/components/github/IssueDetail.vue`, `src/components/github/IssueComments.vue`):** Exibição do cabeçalho completo (`#number`, título, estado, autor, data de atualização, link externo para o GitHub), corpo da issue renderizado, seção do plano em destaque e lista cronológica de comentários com paginação incremental (`commentsCursor` / `"Carregar mais comentários"`).
4. **Edição Manual Soberana de Labels (`src/components/github/IssueLabelChip.vue`, `src/components/github/IssueLabelEditor.vue`):** Exibição diferenciada para labels canônicas do workflow MaxCode (`State`, `Closure`, `Category`) vs. labels externas do repositório, cálculo de diff incremental (`add` / `remove`, nunca substituição total), preview de transição exibindo aviso não-bloqueante quando uma combinação de estado foge do grafo habitual, confirmação extra quando há um ciclo de bugs ativo no repositório e tratamento de resultado parcial (`MutationOutcome::Partial`) ou conflito (`expectedUpdatedAtMs`).
5. **Criação Local de Nova Issue (`src/components/github/IssueCreateModal.vue`):** Modal com validação de limites (`title` obrigatório até 256 caracteres, `body` até 64 KiB, até 100 labels), geração de `requestId` estável por intenção de submissão (impedindo duplicação em duplo clique ou perda de resposta `MutationOutcome::Unknown`), inserção imediata no estado local respeitando a ordenação/filtros ativos (com botão `"Ver issue criada"` quando os filtros atuais a ocultarem) e aviso claro caso o GitHub omita labels por falta de permissão de escrita no repositório.

---

## 2. Arquivos a Criar ou Modificar

| Caminho | Ação | Descrição |
|---------|------|-----------|
| `package.json` | Modificar | Garantir dependências `markdown-it`, `dompurify`, `@types/markdown-it` e `@types/dompurify` |
| `src/composables/useMarkdownRenderer.ts` | Criar | Composable de renderização segura de Markdown (`markdown-it` + `DOMPurify`) e interceptador de clique em links externos |
| `src/composables/useIssuePlan.ts` | Criar | Composable para navegação/scroll até a âncora do `IssuePlan` e metadados de origem/provisoriedade |
| `src/components/github/IssueLabelChip.vue` | Criar | Componente visual de chip de label com contraste automático de cor, ícone semântico por `LabelKind` e botão opcional de remoção |
| `src/components/github/IssueLabelEditor.vue` | Criar | Popover/editor de labels com busca, separação por grupos (`Estado`, `Encerramento`, `Categoria`, `Outras`), preview de avisos e diff `add`/`remove` |
| `src/components/github/IssuePlanSection.vue` | Criar | Seção destacada do `## PLANO DE IMPLEMENTAÇÃO` com âncora, indicador `provisional` e botão de expandir/recolher |
| `src/components/github/IssueComments.vue` | Criar | Lista de comentários sanitizados com destaque no comentário que originou o plano ativo e botão `"Carregar mais comentários"` |
| `src/components/github/IssueDetail.vue` | Criar | Painel de detalhe completo da issue selecionada integrando cabeçalho, `IssueLabelEditor`, corpo Markdown, `IssuePlanSection` e `IssueComments` |
| `src/components/github/IssueCreateModal.vue` | Criar | Modal de criação de nova issue com validação, seletor de labels, `requestId` idempotente e reconciliação de estado `unknown`/`partial` |
| `src/components/github/IssuesTab.vue` | Modificar | Conectar `IssueDetail.vue` e `IssueCreateModal.vue` ao layout principal da aba de Issues |
| `tests/frontend/github/detail-plan-and-mutations.spec.ts` | Criar | Testes Vitest cobrindo sanitização XSS no `useMarkdownRenderer`, destaque do `IssuePlanSection`, diff incremental de labels com aviso de transição e criação de issue (incluindo resultado parcial e `unknown`) |

---

## 3. Contratos TypeScript e Regras de Componentes

### 3.1. Renderizador Seguro de Markdown (`src/composables/useMarkdownRenderer.ts`)

```ts
export interface MarkdownRenderOptions {
    allowRemoteImages?: boolean;
}

export interface UseMarkdownRendererReturn {
    renderMarkdown: (source: string, options?: MarkdownRenderOptions) => string;
    handleMarkdownClick: (event: MouseEvent) => void;
}

export function useMarkdownRenderer(): UseMarkdownRendererReturn;
```

Regras de Segurança em `useMarkdownRenderer`:
1. Instancia `MarkdownIt({ html: false, linkify: true, typographer: false, breaks: true })`.
2. Configura `DOMPurify.sanitize(rawHtml, { ALLOWED_TAGS, ALLOWED_ATTR, FORBID_TAGS: ['script', 'style', 'iframe', 'object', 'embed', 'form', 'input'], FORBID_ATTR: ['style', 'onerror', 'onload', 'onclick', 'onmouseover'] })`.
3. Hook `afterSanitizeAttributes` do `DOMPurify`:
   - Em tags `<a>`: valida `href` via `new URL(href, 'https://github.com')`. Permite apenas protocolos `http:`, `https:` e âncoras locais `#plan-...`. Qualquer `javascript:`, `data:`, `vbscript:`, `file:`, `vscode:` ou `maxterminal:` tem o atributo `href` removido. Adiciona obrigatoriamente `target="_blank"` e `rel="noopener noreferrer"`.
   - Em tags `<img>`: se `allowRemoteImages !== true`, substitui `src` remoto por um placeholder seguro clicável (`data-blocked-src`) para impedir tracking pixels e vazamento de IP/headers.
4. `handleMarkdownClick`: intercepta cliques em `<a href="https://...">`, faz `event.preventDefault()` e aciona o abridor seguro de URL externa do sistema operacional (Fase 09).

### 3.2. Seção do Plano de Implementação (`src/composables/useIssuePlan.ts` e `IssuePlanSection.vue`)

```ts
import type { ComputedRef, Ref } from 'vue';
import type { IssueDetail, IssuePlan } from '@/types/bindings';

export function useIssuePlan(detail: Ref<IssueDetail | null>) {
    const plan: ComputedRef<IssuePlan | null>;
    const isFromComment: ComputedRef<boolean>;
    const planAnchorId: ComputedRef<string>;
    function scrollToPlan(containerEl: HTMLElement | null): void;
    function scrollToSourceComment(containerEl: HTMLElement | null): void;
    return { plan, isFromComment, planAnchorId, scrollToPlan, scrollToSourceComment };
}
```

- Quando `detail.value?.plan` está presente:
  - No topo do `IssueDetail.vue`, exibe o botão de ação rápida `"Ir para o plano"` que rola suavemente até `#issue-plan-section`.
  - Se `plan.provisional === true`, exibe um alerta informativo em `pt-BR`: `"Plano provisório: ainda existem comentários mais recentes não carregados. Carregue todos os comentários para confirmar a versão final do plano."` com botão `"Carregar comentários restantes"`.

### 3.3. Editor Incremental de Labels (`src/components/github/IssueLabelEditor.vue`)

- Recebe `projectId: ProjectId`, `issue: IssueDetail`, `catalog: LabelCatalog`, `repoLabels: LabelInfo[]` e `hasActiveBugCycle: boolean`.
- Calcula `add: string[]` (labels marcadas que não estavam na issue) e `remove: string[]` (labels desmarcadas que estavam na issue). Garante que `add` e `remove` sejam disjuntos.
- **Avisos de Transição (Não-Bloqueantes):** Ao alterar labels de `LabelKind::State`, avalia as regras `catalog.transitions`. Se a combinação resultante não for uma transição padrão do workflow, exibe um alerta amarelo (`--max-warning-500`): `"A combinação selecionada foge da transição habitual do workflow. Como a edição manual é soberana, você pode aplicar mesmo assim."`
- **Proteção Durante Ciclo Ativo:** Se `hasActiveBugCycle === true` e o usuário estiver adicionando/removendo uma label `State` ou `Closure`, exige um passo extra de confirmação (`MaxPopoverConfirm`) alertando que o MaxCode está executando um ciclo ativo neste repositório.
- **Envio (`github_issue_labels_update`):** Passa `expectedUpdatedAtMs: issue.summary.updatedAtMs`. Se retornar `MutationOutcome::Partial`, exibe notificação listando `result.applied` e `result.failed` e atualiza a issue com o estado real retornado pelo servidor.

### 3.4. Modal de Criação de Issue (`src/components/github/IssueCreateModal.vue`)

- Campos do formulário:
  - `Título` (`MaxInput`, obrigatório, máx. 256 caracteres com contador).
  - `Descrição (Markdown)` (`MaxTextarea` com aba de Pré-visualização usando `useMarkdownRenderer`, máx. 64 KiB).
  - `Labels` (seletor multi-opção alimentado pelas labels do repositório + catálogo, máx. 100 labels).
- Gera `requestId` (`crypto.randomUUID()`) ao abrir o modal ou iniciar a intenção de criação, mantendo o **mesmo** `requestId` enquanto os dados não mudarem.
- Tratamento de respostas de `github_issue_create`:
  - `Confirmed`: fecha o modal, insere/atualiza a issue na store, atualiza o contador do badge 🟣 e, se a nova issue não passar no filtro atual da lista, exibe toast com ação `"Ver issue criada"` (que limpa o filtro ou abre o detalhe da issue recém-criada).
  - `Partial` (ex.: issue criada no GitHub, porém sem as labels porque o usuário não tem permissão de triagem no repositório): fecha o modal, seleciona a issue criada e exibe aviso informando que a issue `#<n>` foi criada mas as labels foram omitidas pelo GitHub por restrição de permissão.
  - `Unknown` (timeout/queda após envio): bloqueia reenvio automático com novo ID e exibe estado de reconciliação (`"Não foi possível confirmar se o GitHub concluiu a criação. Sincronize a lista antes de reenviar para evitar duplicidade."`).
