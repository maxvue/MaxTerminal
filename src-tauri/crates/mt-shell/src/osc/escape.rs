//! Utilitários de decodificação de parâmetros `OSC 133` (`cmd_b64`, `cwd_b64`),
//! desescape de sequências `OSC 633` (`\xNN`, `\\`, `\n`) e URIs `file://` (`OSC 7`).

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

/// Decodifica um valor Base64 (com ou sem padding, standard ou url-safe) em `String` UTF-8.
///
/// Sequências inválidas retornam `None`; bytes não-UTF8 após decodificação são convertidos
/// de forma lossy para preservar robustez com caminhos/comandos arbitrários.
#[must_use]
pub fn decode_base64_utf8(encoded: &str) -> Option<String> {
    let trimmed = encoded.trim();
    if trimmed.is_empty() {
        return Some(String::new());
    }

    let bytes = STANDARD
        .decode(trimmed)
        .or_else(|_| STANDARD_NO_PAD.decode(trimmed))
        .or_else(|_| URL_SAFE.decode(trimmed))
        .or_else(|_| URL_SAFE_NO_PAD.decode(trimmed))
        .ok()?;

    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Desfaz o escape de strings `OSC 633` / `OSC 133` (`\\`, `\n`, `\r`, `\t`, `\xNN`).
#[must_use]
pub fn unescape_osc_value(raw: &str) -> String {
    if !raw.contains('\\') {
        return raw.to_string();
    }

    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut idx = 0;

    while idx < bytes.len() {
        if bytes[idx] == b'\\' && idx + 1 < bytes.len() {
            match bytes[idx + 1] {
                b'\\' => {
                    out.push(b'\\');
                    idx += 2;
                }
                b'n' => {
                    out.push(b'\n');
                    idx += 2;
                }
                b'r' => {
                    out.push(b'\r');
                    idx += 2;
                }
                b't' => {
                    out.push(b'\t');
                    idx += 2;
                }
                b';' => {
                    out.push(b';');
                    idx += 2;
                }
                b'x' if idx + 3 < bytes.len() => {
                    if let Ok(hex_str) = std::str::from_utf8(&bytes[idx + 2..idx + 4])
                        && let Ok(val) = u8::from_str_radix(hex_str, 16)
                    {
                        out.push(val);
                        idx += 4;
                    } else {
                        out.push(bytes[idx]);
                        idx += 1;
                    }
                }
                _ => {
                    out.push(bytes[idx]);
                    idx += 1;
                }
            }
        } else {
            out.push(bytes[idx]);
            idx += 1;
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

/// Extrai e valida um caminho absoluto local a partir de uma URI `file://[host]/path` (`OSC 7`)
/// ou caminho direto `/...`, rejeitando hostnames remotos e caracteres de controle.
#[must_use]
pub fn decode_file_uri_cwd(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path_part = if let Some(rest) = trimmed.strip_prefix("file://") {
        let slash_pos = rest.find('/')?;
        let host = &rest[..slash_pos];
        if !host.is_empty()
            && !host.eq_ignore_ascii_case("localhost")
            && !matches_local_hostname(host)
        {
            return None;
        }
        percent_decode_str(&rest[slash_pos..])?
    } else {
        unescape_osc_value(trimmed)
    };

    normalize_and_validate_cwd(&path_part)
}

/// Valida que um caminho de `cwd` começa com `/`, tem no máximo 4096 bytes e não contém caracteres de controle.
#[must_use]
pub fn normalize_and_validate_cwd(raw: &str) -> Option<String> {
    if !raw.starts_with('/') || raw.len() > 4096 {
        return None;
    }
    if raw.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None;
    }

    // Remove barras duplicadas consecutivas e barra final redundante (exceto "/")
    let mut normalized = String::with_capacity(raw.len());
    let mut prev_slash = false;
    for ch in raw.chars() {
        if ch == '/' {
            if !prev_slash {
                normalized.push('/');
            }
            prev_slash = true;
        } else {
            normalized.push(ch);
            prev_slash = false;
        }
    }
    if normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    Some(normalized)
}

fn matches_local_hostname(host: &str) -> bool {
    if let Ok(local_host) = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .or_else(|_| std::fs::read_to_string("/etc/hostname"))
    {
        let trimmed = local_host.trim();
        if !trimmed.is_empty() {
            return host.eq_ignore_ascii_case(trimmed);
        }
    }
    false
}

fn percent_decode_str(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;

    while idx < bytes.len() {
        if bytes[idx] == b'%' && idx + 2 < bytes.len() {
            let hex_str = std::str::from_utf8(&bytes[idx + 1..idx + 3]).ok()?;
            let decoded = u8::from_str_radix(hex_str, 16).ok()?;
            out.push(decoded);
            idx += 3;
        } else {
            out.push(bytes[idx]);
            idx += 1;
        }
    }

    String::from_utf8(out).ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn decodes_base64_and_unescapes_osc633() {
        let encoded = STANDARD.encode("echo 'olá mundo' && exit 0");
        assert_eq!(
            decode_base64_utf8(&encoded).unwrap(),
            "echo 'olá mundo' && exit 0"
        );

        let escaped = r"echo\x20hello\x3bworld\\ok\nfim";
        assert_eq!(unescape_osc_value(escaped), "echo hello;world\\ok\nfim");
    }

    #[test]
    fn decodes_and_validates_file_uri_and_rejects_remote_or_relative() {
        assert_eq!(
            decode_file_uri_cwd("file://localhost/home/user/meu%20projeto"),
            Some("/home/user/meu projeto".to_string())
        );
        assert_eq!(
            decode_file_uri_cwd("file:///tmp//teste/"),
            Some("/tmp/teste".to_string())
        );
        assert_eq!(
            decode_file_uri_cwd("file://remote-server.example.com/tmp"),
            None
        );
        assert_eq!(decode_file_uri_cwd("relativo/sem/barra"), None);
        assert_eq!(decode_file_uri_cwd("/tmp/com\nquebra"), None);
    }
}
