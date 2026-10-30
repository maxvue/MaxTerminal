# Especificação Técnica — Fase 05: Shell Integration e Estados das Abas

> Complementa [`instructions.md`](./instructions.md). Base: fases 01, 02, 03 e 04 e [`../README.md`](../README.md).
> Este é o contrato de implementação; nomes novos estão explicitados como adições da 05.

## 1. Arquitetura e invariantes

```text
pty_spawn → ShellService prepara vínculo/sessão → lifecycle genérico mt-pty → shell
PTY reader → OutputTap somente leitura → scanner/tracker → mailbox limitada → captura
           → frames brutos Data/Replay → hub único da 04 → xterm
tracker → registry runtime → tab://state-changed
captura → StreamSanitizer → retenção limitada → Db::write → shell://command-finished
eventos/snapshot → Pinia → CommandBlockFeed da 04 / indicadores
```

1. `mt-pty` não importa `mt-shell`/`mt-db`; `mt-shell` não importa Tauri. Composição no binário.
2. O `OutputTap` da 02 é somente leitura: **não reescrever OSC, remover nonce ou injetar ids no Channel**. Replay, ACKs e offsets continuam sobre bytes brutos, conforme 02/04.
3. Banco, sanitização pesada e `/proc` não rodam no reader. Scan/tracker usam trabalho O(n), estado limitado e enqueue não bloqueante. A 02 continua responsável por backpressure do terminal; esta fase não promete que o reader nunca pausa.
4. Somente o sequenciador por PTY aplica transições. Fallback, exit e marcas entram nele; nunca dois produtores mutando o mesmo bloco.
5. Estado runtime não espera SQLite. Persistência tem confirmação própria e falha explícita.
6. Blocos armazenam saída combinada stdout/stderr. PTY não preserva a separação dos descritores.

## 2. Contratos reais das bases e extensões

### 2.1. Identidade e preparação de spawn

Ids `TabId`, `PaneId`, `PtyId`, `SessionId`, `CommandBlockId` são os newtypes Uuid v7 da 01; TS os importa de `bindings.ts`. O playground gera tab/pane lógicos e solicita uma sessão mínima; não requer `useTabsStore`, `tabs`, `projects` ou templates.

Da 02 consumir `SpawnSpec { shell, args, login, cwd, env, env_remove, size, owner }`, `PtyOwner`, `PtySummary`, `ExitInfo`, `PtyManager`, `OutputTap`, `PtyEventListener`, `ProcSource` e `ProcessInfo`. **Não existem na base** `PtySpawnArgs`, `PtySpawnDecorator` ou `ProcInspector`.

Adições da implementação da 05:

- Argumento opcional `integration: Option<ShellIntegrationRequest>` ao comando `pty_spawn(spec, integration?)` no binário. Chamadas antigas omitem o argumento; mantendo owner completo, usam configuração padrão; owner ausente mantém PTY bruto sem tracker de aba. O spawn permanece desanexado: `onOutput` pertence exclusivamente a `pty_attach`, conforme a 02.
- O binário prepara uma `SpawnPlan` de `mt-shell`, incluindo sessão e owner validado, antes de chamar o manager. Prepara DB de modo assíncrono; falha retorna plano `persistence=failed` ainda utilizável em memória.
- Adicionar API genérica `PtyManager::spawn_with_lifecycle(spec, sink, hook)`; `spawn(spec,sink)` delega sem hook. `ResolvedSpawn` contém program/args/env/cwd já resolvidos pela 02, `PtyId` alocado e owner. Essa extensão é necessária porque o listener `Spawned` atual ocorre depois de iniciar threads e perderia output inicial.

```rust
// Adição genérica em mt-pty; tipos sem dependência do domínio shell.
pub trait SpawnLifecycle: Send + Sync {
    fn prepare(&self, spawn: &mut ResolvedSpawn) -> PtyResult<()>;
    fn started(&self, summary: &PtySummary) -> PtyResult<()>;
    fn failed(&self, pty_id: PtyId);
}
```

`prepare` ocorre antes de criar o filho; `started` após conhecer pid/start_time e **antes** de iniciar reader/writer/waiter. O adaptador de `mt-shell` injeta args/env no primeiro e registra contexto/scanner/mailbox no segundo. Se qualquer etapa após spawn falhar, a 02 encerra/recolhe o filho e chama `failed`; a 05 fecha sessão preparada e libera estado. Reter o `TapHandle` de `add_output_tap` pelo tempo de vida do serviço. Resolução/execução bloqueante continua em `spawn_blocking`.

### 2.2. Persistência e frontend

- Fase03: `Db::read`, `Db::write`, `DbSetup`, `SanitizedText`, `StreamSanitizer`, `FtsSpec`, `build_match_query`. `DbWriter` é `pub(crate)`; nenhuma chamada a `DbWriter::submit`.
- Fase04: `registerTerminalExtension`, `TerminalInstanceHandle.xterm`, `registerContextProvider`, `registerInputModeHint`, `CommandBlockModel` e `useCommandBlocksStore().feed: CommandBlockFeed`.
- A 04 não oferece `registerCommandBlockProvider`, `useXterm` ou slot `#status`. Usar as extensões/feed reais; nova composição visual, se necessária, é entrega aditiva da 05. `block-actions`/registro `BlockAction` permanecem pontos para 11/12.

## 3. Crate, módulos e tipos runtime

Dependências: `mt-core`, `mt-pty`, `mt-db`, `serde`, `specta`, `thiserror`, `tracing`, `tokio` (sync/rt/time), `crossbeam-channel`, `memchr`, `vte`, `rand`, `sha2`, `parking_lot`. `regex` apenas para padrões suportados, sem look-around; `percent-encoding` somente para diagnóstico OSC7. Dev: `proptest`, `criterion`, `tempfile`, harness PTY da 02. Fixar versões compatíveis no workspace; exemplos abaixo são contratos de domínio, não cópias prontas de APIs externas.

```rust
// Em mt-core::shell: DTOs compartilhados; serde camelCase nos campos,
// snake_case nos valores dos enums; Specta conforme o padrão da 01.
pub struct ShellIntegrationRequest {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub session_id: Option<SessionId>,
    pub enabled: bool,                    // padrão true
    pub continuous: ContinuousPolicy,    // padrão Auto
}
pub enum ContinuousPolicy { Auto, On, Off }
pub enum IntegrationMode { Pending, Hooks, Proc, Disabled }
pub enum ObservationSource { Hook, Proc, PtyLifecycle }
pub enum ObservationConfidence { Correlated, Inferred, Unknown }
pub enum CommandTextQuality { Complete, Partial, Unavailable }
pub enum PersistenceState { Pending, Committed, Failed }
pub enum BlockStatus { Running, Succeeded, Failed, Unknown }
pub enum BlockEndReason {
    ShellReported, ProcDetected, MissingEnd, ModeSwitched,
    PtyExited, CaptureOverflow, Shutdown, Recovery,
}
pub struct RuntimeBinding {
    pub tab_id: TabId, pub pane_id: PaneId, pub pty_id: PtyId,
    pub session_id: SessionId,
}
pub struct ExecutionRef {
    pub pty_id: PtyId, pub block_id: CommandBlockId, pub generation: u64,
}
pub struct StreamPosition {
    pub frame_seq: u64, pub byte_offset: u32,
}
```

Todos os contadores `u64` (revision, generation, frame_seq, seq, duration_ms e contagem de bytes) são serializados como **string decimal** para JS (evitar perda >2⁵³), com tipo Specta refletindo essa conversão; comparar com `BigInt` nos adaptadores, manter strings serializáveis em Pinia. Converter duração para number somente após validar faixa segura na view. Timestamp epoch ms cabe em number; duração usa relógio monotônico do backend, não subtração do relógio de parede. Posições são o primeiro byte após a marca para início e primeiro byte de D para fim, no stream bruto. O início de uma marca partida referencia seu frame/offset original; o limite após C referencia o frame onde C terminou.

```rust
// Em mt-shell; assinaturas de serviço, não comandos IPC.
pub struct ShellService { /* Db, PtyManager, registry, bounded caches, config */ }
pub struct CommandTracker { /* binding, mode, phase, metadata, generation, open */ }
pub struct OpenBlock { /* ExecutionRef, seq da sessão, clocks, source, range */ }
pub struct TabStateRegistry { /* vínculos runtime, estados, revisão por aba */ }
pub struct ShellConfig { /* limites de scanner, filas, captura, timeouts */ }
impl ShellService {
    pub async fn prepare_spawn(&self, spec: &SpawnSpec, request: ShellIntegrationRequest)
        -> Result<SpawnPlan, MtError>;
    pub fn registry(&self) -> &TabStateRegistry;
    pub async fn error_context(&self, id: CommandBlockId, max_lines: u32)
        -> Result<ErrorContext, MtError>;
    pub async fn shutdown(&self) -> Result<(), MtError>;
}
```

`runtime.rs` vincula owner→PTY→sessão antes do reader e recusa reuso simultâneo do mesmo pane. Troca de PTY invalida ACKs/eventos antigos; ids de PTY nunca reutilizados. `SessionProvider` é trait de domínio assíncrona (future boxed), com `prepare`, `close` e `abort`; implementação padrão `PtyScopedSessionProvider` usa `Db`, uma sessão por spawn. Sessão fornecida precisa existir, estar ativa, pertencer ao mesmo tab/pane e não possuir outro PTY vivo. Reuso sequencial incrementa `seq` transacionalmente (não reiniciar em 1).

## 4. Hooks e protocolo auxiliar seguro

### 4.1. Formato no fio

OSC133 normal delimita: A início do prompt, B fim do prompt/início de input, C início de execução, D fim com código opcional. Não contém linha completa/cwd. A variante local MaxTerminal acrescenta correlação; não se anuncia compatibilidade integral com protocolos de outros terminais:

```text
ESC ] 633 ; P ; MT=1 ; n=<nonce> BEL                   handshake
ESC ] 633 ; E ; <escaped-command> ; q=<quality> ; n=<nonce> ; g=<generation> BEL
ESC ] 633 ; P ; Cwd=<escaped-path> ; n=<nonce> ; g=<generation> BEL
ESC ] 133 ; A ; n=<nonce> ; g=<generation> BEL
ESC ] 133 ; B ; n=<nonce> ; g=<generation> BEL
ESC ] 133 ; C ; n=<nonce> ; g=<generation> BEL
ESC ] 133 ; D [ ; <exit-code> ] ; n=<nonce> ; g=<generation> BEL
```

Também aceitar `ESC \\` (ST) como terminador. Ordem inicial: handshake, cwd(g=0), A/B(g=0). Antes de executar: incrementar g, E + cwd(g) + C(g). Ao terminar: D(g), cwd(g), A/B(g). Hook D captura código **antes** de qualquer helper; código ausente não vira zero. D do shell pode reportar 130 após Ctrl+C; saída sinalizada do PTY usa `ExitInfo.signal`, sem inventar `128+signal`.

Escaping opera sobre bytes: `\\→\\\\`, `;→\\x3b`, controles C0, DEL e C1→`\\xHH` (hex minúsculo); demais bytes UTF-8 intactos. Decodificar uma única vez, validar UTF-8, rejeitar escape inválido e NUL; nunca `eval`, interpolação como código ou `printf` usando conteúdo como formato. Formato dos parâmetros é fechado, sem duplicatas/chaves extras; limite total OSC 32 KiB, comando decodificado 16 KiB, cwd 8 KiB. Acima do limite, omitir comando/cwd e declarar qualidade indisponível, não cortar um segredo para depois sanitizá-lo. Cwd deve ser caminho absoluto local, sem controles; preservar caminho lógico com symlinks, sem `canonicalize` bloqueante no reader.

OSC7 `file://host/path` e OSC633 de terceiros podem ser observados como diagnóstico **não confiável**. Não atualizar cwd autoritativo, executar navegação, criar blocos ou conceder permissões a partir deles. Cwd confiável usa o envelope auxiliar com nonce/g; fallback `/proc` tem origem inferida própria.

### 4.2. Modelo de confiança

Nonce aleatório 128 bits por PTY, fora de argv/logs/DB/DTOs. Transportado no ambiente inicial, copiado para variável **não exportada** e removido antes de carregar configuração do usuário sempre que o bootstrap permitir. Há limitações: `/proc`, configuração do próprio shell e processo do mesmo UID podem observar ambiente inicial/memória/saída ou acessar variáveis via código executado no shell. O token é **correlação contra texto acidental e replay entre PTYs, não fronteira de segurança contra o mesmo usuário**.

Output bruto continua contendo as marcas (inclusive nonce) no transporte local da 02, embora OSC não seja texto visual. Nunca afirmar que foi removido do Channel. Armazenamento/cópia externa de metadados/scrollback passa pelo filtro de controles e sanitizador; logs não incluem payloads. Nenhuma automação privilegiada/execução/autorização pode depender apenas de marca recebida. `ObservationConfidence::Correlated` significa nonce + geração + protocolo válidos, não autenticação criptográfica do processo emissor.

Handshake válido habilita `hooks`; E/cwd/C/D exigem nonce correto e sequência permitida. Geração repetida/antiga não cria outra execução; metadado só se liga à geração declarada. Marcas nativas do fish sem envelope não duplicam blocos.

### 4.3. Injeção sem dotfiles

Scripts embarcados via `include_str!`, materializados em `$XDG_RUNTIME_DIR/maxterminal/shell-integration/<sha256>/`, diretório 0700/arquivos 0600, dono validado, sem seguir symlinks, escrita atômica. Sem runtime seguro, usar cache privado validado; se falhar, seguir em `proc` com motivo `scripts_unavailable`. Não criar arquivos em HOME nem alterar dotfiles. Versão/hash dos scripts em `IntegrationInfo`.

| Shell | Bootstrap e obrigações |
|---|---|
| zsh ≥5.8 | Wrapper ZDOTDIR para `.zshenv/.zprofile/.zshrc/.zlogin`; salvar e acompanhar mudanças legítimas do ZDOTDIR do usuário, evitar recursão/double-source, respeitar login/non-login e RCS/GLOBAL_RCS. Instalar hooks após startup aplicável e restaurar ZDOTDIR original/atualizado para filhos. Flags que impedem bootstrap entram em `proc`. |
| bash ≥4.4 | Non-login usa `--rcfile <bootstrap> -i`, carrega `.bashrc` conforme política da distribuição sem duplicar bashrc global, depois instala hooks. Login preserva `--login -i` e startup/logout nativos; bootstrap inicial via `PROMPT_COMMAND` herdado aponta somente para o script privado, com restauração/encadeamento do valor original. Se profile sobrescrever esse bootstrap, não forçar a configuração: timeout → `proc` + motivo. `login_shell` é somente leitura: nunca tentar ativá-lo por `shopt`, nem fingir login carregando profiles em shell non-login. |
| fish ≥3.6 | `--init-command` após config/conf.d, preservando comandos iniciais existentes e quoting do path. Config anterior pode ter visto nonce: limitação da confiança. Registrar `fish_preexec`, `fish_postexec`, prompt e wrapper idempotente que chama prompt original; função original ausente não é erro. |

Não injetar `-c`, shells não interativos, SSH remoto ou shells aninhados. O opt-out `enabled=false`/`MAXTERMINAL_DISABLE_SHELL_INTEGRATION=1` desliga hooks e ativa `proc` imediatamente; `Disabled` fica reservado à desativação total de observação, sem fallback. Handshake ausente por 5 s → `proc`; prompt lento não é erro do comando. No bootstrap tardio do bash login, assim como no fish, configuração anterior pode observar/herdar o nonce: esse limite integra o modelo de confiança. Referência consultada após indisponibilidade do Context7: [manual GNU Bash, login_shell](https://www.gnu.org/software/bash/manual/html_node/The-Shopt-Builtin.html).

### 4.4. Semântica dos hooks

- **zsh:** preexec captura linha via argumento completo; precmd captura status antes de outros hooks e o devolve preservado. Função `precmd` existente pode rodar antes de `precmd_functions`; envolver/encadear preservando `$?`, não assumir que inserir no array garante ser primeiro. Envolver prompt A/B com escapes não imprimíveis `%{…%}` e não duplicar em temas que recalculam PS1.
- **bash:** adaptador preexec testado, guardas de reentrância/COMP_LINE, encadeamento DEBUG e `PROMPT_COMMAND` string/array, sem extrair `trap -p` com regex/sed para reconstruir código arbitrário. D captura `$?` e PIPESTATUS na entrada; comandos do prompt/DEBUG não iniciam bloco. PROMPT_COMMAND executado antes de armar a próxima execução; A/B em `\\[...\\]` para largura correta. DEBUG fornece comando simples, **não** a linha completa: usar histórico só com correlação inequívoca e suporte multiline. `HISTCONTROL=ignorespace/ignoredups`, HISTIGNORE, `set +o history`, funções/pipelines exigem `quality=partial/unavailable` quando não há linha íntegra; nunca anexar história anterior. Não inventar execução ao pressionar Enter vazio.
- **fish:** `fish_preexec` recebe linha, `fish_postexec` salva `$status` antes de helpers; `fish_prompt` preserva status e prompt personalizado. Não assumir que mudar `fish_function_path` detecta toda redefinição; reload explícito/revalidação idempotente, sem wrapper recursivo.
- Helpers não alteram status, diretório, opções/histórico permanentemente nem disparam jobs externos a cada byte; usar builtins para escaping. Instalação incompatível degrada capacidade em `IntegrationInfo` (`commandTextQuality`, `promptMarkersAvailable`), sem prometer compatibilidade universal com plugins.

## 5. Scanner, tracker, captura e fallback

### 5.1. Parser incremental

```rust
pub enum ScanState { Ground, Esc, OscBody, OscEsc, DiscardOsc, DiscardOscEsc }
pub enum ShellMark {
    Handshake, PromptStart, PromptEnd, CommandLine { text: Option<String>, quality: CommandTextQuality },
    Cwd { path: String }, CommandExecuted, CommandFinished { exit_code: Option<i32> },
}
pub struct ScannedMark { /* mark, nonce_valid, generation, start/end StreamPosition */ }
pub struct OscScanner { /* estado, buffer ≤32KiB, origem da marca, contador bruto */ }
impl OscScanner {
    pub fn feed(&mut self, seq: u64, bytes: &[u8], emit: impl FnMut(ScannedMark));
    pub fn finish(&mut self); // EOF: descarta marca incompleta; não emite D sintético
}
```

Algoritmo normativo:

1. `Ground`: fast path `memchr(ESC)`; reconhecer OSC apenas com `ESC ]`. Não interpretar bytes OSC dentro de DCS/APC/PM/SOS: manter estado de string ignorada até ST, limitado e sem reter conteúdo. O enum mostrado é o núcleo OSC; estados auxiliares dessas strings e CSI são necessários para evitar falso prefixo interno.
2. Guardar OSC candidato até BEL ou `ESC \\`, inclusive ESC final de chunk; entrada é bytes, decodificar somente o payload completo. ST de 8 bits não é habilitado por padrão (evita confusão com UTF-8).
3. `CAN`/`SUB` cancelam sequência. ESC que não inicia ST cancela o OSC e reprocessa escape seguinte; testar ressincronização sem consumir texto útil do scanner. O stream original nunca muda.
4. Limite 32 KiB → descartar **interpretação** até terminador/cancelamento, sem alocação crescente; modo discard também precisa estado ESC para ST dividido. Outros OSC passam sem produzir marca. EOF remove pendências.
5. Validar gramática, UTF-8, limites, nonce/geração; callback incremental não acumula `Vec` proporcional a rajada de marcas. Marcas inválidas contam apenas métricas sem payload, rate-limit.

Offsets do tracker incluem todos os bytes brutos, não se confundem com `seq` (por frame na 02) ou posições de linha do xterm. Separar segmentos dentro do mesmo chunk: `texto anterior → C → texto do bloco → D → prompt`. Não enviar todos os marks antes do chunk inteiro: isso atribuiria a saída ao bloco errado.

### 5.2. Tracker

Fases `Unknown/Prompt/Input/Executing`. Handshake não cria bloco. A/B alteram apenas fase do prompt. E/cwd são pendentes por g; C válido cria `CommandBlockId`, `ExecutionRef` e `seq` de sessão. Comando ausente fica `None`; UI exibe “Comando indisponível”, não usa `cmdline` como verdade retroativa.

`g` no fio é `hookGeneration`; `ExecutionRef.generation` é contador runtime crescente por PTY, também usado pelo fallback. Manter mapeamento separado hookGeneration→ExecutionRef para evitar colisão quando `/proc` já gerou execuções antes do primeiro C. Guardar só geração atual/última e pendente (limite constante); replay antigo não abre novo bloco. Eventos/DB/ACK usam sempre geração runtime.

- C duplicado da mesma g: idempotente. C de g maior com bloco aberto: fecha anterior `unknown/MissingEnd`, abre novo. D só fecha execução correspondente uma vez.
- D inexistente, g antiga, outro PTY: ignorar. A confiável após execução sem D: fechar `unknown/MissingEnd`; prompt não prova código zero.
- Cwd antes de C é o diretório de execução; cwd após D é o diretório do próximo prompt. `cd` não reescreve retrospectivamente o cwd do próprio bloco.
- Exit do PTY após D não altera resultado já conhecido. Exit com bloco aberto fecha `unknown/PtyExited`, salvo evidência explícita de que o código pertence ao comando (D): código do shell é mantido em `ptyExit`, não atribuído arbitrariamente ao filho.
- Relógio monotônico para duration; wall clock apenas timestamps. Desligamento drena parser/captura e encerra blocos desconhecidos antes de fechar DB.

### 5.3. Fila e captura

Mailbox por PTY limitada a 8 MiB **e** 4096 mensagens, output por segmento ≤64 KiB, metadados ≤limites do protocolo. Reservar 64 posições de controle e um slot terminal fora da fila para `Exit/Overflow`; não usar canal unbounded “apenas para marcas”. Um sequenciador ordena `Start`, `Bytes`, `Gap`, `End`, `Mode`, `Exit` com ordinal interno.

- Pressão de dados: descartar captura, não saída ao xterm; agregar `Gap { droppedBytes }` no próximo controle reservado. `outputTruncated=true` e contagem de bytes real permanecem.
- Saturação também de controle: latch `capture_overflow`, desabilitar tracking fino, invalidar bloco aberto como `unknown`, descartar metadata pendente e reconciliar no próximo handshake/prompt seguro. Slot terminal garante que exit seja aplicado; não prometer entrega exata de todos os blocos sob sobrecarga ilimitada.
- Registry é atualizado pelo sequenciador antes de awaits de DB. Worker de captura e ponte de persistência usam filas limitadas; cada mensagem carrega id/g, de modo que atraso de bloco antigo não reverta estado novo.
- Captura: parser ANSI/UTF-8 incremental (`vte`), CSI/OSC/DCS descartados; CRLF preservado, CR isolado reinicia linha de progresso. `StreamSanitizer` antes de qualquer recorte: só texto seguro entra em cabeça 16 KiB, cauda 48 KiB e ring de até 200 linhas ×4 KiB.
- Não sanitizar apenas após head/tail: um corte pode remover `Bearer`/`TOKEN=` e vazar o valor. Em linha/contexto parcial perdido por `Gap`, invalidar carry, descartar fragmento até fronteira segura e inserir marcador de omissão. PEM ou estado sensível incompleto após gap suprime restante do bloco se não for possível provar ressincronização. Não concatenar lados de lacuna como texto contíguo.
- Campos livres de repositório são `SanitizedText`; command/cwd também sanitizados antes de eventos e DB. Contexto de erro é a cauda sanitizada do próprio bloco falho, sem prompt, eco anterior ou bloco seguinte.

### 5.4. Fallback `/proc`

Usar `PtyManager::{pids,foreground_process,process_report}`/`ProcSource`, fora do reader. Tick 250 ms quando ativo (cache/deduplicação), identidade `(pid,start_time)`; PGID não é necessariamente PID vivo. Resolver membro existente do grupo conforme base02. Falha/ausência de leitura não significa processo encerrado.

Novo grupo foreground pertencente à sessão sugere execução; retorno do foreground ao grupo do shell sugere fim `exitCode=None`, `confidence=inferred`. Builtins, funções sem filho, pipelines curtos e tarefas entre polls podem passar despercebidos. Mesmo PGID do shell **não prova prompt** (builtin pode estar executando); não autorizar input rico por essa observação. Background/`jobs` não vira automaticamente execução foreground.

`cmdline` é argv reconstruído, não linha digitada: qualidade parcial; redigir antes de emitir. Não coletar ambiente de processos. Mudança de cwd observada tem origem inferida.

Na chegada tardia de handshake confiável, cancelar monitor e serializar troca: encerrar observação inferida como `unknown/ModeSwitched`; iniciar próxima geração runtime apenas em C válido. Não ressuscitar bloco já fechado nem tentar unir execuções pela semelhança de cmdline. Se o handshake chegar no meio de comando sem C posterior, aguardar próximo prompt/comando; observação fica incompleta em vez de inventar execução.

## 6. Máquina de estados, visualização e agregação

```rust
pub enum TabStatus { Running, Continuous, Finished, Error, Idle }
pub struct PaneState {
    pub binding: RuntimeBinding,
    pub alive: bool,
    pub status: TabStatus,
    pub execution: Option<ExecutionRef>,
    pub completion: Option<CompletionSnapshot>,
    pub seen_execution: Option<ExecutionRef>,
    pub attention: Option<AttentionSnapshot>,
    pub policy: ContinuousPolicy,
    pub revision: u64,
}
pub struct CompletionSnapshot {
    pub execution: ExecutionRef,
    pub outcome: BlockStatus, pub exit_code: Option<i32>,
    pub reason: BlockEndReason, pub completed_at_ms: i64,
}
pub struct SeenRequest {
    pub tab_id: TabId, pub pane_id: PaneId, pub execution: ExecutionRef,
    pub observed_revision: u64,
}
```

`seen` no DTO é derivado de igualdade da conclusão atual com `seen_execution`; `running/continuous/idle` não têm badge pendente. Preservar `lastExitCode`, último bloco e conclusão mesmo após visualização; seen não é booleano solto reutilizado na próxima execução. `AttentionSnapshot` (extensão 13) tem `attentionId`, `execution`, `revision`, `seen`; confirmar atenção é separado de confirmar conclusão, sem encerrar bloco em execução.

### 6.1. Transições normativas

Redutor puro `transition(state, input) -> Option<StateChange>`, inputs com binding/ExecutionRef. `None` significa idempotência ou evento incompatível. `revision` cresce apenas quando conteúdo muda; timestamps/reason não geram mudança sozinhos.

| Entrada/condição | Estado/resultados |
|---|---|
| Bind/spawn confirmado | `idle`, alive, sem execução; sessão independente da 07 |
| Started com nova g e binding atual | `running` ou `continuous`; anterior não vista não contamina nova g; histórico mantém resultado anterior |
| Started repetido para mesmo id/g | Nenhuma mudança |
| Finished da execução atual, código 0 | `finished`, outcome succeeded, `seen=false` até ACK desta conclusão |
| Finished atual, código ≠0 (inclusive D=130) | `error`, outcome failed, `seen=false` |
| Finished atual, código ausente | `idle`, outcome unknown, sem badge de sucesso/erro; mostrar “Resultado desconhecido” no bloco |
| Finished antigo, outro PTY/bloco ou duplicado | Não altera execução atual; persistência do bloco antigo pode completar |
| Seen de conclusão atual efetivamente apresentada | Mantém status/resultado, `seen=true`; bucket finished/error → idle |
| Seen de geração anterior ou durante running | Ignorar; nunca autoriza confirmação futura |
| Política On/Off durante execução | `continuous`/`running`; Auto reavalia heurística; não altera resultado concluído |
| Exit PTY com bloco aberto | Fecha bloco unknown, alive=false, status idle; erro/sinal do PTY em campo separado |
| Exit PTY após conclusão conhecida | alive=false, preserva conclusão/status/seen |
| Novo binding/reset após encerrar PTY anterior | idle e nova identidade; ignorar mensagens do binding anterior |
| AttentionRequired atual (futuro) | Overlay de atenção não vista produz error no badge; execução/atividade permanecem |
| AttentionCleared/ACK de attentionId atual | Remove/confirma overlay; estado da execução permanece |

### 6.2. Foco e ACK por geração

`tab_set_focus` registra aba/pane visível e revisão de foco, mas **não marca automaticamente nenhuma execução**. O frontend, com janela focada, documento visível e painel apresentado, após renderizar snapshot de conclusão, envia `SeenRequest` contendo a revisão recebida. Mesmo aba continuamente focada precisa ACK após D. Blur/ocultação cancelam ACK pendente; não debounçar blur. Usar sequência monotônica de foco por janela para ignorar IPC atrasado.

Backend valida janela autorizada, binding atual, conclusão correspondente, revisão observada ≥ revisão da conclusão e ≤ revisão atual, e foco ainda válido. `tab_mark_seen` para ação explícita também recebe a execução exata observada; nunca “marcar tudo que houver agora”. Pane oculto dentro de split futuro não é confirmado pelo simples foco da aba. O playground fornece visibilidade sem depender da 07.

### 6.3. Classificação contínua

`ContinuousPolicy::On` vence tudo; `Off` impede heurísticas; `Auto` usa padrões conservadores. Política enviada no spawn vale para o vínculo até override; a futura 07 pode restringir por comando inicial alterando política na próxima geração. Não adivinhar duração.

`ContinuousDetector` retorna `{ continuous, source, confidence }`. Tokenizar de modo conservador, sem executar/expandir shell; comando parcial/composto ambíguo não promove. Padrões positivos: `npm/pnpm/yarn/bun run dev`, `vite dev`, `php artisan serve|horizon|queue:work`, `cargo watch`, `tail -f`, servidor Python. Excluir `build`, `--help`, `--version`, `docker compose up -d/--detach`. Não usar lookahead no crate `regex`; filtros negativos separados. Detector de portas é extensão futura, não requisito.

### 6.4. Agregação reutilizável

`TabStateSnapshot` contém `tabId`, `revision`, `status`, `seen`, `badgeBucket`, `representativePaneId`, `panes: PaneStateDto[]`, `hasActiveProcesses`, `lastExitCode`, `lastBlockId`, `changedAtMs`, `reason`. DTO do pane inclui source/confidence/mode, policy e conclusão; não vaza nonce.

Prioridade do bucket por pane: atenção/erro **não visto** > running > continuous > finished **não visto** > idle (incluindo conclusões vistas e unknown). Em empate: ordinal runtime da última mudança, depois PaneId lexical; não depender de relógio de parede. Campo status representa pane vencedor e badgeBucket sua projeção, portanto status pode continuar error com badgeBucket idle quando visto.

`countsFor` deduplica TabIds e conta cada aba registrada em exatamente um bucket. Aba removida não conta como idle. `hasActiveProcesses` é OR de execuções ativas de **todos** os panes (mesmo com overlay error ou pane encerrado como representante). Remoção de último pane emite `tab://state-removed {tabId, revision}`; tombstone impede ressurreição por evento atrasado. Splits/layout reais ficam na 07.

## 7. Persistência: sessões mínimas, blocos e índices

### 7.1. Ownership

Esta fase cria `sessions` mínima; 07 a estende por migration posterior, sem recriar tabela ou exigir que já exista. `tab_id`, `pane_id`, `project_id` são identidades lógicas, sem FK para tabelas futuras. A 05 mantém integridade do vínculo no serviço. Sessão corresponde a um pane; encerramento fecha sua sessão. `SessionProvider` reutilizável evita dependência circular.

Migration `NNNN_05_shell_sessions_command_blocks.sql`, próximo número global após base03. Tabelas STRICT; INTEGER PRIMARY KEY estabiliza rowid para FTS mesmo em manutenção. Uuid continua id público TEXT UNIQUE; rowid nunca sai por IPC.

```sql
CREATE TABLE sessions (
    id TEXT NOT NULL PRIMARY KEY,
    tab_id TEXT NOT NULL,
    pane_id TEXT NOT NULL,
    project_id TEXT,
    pty_id TEXT,
    shell TEXT NOT NULL,
    cwd TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    exit_code INTEGER,
    exit_signal INTEGER,
    next_seq INTEGER NOT NULL DEFAULT 1 CHECK (next_seq > 0)
) STRICT;
CREATE INDEX idx_sessions_tab_started ON sessions(tab_id, started_at DESC);
CREATE INDEX idx_sessions_active ON sessions(status) WHERE status = 'active';

CREATE TABLE command_blocks (
    rowid INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    tab_id TEXT NOT NULL,
    pane_id TEXT NOT NULL,
    pty_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    seq INTEGER NOT NULL CHECK (seq > 0),
    command TEXT,
    command_quality TEXT NOT NULL CHECK (command_quality IN ('complete','partial','unavailable')),
    cwd TEXT,
    shell TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('hooks','proc')),
    source TEXT NOT NULL,
    confidence TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running','succeeded','failed','unknown')),
    exit_code INTEGER,
    end_reason TEXT,
    continuous INTEGER NOT NULL CHECK (continuous IN (0,1)),
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    output_start_seq INTEGER NOT NULL,
    output_start_offset INTEGER NOT NULL CHECK (output_start_offset >= 0),
    output_end_seq INTEGER,
    output_end_offset INTEGER,
    output_bytes INTEGER NOT NULL DEFAULT 0 CHECK (output_bytes >= 0),
    output_truncated INTEGER NOT NULL DEFAULT 0 CHECK (output_truncated IN (0,1)),
    dropped_bytes INTEGER NOT NULL DEFAULT 0 CHECK (dropped_bytes >= 0),
    output_text TEXT,
    error_tail TEXT,
    UNIQUE(session_id, seq),
    UNIQUE(pty_id, generation),
    CHECK ((status = 'running' AND ended_at IS NULL AND exit_code IS NULL)
        OR (status = 'succeeded' AND ended_at IS NOT NULL AND exit_code IS NOT NULL AND exit_code = 0)
        OR (status = 'failed' AND ended_at IS NOT NULL AND exit_code IS NOT NULL AND exit_code != 0)
        OR (status = 'unknown' AND ended_at IS NOT NULL AND exit_code IS NULL))
) STRICT;
CREATE INDEX idx_blocks_tab_started ON command_blocks(tab_id, started_at DESC);
CREATE INDEX idx_blocks_failed ON command_blocks(session_id, started_at DESC) WHERE status = 'failed';
CREATE INDEX idx_blocks_running ON command_blocks(status) WHERE status = 'running';

CREATE VIRTUAL TABLE command_blocks_fts USING fts5(
    command, cwd, output_text,
    content='command_blocks', content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2', prefix='2 3 4'
);
CREATE TRIGGER command_blocks_fts_ai AFTER INSERT ON command_blocks BEGIN
    INSERT INTO command_blocks_fts(rowid, command, cwd, output_text)
    VALUES (new.rowid, new.command, new.cwd, new.output_text);
END;
CREATE TRIGGER command_blocks_fts_ad AFTER DELETE ON command_blocks BEGIN
    INSERT INTO command_blocks_fts(command_blocks_fts, rowid, command, cwd, output_text)
    VALUES ('delete', old.rowid, old.command, old.cwd, old.output_text);
END;
CREATE TRIGGER command_blocks_fts_au AFTER UPDATE OF command, cwd, output_text ON command_blocks BEGIN
    INSERT INTO command_blocks_fts(command_blocks_fts, rowid, command, cwd, output_text)
    VALUES ('delete', old.rowid, old.command, old.cwd, old.output_text);
    INSERT INTO command_blocks_fts(rowid, command, cwd, output_text)
    VALUES (new.rowid, new.command, new.cwd, new.output_text);
END;
```

`UNIQUE(session_id,seq)` atende paginação reversa; não duplicar índice idêntico DESC. Inteiros persistidos limitados a i64 positivo; overflow vira erro de domínio, nunca wrap. `status` de sessão validado no repo (`active/closed` nesta fase), aberto para `archived` na 07. Registro `FtsSpec` equivalente em `DbSetup.fts`; DDL criado uma vez pela migration, registry usado pela manutenção. Não usar IF NOT EXISTS para esconder schema incompatível.

### 7.2. Repositórios e consistência

`SessionsRepo::{prepare,close,abort,get}` e `CommandBlocksRepo::{start,finish,get,list,search,close_dangling}` em `mt-db`, tipos de entrada em `mt-core`/repo para evitar dependência reversa mt-db→mt-shell. Texto livre de escrita exclusivamente `Option<SanitizedText>`.

- `prepare_spawn` lê `sessions.next_seq` antes de liberar o reader; como há um único PTY vivo por sessão, o sequenciador reserva seq local imediatamente em C e pode emitir Started sem esperar DB. Start confirma a reserva, incrementa next_seq e insere bloco na mesma transação; usar guarda do valor esperado e validar id/g antes de incrementar em retry. No runtime sem DB, reserva local declara persistência falha. Reconciliação nunca sobrescreve sequência de outro bloco; inserções falhadas deixam lacunas explícitas, não renumeram eventos já emitidos.
- INSERT e UPDATE finais serializados por execução via `Db::write`, sem delete/reinsert em retry. Finalização usa guarda `status='running'`; duplicata idêntica é sucesso, conflito é erro.
- Cache sanitizado final limitado a 32 blocos/4 MiB globais atende consulta antes/sem commit; eviction retorna `shell.context_unavailable`. Sem fila infinita ou spool cru em disco.
- Após tentativa de commit (retry transitório limitado), evento Finished inclui `persistence=committed|failed`. Em falha, não apagar INSERT anterior nem emitir promessa de commit; registrar erro sem payload e tornar snapshot consultável. Recovery fecha running pendente como unknown.
- Startup antes de aceitar novos spawns: `close_dangling` fecha execuções de processo anterior `unknown/Recovery`, sem inventar exit code; nunca varrer execuções vivas do processo atual. Shutdown: parar inputs, drenar PTY/captura com prazo, fechar pendentes unknown, flush e só então `Db::shutdown`.
- Queries parametrizadas; `build_match_query` da 03 neutraliza sintaxe FTS. Query vazia → lista vazia, >4096 bytes → `shell.invalid_argument`; max100 resultados, snippet textual escapado pela UI.

## 8. IPC, eventos e DTOs

### 8.1. Comandos

Todos retornam `Result<T,MtError>` e usam capabilities da janela principal. IDs validados contra o vínculo runtime; paths recebidos de OSC não são usados para executar comandos. Registro Specta gera TS, wrappers usam `unwrap` da 01.

| Comando | Args | Retorno |
|---|---|---|
| `pty_spawn` (extensão da 02 no binário) | spec, integration? | PtySummary da 02, desanexado; Channel somente em pty_attach |
| `tab_state_list` | — | TabStateSnapshot[] e watermark de registry |
| `tab_state_get` | tabId | TabStateSnapshot |
| `tab_mark_seen` | request: SeenRequest | TabStateSnapshot (stale ACK é no-op) |
| `tab_set_focus` | focus: FocusSnapshot | () |
| `tab_set_continuous` | tabId, paneId, policy | TabStateSnapshot |
| `shell_integration_info` | ptyId | IntegrationInfo |
| `command_block_list` | sessionId, beforeSeq?, limit (1..500) | Page<CommandBlockDto> |
| `command_block_get` | blockId, includeOutput=false | CommandBlockDto |
| `command_block_error_context` | blockId, maxLines? (default50, 1..200) | ErrorContext |
| `command_block_search` | query, sessionId?, limit (1..100) | CommandBlockHit[] |

`FocusSnapshot`: `tabId: Option<TabId>`, `visiblePaneIds`, `windowFocused`, `documentVisible`, `focusRevision` decimal. Backend identifica janela a partir da chamada, não de string arbitrária fornecida. Falta de foco/ACK não altera resultado. Registry removal/reset é API interna, não comando para matar PTY; ciclo de vida continua pertencendo à 02/07.

Erros: `shell.tab_not_found`, `shell.pane_not_found`, `shell.pty_not_found`, `shell.binding_conflict`, `shell.session_mismatch`, `shell.invalid_argument`, `shell.block_not_found`, `shell.block_running`, `shell.block_not_failed`, `shell.context_unavailable`, mais `db.*`. Mensagens pt-BR sem comando cru/SQL/nonce.

### 8.2. DTOs públicos

```rust
pub struct CommandStarted {
    pub binding: RuntimeBinding, pub execution: ExecutionRef,
    pub seq: u64, pub command: Option<String>, pub command_quality: CommandTextQuality,
    pub cwd: Option<String>, pub started_at_ms: i64, pub output_start: StreamPosition,
    pub continuous: bool, pub source: ObservationSource,
    pub confidence: ObservationConfidence, pub persistence: PersistenceState,
}
pub struct CommandFinished {
    pub binding: RuntimeBinding, pub execution: ExecutionRef,
    pub status: BlockStatus, pub exit_code: Option<i32>,
    pub ended_at_ms: i64, pub duration_ms: u64,
    pub output_end: Option<StreamPosition>, pub output_bytes: u64,
    pub output_truncated: bool, pub dropped_bytes: u64,
    pub end_reason: BlockEndReason, pub persistence: PersistenceState,
    pub has_error_context: bool,
}
pub struct ErrorContext {
    pub execution: ExecutionRef, pub session_id: SessionId,
    pub command: Option<String>, pub command_quality: CommandTextQuality,
    pub cwd: Option<String>, pub exit_code: i32,
    pub tail: String, pub lines: u32, pub truncated: bool,
    pub output_kind: OutputKind, // CombinedPty; nunca Stderr
    pub source: ObservationSource, pub confidence: ObservationConfidence,
    pub persistence: PersistenceState,
}
```

`CommandBlockDto` reúne Started + campos finais opcionais, `id=execution.blockId`, `shell`, `mode`, `outputText` apenas sob includeOutput. `Page<T>={items,nextBeforeSeq}`. `CommandBlockHit={block,snippet,rank}`. `IntegrationInfo={ptyId,shell,mode,reason,scriptsVersion,lastCorrelatedMarkAt,commandTextQuality,promptMarkersAvailable,persistence}`. DTOs de texto sempre sanitizados; `ErrorContext` só para failed conhecido. Campo `truncated` também verdadeiro para captura incompleta/limite de linhas.

### 8.3. Eventos e recuperação

| Evento | Payload/garantia |
|---|---|
| `tab://state-changed` | Snapshot revisionado, após transição runtime, sem esperar DB |
| `tab://state-removed` | tabId/revision tombstone |
| `shell://command-started` | CommandStarted sanitizado, uma vez por execução no fluxo nominal |
| `shell://command-finished` | CommandFinished após resultado da tentativa de persistência; committed garante leitura no DB, failed consulta cache ou erro explícito |
| `shell://cwd-changed` | binding/cwd/source/confidence/revision; debounce100ms sem mudar cwd já capturado no bloco |
| `shell://integration-mode` | binding/mode/reason/revision, inclusive degradação por overflow |

Eventos de shell têm envelope `{eventSeq, binding, payload}`; revisão monotônica por vínculo. Broadcast é apenas notificação, não transporte confiável para DB/registry. Consumidor atrasado detecta lacuna e consulta snapshot/blocos; app interno usa fila direta limitada para persistir. No reload: assinar eventos, carregar snapshots, mesclar por revisão e depois aplicar buffer; atualizar periodicamente e ao recuperar foco para cobrir última notificação perdida sem evento seguinte. Não enviar output pelo bus global.

## 9. Frontend e contratos TS

```ts
import { commands } from '@/types/bindings';
import type { SeenRequest, TabStateSnapshot, CommandBlockId } from '@/types/bindings';
import { unwrap } from '@/ipc/core';

export const markSeen = (request: SeenRequest): Promise<TabStateSnapshot> =>
    unwrap(commands.tabMarkSeen(request));
export const errorContext = (blockId: CommandBlockId, maxLines = 50) =>
    unwrap(commands.commandBlockErrorContext(blockId, maxLines));
```

Não escrever tipos TS paralelos aos DTOs Rust. View models locais podem acrescentar `live`, `positionQuality: 'approximate'|'unavailable'`, ids de marcadores; objetos xterm/IMarker ficam fora de Pinia.

### 9.1. Stores e composables

| Arquivo/contrato | Responsabilidade |
|---|---|
| `stores/tabStatus.ts` / `useTabStatusStore` | byTab, tombstones, readiness; init/dispose idempotentes, merge por revisão, countsFor, unseenTabIds, hasActiveProcesses, markSeen/setFocus/setContinuous |
| `stores/shellHistory.ts` / `useShellHistoryStore` | DTOs por id e paginação sessão, máximo500 por pane; detalhes/caches limitados, erro de consulta explícito |
| `useShellCommandBlocks` | Consumir eventos/snapshot e alimentar o feed existente da 04; deduplicar por ExecutionRef |
| `useShellMarkers` | Posição visual best-effort e descarte de markers após trim/gap/reset; nunca alterar estado backend |
| `useTabSeenTracker` | Foco/visibilidade → render da conclusão → ACK identidade/revisão; cancelar pendências no blur/desmontagem |
| `terminal/extensions/shellIntegration.ts` | `registerTerminalExtension`, contexto cwd correlacionado/inferido e dica de input por fase confiável do prompt |

Init instala listeners antes de pedir estado. Não aplicar otimisticamente `seen=true` ao snapshot inteiro: guardar confirmação pendente por ExecutionRef e esperar retorno; falha não reverte snapshot de outra geração. Dispose remove listeners/timers e invalida promises pendentes por token de inicialização. Retirar registro de pane quando vínculo encerrar/remover, sem recriar outro canal/ACK.

### 9.2. Feed da 04 e posição visual

`feed.begin(paneId,{id,command,cwd,startedAt,line?})`, `markOutputStart`, `end(paneId,id,{exitCode,finishedAt,line?})`, `patchMeta`, `clear` são as APIs reais. `command=null` vira rótulo local “Comando indisponível”; quality permanece em meta. Mapeamento: running→running, succeeded→success, failed→error, unknown→aborted. `meta` inclui source/confidence/persistence/outputTruncated/durationMs/positionQuality; não mudar enum da 04 silenciosamente.

O backend transmite bytes originais e não injeta `mtb=<id>`. Eventos globais podem chegar antes/depois do parse do mesmo output; portanto **não garantir posição exata** a partir do cursor no recebimento do evento. Nesta fase, posicionar apenas após flush do renderer, rotular `approximate` e omitir decoração se não houver âncora válida (alternate screen, gap, replay incompleto). A extensão da 04 deve fornecer callback local pós-flush para esse adaptador, adição da 05; não criar novo parser que muda estado por OSC bruto. Marcadores/linhas não são offsets persistidos nem evidência de execução.

Para produto que exija futuramente marcadores exatos, será necessário protocolo de associação frame/offset→parser no hub, não presumir OSC canônico inexistente. Aqui os blocos e status autoritativos são exatos quanto à execução; a decoração é explicitamente aproximada. Replay nunca chama start/end backend nem duplica histórico. `clear` visual não apaga DB nem redefine geração.

### 9.3. Componentes e input

- `TabStatusIndicator.vue`: props `status`, `seen`, `confidence`, `size`; badge usa bucket; label pt-BR, `role=status`, tooltip de resultado desconhecido, tokens success/info/warning/danger/background.
- `CommandBlockStatus.vue`: props `status`, `exitCode`, `durationMs`, `continuous`, `persistence`, `outputTruncated`; “Código de saída 1”, “Resultado desconhecido”, “Histórico não salvo”, “Saída parcial”. Usar MaxIcon; copiar/inspecionar são ações do registro, sem novo slot fictício.
- Composição opcional do indicador no `CommandBlock.vue` deve ser adição explícita; feed e contrato da 04 permanecem válidos. SFC template/script/style, 4 espaços, tags em linha única.
- Dica `rich` somente após prompt correlacionado A/B e sem execução; C/executando/unknown/proc recomenda passthrough. Prioridades e override obedecem resolver da 04; alt-screen/TUI conserva fidelidade. Cwd não confiável não determina contexto de execução de ações.

## 10. Fluxos e falhas

### Fluxo nominal

1. Playground cria TabId/PaneId da 01 → pty_spawn(integration). `prepare_spawn` prepara sessão; lifecycle injeta e registra binding antes do reader.
2. Handshake/cwd/A/B → hooks e prompt; E/cwd/C(g) → tracker cria bloco, registry running/continuous, Started sanitizado e INSERT.
3. Segmentar output→remover ANSI→sanitizar streaming→reter cabeça/cauda; xterm continua recebendo bytes originais.
4. D(g,code) → estado de conclusão imediatamente; fim de captura/commit em paralelo; Finished anuncia persistence. UI renderiza resultado e envia ACK(g,blockId,revision) se realmente visível.
5. IPC `command_block_error_context` valida falha, retorna cauda combinada sanitizada do DB/cache; 11/12 decidem ações posteriores.

### Falhas obrigatórias

| Situação | Tratamento |
|---|---|
| Spawn falha depois de criar filho | Lifecycle abort + cleanup da 02; sessão sem execução fechada |
| Hook/setup incompatível | proc + motivo; não alterar semântica solicitada silenciosamente |
| OSC inválido/sem fim | Ignorar interpretação, memória limitada, stream intacto |
| Erro `/proc`/PID reutilizado | Unknown/inferred, não inventar fim/sucesso |
| Fila/worker saturado ou encerrado | Degradação visível, unknown, não travar PTY; próximo ponto seguro ressincroniza |
| DB ocupado/falhou | Retry limitado sem duplicata; runtime segue, persistence failed, recovery posterior |
| Captura com gap | Sanitização fail-closed do contexto parcial, outputTruncated; não vazar fragmentos |
| Evento perdido/reordenado | Revisões/tombstones + hidratação; consulta autoritativa |
| `exec`/shell encerra sem D | Bloco unknown; código/sinal do PTY separado; nenhum running órfão |
| App reinicia | Sessions/blocks pendentes fechados unknown por recovery; sem sucesso fabricado |

## 11. Verificação e desempenho

Testes de scanner exaustivos em 2/3 chunks de corpus curto e property tests com chunking aleatório de corpus longo; incluir muitas marcas num único frame, strings de controle aninhadas, fim BEL/ST e limites. Testar atribuição de output quando C/texto/D/A/B coexistem no mesmo chunk.

Tabela de estados inclui confirmação velha, finish após novo start, evento do PTY substituído, foco anterior à conclusão, atenção sem parar execução e agregação de dois panes com erro + processo contínuo. Shells reais usam HOME temporário, profiles com contadores, prompts customizados, traps/arrays, status e nonce nos filhos; testes não confundem sintaxe POSIX com fish.

DB: migration/base03, strict constraints, FTS sincronizado/cascade/manutenção, retry idempotente, crash entre start/end, filas cheias, segredo cortado em todo ponto de chunk/head/tail/gap. Frontend: listeners-before-snapshot, revisões/tombstones, ausência de atualização otimista sobre outra geração, feed único, cleanup, replay/gap e erro de consulta.

Benchmarks preservados: scanner ≥1 GiB/s sem ESC e ≥300 MiB/s corpus ANSI/OSC; overhead ≤5% sobre pipeline02; estado p95≤50ms nominal após D independente de DB. Medir e registrar resultados reais na implementação. Memória por PTY: scanner32KiB + mailbox8MiB + captura≤~1MiB + metadata limitada; cache final global4MiB. Limitar quantidade de workers ao teto de PTYs da02 e testar shutdown/join.

## 12. Dependências e consumidores posteriores

| Contrato publicado pela 05 | Consumidor |
|---|---|
| RuntimeBinding / SessionProvider / lifecycle de spawn / remoção de pane | 07, mantendo autoridade PTY da 02 |
| sessions mínima e migration extensível | 07 (arquivar/reiniciar/histórico) |
| `countsFor`, TabStateSnapshot, geração/ACK | 06 e 07 |
| ContinuousPolicy / detector opcional | 07 e 08 |
| hasActiveProcesses / eventos revisionados | 09; não inferir atividade apenas do badge |
| ErrorContext combinado, sanitizado e limitado | 11 e 12 |
| AttentionSnapshot interno sem conclusão fictícia | 13 |
| command_block_search / FTS | 17 |

Não há dependência de runtime/schema da 07. Adições necessárias nas bases durante implementação: lifecycle genérico na02, argumento opcional no wrapper/binário pty_spawn, repositórios/migration/registro FTS da05 via fachada03 e callback pós-flush/adaptador feed da04. Consulta Context7 tentada nesta revisão retornou cota excedida; APIs externas devem ser confirmadas na implementação contra versões fixadas, sem alegar validação documental inexistente.
