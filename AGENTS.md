# MaxTerminal — Diretrizes Canônicas e Especificação Técnica (`AGENTS.md`)

Este documento constitui a **fonte da verdade** e as diretrizes canônicas mandatórias para todos os assistentes de IA (Gemini, Claude Code, Antigravity `agy`, Codex, OpenCode, Cursor) atuando no desenvolvimento e manutenção do projeto **MaxTerminal**.

---

## Diretrizes Operacionais e de Desenvolvimento

### 1. Idioma
1. **Português do Brasil (`pt-BR`) obrigatório para:**
   - Comunicação entre Usuário ⇄ Agente de IA
   - Comentários no código (TypeScript, Vue, SCSS e Rust)
   - Mensagens de commit e descrições de Pull Requests / Issues
   - Textos visíveis na interface (rótulos, botões, tooltips, notificações, mensagens de erro)
2. **Inglês (`en-US`) para:**
   - Identificadores de código (nomes de funções, variáveis, structs, traits, tipos, interfaces e classes)
   - Nomes de arquivos, diretórios, módulos e pacotes

### 2. Regras Obrigatórias de Git Worktree
- **Toda e qualquer alteração de código ou arquivos** DEVE ser realizada obrigatoriamente em um **git worktree separado** (ex.: `.worktrees/wt-<nome-da-tarefa>` ou `.max-code-worktrees/`).
- É **terminantemente proibido** editar arquivos diretamente na branch principal (`main`/`dev`) ou na raiz do repositório principal fora de uma worktree dedicada.
- Após concluir, validar e integrar o trabalho (mediante confirmação quando aplicável), remova a worktree temporária (`git worktree remove`).

### 3. Padrões de Código Frontend (Vue 3.6 + TypeScript + UnoCSS + SCSS)
- **Estrutura de SFC Vue (`.vue`) — Ordem Obrigatória:**
  1. `<template>`
  2. `<script setup lang="ts">`
  3. `<style lang="scss" scoped>`
- **Composition API Exclusiva:** Uso estrito de `<script setup lang="ts">` com tipagem forte (`defineProps<Interface>()` e `defineEmits<{...}>()`). Options API é proibida.
- **Tags de abertura em linha única:** No `<template>`, mantenha o nome da tag, todos os atributos, props, diretivas, eventos e o fechamento `>` ou `/>` na mesma linha.
- **Estilo e Formatação:**
  - Recuo de 4 espaços (inclusive `baseIndent: 1` dentro de `<script setup>`).
  - Aspas simples (`'single'`), ponto e vírgula obrigatório, sem vírgula final (`comma-dangle: 'never'`).
  - Tokens semânticos do `@maxvue/max-components-ui` (`--max-primary-*`, `--max-success-*`, `--max-warning-*`, `--max-danger-*`, `--max-info-*`, `--background-*`).

### 4. Padrões de Código Backend Nativo (Tauri v2 + Rust)
- Gerenciamento seguro de ciclo de vida de PTYs (`portable-pty`) e processos filhos: nenhum processo órfão pode permanecer vivo sem supervisão quando encerrado explicitamente pelo usuário.
- Comunicação assíncrona eficiente entre Rust e Frontend (eventos Tauri / Channels) evitando bloqueio da thread principal da UI durante rajadas intensas de output de terminal.
- Persistência SQLite sempre em modo **WAL** (`PRAGMA journal_mode=WAL;`) com sanitização prévia obrigatória de segredos antes de gravar comandos ou scrollback em disco.

---

# Especificação do Projeto: MaxTerminal

## 1. Visão Geral
- **Nome:** MaxTerminal
- **Diretório:** `~/GitHub/MaxTerminal`
- **Plataforma Alvo:** Linux Desktop (CachyOS) via Tauri v2
- **Descrição:** Emulador de terminal acelerado por GPU e orquestrador de Agentes de IA integrado nativamente ao ecossistema Max (`MaxComponentsUi`, `MaxAiManager`, `MaxCode`, `MaxSkills`).
- **Inspirações:** Tabby Terminal, Warp Terminal, Hermes Agent e Open Claw.

---

## 2. Stack Tecnológica, Bibliotecas e Otimizações
- **Desktop & Backend Nativo:** **Tauri v2** (Rust) + `portable-pty` (gerenciamento real de PTYs) + monitoramento de processos filhos via `/proc` no Linux.
- **Frontend:** **Vue `v3.6.0-rc.13`** + TypeScript estrito (`tsgo`) + **Vite 8** + **UnoCSS** (`presetMaxUno`) + SCSS Scoped.
- **Ecossistema MaxVue (Links Locais):** `@maxvue/max-components-ui` (incluindo `MaxIcon` com Iconify), `@maxvue/max-use` e `@maxvue/max-pinia` vinculados localmente com script `sync:local-libs`.
- **Motor Híbrido de Terminal:**
  - **`@xterm/xterm`** + `@xterm/addon-webgl` (aceleração GPU), `@xterm/addon-fit`, `@xterm/addon-search` e `@xterm/addon-web-links` para renderização fiel de TUIs interativos (`claude`, `opencode`, `agy`, `codex`, `vim`, `htop`).
  - **`vue-web-terminal` / Camada de Blocos e Input Rico (estilo Warp):** Barra de entrada estruturada com suporte a popups de autocompletar (`>`, `>>`, `@`, `?`, `/`), agrupamento visual de comandos executados e ações por bloco.
- **Layout Redimensionável:** `splitpanes` para divisão redimensionável entre as 2 colunas principais e para **Split Panes** (divisão horizontal/vertical) dentro de uma mesma aba.
- **Banco de Dados:** **SQLite** local gerenciado via Rust (`rusqlite` / `tauri-plugin-sql`) em modo **WAL** com índice **FTS5 (Full-Text Search)** para busca instantânea no histórico de sessões, comandos e saídas.
- **Temas e Cores:** Importador nativo compatível com esquemas de cores YAML/JSON do **Tabby Terminal**, integrado aos tokens semânticos do `MaxComponentsUi` (tema padrão Max Navy/Teal).

---

## 3. Experiência no Sistema Operacional (CachyOS / Linux)
- **System Tray (Bandeja do Sistema):** Ao fechar a janela quando houver abas em estado `Executando` ou `Execução Contínua`, o app minimiza para a bandeja mantendo os PTYs e servidores vivos.
- **Atalho Global (Quake Mode / Quick Toggle):** Atalho global de teclado para exibir/ocultar rapidamente a janela do MaxTerminal em qualquer workspace.
- **Links Clicáveis para o Editor:** Clicar em caminhos de arquivos e linhas no terminal (ex.: `app/Models/User.php:42`) abre o arquivo diretamente na linha correspondente no VS Code / MaxCode.
- **Sanitização de Segredos:** Redação/mascaramento automático de senhas, `APP_KEY`, cookies e tokens `Bearer` antes de persistir logs no SQLite ou enviar erros para inspeção externa.

---

## 4. Arquitetura de Interface (2 Colunas Redimensionáveis)

### 4.1. Coluna da Esquerda — Lista de Projetos
- **Cadastro e Importação:**
  - Permite adicionar projetos individualmente ou adicionar uma **"Pasta de Projetos"** (ex.: `~/GitHub`), importando cada subpasta como um projeto.
  - **Auto-descoberta (`fs::watch`):** Monitora as pastas-mãe cadastradas para detectar e sugerir/importar automaticamente novos diretórios criados.
  - **Auto-detecção de Identidade:** Detecta a stack da pasta (Laravel, Vue, Tauri, Extensão VS Code, Python, Rust) para atribuir **Ícone Iconify** e **Cor** coerentes (inspirado no `max-big-icon-pack`), usando cor e ícone aleatórios como fallback.
- **Dados do Projeto:** `Título`, `Ícone` (`MaxIcon`), `Cor` e `Pasta`.
- **Contadores de Status por Projeto (Badges Semânticos):**
  1. 🟢 **Executando (`--max-success-500` / Verde):** Abas com comando temporário em execução no momento.
  2. 🔵 **Execução Contínua (`--max-info-500` / Azul-Teal):** Abas rodando processos contínuos/daemons (ex.: `npm run dev`, `horizon`).
  3. 🟠 **Finalizado (`--max-warning-500` / Âmbar):** Abas cujo comando terminou com sucesso (`exit 0`), mas que ainda não foram abertas/visualizadas pelo usuário.
  4. 🔴 **Com Erro (`--max-danger-500` / Vermelho):** Abas cujo último comando falhou (`exit != 0`) ou em que um Agente aguarda intervenção, ainda não visualizadas.
  5. ⚪ **Parado (Cinza Neutro):** Abas ociosas no prompt aguardando comando.
  6. 🟣 **Issues Abertas:** Contador exibindo a quantidade de Issues abertas (não resolvidas) quando o projeto for um repositório GitHub.

### 4.2. Coluna da Direita — Workspace de Abas e Sessões do Projeto
Ao selecionar um projeto, exibe suas abas exclusivas:
- **Controles de Abas:**
  - **Botão "Adicionar nova aba":** Cria e abre imediatamente uma aba usando o **Template Padrão** do projeto.
  - **Botão "Iniciar Aba Template":** Permite escolher qualquer Template de Aba configurado no projeto para abrir.
  - **Suporte a Split Panes:** Permite dividir uma mesma aba em múltiplos painéis de terminal (horizontal/vertical).
- **Indicadores Contextuais na Barra da Aba:**
  - **Git & Worktree:** Exibe a branch atual, status de alterações (`dirty`) e indica se está em uma worktree (`.worktrees/` ou `.max-code-worktrees/`).
  - **Detector de Portas Ativas:** Identifica quando o processo da aba abre uma porta local (ex.: `:5177`, `:8000`) e exibe um badge clicável para abrir no navegador ou encerrar o processo da porta.
- **Templates de Abas:**
  - Todo projeto possui pelo menos um Template marcado como **Padrão**.
  - **Parâmetros do Template:**
    - `Nome` e `Ícone`
    - `Pasta personalizada` (opcional; se omitida, usa a pasta raiz do projeto)
    - `Comando Inicial` (opcional; executado automaticamente ao abrir a aba)
    - `Adicionar ao Workspace` (se marcado, abre automaticamente ao iniciar o MaxTerminal)
    - `Execução Contínua` (se marcado, classifica a aba no contador de execução contínua)
    - `Tipo de Ambiente / Agente` (Shell padrão ou Agente CLI específico + variáveis de ambiente)
- **Sistema de Sessões (SQLite):**
  - Cada aba em execução registra seus eventos em uma **Sessão** no SQLite (comandos executados, timestamps, diretórios, códigos de saída e scrollback sanitizado).
  - **Botão "Iniciar nova Sessão":** Arquiva a sessão atual no SQLite, reinicia o PTY limpo na aba e permite consultar o histórico de sessões anteriores daquela aba.

---

## 5. Inteligência Artificial e Agentes CLI

### 5.1. Integração com `MaxAiManager`
Conectado ao gateway local do `MaxAiManager` com header `X-Agent-Name: MaxTerminal`:
- **Comando por Linguagem Natural (`@` ou `?`):** Digitar `@` ou `?` no input converte solicitações em português em comandos shell prontos para execução.
  - `@` -> Permite selecionar qual modelo / Harness ou Nível de Raciocínio será usado - Claude Code, Codex, Antigravity (agy) ou OpenCode:
    - Ao digitar `@` abre-se um popup seletor de harness.
    - Ao selecionar o Harness, mostra opções dos modelos.
    - Ao selecionar o modelo, mostra opções de Nível de Raciocínio.
  - `?` -> Utiliza o modelo / Harness e Nível de Raciocínio padrão configurado.
- **Diagnóstico Rápido Inline:** Quando um comando falha, exibe um resumo curto da causa e sugestão de comando corretivo ao lado do botão "Inspecionar".
- **Geração Assistida:** Auxilia no preenchimento do modal de novos Comandos Artisan e na sugestão de Grupos de Comandos.
- **Consulta RAG:** Permite consultar a documentação indexada da stack via `/v1/rags/{slug}/search` direto do terminal.

### 5.2. Compatibilidade com Agentes CLI (`Claude Code`, `Codex`, `Antigravity (agy)`, `OpenCode`)
- **Templates e Atalhos Dedicados:** Abertura rápida de abas pré-configuradas para cada um dos 4 agentes.
- **Isolamento Opcional em Git Worktree:** Opção para criar/vincular automaticamente uma worktree isolada ao abrir uma aba de Agente CLI.
- **Telemetria na Aba (Estilo `StatusLine`):** Exibe o modelo ativo, percentual de uso da janela de contexto e consumo das cotas (5h e semanal).
- **Alerta de Permissão/Input:** Detecta quando o agente pausa aguardando resposta ou aprovação de ferramenta, destacando a aba/projeto e disparando notificação nativa no CachyOS.
- **Integração com `MaxSkills` + Memória Contextual:** Sincroniza as skills do repositório (`~/GitHub/MaxSkills`) e mantém memória contextual de comandos recorrentes do projeto no SQLite.

---

## 6. Integração Bidirecional com a Extensão `MaxCode`
Comunicação via **Loopback HTTP/WebSocket (`127.0.0.1`)** combinada com **URI Scheme (`vscode://johnattas.maxcode/...` e `maxterminal://...`)** (sincroniza em tempo real e acorda o aplicativo caso esteja fechado):
- **Sincronização de Projeto Ativo:** Ao abrir um projeto no MaxCode, o mesmo projeto é automaticamente aberto/focado no MaxTerminal.
- **Monitoramento Shell Integration (OSC 133) + Botão "Inspecionar":**
  - Hooks automáticos no shell (`zsh`/`bash`/`fish`) + leitura de `/proc` identificam início, fim, código de saída, `${comando}` e `${erro}` de cada execução.
  - Se um comando retornar erro (`exit != 0`), exibe o botão **"Inspecionar"** no bloco do comando.
  - Clicar em **"Inspecionar"** abre uma nova conversa em **Plan Mode** no MaxCode com o prompt:
    `Ao executar o comando "${comando}" tive o seguinte erro: "${erro}"`

---

## 7. Aba Fixa de Issues do GitHub (Padrão `MaxCode`)
Exibida automaticamente como aba fixa sempre que a pasta do projeto for um repositório GitHub (usando `gh` CLI com fallback para REST API):
- **Labels Canônicas do `MaxCode`:** Suporte integral às 24 labels do workflow do MaxCode (15 de estado como `Waiting Plan`, `Planned`, `Waiting Execute`, `Executed`, `Waiting Check`, `Checked Approved`, etc.; 3 de encerramento e 6 de categoria).
- **Ações por Issue:**
  - Enviar para o MaxCode com 1 clique: **Abrir**, **Analisar (`BUG PLAN`)** ou **Corrigir (`BUG FIX`)**.
  - **Abrir Aba no MaxTerminal:** Abre uma nova aba de Terminal ou Agente CLI já posicionada na worktree/branch daquela issue.
  - **Gestão Completa Local:** Criar nova issue, visualizar descrição/comentários/plano (`## PLANO DE IMPLEMENTAÇÃO`) e editar labels manualmente.
- **Ação Global em Lote:**
  - **Botão "Corrigir todos os Bugs":** Dispara o ciclo automatizado de resolução de bugs em fila (seguindo a orquestração de deduplicação, planejamento, execução e verificação do MaxCode).

---

## 8. Comandos Rápidos, Artisan (`>`) e Grupos de Comandos (`>>`)

### 8.1. Comandos PHP Artisan (`>`)
- **Gatilho e Autocompletar:** Ao digitar `>` em um projeto Laravel, abre um popup listando os comandos Artisan do projeto (descobertos via `php artisan list --format=json` com cache compatível em `.max-code/commands/artisan.json`).
- **Filtro e Flags:** Filtra em tempo real conforme a digitação (navegável por setas) e, após selecionar o comando, exibe o autocompletar das flags/opções disponíveis ao digitar `-`.
- **Opção Final "Criar comando":**
  - Exibe no fim da lista (ou quando nenhum item corresponder) a ação **"Criar comando"**, que abre um modal com formulário completo:
    - `Nome da Classe`, `Signature`, `Descrição`, construtor visual de `Argumentos e Flags (--options)`, checkboxes para `Isolatable`, `PromptsForMissingInput` e `Agendamento (Schedule)`, além do campo `Objetivo / Regras do Comando`.
  - **Ações do Modal:**
    1. **Criar:** Gera imediatamente o arquivo PHP do comando Artisan (`app/Console/Commands/...`) estruturado com os dados do formulário.
    2. **Criar com MaxCode:** Abre uma nova conversa no MaxCode em **Plan Mode** com a instrução:
       `"Criar comando artisan:\n${dados_formatados_do_formulario}"`

### 8.2. Grupos de Comandos (`>>`)
- **Escopo Duplo:** Suporta Grupos de Comandos **Locais** (específicos do projeto) e **Globais** (disponíveis em todos os projetos).
- **Gatilho e Ordenação:** Ao digitar `>>`, abre o popup listando primeiro os grupos do projeto atual e por último os grupos globais, filtrando conforme a digitação.
- **Modos de Execução do Grupo:**
  - **Sequencial na mesma aba:** Executa os comandos em fila (`&&` ou `;`) na aba atual.
  - **Paralelo em Background:** Dispara as tarefas concorrentemente.
  - **Multi-Abas:** Abre uma aba dedicada para cada comando do grupo (ideal para subir todo o ambiente de desenvolvimento de uma vez).
- **Opções Finais de Criação:** Exibe sempre no final da lista (ou quando vazia):
  - **"Criar grupo de comandos local"**
  - **"Criar grupo de comandos global"**

### 8.3. Atalhos Universais de Projeto (`/`)
- Suporte aos atalhos rápidos do `MaxCode` (`/install`, `/install-force`, `/update`, `/update-force`, `/build`, `/deploy`, `/test`), detectando automaticamente o gerenciador de pacotes do projeto (`composer`, `npm`, `pnpm`, `yarn`, `bun`, `cargo`).
