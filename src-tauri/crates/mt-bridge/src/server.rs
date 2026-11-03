//! Servidor HTTP/1.1 e WebSocket RFC 6455 em loopback (`127.0.0.1`) para a ponte `MaxTerminal` ⇄ `MaxCode`.
//!
//! Atende exclusivamente em `127.0.0.1`:
//! - `GET /v1/health`: diagnóstico local (protegido contra origens de navegador).
//! - `POST /v1/rpc`: envio/recebimento de `RpcEnvelope` autenticado por `Authorization: Bearer <token>`.
//! - `GET /v1/ws`: sessão WebSocket bidirecional autenticada com handshake `hello`/`welcome` e limite de 256 KiB por frame.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use http::{HeaderMap, HeaderName, HeaderValue};
use parking_lot::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

use crate::auth::{is_allowed_origin, verify_request_headers};
use crate::config::BridgeSettings;
use crate::discovery::{
    DiscoveryEndpoint, MAXTERMINAL_ENDPOINT_FILE, remove_if_owned, resolve_discovery_dir,
    write_atomic,
};
use crate::error::{BridgeError, BridgeErrorCode};
use crate::handler::{InboundContext, dispatch_inbound, validate_hello};
use crate::protocol::{
    ErrorPayload, MAX_ENVELOPE_BYTES, PROTOCOL_VERSION, PeerRole, RpcEnvelope, RpcPayload,
    WelcomePayload, current_unix_ms, default_maxterminal_capabilities,
};
use crate::session::{PeerInfo, SessionRegistry};

/// GUID padrão da RFC 6455 para cálculo de `Sec-WebSocket-Accept`.
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Limite máximo de bytes para o cabeçalho HTTP inicial (16 KiB).
const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;

/// Tempo máximo para aguardar a mensagem `hello` inicial após o upgrade WebSocket (5 segundos).
const WS_HELLO_TIMEOUT: Duration = Duration::from_secs(5);

/// Configuração necessária para iniciar o servidor loopback do `MaxTerminal`.
pub struct LoopbackServerConfig {
    /// Porta preferencial (`Some(1024..=65535)`) ou `None` para porta efêmera (`127.0.0.1:0`).
    pub preferred_port: Option<u16>,
    /// Diretório customizado de descoberta (usado em testes de integração).
    pub discovery_dir_override: Option<PathBuf>,
    /// Token CSPRNG de 256 bits em hexadecimal.
    pub token: String,
    /// Identificador único desta instância do `MaxTerminal`.
    pub instance_id: String,
    /// Versão semântica do `MaxTerminal`.
    pub app_version: String,
    /// Configurações compartilhadas da ponte.
    pub settings: Arc<RwLock<BridgeSettings>>,
    /// Registro de sessões e ACKs pendentes.
    pub sessions: Arc<SessionRegistry>,
    /// Contexto de despacho de mensagens recebidas.
    pub inbound_ctx: Arc<InboundContext>,
}

/// Handle de controle do servidor loopback em execução.
pub struct LoopbackServerHandle {
    /// Porta TCP efetivamente alocada em `127.0.0.1`.
    pub port: u16,
    /// Token ativo do servidor.
    pub token: String,
    /// Caminho do arquivo `maxterminal.endpoint.json` publicado.
    pub endpoint_path: PathBuf,
    /// Identificador da instância proprietária.
    pub instance_id: String,
    shutdown_tx: Option<oneshot::Sender<()>>,
    join_handle: Option<tokio::task::JoinHandle<()>>,
}

impl LoopbackServerHandle {
    /// Encerra graciosamente o servidor loopback e remove o arquivo `maxterminal.endpoint.json` se pertencer a esta instância.
    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(jh) = self.join_handle.take() {
            let _ = jh.await;
        }
        remove_if_owned(&self.endpoint_path, &self.instance_id);
    }
}

impl Drop for LoopbackServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        remove_if_owned(&self.endpoint_path, &self.instance_id);
    }
}

/// Inicia o servidor loopback em `127.0.0.1`, publica atomicamente `maxterminal.endpoint.json` (`0600`)
/// e dispara a task assíncrona de atendimento HTTP/WebSocket.
pub async fn start_loopback_server(
    cfg: LoopbackServerConfig,
) -> Result<LoopbackServerHandle, BridgeError> {
    let listener = bind_loopback_listener(cfg.preferred_port).await?;
    let local_addr = listener.local_addr().map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao obter endereço local do listener loopback: {e}"),
        )
    })?;
    let port = local_addr.port();

    let discovery_dir = resolve_discovery_dir(cfg.discovery_dir_override.as_deref())?;
    let endpoint_path = discovery_dir.join(MAXTERMINAL_ENDPOINT_FILE);

    let endpoint = DiscoveryEndpoint {
        protocol_version: PROTOCOL_VERSION,
        role: PeerRole::MaxTerminal,
        instance_id: cfg.instance_id.clone(),
        pid: std::process::id(),
        host: "127.0.0.1".to_string(),
        port,
        ws_path: "/v1/ws".to_string(),
        http_rpc_path: "/v1/rpc".to_string(),
        token: cfg.token.clone(),
        started_at_ms: current_unix_ms(),
        capabilities: default_maxterminal_capabilities(),
    };

    write_atomic(&endpoint_path, &endpoint)?;
    cfg.sessions.set_server_port(Some(port));

    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    let token = cfg.token.clone();
    let instance_id = cfg.instance_id.clone();
    let app_version = cfg.app_version.clone();
    let settings = Arc::clone(&cfg.settings);
    let sessions = Arc::clone(&cfg.sessions);
    let inbound_ctx = Arc::clone(&cfg.inbound_ctx);

    let join_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    break;
                }
                accept_res = listener.accept() => {
                    match accept_res {
                        Ok((stream, peer_addr)) => {
                            if !peer_addr.ip().is_loopback() {
                                continue;
                            }
                            let conn_ctx = ConnectionState {
                                port,
                                token: token.clone(),
                                instance_id: instance_id.clone(),
                                app_version: app_version.clone(),
                                settings: Arc::clone(&settings),
                                sessions: Arc::clone(&sessions),
                                inbound_ctx: Arc::clone(&inbound_ctx),
                            };
                            tokio::spawn(async move {
                                let _ = handle_tcp_connection(stream, conn_ctx).await;
                            });
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });

    Ok(LoopbackServerHandle {
        port,
        token: cfg.token,
        endpoint_path,
        instance_id: cfg.instance_id,
        shutdown_tx: Some(shutdown_tx),
        join_handle: Some(join_handle),
    })
}

async fn bind_loopback_listener(preferred_port: Option<u16>) -> Result<TcpListener, BridgeError> {
    if let Some(port) = preferred_port {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
        if let Ok(listener) = TcpListener::bind(addr).await {
            return Ok(listener);
        }
    }
    let fallback_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    TcpListener::bind(fallback_addr).await.map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Não foi possível abrir porta TCP em 127.0.0.1: {e}"),
        )
    })
}

#[derive(Clone)]
struct ConnectionState {
    port: u16,
    token: String,
    instance_id: String,
    app_version: String,
    settings: Arc<RwLock<BridgeSettings>>,
    sessions: Arc<SessionRegistry>,
    inbound_ctx: Arc<InboundContext>,
}

struct ParsedHttpRequest {
    method: String,
    path: String,
    headers: HeaderMap,
    body_prefix: Vec<u8>,
}

async fn handle_tcp_connection(
    mut stream: TcpStream,
    state: ConnectionState,
) -> Result<(), BridgeError> {
    let req = read_http_request_head(&mut stream).await?;

    // Qualquer requisição com Origin de navegador não autorizada recebe 403 imediatamente
    if let Some(origin_val) = req.headers.get(http::header::ORIGIN) {
        let origin_ok = origin_val.to_str().is_ok_and(is_allowed_origin);
        if !origin_ok {
            write_http_json_response(
                &mut stream,
                403,
                "Forbidden",
                r#"{"error":"Origem HTTP não autorizada."}"#.as_bytes(),
            )
            .await?;
            return Ok(());
        }
    }

    // Validação estrita do cabeçalho Host (127.0.0.1:<porta> ou localhost:<porta>, sem duplicatas)
    if !is_valid_loopback_host(&req.headers, state.port) {
        write_http_json_response(
            &mut stream,
            403,
            "Forbidden",
            r#"{"code":"bridge.host_forbidden","error":"Cabeçalho Host inválido para o servidor loopback."}"#
                .as_bytes(),
        )
        .await?;
        return Ok(());
    }

    if req.method == "GET" && req.path == "/v1/health" {
        let body = serde_json::json!({
            "status": "ok",
            "role": "max_terminal",
            "protocolVersion": PROTOCOL_VERSION,
            "instanceId": state.instance_id,
            "pid": std::process::id(),
            "version": state.app_version,
        });
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        write_http_json_response(&mut stream, 200, "OK", &bytes).await?;
        return Ok(());
    }

    // Demais rotas exigem Authorization: Bearer <token> válido em tempo constante
    if let Err(auth_err) = verify_request_headers(&req.headers, &state.token) {
        let err_body = serde_json::to_vec(&serde_json::json!({
            "code": auth_err.code.as_str(),
            "message": auth_err.message,
        }))
        .unwrap_or_default();
        write_http_json_response(&mut stream, 401, "Unauthorized", &err_body).await?;
        return Ok(());
    }

    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/v1/rpc" | "/v1/messages") => handle_http_rpc(&mut stream, req, &state).await,
        ("GET", "/v1/ws") => handle_ws_upgrade(stream, req, state).await,
        _ => {
            write_http_json_response(
                &mut stream,
                404,
                "Not Found",
                r#"{"error":"Rota não encontrada."}"#.as_bytes(),
            )
            .await
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn handle_http_rpc(
    stream: &mut TcpStream,
    req: ParsedHttpRequest,
    state: &ConnectionState,
) -> Result<(), BridgeError> {
    let content_len = req
        .headers
        .get(http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(req.body_prefix.len());

    if content_len > MAX_ENVELOPE_BYTES {
        write_http_json_response(
            stream,
            413,
            "Payload Too Large",
            r#"{"code":"payload_too_large","message":"Envelope excede o limite de 256 KiB."}"#
                .as_bytes(),
        )
        .await?;
        return Ok(());
    }

    let mut body = req.body_prefix;
    if body.len() < content_len {
        let remaining = content_len - body.len();
        let mut rest = vec![0u8; remaining];
        stream.read_exact(&mut rest).await.map_err(|e| {
            BridgeError::new(
                BridgeErrorCode::TransportIo,
                format!("Falha ao ler corpo HTTP RPC: {e}"),
            )
        })?;
        body.extend_from_slice(&rest);
    } else if body.len() > content_len {
        body.truncate(content_len);
    }

    let envelope: RpcEnvelope = match serde_json::from_slice(&body) {
        Ok(env) => env,
        Err(e) => {
            let err_env = RpcEnvelope::new(RpcPayload::Error(ErrorPayload {
                request_id: None,
                code: BridgeErrorCode::InvalidEnvelope.as_str().to_string(),
                message: format!("JSON de envelope RPC inválido: {e}"),
            }));
            let bytes = serde_json::to_vec(&err_env).unwrap_or_default();
            write_http_json_response(stream, 400, "Bad Request", &bytes).await?;
            return Ok(());
        }
    };

    state.sessions.touch_last_seen();

    if let RpcPayload::Hello(ref hello) = envelope.message {
        if let Err(err) = validate_hello(hello) {
            let err_env = RpcEnvelope::with_id(
                envelope.request_id,
                RpcPayload::Error(ErrorPayload {
                    request_id: Some(envelope.request_id),
                    code: err.code.as_str().to_string(),
                    message: err.message,
                }),
            );
            let bytes = serde_json::to_vec(&err_env).unwrap_or_default();
            write_http_json_response(stream, 400, "Bad Request", &bytes).await?;
            return Ok(());
        }
        state.sessions.update_peer_metadata(PeerInfo {
            instance_id: hello.instance_id.clone(),
            pid: hello.pid,
            version: hello.version.clone(),
            capabilities: hello.capabilities.clone(),
        });
        state
            .inbound_ctx
            .sink
            .emit_status_changed(&state.sessions.snapshot());
    }

    if state.sessions.resolve_if_ack(&envelope) {
        let ack_body =
            serde_json::to_vec(&serde_json::json!({ "accepted": true })).unwrap_or_default();
        write_http_json_response(stream, 200, "OK", &ack_body).await?;
        return Ok(());
    }

    let current_settings = state.settings.read().clone();
    match dispatch_inbound(&state.inbound_ctx, &current_settings, envelope.clone()).await {
        Ok(Some(reply_env)) => {
            let bytes = serde_json::to_vec(&reply_env).unwrap_or_default();
            write_http_json_response(stream, 200, "OK", &bytes).await
        }
        Ok(None) => {
            let ack_body =
                serde_json::to_vec(&serde_json::json!({ "accepted": true })).unwrap_or_default();
            write_http_json_response(stream, 200, "OK", &ack_body).await
        }
        Err(err) => {
            state.inbound_ctx.sink.emit_error(&err);
            let err_env = RpcEnvelope::with_id(
                envelope.request_id,
                RpcPayload::Error(ErrorPayload {
                    request_id: Some(envelope.request_id),
                    code: err.code.as_str().to_string(),
                    message: err.message,
                }),
            );
            let bytes = serde_json::to_vec(&err_env).unwrap_or_default();
            write_http_json_response(stream, 400, "Bad Request", &bytes).await
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn handle_ws_upgrade(
    mut stream: TcpStream,
    req: ParsedHttpRequest,
    state: ConnectionState,
) -> Result<(), BridgeError> {
    let ws_key = req
        .headers
        .get("sec-websocket-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            BridgeError::new(
                BridgeErrorCode::InvalidEnvelope,
                "Cabeçalho Sec-WebSocket-Key ausente no upgrade WebSocket.",
            )
        })?;

    let accept_key = compute_ws_accept_key(ws_key);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept_key}\r\n\r\n"
    );
    stream.write_all(response.as_bytes()).await.map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao enviar 101 Switching Protocols: {e}"),
        )
    })?;

    // Aguarda obrigatoriamente o frame Hello nos primeiros 5 segundos
    let first_frame = tokio::time::timeout(WS_HELLO_TIMEOUT, read_ws_frame(&mut stream))
        .await
        .map_err(|_| {
            BridgeError::new(
                BridgeErrorCode::AckTimeout,
                "Timeout aguardando handshake hello na conexão WebSocket.",
            )
        })??;

    let WsFrame::Text(raw_hello) = first_frame else {
        let _ = write_ws_close(&mut stream, 1002).await;
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            "Primeiro frame WebSocket deve ser texto JSON contendo hello.",
        ));
    };

    let hello_env: RpcEnvelope = serde_json::from_str(&raw_hello).map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            format!("Envelope hello inválido no WebSocket: {e}"),
        )
    })?;

    let RpcPayload::Hello(hello) = hello_env.message else {
        let _ = write_ws_close(&mut stream, 1002).await;
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            "Primeira mensagem WebSocket deve ser do tipo 'hello'.",
        ));
    };

    if let Err(err) = validate_hello(&hello) {
        let err_env = RpcEnvelope::with_id(
            hello_env.request_id,
            RpcPayload::Error(ErrorPayload {
                request_id: Some(hello_env.request_id),
                code: err.code.as_str().to_string(),
                message: err.message.clone(),
            }),
        );
        if let Ok(txt) = serde_json::to_string(&err_env) {
            let _ = write_ws_text(&mut stream, &txt, false).await;
        }
        let _ = write_ws_close(&mut stream, 1002).await;
        return Err(err);
    }

    let (outbound_tx, mut outbound_rx) = mpsc::channel::<RpcEnvelope>(64);
    state.sessions.register_peer(
        PeerInfo {
            instance_id: hello.instance_id.clone(),
            pid: hello.pid,
            version: hello.version.clone(),
            capabilities: hello.capabilities.clone(),
        },
        outbound_tx,
    );
    state
        .inbound_ctx
        .sink
        .emit_status_changed(&state.sessions.snapshot());

    // Envia Welcome
    let active_project_path = state.inbound_ctx.host.active_project_path().await;
    let welcome_env = RpcEnvelope::with_id(
        hello_env.request_id,
        RpcPayload::Welcome(WelcomePayload {
            protocol_version: PROTOCOL_VERSION,
            role: PeerRole::MaxTerminal,
            instance_id: state.instance_id.clone(),
            pid: std::process::id(),
            version: state.app_version.clone(),
            active_project_path,
            capabilities: default_maxterminal_capabilities(),
        }),
    );
    let welcome_json = serde_json::to_string(&welcome_env).unwrap_or_default();
    write_ws_text(&mut stream, &welcome_json, false).await?;

    // Loop principal da sessão WebSocket
    loop {
        tokio::select! {
            out_msg = outbound_rx.recv() => {
                match out_msg {
                    Some(env) => {
                        if let Ok(json) = serde_json::to_string(&env)
                            && write_ws_text(&mut stream, &json, false).await.is_err()
                        {
                            break;
                        }
                    }
                    None => break,
                }
            }
            frame_res = read_ws_frame(&mut stream) => {
                match frame_res {
                    Ok(WsFrame::Text(text)) => {
                        state.sessions.touch_last_seen();
                        let Ok(envelope) = serde_json::from_str::<RpcEnvelope>(&text) else {
                            continue;
                        };
                        if state.sessions.resolve_if_ack(&envelope) {
                            continue;
                        }
                        let current_settings = state.settings.read().clone();
                        match dispatch_inbound(&state.inbound_ctx, &current_settings, envelope.clone()).await {
                            Ok(Some(reply)) => {
                                if let Ok(json) = serde_json::to_string(&reply)
                                    && write_ws_text(&mut stream, &json, false).await.is_err()
                                {
                                    break;
                                }
                            }
                            Ok(None) => {}
                            Err(err) => {
                                state.inbound_ctx.sink.emit_error(&err);
                                let err_env = RpcEnvelope::with_id(
                                    envelope.request_id,
                                    RpcPayload::Error(ErrorPayload {
                                        request_id: Some(envelope.request_id),
                                        code: err.code.as_str().to_string(),
                                        message: err.message,
                                    }),
                                );
                                if let Ok(json) = serde_json::to_string(&err_env) {
                                    let _ = write_ws_text(&mut stream, &json, false).await;
                                }
                            }
                        }
                    }
                    Ok(WsFrame::Ping(payload)) => {
                        if write_ws_frame(&mut stream, 0xA, &payload, false).await.is_err() {
                            break;
                        }
                    }
                    Ok(WsFrame::Pong(_)) => {
                        state.sessions.touch_last_seen();
                    }
                    Ok(WsFrame::Close) => {
                        let _ = write_ws_close(&mut stream, 1000).await;
                        break;
                    }
                    Err(err) => {
                        if err.code == BridgeErrorCode::PayloadTooLarge {
                            let _ = write_ws_close(&mut stream, 1009).await;
                        }
                        break;
                    }
                }
            }
        }
    }

    state.sessions.clear_peer();
    state
        .inbound_ctx
        .sink
        .emit_status_changed(&state.sessions.snapshot());
    Ok(())
}

/// Frame WebSocket decodificado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsFrame {
    /// Frame de texto UTF-8 (`opcode = 0x1`).
    Text(String),
    /// Frame de ping (`opcode = 0x9`).
    Ping(Vec<u8>),
    /// Frame de pong (`opcode = 0xA`).
    Pong(Vec<u8>),
    /// Frame de encerramento (`opcode = 0x8`).
    Close,
}

/// Lê um único frame WebSocket (com suporte a máscara de cliente e limite estrito de `MAX_ENVELOPE_BYTES`).
pub async fn read_ws_frame(stream: &mut TcpStream) -> Result<WsFrame, BridgeError> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header).await.map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::PeerDisconnected,
            format!("Conexão WebSocket encerrada: {e}"),
        )
    })?;

    let opcode = header[0] & 0x0F;
    let masked = (header[1] & 0x80) != 0;
    let mut payload_len = (header[1] & 0x7F) as usize;

    if payload_len == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext).await.map_err(|e| {
            BridgeError::new(
                BridgeErrorCode::TransportIo,
                format!("Falha ao ler comprimento estendido (16 bits) do frame WS: {e}"),
            )
        })?;
        payload_len = u16::from_be_bytes(ext) as usize;
    } else if payload_len == 127 {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext).await.map_err(|e| {
            BridgeError::new(
                BridgeErrorCode::TransportIo,
                format!("Falha ao ler comprimento estendido (64 bits) do frame WS: {e}"),
            )
        })?;
        let len64 = u64::from_be_bytes(ext);
        if len64 > MAX_ENVELOPE_BYTES as u64 {
            return Err(BridgeError::new(
                BridgeErrorCode::PayloadTooLarge,
                "Frame WebSocket excede o limite máximo de 256 KiB.",
            ));
        }
        #[allow(clippy::cast_possible_truncation)]
        {
            payload_len = len64 as usize;
        }
    }

    if payload_len > MAX_ENVELOPE_BYTES {
        return Err(BridgeError::new(
            BridgeErrorCode::PayloadTooLarge,
            "Frame WebSocket excede o limite máximo de 256 KiB.",
        ));
    }

    let mut mask_key = [0u8; 4];
    if masked {
        stream.read_exact(&mut mask_key).await.map_err(|e| {
            BridgeError::new(
                BridgeErrorCode::TransportIo,
                format!("Falha ao ler máscara do frame WS: {e}"),
            )
        })?;
    }

    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 {
        stream.read_exact(&mut payload).await.map_err(|e| {
            BridgeError::new(
                BridgeErrorCode::TransportIo,
                format!("Falha ao ler payload do frame WS: {e}"),
            )
        })?;
    }

    if masked {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask_key[i & 3];
        }
    }

    match opcode {
        0x1 => {
            let text = String::from_utf8(payload).map_err(|_| {
                BridgeError::new(
                    BridgeErrorCode::InvalidEnvelope,
                    "Frame de texto WebSocket não contém UTF-8 válido.",
                )
            })?;
            Ok(WsFrame::Text(text))
        }
        0x8 => Ok(WsFrame::Close),
        0x9 => Ok(WsFrame::Ping(payload)),
        0xA => Ok(WsFrame::Pong(payload)),
        _ => Err(BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            format!("Opcode WebSocket não suportado: {opcode}"),
        )),
    }
}

/// Escreve um frame de texto WebSocket (`masked = true` quando enviado por cliente, `false` pelo servidor).
pub async fn write_ws_text(
    stream: &mut TcpStream,
    text: &str,
    masked: bool,
) -> Result<(), BridgeError> {
    write_ws_frame(stream, 0x1, text.as_bytes(), masked).await
}

/// Escreve um frame Close WebSocket com código de status.
pub async fn write_ws_close(stream: &mut TcpStream, code: u16) -> Result<(), BridgeError> {
    let payload = code.to_be_bytes();
    write_ws_frame(stream, 0x8, &payload, false).await
}

/// Escreve um frame WebSocket genérico.
pub async fn write_ws_frame(
    stream: &mut TcpStream,
    opcode: u8,
    payload: &[u8],
    masked: bool,
) -> Result<(), BridgeError> {
    if payload.len() > MAX_ENVELOPE_BYTES {
        return Err(BridgeError::new(
            BridgeErrorCode::PayloadTooLarge,
            "Frame WebSocket de saída excede o limite de 256 KiB.",
        ));
    }

    let mut frame = Vec::with_capacity(14 + payload.len());
    frame.push(0x80 | (opcode & 0x0F));

    let mask_bit = if masked { 0x80 } else { 0x00 };
    let len = payload.len();
    if len < 126 {
        #[allow(clippy::cast_possible_truncation)]
        frame.push(mask_bit | (len as u8));
    } else if let Ok(len16) = u16::try_from(len) {
        frame.push(mask_bit | 0x7E);
        frame.extend_from_slice(&len16.to_be_bytes());
    } else {
        frame.push(mask_bit | 0x7F);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }

    if masked {
        let mask: [u8; 4] = [0x37, 0xFA, 0x21, 0x3D];
        frame.extend_from_slice(&mask);
        for (i, &b) in payload.iter().enumerate() {
            frame.push(b ^ mask[i & 3]);
        }
    } else {
        frame.extend_from_slice(payload);
    }

    stream.write_all(&frame).await.map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao gravar frame WebSocket: {e}"),
        )
    })
}

async fn read_http_request_head(stream: &mut TcpStream) -> Result<ParsedHttpRequest, BridgeError> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];

    let header_end = loop {
        let n = stream.read(&mut chunk).await.map_err(|e| {
            BridgeError::new(
                BridgeErrorCode::TransportIo,
                format!("Falha ao ler cabeçalho HTTP: {e}"),
            )
        })?;
        if n == 0 {
            return Err(BridgeError::new(
                BridgeErrorCode::PeerDisconnected,
                "Conexão encerrada antes de enviar cabeçalho HTTP.",
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_double_crlf(&buf) {
            break pos;
        }
        if buf.len() > MAX_HTTP_HEADER_BYTES {
            return Err(BridgeError::new(
                BridgeErrorCode::PayloadTooLarge,
                "Cabeçalhos HTTP excedem o limite de 16 KiB.",
            ));
        }
    };

    let head_bytes = &buf[..header_end];
    let body_prefix = buf[(header_end + 4)..].to_vec();

    let head_str = std::str::from_utf8(head_bytes).map_err(|_| {
        BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            "Cabeçalhos HTTP não contêm ASCII/UTF-8 válido.",
        )
    })?;

    let mut lines = head_str.split("\r\n");
    let request_line = lines.next().ok_or_else(|| {
        BridgeError::new(BridgeErrorCode::InvalidEnvelope, "Request line HTTP vazia.")
    })?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_uppercase();
    let path = parts.next().unwrap_or_default().to_string();

    let mut headers = HeaderMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, val)) = line.split_once(':')
            && let Ok(hn) = HeaderName::from_bytes(name.trim().as_bytes())
            && let Ok(hv) = HeaderValue::from_str(val.trim())
        {
            headers.append(hn, hv);
        }
    }

    Ok(ParsedHttpRequest {
        method,
        path,
        headers,
        body_prefix,
    })
}

fn is_valid_loopback_host(headers: &HeaderMap, port: u16) -> bool {
    let mut iter = headers.get_all(http::header::HOST).iter();
    let Some(first) = iter.next() else {
        return false;
    };
    if iter.next().is_some() {
        return false;
    }
    let Ok(raw) = first.to_str() else {
        return false;
    };
    let trimmed = raw.trim();
    let expected_ip = format!("127.0.0.1:{port}");
    let expected_localhost = format!("localhost:{port}");
    trimmed == expected_ip || trimmed.eq_ignore_ascii_case(&expected_localhost)
}

async fn write_http_json_response(
    stream: &mut TcpStream,
    status_code: u16,
    reason: &str,
    body: &[u8],
) -> Result<(), BridgeError> {
    let head = format!(
        "HTTP/1.1 {status_code} {reason}\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao gravar resposta HTTP: {e}"),
        )
    })?;
    stream.write_all(body).await.map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao gravar corpo HTTP: {e}"),
        )
    })?;
    Ok(())
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Calcula o valor `Sec-WebSocket-Accept` (RFC 6455) usando SHA-1 + Base64.
#[must_use]
pub fn compute_ws_accept_key(sec_websocket_key: &str) -> String {
    let mut input = String::with_capacity(sec_websocket_key.trim().len() + WS_GUID.len());
    input.push_str(sec_websocket_key.trim());
    input.push_str(WS_GUID);
    let digest = sha1_digest(input.as_bytes());
    BASE64_STD.encode(digest)
}

/// Implementação compacta e determinística de SHA-1 (FIPS 180-4) exclusiva para o handshake `Sec-WebSocket-Accept` da RFC 6455.
#[must_use]
#[allow(clippy::many_single_char_names)]
fn sha1_digest(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    let bit_len = (data.len() as u64) * 8;
    let mut padded = Vec::with_capacity((data.len() + 72) & !63);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

/// Envia uma requisição HTTP/1.1 `POST /v1/rpc` (ou `GET /v1/health`) para um servidor loopback `127.0.0.1:<port>`.
pub async fn send_http_rpc_envelope(
    port: u16,
    token: &str,
    envelope: &RpcEnvelope,
) -> Result<(u16, Option<RpcEnvelope>), BridgeError> {
    let body = serde_json::to_vec(envelope).map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            format!("Falha ao serializar envelope RPC: {e}"),
        )
    })?;
    if body.len() > MAX_ENVELOPE_BYTES {
        return Err(BridgeError::new(
            BridgeErrorCode::PayloadTooLarge,
            "Envelope RPC excede o limite de 256 KiB.",
        ));
    }

    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect(addr).await.map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::PeerDisconnected,
            format!("Não foi possível conectar ao endpoint loopback 127.0.0.1:{port}: {e}"),
        )
    })?;

    let req_head = format!(
        "POST /v1/rpc HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Authorization: Bearer {token}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(req_head.as_bytes()).await.map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao enviar cabeçalho HTTP RPC: {e}"),
        )
    })?;
    stream.write_all(&body).await.map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao enviar corpo HTTP RPC: {e}"),
        )
    })?;

    let mut resp_bytes = Vec::new();
    stream.read_to_end(&mut resp_bytes).await.map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao ler resposta HTTP RPC: {e}"),
        )
    })?;

    let Some(header_end) = find_double_crlf(&resp_bytes) else {
        return Err(BridgeError::new(
            BridgeErrorCode::TransportIo,
            "Resposta HTTP incompleta do peer loopback.",
        ));
    };

    let head_str = std::str::from_utf8(&resp_bytes[..header_end]).unwrap_or_default();
    let status_line = head_str.lines().next().unwrap_or_default();
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(500);

    let resp_body = &resp_bytes[(header_end + 4)..];
    let parsed_env = serde_json::from_slice::<RpcEnvelope>(resp_body).ok();
    Ok((status_code, parsed_env))
}
