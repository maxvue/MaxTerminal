# Plano de Execução — Implementação 06: Composição HybridTerminal, Playground de Desenvolvimento e Fixtures de Validação

> Especificação de referência: [`./spec.md`](./spec.md)
> Fase 04: Motor de Terminal Híbrido

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Implementar `HybridTerminal.vue` (`src/components/terminal/HybridTerminal.vue`)
1. Criar `src/components/terminal/HybridTerminal.vue` compondo:
   - Banner superior de aviso de truncamento de replay (`flow.droppedBytes !== '0'`).
   - `TerminalSearchBar.vue` condicional (`v-if="instanceState?.search.open"`).
   - Área central com `TerminalView.vue` e `CommandBlockLayer.vue` sobreposta (repassando `<template #block-actions="{ block }">`).
   - `TerminalExitBanner.vue` condicional (`v-if="instanceState?.exit"`).
   - `RichInput.vue` condicional (`v-if="richInput && !isPassthrough"`).
2. Adicionar handlers de atalho de painel (`Ctrl+Shift+F` para busca, `Ctrl+Shift+I` para alternar modo de input) e restauração inteligente de foco ao sair de `altScreen`.

### Passo 2: Criar Scripts e Arquivos de Fixture (`tests/fixtures/terminal/`)
1. Criar `tests/fixtures/terminal/truecolor.py` (gradiente 24 bits).
2. Criar `tests/fixtures/terminal/unicode-width.txt` (tabela alinhada com CJK, combinantes, emojis simples e ZWJ).
3. Criar `tests/fixtures/terminal/burst.sh` (`seq 1 3000000` + `yes ... | head -c 200M`).
4. Criar `tests/fixtures/terminal/mouse-report.py` (SGR mouse tracking com cleanup garantido em `finally`).

### Passo 3: Implementar `TerminalPlayground.vue` e Rota `/dev/terminal`
1. Criar `src/components/terminal/__dev__/TerminalPlayground.vue` implementando:
   - Seletor de shell e controles `Iniciar`, `Reiniciar` e `Encerrar` usando exclusivamente os contratos da Fase 02.
   - Modo de 2 abas/instâncias (`pane-1` e `pane-2`) para testar suspensão de WebGL e preservação de buffer.
   - Botão `"Perder contexto WebGL"` chamando `forceWebglContextLossForDebug()`.
   - Registro/desregistro dos 5 gatilhos simulados (`>`, `>>`, `@`, `?`, `/`) exercitando todas as capacidades da `CompletionSession`.
   - Simulador de `CommandBlockFeed` (`begin` → `markOutputStart` → `end` com `0`, `1` e `null`).
   - Painel de telemetria exibindo `renderer`, `webglCount`, `cols×rows`, `pendingBytes`, `paused`, `droppedBytes`, `frameErrors`, latência de spawn/montagem e `longtask`.
2. Registrar a rota `/dev/terminal` no roteador da aplicação apenas quando `import.meta.env.DEV` for verdadeiro.

### Passo 4: Criar Testes de Integração (`tests/terminal/HybridTerminal.spec.ts`) e Verificar Cobertura Total
1. Criar `tests/terminal/HybridTerminal.spec.ts` validando:
   - Montagem completa do `HybridTerminal.vue` sem importar módulos das Fases 05 ou 07.
   - Unicidade de assinatura no `ptyStreamHub`, unicidade de listener `onData`/`onBinary` no `InputPipeline` e loteamento no `WriteBatcher`.
   - Alternância entre 2 instâncias preservando o estado e mantendo apenas 1 contexto WebGL ativo (`webglCount === 1`).
   - Atalhos `Ctrl+Shift+F` e `Ctrl+Shift+I`, exibição do banner de `Gap` e exibição do `TerminalExitBanner` somente após `End`.
   - Sequências UTF-8 de 2, 3 e 4 bytes e escapes CSI/OSC divididos em múltiplos offsets de frame.
2. Executar toda a suíte `tests/terminal/*.spec.ts` com verificação de cobertura (`>= 90%` em `src/terminal/**` e `>= 80%` em `src/components/terminal/**`).

---

## 2. Comandos de Verificação e Validação Final da Fase 04

```bash
# 1. Verificação completa de tipos TypeScript (tsgo)
npm run typecheck

# 2. Linting de código e estilos (ESLint + Stylelint)
npm run lint
npm run lint:style

# 3. Execução de toda a suíte de testes da Fase 04 com cobertura
npx --no-install vitest run tests/terminal --coverage

# 4. Build do frontend para garantir ausência de imports quebrados
npm run build

# 5. Verificação do workspace Rust
cargo clippy --workspace -- -D warnings
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 04)

- **[CA-5.1.1 / CA-5.1.2 / CA-5.1.4]** `npm run typecheck`, `npm run lint`, `npm run lint:style`, `cargo clippy --workspace -- -D warnings` e `npx vitest run tests/terminal` passam com 100% de sucesso, atingindo cobertura ≥ 90% de linhas em `src/terminal/**` e ≥ 80% em `src/components/terminal/**`.
- **[CA-5.1.5]** Sequências UTF-8 de 2/3/4 bytes e sequências CSI/OSC divididas em todos os offsets preservam o resultado no xterm; apenas 1 write em voo; nenhum eco ou envio duplicado ao alternar modos `rich` ↔ `passthrough`.
- **[CA-5.2.1 a CA-5.2.13]** Rota `/dev/terminal` permite validar em `cargo tauri dev` todas as metas observáveis: abertura de shell real (< 300 ms após spawn), gradiente 24 bits (`truecolor.py`), tabela Unicode (`unicode-width.txt`), rajada de 3 milhões de linhas / 200 MiB (`burst.sh`) com UI responsiva e `Ctrl+C` < 500 ms, redimensionamento deduplicado, busca `Ctrl+Shift+F`, links seguros `Ctrl+Clique`, perda de contexto WebGL sem perder scrollback, troca entre 2 abas mantendo apenas 1 WebGL ativo, input rico com os 5 gatilhos fictícios (`>`, `>>`, `@`, `?`, `/`), blocos ancorados pelo `CommandBlockFeed` simulado e reinício limpo de PTY sem processos órfãos.
