# Especificação Técnica — Implementação 05: Integração Tauri IPC, Channel Binário, Eventos Globais e Capabilities

## 1. Contexto, Objetivo e Escopo

Esta é a quinta unidade de implementação da **Fase 02 — Core PTY e Supervisão de Processos**. Seu objetivo é acoplar o crate `mt-pty` ao crate binário Tauri (`src-tauri/src/`), expondo os contratos IPC tipados para o frontend sem degradar a vazão binária nem bloquear o event loop do Tauri:
1. O adaptador `BinaryPtyFrame` e `ChannelSink` (`src-tauri/src/events/pty.rs`), implementando `tauri::ipc::IpcResponse` com `InvokeResponseBody::Raw(Vec<u8>)`, com integração Specta sujeita ao gate T-06 (§3.1) e alias manual TS `PtyChannelMessage = ArrayBuffer`, de modo que o WebView receba `ArrayBuffer` puro sem serialização JSON ou base64.
2. O `TauriEventListener` (`src-tauri/src/events/pty.rs`), convertendo `PtyEvent` nos quatro eventos globais (`pty://spawned`, `pty://exit`, `pty://foreground-changed`, `pty://cleanup-failed`) registrados no builder único do `tauri-specta`.
3. Os nove comandos Tauri (`src-tauri/src/commands/pty.rs`): `pty_spawn`, `pty_write`, `pty_resize`, `pty_kill`, `pty_list`, `pty_process_info`, `pty_ack`, `pty_attach` (com `tauri::ipc::Channel<BinaryPtyFrame>` como argumento direto) e `pty_detach`.
4. A inicialização do `Arc<PtyManager>` em `src-tauri/src/lib.rs`, instalação de guards no startup, distinção entre `WindowEvent::CloseRequested` (que **não** mata PTYs) e `RunEvent::ExitRequested` / `RunEvent::Exit` (que coordena `PtyManager::shutdown(ExitReason::Shutdown)` em worker assíncrono antes de liberar o encerramento do processo).
5. As permissões ACL restritas à janela `main` (`src-tauri/capabilities/default.json` e `src-tauri/build.rs`) e a regeneração determinística de `src/types/bindings.ts`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Criados
- `src-tauri/src/events/mod.rs` — declaração do módulo `pub mod pty;`.
- `src-tauri/src/events/pty.rs` — `BinaryPtyFrame`, `ChannelSink`, `TauriEventListener` e structs `tauri_specta::Event` para os 4 eventos `pty://*`.
- `src-tauri/src/commands/pty.rs` — implementação dos 9 comandos `#[tauri::command]` + `#[specta::specta]`.

### 2.2. Arquivos Modificados
- `src-tauri/Cargo.toml` — adição da dependência `mt-pty = { path = "crates/mt-pty" }` e `bytes` no crate binário `maxterminal`.
- `src-tauri/build.rs` — registro dos 9 comandos `pty_*` no `tauri_build::InlinedPlugin` / manifesto ACL do aplicativo para gerar as permissões `allow-pty-*`.
- `src-tauri/capabilities/default.json` — adição das permissões dos comandos `pty_*` restritas a `"windows": ["main"]`.
- `src-tauri/src/commands/mod.rs` — exposição de `pub mod pty;`.
- `src-tauri/src/lib.rs` — registro dos 9 comandos e 4 eventos no builder `tauri_specta`, gerenciamento do estado `Arc<PtyManager>`, hooks `WindowEvent::CloseRequested`, `RunEvent::ExitRequested` e `RunEvent::Exit`, e testes de integração IPC (`mock_builder` + teste de transporte `BinaryPtyFrame`).
- `src/types/bindings.ts` — regenerado automaticamente via `cargo run --bin export_bindings` (sem edição manual).

---

## 3. Especificação Técnica Detalhada e Assinaturas Rust

### 3.1. `BinaryPtyFrame`, `ChannelSink` e `TauriEventListener` (`src-tauri/src/events/pty.rs`)

```rust
use std::sync::Arc;
use bytes::Bytes;
use mt_pty::{
    OutputSink, PtyCleanupFailedEvent, PtyEvent, PtyEventListener, PtyExitEvent,
    PtyForegroundChangedEvent, PtySpawnedEvent, SinkClosed,
};
use tauri::{AppHandle, Emitter, Runtime};
use tauri::ipc::{Channel, InvokeResponseBody, IpcResponse};

/// Envelope binário enviado pelo `tauri::ipc::Channel` diretamente como `InvokeResponseBody::Raw`
/// e entregue ao JavaScript como `ArrayBuffer`. **Não** é um payload JSON: ver contrato abaixo.
#[derive(Clone, Debug)]
pub struct BinaryPtyFrame(pub Bytes);

impl IpcResponse for BinaryPtyFrame {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        Ok(InvokeResponseBody::Raw(self.0.to_vec()))
    }
}

// Contrato Specta de `BinaryPtyFrame`: ver "Contrato de `BinaryPtyFrame` (T-06)" logo abaixo.

pub struct ChannelSink {
    channel: Channel<BinaryPtyFrame>,
}

impl ChannelSink {
    pub fn new(channel: Channel<BinaryPtyFrame>) -> Self {
        Self { channel }
    }
}

impl OutputSink for ChannelSink {
    fn send(&self, frame: Bytes) -> Result<(), SinkClosed> {
        self.channel
            .send(BinaryPtyFrame(frame))
            .map_err(|_| SinkClosed)
    }
}

pub struct TauriEventListener<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriEventListener<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> PtyEventListener for TauriEventListener<R> {
    fn on_event(&self, event: &PtyEvent) {
        match event.clone() {
            PtyEvent::Spawned { summary, revision } => {
                let _ = self.app.emit(
                    "pty://spawned",
                    PtySpawnedEvent { summary, revision },
                );
            }
            PtyEvent::ForegroundChanged {
                pty_id,
                pgid,
                process,
                revision,
            } => {
                let _ = self.app.emit(
                    "pty://foreground-changed",
                    PtyForegroundChangedEvent {
                        pty_id,
                        pgid,
                        process,
                        revision,
                    },
                );
            }
            PtyEvent::Exited {
                pty_id,
                owner,
                exit,
                final_seq,
                revision,
            } => {
                let _ = self.app.emit(
                    "pty://exit",
                    PtyExitEvent {
                        pty_id,
                        owner,
                        exit,
                        final_seq,
                        revision,
                    },
                );
            }
            PtyEvent::CleanupFailed {
                pty_id,
                survivors,
                revision,
            } => {
                let _ = self.app.emit(
                    "pty://cleanup-failed",
                    PtyCleanupFailedEvent {
                        pty_id,
                        survivors,
                        revision,
                    },
                );
            }
        }
    }
}
```

#### Contrato de `BinaryPtyFrame` (T-06)

1. **Transporte:** `ChannelSink::send` entrega `BinaryPtyFrame(frame)` pelo `Channel`; `IpcResponse::body` produz
   `InvokeResponseBody::Raw(Vec<u8>)`. Nunca `InvokeResponseBody::Json`, base64 nem array JSON de números.
2. **Specta — gate T-06 pendente de execução:** `BinaryPtyFrame` **não é tipo Specta de payload JSON**.
   `specta = "=2.0.0-rc.27"` não possui primitiva `ArrayBuffer`. Um *marcador opaco* para satisfazer os limites
   de `Channel<T>` é uma estratégia proposta, **não uma implementação demonstrada por esta especificação**.
   O bloco Rust acima está incompleto para registro no Specta até o gate abaixo; comentário ou alias TS não
   implementam `specta::Type`. Não presumir que um derive sem campos, `#[specta(skip)]` em `Bytes` ou uma
   representação vazia seja aceito pelo exportador. O eventual símbolo gerado representa somente metadados
   do canal, nunca um DTO JSON a ser serializado como frame.
3. **TypeScript (declaração manual):** o frontend recebe cada mensagem do canal como `ArrayBuffer`, por um tipo
   declarado manualmente em `src/ipc/ptyFrame.ts` (`export type PtyChannelMessage = ArrayBuffer;`) com comentário
   TSDoc explícito. Os wrappers usam `new Channel<PtyChannelMessage>()` e, no ponto único de chamada a
   `commands.ptyAttach`, o canal é entregue ao parâmetro gerado por um único cast localizado (`as` documentado), pois
   a representação estática do canal será confirmada no gate. Derivar o alvo do cast de
   `Parameters<typeof commands.ptyAttach>[2]`, sem duplicar a assinatura gerada nem usar `any`; o cast não
   converte o payload nem serve como evidência do transporte.
4. **Exceção única à regra de bindings:** `PtyChannelMessage` é a **única** exceção à regra "tipos só via
   `tauri-specta`" (`README.md` §2.2). Qualquer outro tipo de PTY (`PtySummary`, `AttachInfo`, `OutputCursor` etc.)
   continua gerado em `src/types/bindings.ts`, sem espelho manual.
5. **Verificação do alias:** `rg -n '^export type PtyChannelMessage = ArrayBuffer;' src/ipc/ptyFrame.ts` deve
   localizar a declaração; `rg -n 'PtyChannelMessage|ArrayBuffer' src/ipc src/types` serve para inspecionar usos
   e duplicações, não para provar ausência de espelhos. Ausência em `src/types/` é esperada para esse alias e
   não verifica um arquivo que está em `src/ipc/`. Um teste de contrato deve contar exatamente uma declaração
   manual de `PtyChannelMessage`, nesse caminho, além do teste de drift do arquivo gerado.

**Gate em duas etapas (CA-03/CA-35): Rust/exportador antes de iniciar 02/06; TS/WebView antes do aceite conjunto de 02/05–02/06:**

- Com o lockfile e os pins da Fase 01, inspecionar as fontes locais de `specta`, `specta-typescript`,
  `tauri-specta` e Tauri para verificar os limites de `Channel<T>`, a assinatura real de `specta::Type` e a
  representação aceita pelo exportador. Registrar versões, trechos relevantes e o tipo TS efetivamente emitido.
- Implementar na futura aplicação uma prova mínima com o **mesmo** `BinaryPtyFrame(pub Bytes)`, seu `IpcResponse`
  Raw e `pty_attach` no builder único; fornecer a implementação completa de `specta::Type` ou o mecanismo
  suportado constatado nessas fontes. Executar `cargo check --locked` e `cargo run --locked --bin export_bindings`
  em `src-tauri/`, verificar os nove comandos e `Result<AttachInfo, MtError>`, e comparar duas exportações
  temporárias byte a byte. Nenhuma edição manual de `bindings.ts` ou conversão para JSON é permitida.
- Na etapa frontend, executar `npm run typecheck` com o alias e o cast localizado; o aceite conjunto exige
  teste em **WebView real** recebendo `ArrayBuffer` e preservando bytes/ordem (inclusive `0x00`, `0xff`, UTF-8 e
  ANSI partidos), sem base64 ou array JSON. Testar também `IpcResponse::body()` como `Raw` e ACK apenas após
  consumo; `mock_builder` ou teste de cast isolado não substituem a prova de transporte.
- Se a implementação do marcador, a geração ou o transporte falhar, manter **T-06/CA-35 bloqueado**, registrar
  o erro reproduzível e submeter o contrato à arquitetura antes de prosseguir. Não remover `pty_attach` do
  builder nem tratar `unknown`, cast ou corpo JSON como solução. Nesta revisão documental nenhum desses
  testes de compilação/runtime foi executado; o gate permanece aberto até existir evidência da aplicação.

### 3.2. Os 9 Comandos Tauri (`src-tauri/src/commands/pty.rs`)

Todos os comandos são `async`, anotados com `#[tauri::command]` e `#[specta::specta]`, recebem `state: tauri::State<'_, Arc<PtyManager>>` e retornam `Result<T, MtError>`. Comandos que realizam I/O de sistema de arquivos ou spawn (`pty_spawn`, `pty_process_info`) executam em `tauri::async_runtime::spawn_blocking`.

```rust
use std::sync::Arc;
use bytes::Bytes;
use mt_core::{ErrorCode, MtError, ids::PtyId};
use mt_pty::{
    AttachInfo, AttachmentId, KillMode, OutputCursor, PtyError, PtyInput, PtyManager,
    PtyProcessReport, PtySize, PtySummary, SpawnSpec,
};
use serde_json::json;
use tauri::State;
use tauri::ipc::Channel;
use crate::events::pty::{BinaryPtyFrame, ChannelSink};

#[tauri::command]
#[specta::specta]
pub async fn pty_spawn(
    state: State<'_, Arc<PtyManager>>,
    spec: SpawnSpec,
) -> Result<PtySummary, MtError> {
    let manager = Arc::clone(&*state);
    tauri::async_runtime::spawn_blocking(move || manager.spawn(spec, None).map_err(MtError::from))
        .await
        .map_err(|e| {
            MtError::new(ErrorCode("pty.spawn_failed"), format!("Falha na tarefa de spawn: {e}"))
                .with_details(json!({ "stage": "spawn_blocking_join" }))
        })?
}

#[tauri::command]
#[specta::specta]
pub async fn pty_write(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    input: PtyInput,
) -> Result<(), MtError> {
    let data = match input {
        PtyInput::Text { text } => Bytes::from(text.into_bytes()),
        PtyInput::Bytes { bytes } => Bytes::from(bytes),
    };
    state.write(pty_id, data).map_err(MtError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn pty_resize(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    size: PtySize,
) -> Result<(), MtError> {
    let manager = Arc::clone(&*state);
    tauri::async_runtime::spawn_blocking(move || manager.resize(pty_id, size).map_err(MtError::from))
        .await
        .map_err(|e| {
            MtError::new(ErrorCode("pty.io"), format!("Falha na tarefa de resize: {e}"))
                .with_details(json!({ "stage": "spawn_blocking_join" }))
        })?
}

#[tauri::command]
#[specta::specta]
pub async fn pty_kill(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    mode: Option<KillMode>,
) -> Result<(), MtError> {
    state
        .kill(pty_id, mode.unwrap_or(KillMode::Graceful))
        .map_err(MtError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn pty_list(
    state: State<'_, Arc<PtyManager>>,
) -> Result<Vec<PtySummary>, MtError> {
    Ok(state.list())
}

#[tauri::command]
#[specta::specta]
pub async fn pty_process_info(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
) -> Result<PtyProcessReport, MtError> {
    let manager = Arc::clone(&*state);
    tauri::async_runtime::spawn_blocking(move || {
        manager.process_report(pty_id).map_err(MtError::from)
    })
    .await
    .map_err(|e| {
        MtError::new(ErrorCode("pty.proc_unavailable"), format!("Falha na tarefa de inspeção: {e}"))
            .with_details(json!({ "stage": "spawn_blocking_join" }))
    })?
}

#[tauri::command]
#[specta::specta]
pub async fn pty_ack(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    attachment_id: AttachmentId,
    consumed_bytes: String,
) -> Result<(), MtError> {
    let parsed = consumed_bytes.parse::<u64>().map_err(|_| {
        MtError::from(PtyError::AckInvalid(format!(
            "Valor consumedBytes inválido: {consumed_bytes}"
        )))
    })?;
    state
        .ack(pty_id, attachment_id, parsed)
        .map_err(MtError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn pty_attach(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    cursor: Option<OutputCursor>,
    on_output: Channel<BinaryPtyFrame>,
) -> Result<AttachInfo, MtError> {
    let sink = Arc::new(ChannelSink::new(on_output));
    state.attach(pty_id, sink, cursor).map_err(MtError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn pty_detach(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    attachment_id: AttachmentId,
) -> Result<(), MtError> {
    state
        .detach(pty_id, attachment_id)
        .map_err(MtError::from)
}
```

### 3.3. Ciclo de Vida do Aplicativo em `src-tauri/src/lib.rs` (CA-24)

1. **Startup (`setup`)**:
   - Instancia `Arc<PtyManager>` com `PtyConfig::default()`.
   - Chama `manager.install_guards()` (configurando subreaper, panic hook, signal handler e recovery de instâncias mortas).
   - Registra `Arc::new(TauriEventListener::new(app.handle().clone()))` via `manager.add_listener(...)`.
   - Gerencia o estado no Tauri (`app.manage(manager)`).
2. **`WindowEvent::CloseRequested`**:
   - **Não** chama `kill` nem `shutdown` diretamente (**CA-24**), preservando a separação arquitetural para a Fase 09 (System Tray).
3. **`RunEvent::ExitRequested { api, .. }`**:
   - Se o shutdown coordenado ainda não foi concluído:
     - Chama `api.prevent_exit()`.
     - Dispara uma única vez uma thread/task dedicada (`tauri::async_runtime::spawn_blocking`) que executa `let report = manager.shutdown(ExitReason::Shutdown);`.
     - Se `report.pending.is_empty()` (todos os PTYs limpos com `populated=0`), marca `shutdown_complete = true` e chama `app_handle.exit(0)`.
     - Se `report.pending` não estiver vazio (`CleanupFailed`), mantém a supervisão ativa e emite o evento/erro correspondente sem abandonar processos órfãos.
4. **`RunEvent::Exit`**:
   - Verifica se `shutdown_complete` está marcado; como última salvaguarda síncrona não bloqueante, chama `manager.emergency_kill_all()` (notificando o watchdog pelo FD pré-aberto).

### 3.4. Permissões e Capabilities (`src-tauri/capabilities/default.json` e `src-tauri/build.rs`)

- Em `src-tauri/build.rs`: declarar os comandos da aplicação para que o sistema de permissões do Tauri v2 gere as permissões correspondentes (`allow-pty-spawn`, `allow-pty-write`, `allow-pty-resize`, `allow-pty-kill`, `allow-pty-list`, `allow-pty-process-info`, `allow-pty-ack`, `allow-pty-attach`, `allow-pty-detach`).
- Em `src-tauri/capabilities/default.json`: restringir `"windows": ["main"]` e listar explicitamente as permissões dos nove comandos `pty_*`.

---

## 4. Regras de Negócio, Segurança e Casos de Borda

1. **`Channel` como Argumento Direto (CA-35)**: `on_output: Channel<BinaryPtyFrame>` é parâmetro direto de `pty_attach`, nunca aninhado dentro de `SpawnSpec` ou outro struct DTO.
2. **ACK representa Consumo, não Envio no Transporte (CA-35)**: `ChannelSink::send` apenas invoca `channel.send(BinaryPtyFrame(frame))` e retorna `Ok(())`. Isso **não** decrementa `unacked` no `FlowControl`. Somente o comando `pty_ack` invocado pelo frontend após o término de `consumer.write()` libera crédito.
3. **Sincronia de `src/types/bindings.ts` (CA-03)**: O exportador `export_bindings` da Fase 01 deve produzir `src/types/bindings.ts` contendo os 9 comandos, `SpawnSpec`, `PtySummary`, `PtyExitEvent`, `ProcessInfo`, `PtyProcessReport`, `AttachInfo`, `OutputCursor`, etc., com `git diff --exit-code src/types/bindings.ts` limpo.

---

## 5. Critérios de Aceite Mapeados

- **CA-03 (parte Rust/Specta)**: `cargo run --bin export_bindings` gera `src/types/bindings.ts` contendo `PtySummary`, `SpawnSpec`, `PtyExitEvent`, `ProcessInfo` e os nove comandos sem divergências no `git diff`.
- **CA-24**: `CloseRequested` não encerra PTYs; `ExitRequested` bloqueia novos spawns, coordena `shutdown()` fora do event loop e só então conclui `Exit`.
- **CA-29**: Teste de integração Tauri (`tauri::test::mock_builder`) exercita os nove comandos `pty_*` e comprova a serialização de `MtError` (`{ code, message, details? }`) em `pt-BR`.
- **CA-35 (parte Rust)**: `pty_attach` recebe `Channel` como argumento direto, preserva `Result<AttachInfo, MtError>` no Specta RC27 e comprova que `ChannelSink::send(Ok)` sem `pty_ack` não libera crédito no `FlowControl`.
