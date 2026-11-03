//! Parser hexadecimal puro para `/proc/net/tcp` e `/proc/net/tcp6` (estado `0x0A` = `TCP_LISTEN`).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::types::{IpFamily, PortAddress};

/// Estado hexadecimal `TCP_LISTEN` no kernel Linux (`0x0A`).
pub const TCP_STATE_LISTEN: u8 = 0x0A;

/// Entrada bruta em estado `TCP_LISTEN` extraída de `/proc/net/tcp` ou `/proc/net/tcp6`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTcpListenEntry {
    /// Família IP (`Ipv4` ou `Ipv6`).
    pub family: IpFamily,
    /// Endereço IP local decodificado.
    pub local_ip: IpAddr,
    /// Porta TCP local (`1..=65535`).
    pub local_port: u16,
    /// Inode do socket no VFS `sockfs`.
    pub inode: u64,
    /// UID efetivo do socket conforme reportado pelo kernel.
    pub uid: u32,
}

impl RawTcpListenEntry {
    /// Converte o endereço local em [`PortAddress`] normalizado.
    #[must_use]
    pub fn to_port_address(&self) -> PortAddress {
        match self.local_ip {
            IpAddr::V4(v4) => {
                let is_wildcard = v4.is_unspecified();
                let is_loopback = v4.is_loopback();
                PortAddress {
                    family: IpFamily::Ipv4,
                    bind_addr: v4.to_string(),
                    is_wildcard,
                    is_loopback,
                }
            }
            IpAddr::V6(v6) => {
                // Verifica se é IPv4-mapped (::ffff:a.b.c.d)
                if let Some(mapped_v4) = v6.to_ipv4_mapped() {
                    let is_wildcard = mapped_v4.is_unspecified();
                    let is_loopback = mapped_v4.is_loopback();
                    PortAddress {
                        family: IpFamily::Ipv6,
                        bind_addr: format!("::ffff:{mapped_v4}"),
                        is_wildcard,
                        is_loopback,
                    }
                } else {
                    let is_wildcard = v6.is_unspecified();
                    let is_loopback = v6.is_loopback();
                    PortAddress {
                        family: IpFamily::Ipv6,
                        bind_addr: v6.to_string(),
                        is_wildcard,
                        is_loopback,
                    }
                }
            }
        }
    }
}

/// Faz o parse do conteúdo textual de `/proc/net/tcp` (`family == Ipv4`) ou `/proc/net/tcp6` (`family == Ipv6`),
/// retornando apenas sockets em estado `0x0A` (`TCP_LISTEN`) com `inode != 0` e `local_port > 0`.
///
/// Se `filter_uid` for `Some(uid)`, mantém apenas entradas cujo campo `uid` coincida com `uid`.
#[must_use]
pub fn parse_proc_net_tcp(
    content: &str,
    family: IpFamily,
    filter_uid: Option<u32>,
) -> Vec<RawTcpListenEntry> {
    let mut results = Vec::new();

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Pula cabeçalho ("sl  local_address rem_address   st ...")
        if idx == 0 && trimmed.starts_with("sl") {
            continue;
        }

        let mut cols = trimmed.split_whitespace();
        let _sl = cols.next();
        let Some(local_addr_col) = cols.next() else {
            continue;
        };
        let _rem_addr_col = cols.next();
        let Some(state_col) = cols.next() else {
            continue;
        };
        // Colunas 4, 5, 6: tx_queue:rx_queue, tr:tm->when, retrnsmt
        let _tx_rx = cols.next();
        let _tr_tm = cols.next();
        let _retrnsmt = cols.next();
        let Some(uid_col) = cols.next() else {
            continue;
        };
        let _timeout = cols.next();
        let Some(inode_col) = cols.next() else {
            continue;
        };

        let Ok(state) = u8::from_str_radix(state_col, 16) else {
            continue;
        };
        if state != TCP_STATE_LISTEN {
            continue;
        }

        let Ok(uid) = uid_col.parse::<u32>() else {
            continue;
        };
        if let Some(expected_uid) = filter_uid
            && uid != expected_uid
        {
            continue;
        }

        let Ok(inode) = inode_col.parse::<u64>() else {
            continue;
        };
        if inode == 0 {
            continue;
        }

        let Some((ip_hex, port_hex)) = local_addr_col.split_once(':') else {
            continue;
        };
        let Ok(local_port) = u16::from_str_radix(port_hex, 16) else {
            continue;
        };
        if local_port == 0 {
            continue;
        }

        let local_ip = match family {
            IpFamily::Ipv4 => {
                let Some(v4) = decode_ipv4_hex(ip_hex) else {
                    continue;
                };
                IpAddr::V4(v4)
            }
            IpFamily::Ipv6 => {
                let Some(v6) = decode_ipv6_hex(ip_hex) else {
                    continue;
                };
                IpAddr::V6(v6)
            }
        };

        results.push(RawTcpListenEntry {
            family,
            local_ip,
            local_port,
            inode,
            uid,
        });
    }

    results
}

/// Decodifica um endereço IPv4 hexadecimal de 8 caracteres em little-endian (padrão `x86_64` Linux `/proc/net/tcp`).
#[must_use]
pub fn decode_ipv4_hex(hex_str: &str) -> Option<Ipv4Addr> {
    if hex_str.len() != 8 {
        return None;
    }
    let word = u32::from_str_radix(hex_str, 16).ok()?;
    let octets = word.to_ne_bytes();
    Some(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]))
}

/// Decodifica um endereço IPv6 hexadecimal de 32 caracteres em 4 palavras `u32` little-endian (`/proc/net/tcp6`).
#[must_use]
pub fn decode_ipv6_hex(hex_str: &str) -> Option<Ipv6Addr> {
    if hex_str.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for i in 0..4 {
        let chunk = &hex_str[i * 8..(i + 1) * 8];
        let word = u32::from_str_radix(chunk, 16).ok()?;
        let word_bytes = word.to_ne_bytes();
        bytes[i * 4..(i + 1) * 4].copy_from_slice(&word_bytes);
    }
    Some(Ipv6Addr::from(bytes))
}
