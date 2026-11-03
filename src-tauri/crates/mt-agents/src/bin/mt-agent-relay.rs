//! Binário sidecar leve `mt-agent-relay` para encaminhamento de `statusLine`, hooks e `codex-notify`
//! ao `RelayServer` do MaxTerminal via Unix domain socket, com encadeamento (`--chain`) do comando original.

#![forbid(unsafe_code)]
#![allow(
    missing_docs,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::expect_used
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde_json::{Value, json};

const MAX_STDIN_BYTES: usize = 64 * 1024;
const SOCKET_TIMEOUT: Duration = Duration::from_millis(150);
const CHAIN_TIMEOUT: Duration = Duration::from_secs(1);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(subcmd) = args.first().map(String::as_str) else {
        std::process::exit(0);
    };

    match subcmd {
        "statusline" => handle_statusline(&args[1..]),
        "hook" => handle_hook(&args[1..]),
        "codex-notify" => handle_codex_notify(&args[1..]),
        _ => {}
    }
    std::process::exit(0);
}

fn read_stdin_bounded() -> Vec<u8> {
    let mut buf = Vec::new();
    let stdin = std::io::stdin();
    let mut handle = stdin.lock().take((MAX_STDIN_BYTES + 1) as u64);
    let _ = handle.read_to_end(&mut buf);
    if buf.len() > MAX_STDIN_BYTES {
        buf.truncate(MAX_STDIN_BYTES);
    }
    buf
}

fn decode_base64_str(encoded: &str) -> Option<String> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .or_else(|_| URL_SAFE_NO_PAD.decode(encoded.trim()))
        .ok()?;
    String::from_utf8(bytes).ok()
}

fn send_to_relay(msg: Value) {
    let Ok(token) = std::env::var("MAXTERMINAL_AGENT_TOKEN") else {
        return;
    };
    let Ok(sock_path) = std::env::var("MAXTERMINAL_RELAY_SOCK") else {
        return;
    };
    if token.trim().is_empty() || sock_path.trim().is_empty() {
        return;
    }

    let mut envelope = match msg {
        Value::Object(map) => map,
        _ => return,
    };
    envelope.insert("v".to_string(), json!(1));
    envelope.insert("token".to_string(), Value::String(token));

    let Ok(mut payload_bytes) = serde_json::to_vec(&Value::Object(envelope)) else {
        return;
    };
    payload_bytes.push(b'\n');

    let Ok(mut stream) = UnixStream::connect(sock_path.trim()) else {
        return;
    };
    let _ = stream.set_write_timeout(Some(SOCKET_TIMEOUT));
    let _ = stream.set_read_timeout(Some(SOCKET_TIMEOUT));
    if stream.write_all(&payload_bytes).is_err() {
        return;
    }
    let _ = stream.flush();

    let mut reader = BufReader::new(stream);
    let mut ack_line = String::new();
    let _ = reader.read_line(&mut ack_line);
}

fn run_chained_command(
    mut child: std::process::Child,
    stdin_bytes: Option<&[u8]>,
    copy_stdout: bool,
) {
    if let Some(bytes) = stdin_bytes
        && let Some(mut child_stdin) = child.stdin.take()
    {
        let _ = child_stdin.write_all(bytes);
    }

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                if copy_stdout && let Some(mut out) = child.stdout.take() {
                    let mut out_bytes = Vec::new();
                    let _ = out.read_to_end(&mut out_bytes);
                    let _ = std::io::stdout().write_all(&out_bytes);
                    let _ = std::io::stdout().flush();
                }
                break;
            }
            Ok(None) => {
                if start.elapsed() >= CHAIN_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn parse_chain_arg(args: &[String]) -> (Option<String>, Vec<String>) {
    let mut chain: Option<String> = None;
    let mut rest = Vec::new();
    let mut idx = 0;
    while idx < args.len() {
        if args[idx] == "--chain" {
            if idx + 1 < args.len() {
                chain = Some(args[idx + 1].clone());
                idx += 2;
                continue;
            }
            idx += 1;
        } else {
            rest.push(args[idx].clone());
            idx += 1;
        }
    }
    (chain, rest)
}

fn handle_statusline(args: &[String]) {
    let (chain_b64, _) = parse_chain_arg(args);
    let stdin_bytes = read_stdin_bounded();
    let parsed: Value = serde_json::from_slice(&stdin_bytes).unwrap_or(Value::Null);

    if !parsed.is_null() {
        send_to_relay(json!({
            "kind": "statusline",
            "payload": parsed
        }));
    }

    if let Some(b64) = chain_b64
        && let Some(cmd_str) = decode_base64_str(&b64)
    {
        if let Ok(child) = Command::new("/bin/sh")
            .args(["-c", &cmd_str])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            run_chained_command(child, Some(&stdin_bytes), true);
        }
        return;
    }

    let display_name = parsed
        .get("model")
        .and_then(|m| {
            m.get("display_name")
                .or_else(|| m.get("displayName"))
                .or_else(|| m.get("id"))
        })
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Claude");

    let used_pct = parsed
        .get("context_window")
        .or_else(|| parsed.get("contextWindow"))
        .and_then(|cw| {
            cw.get("used_percentage")
                .or_else(|| cw.get("usedPercentage"))
        })
        .and_then(Value::as_f64);

    if let Some(pct) = used_pct
        && pct.is_finite()
    {
        #[allow(clippy::cast_possible_truncation)]
        let rounded = pct.round() as i64;
        print!("{display_name} · ctx {rounded}%");
    } else {
        print!("{display_name}");
    }
    let _ = std::io::stdout().flush();
}

fn handle_hook(args: &[String]) {
    let event_name = args.first().cloned().unwrap_or_default();
    let stdin_bytes = read_stdin_bounded();
    let parsed: Value = serde_json::from_slice(&stdin_bytes).unwrap_or(Value::Null);

    if !event_name.is_empty() {
        send_to_relay(json!({
            "kind": "hook",
            "event": event_name,
            "payload": parsed
        }));
    }
    // Sempre exit 0 e stdout completamente vazio (CA-21).
}

fn handle_codex_notify(args: &[String]) {
    let (chain_b64, positional) = parse_chain_arg(args);
    let raw_json = positional.last().cloned().unwrap_or_default();
    let parsed: Value = serde_json::from_str(&raw_json).unwrap_or(Value::Null);

    if !parsed.is_null() {
        send_to_relay(json!({
            "kind": "codex_notify",
            "payload": parsed
        }));
    }

    if let Some(b64) = chain_b64
        && let Some(json_argv) = decode_base64_str(&b64)
        && let Ok(argv) = serde_json::from_str::<Vec<String>>(&json_argv)
        && let Some((prog, prog_args)) = argv.split_first()
    {
        let mut cmd = Command::new(prog);
        cmd.args(prog_args);
        if !raw_json.is_empty() {
            cmd.arg(&raw_json);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Ok(child) = cmd.spawn() {
            run_chained_command(child, None, false);
        }
    }
}
