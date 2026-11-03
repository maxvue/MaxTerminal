# Especificação técnica — decoração nativa e atualização por F5

## 1. Contexto & Objetivo

- Projeto: MaxTerminal. Data: 03/11/2026. Worktree: `wt-7c84dcfa`, branch homônima.
- Raiz exclusiva: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-7c84dcfa`.
- Solicitação: usar a barra de títulos do sistema com suas cores; se a integração correta não estiver disponível, usar barra escura quando o layout for escuro. Permitir F5 para atualizar a página interna.
- A janela já tem `decorations: true` em `src-tauri/tauri.conf.json:24`. `App.vue` não preenche o slot `titlebar` de `AppShell`: não há justificativa para criar outra barra em HTML.
- `useThemeApplier` atualmente força a polaridade do conteúdo na janela. O adaptador chama `Window.setTheme`, mas `capabilities/default.json` só concede `core:event:default` entre as permissões core. Há incompatibilidade estática entre chamada e autorização, não uma falha nativa reproduzida nesta sessão.
- F5 não tem tratamento explícito encontrado. O terminal processa teclas e pode entregar F5 ao processo, portanto depender do comportamento padrão do WebView não é suficiente.

## 2. Escopo

### Incluído

1. Manter decoração nativa, título, botões e ações do gerenciador de janelas.
2. Priorizar aparência do sistema, separando-a da polaridade do conteúdo. Aplicar fallback nativo escuro onde a integração de cores não for efetiva e o tema efetivo do conteúdo for escuro.
3. Autorizar apenas a operação nativa necessária e testar a allowlist.
4. Capturar F5 sem modificadores na raiz da interface, inclusive com foco em terminal, formulários e modais; atualizar somente o documento atual.
5. Testar política de tema, concorrência, atalho e recuperação da interface; verificar manualmente em Tauri a continuidade dos processos.

### Fora de escopo

- Barra de títulos customizada, novo seletor de decoração, alteração do tema do SO ou de variáveis globais GTK do usuário.
- Trocar dependências, reconstruir bibliotecas irmãs, migrar stores, mudar esquemas SQLite, criar nova sessão de terminal ou reiniciar o aplicativo por F5.
- Preservar rascunhos e buffers exclusivamente em memória após reload; adicionar hard reload, Ctrl+R ou novos atalhos globais.
- Commit, merge, push, limpeza da worktree e execução neste estágio de planejamento.

## 3. Arquitetura & Mudanças Técnicas

### Evidência e versões

- `@tauri-apps/api` e CLI: `2.5.0` no manifesto; crate Tauri: `2.11.6` no Cargo.lock.
- Contrato instalado `node_modules/@tauri-apps/api/window.d.ts:1121–1131`: `setTheme(theme?: Theme | null)`; `null`/`undefined` segue o sistema; em Linux/macOS o efeito é de aplicação, não só de janela. `setTitleBarStyle` é exclusivo de macOS e não resolve o alvo Linux.
- `core:window:allow-set-theme` existe em `src-tauri/gen/schemas/linux-schema.json`; não editar schemas gerados.
- Documentação oficial consultada via Context7: `/tauri-apps/tauri-docs`, guia `learn/window-customization.mdx` e migração das permissões core. Os tipos instalados são a evidência específica da API 2.5.0; os exemplos oficiais atuais não comprovam cores reais no desktop do usuário.
- Frontend já instala `createMaxPinia` em `src/app/plugins.ts`. Usar `useThemeStore` como fonte única de estado, sem GET direto ou persistência paralela. A store atual tem cache e IPC próprios: não convertê-la incidentalmente para outro modelo de cache.
- Consultados os contratos instalados de MaxComponentsUi 1.1.36 (`MaxButton`, já usado em App.vue) e MaxUse 2.0.1 (reexport de VueUse em `dist/Helpers/VueUse/core.d.ts`). Não criar componentes novos; usar `useEventListener` via MaxUse para o listener com descarte por escopo, após confirmar seu export efetivo no preflight.

### Política de decoração e limite de viabilidade

Preservar `decorations: true` e ausência de tema fixo na configuração. Separar a entrada `polarity: 'dark' | 'light'` do alvo de janela `NativeWindowThemeTarget = 'dark' | 'light' | null`. `null` significa seguir o sistema, não ausência de pedido.

Fluxo:

```text
bootstrap do DOM / tema efetivo da store
    → política de integração nativa verificada para a plataforma
    → alvo null (sistema) OU dark (fallback escuro)
    → adaptador serializado → Window.setTheme → decoração nativa
```

- Caminho principal: `setTheme(null)` deixa o SO decidir as cores. Tema claro/escuro do conteúdo não deve substituir cores nativas que já funcionam.
- Fallback: quando a integração nativa de cores não estiver disponível no backend suportado, conteúdo escuro pede `dark`; conteúdo claro volta a `null`. Não exigir uma barra clara artificial, pois não foi solicitada.
- Não confundir rejeição por ACL com ausência de cores do sistema. Primeiro corrigir a autorização. Nem sucesso do IPC nem `Window.theme()` provam correspondência visual com a barra do SO.
- **Gate técnico obrigatório da tarefa T1:** identificar por leitura da implementação Tauri/GTK efetivamente resolvida como distinguir o caminho nativo do fallback; confirmar em janela descartável do aplicativo. A API JS lida não oferece detecção de fidelidade visual. Não inventar uma detecção por pixel, por cor do DOM, por simples nome do sistema operacional ou por erro de IPC. O nome Linux sozinho não comprova indisponibilidade.
- Se não houver sinal confiável, manter caminho do sistema e registrar a pendência do fallback como `manualPending`, com proposta concreta baseada no backend observado. Não considerar o requisito inteiro entregue nem adicionar barra HTML ou configuração nova sem revisão. Esse gate é uma limitação técnica explícita, não uma decisão de produto reaberta.
- A deduplicação deve aceitar `null` como alvo válido, usando sentinela distinta para estado ainda não aplicado e estado sem pendência. Preservar serialização, última solicitação prevalecendo, tentativa após erro e resultado booleano documentado. Cobrir concorrência com `null`, incluindo dark → null → dark.
- Não permitir que o próprio tema forçado em GTK contamine a decisão de seguir o sistema via `matchMedia`. Manter a lógica existente dos temas de conteúdo e verificar esse risco na aceitação nativa.

### Atualização interna

```text
keydown F5 sem Ctrl/Alt/Shift/Meta
    → captura na raiz → preventDefault + interromper propagação
    → bloqueio de repetição/segunda chamada → window.location.reload()
    → boot normal → listeners + snapshots → reanexo aos PTYs existentes
```

- Novo `src/composables/usePageReloadShortcut.ts`, instalado uma única vez em `src/app/App.vue`, antes dos listeners dos componentes descendentes. Listener em captura e descarte por escopo via MaxUse.
- Consumir também repetições da tecla reconhecida, mas executar reload só na primeira. Não disparar durante composição. Ignorar modificadores sem mudar seus comportamentos anteriores.
- Reservar `F5` em `RESERVED_APP_SHORTCUTS`, evitando ação simultânea de agente. O comportamento intencional de F5 passa a ser da aplicação inclusive em TUIs; demais teclas permanecem intactas.
- Não instalar atalho do SO, abrir URL, fechar janela, chamar quit, matar PTY, abrir aba ou chamar `startNewSession` para atualizar a página.
- Testar reload por dependência substituível no composable, sem executar navegação real no happy-dom. Não ampliar API de produção só para expor estado interno desnecessário.
- Reutilizar `useWorkspaceEvents` (listeners, buffer, snapshot, boot/event sequence), `useTabsStore.refreshSnapshot` e o reanexo de `ptyStreamHub`. Reload não pode depender de `onUnmounted` ou de IPC assíncrono em unload para manter processos vivos.

### Arquivos

Alterações previstas em existentes: `src/theme/nativeWindow.ts`, `src/composables/useThemeApplier.ts`, `src/app/App.vue`, `src/composables/useAgentShortcuts.ts`, `src-tauri/capabilities/default.json`, `src-tauri/tests/capabilities.rs`, `tests/theme/nativeWindow.spec.ts`, `tests/composables/useThemeApplier.spec.ts`.

Novos propostos: `src/composables/usePageReloadShortcut.ts`, `tests/composables/usePageReloadShortcut.spec.ts`. Localizar os testes existentes de atalhos/workspace e estendê-los apenas onde necessário. Mudanças adicionais no bootstrap, streaming ou backend só após evidência de regressão de reload e delimitação do problema; alterações extensas exigem revisão de escopo.

## 4. Plano de Execução Faseado

1. T1: preflight por leitura, confirmar API, ACL, política de integração de cores e lifecycle de reload; bloquear o que não tiver viabilidade demonstrada.
2. T2: ajustar política/adaptador/composable de tema e permissão mínima; atualizar testes unitários e de capabilities juntos.
3. T3: implementar composable de F5, integração raiz e reserva do atalho; adicionar testes de captura, descarte e exclusões.
4. T4: completar regressões de reidratação/streams e revisão de segurança; só então validar o lote completo.
5. T5: aceitação nativa segura da decoração e F5 com processo descartável; registrar pendências sem declarar aprovação integral.

## 5. Critérios de Aceite & Validação

- Barra nativa única com controles funcionais e cores do SO quando disponíveis; fallback escuro verificado na combinação realmente sem integração correta; retorno ao sistema ao sair do fallback.
- Autorização restrita à janela local `main`, sem `core:default`, permissão ampla de janela ou concessão a conteúdo remoto.
- F5 recarrega uma vez por acionamento, inclusive sobre xterm e modal; não chega ao PTY nem aciona agente; outras teclas e combinações não mudam.
- Backend, IDs dos PTYs/sessões e processos vivos permanecem; não reexecutar comando inicial nem criar duplicatas. A interface reconecta e volta a aceitar input/output. Replay continua sujeito aos limites atuais, com indicação de visão parcial se aplicável.
- Após implementação e testes completos, usar `npm run check` (script canônico agrega bibliotecas, lint, tipos, cobertura, build, fmt/clippy/test Rust). Antes, provar isolamento dos recursos que os testes usam; não rodar contra dados reais.
- Não foram executados testes, build, inicialização nativa ou preflight operacional nesta sessão. Aprovação de dublês não prova aparência nem preservação real de PTYs; sem ambiente gráfico seguro, T5 fica `manualPending`.
