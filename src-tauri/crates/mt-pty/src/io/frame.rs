//! Codificação e decodificação de frames binários versão 1 (cabeçalho fixo de 24 bytes Little-Endian).
//!
//! O protocolo binário v1 elimina qualquer overhead de JSON ou Base64 no caminho quente de saída
//! do terminal e preserva sequências UTF-8 multibyte ou códigos ANSI divididos entre leituras.

use bytes::{BufMut, Bytes, BytesMut};

use crate::error::{PtyError, PtyResult};
use crate::types::ExitInfo;

/// Versão única do protocolo binário de frames (`0x01`).
pub const FRAME_VERSION: u8 = 0x01;

/// Tamanho fixo em bytes do cabeçalho binário v1 (`24` bytes).
pub const FRAME_HEADER_LEN: usize = 24;

/// Tamanho máximo permitido para o payload de um frame `Data` ou `Replay` (`256 KiB`).
pub const MAX_DATA_FRAME_BYTES: usize = 256 * 1024;

/// Tamanho exato do payload de um frame `Gap` (`8` bytes — `u64` Little-Endian).
pub const GAP_PAYLOAD_LEN: usize = 8;

/// Tamanho máximo permitido para o payload JSON de um frame `End` (`4 KiB`).
pub const MAX_END_PAYLOAD_LEN: usize = 4 * 1024;

/// Tipo semântico do frame binário de saída do PTY.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameKind {
    /// Dados ao vivo produzidos pelo PTY (`0x01`).
    Data = 0x01,
    /// Dados reenfileirados a partir do `ReplayRing` durante `attach` (`0x02`).
    Replay = 0x02,
    /// Notificação de truncamento indicando bytes descartados do ring buffer (`0x03`).
    Gap = 0x03,
    /// Marcador final de encerramento do PTY contendo `ExitInfo` serializado em JSON (`0x04`).
    End = 0x04,
}

impl FrameKind {
    /// Converte o discriminante numérico `u8` em `FrameKind`.
    #[must_use]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::Data),
            0x02 => Some(Self::Replay),
            0x03 => Some(Self::Gap),
            0x04 => Some(Self::End),
            _ => None,
        }
    }
}

/// Cabeçalho binário fixo de 24 bytes em Little-Endian do protocolo v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// Versão do protocolo (`FRAME_VERSION = 1`).
    pub version: u8,
    /// Tipo do frame (`Data`, `Replay`, `Gap` ou `End`).
    pub kind: FrameKind,
    /// Campo reservado para alinhamento (`0`).
    pub reserved: u16,
    /// Número de sequência do frame (`>= 1` para `Data`/`Replay`; `final_seq` em `End`).
    pub seq: u64,
    /// Offset absoluto de bytes no stream bruto antes deste frame.
    pub byte_offset: u64,
    /// Comprimento do payload em bytes.
    pub len: u32,
}

/// Frame binário decodificado e validado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    /// Tipo semântico do frame.
    pub kind: FrameKind,
    /// Número de sequência do frame.
    pub seq: u64,
    /// Offset absoluto no stream bruto de bytes.
    pub byte_offset: u64,
    /// Carga útil bruta do frame (slice zero-copy de `Bytes`).
    pub payload: Bytes,
}

fn validate_payload_len(kind: FrameKind, len: usize) -> PtyResult<u32> {
    match kind {
        FrameKind::Data | FrameKind::Replay => {
            if len == 0 || len > MAX_DATA_FRAME_BYTES {
                return Err(PtyError::InputInvalid(format!(
                    "Tamanho de payload inválido para frame {kind:?}: {len} bytes (permitido 1..={MAX_DATA_FRAME_BYTES})"
                )));
            }
        }
        FrameKind::Gap => {
            if len != GAP_PAYLOAD_LEN {
                return Err(PtyError::InputInvalid(format!(
                    "Tamanho de payload inválido para frame Gap: {len} bytes (esperado {GAP_PAYLOAD_LEN})"
                )));
            }
        }
        FrameKind::End => {
            if len == 0 || len > MAX_END_PAYLOAD_LEN {
                return Err(PtyError::InputInvalid(format!(
                    "Tamanho de payload inválido para frame End: {len} bytes (permitido 1..={MAX_END_PAYLOAD_LEN})"
                )));
            }
        }
    }

    u32::try_from(len)
        .map_err(|_| PtyError::InputInvalid(format!("Tamanho de payload excede u32::MAX: {len}")))
}

/// Codifica um frame binário v1 validando os limites do payload segundo `FrameKind`.
pub fn encode_frame(
    kind: FrameKind,
    seq: u64,
    byte_offset: u64,
    payload: &[u8],
) -> PtyResult<Bytes> {
    let len_u32 = validate_payload_len(kind, payload.len())?;
    let mut buf = BytesMut::with_capacity(FRAME_HEADER_LEN + payload.len());
    buf.put_u8(FRAME_VERSION);
    buf.put_u8(kind as u8);
    buf.put_u16_le(0);
    buf.put_u64_le(seq);
    buf.put_u64_le(byte_offset);
    buf.put_u32_le(len_u32);
    buf.extend_from_slice(payload);
    Ok(buf.freeze())
}

/// Codifica um frame `Data` (`0x01`) com carga útil de `1..=MAX_DATA_FRAME_BYTES`.
pub fn encode_data_frame(seq: u64, byte_offset: u64, payload: &[u8]) -> PtyResult<Bytes> {
    encode_frame(FrameKind::Data, seq, byte_offset, payload)
}

/// Codifica um frame `Replay` (`0x02`) com carga útil de `1..=MAX_DATA_FRAME_BYTES`.
pub fn encode_replay_frame(seq: u64, byte_offset: u64, payload: &[u8]) -> PtyResult<Bytes> {
    encode_frame(FrameKind::Replay, seq, byte_offset, payload)
}

/// Codifica um frame `Gap` (`0x03`) indicando quantos bytes foram descartados antes do primeiro frame retido.
#[must_use]
pub fn encode_gap_frame(
    first_retained_seq: u64,
    first_retained_offset: u64,
    dropped_bytes: u64,
) -> Bytes {
    let mut buf = BytesMut::with_capacity(FRAME_HEADER_LEN + GAP_PAYLOAD_LEN);
    buf.put_u8(FRAME_VERSION);
    buf.put_u8(FrameKind::Gap as u8);
    buf.put_u16_le(0);
    buf.put_u64_le(first_retained_seq);
    buf.put_u64_le(first_retained_offset);
    buf.put_u32_le(8);
    buf.put_u64_le(dropped_bytes);
    buf.freeze()
}

/// Codifica um frame `End` (`0x04`) serializando `ExitInfo` em JSON UTF-8.
pub fn encode_end_frame(final_seq: u64, total_bytes: u64, exit: &ExitInfo) -> PtyResult<Bytes> {
    let payload = serde_json::to_vec(exit).map_err(|e| {
        PtyError::InputInvalid(format!("Falha ao serializar ExitInfo do frame End: {e}"))
    })?;
    encode_frame(FrameKind::End, final_seq, total_bytes, &payload)
}

/// Decodifica e valida integralmente um frame binário v1 a partir de `Bytes`.
#[allow(clippy::needless_pass_by_value)]
pub fn decode_frame(raw: Bytes) -> PtyResult<DecodedFrame> {
    if raw.len() < FRAME_HEADER_LEN {
        return Err(PtyError::InputInvalid(format!(
            "Frame truncado: {} bytes (mínimo {FRAME_HEADER_LEN})",
            raw.len()
        )));
    }

    let version = raw[0];
    if version != FRAME_VERSION {
        return Err(PtyError::InputInvalid(format!(
            "Versão de frame inválida: {version:#04x} (esperado {FRAME_VERSION:#04x})"
        )));
    }

    let kind_raw = raw[1];
    let Some(kind) = FrameKind::from_u8(kind_raw) else {
        return Err(PtyError::InputInvalid(format!(
            "Tipo de frame inválido: {kind_raw:#04x}"
        )));
    };

    let reserved = u16::from_le_bytes([raw[2], raw[3]]);
    if reserved != 0 {
        return Err(PtyError::InputInvalid(format!(
            "Campo reservado do cabeçalho deve ser 0, recebido {reserved:#06x}"
        )));
    }

    let seq = u64::from_le_bytes([
        raw[4], raw[5], raw[6], raw[7], raw[8], raw[9], raw[10], raw[11],
    ]);
    let byte_offset = u64::from_le_bytes([
        raw[12], raw[13], raw[14], raw[15], raw[16], raw[17], raw[18], raw[19],
    ]);
    let len_u32 = u32::from_le_bytes([raw[20], raw[21], raw[22], raw[23]]);
    let payload_len = usize::try_from(len_u32).map_err(|_| {
        PtyError::InputInvalid(format!("Comprimento de payload inválido: {len_u32}"))
    })?;

    let expected_total = FRAME_HEADER_LEN.checked_add(payload_len).ok_or_else(|| {
        PtyError::InputInvalid("Overflow ao calcular tamanho total do frame".into())
    })?;

    if raw.len() != expected_total {
        return Err(PtyError::InputInvalid(format!(
            "Comprimento total do frame ({}) diverge de 24 + len ({expected_total})",
            raw.len()
        )));
    }

    validate_payload_len(kind, payload_len)?;

    let payload = raw.slice(FRAME_HEADER_LEN..);
    Ok(DecodedFrame {
        kind,
        seq,
        byte_offset,
        payload,
    })
}

/// Extrai a quantidade de bytes descartados (`dropped_bytes`) de um frame `Gap` decodificado.
pub fn decode_gap_payload(frame: &DecodedFrame) -> PtyResult<u64> {
    if frame.kind != FrameKind::Gap {
        return Err(PtyError::InputInvalid(format!(
            "Esperado frame Gap, recebido {:?}",
            frame.kind
        )));
    }
    if frame.payload.len() != GAP_PAYLOAD_LEN {
        return Err(PtyError::InputInvalid(format!(
            "Payload de Gap inválido: {} bytes",
            frame.payload.len()
        )));
    }
    let b = &frame.payload;
    Ok(u64::from_le_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

/// Desserializa o `ExitInfo` contido no payload JSON de um frame `End` decodificado.
pub fn decode_end_payload(frame: &DecodedFrame) -> PtyResult<ExitInfo> {
    if frame.kind != FrameKind::End {
        return Err(PtyError::InputInvalid(format!(
            "Esperado frame End, recebido {:?}",
            frame.kind
        )));
    }
    validate_payload_len(FrameKind::End, frame.payload.len())?;
    serde_json::from_slice(&frame.payload)
        .map_err(|e| PtyError::InputInvalid(format!("Payload JSON inválido no frame End: {e}")))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::types::{ExitReason, U64Decimal};
    use proptest::prelude::*;

    #[test]
    fn encodes_and_decodes_all_frame_kinds_exact_24_byte_header() {
        let data_raw = encode_data_frame(1, 0, &[0x00, 0xFF, 0x1B, b'[']).unwrap();
        assert_eq!(data_raw.len(), FRAME_HEADER_LEN + 4);
        assert_eq!(data_raw[0], FRAME_VERSION);
        assert_eq!(data_raw[1], FrameKind::Data as u8);
        assert_eq!(&data_raw[2..4], &[0, 0]);

        let decoded_data = decode_frame(data_raw).unwrap();
        assert_eq!(decoded_data.kind, FrameKind::Data);
        assert_eq!(decoded_data.seq, 1);
        assert_eq!(decoded_data.byte_offset, 0);
        assert_eq!(&decoded_data.payload[..], &[0x00, 0xFF, 0x1B, b'[']);

        let replay_raw = encode_replay_frame(42, 4096, b"replay chunk").unwrap();
        let decoded_replay = decode_frame(replay_raw).unwrap();
        assert_eq!(decoded_replay.kind, FrameKind::Replay);
        assert_eq!(decoded_replay.seq, 42);
        assert_eq!(decoded_replay.byte_offset, 4096);
        assert_eq!(&decoded_replay.payload[..], b"replay chunk");

        let gap_raw = encode_gap_frame(10, 65536, 32768);
        assert_eq!(gap_raw.len(), FRAME_HEADER_LEN + GAP_PAYLOAD_LEN);
        let decoded_gap = decode_frame(gap_raw).unwrap();
        assert_eq!(decoded_gap.kind, FrameKind::Gap);
        assert_eq!(decoded_gap.seq, 10);
        assert_eq!(decoded_gap.byte_offset, 65536);
        assert_eq!(decode_gap_payload(&decoded_gap).unwrap(), 32768);

        let exit = ExitInfo {
            code: Some(0),
            signal: None,
            signal_name: None,
            reason: ExitReason::Exited,
            at_ms: U64Decimal(1_700_000_000_000),
        };
        let end_raw = encode_end_frame(99, 123_456, &exit).unwrap();
        let decoded_end = decode_frame(end_raw).unwrap();
        assert_eq!(decoded_end.kind, FrameKind::End);
        assert_eq!(decoded_end.seq, 99);
        assert_eq!(decoded_end.byte_offset, 123_456);
        assert_eq!(decode_end_payload(&decoded_end).unwrap(), exit);
    }

    #[test]
    fn rejects_invalid_headers_and_payload_bounds() {
        // Menor que 24 bytes
        assert!(decode_frame(Bytes::from_static(&[1, 1, 0])).is_err());

        // Data vazio ou acima de 256 KiB
        assert!(encode_data_frame(1, 0, &[]).is_err());
        let oversized = vec![b'a'; MAX_DATA_FRAME_BYTES + 1];
        assert!(encode_data_frame(1, 0, &oversized).is_err());

        // Versão inválida
        let mut bad_ver = BytesMut::from(&encode_data_frame(1, 0, b"ok").unwrap()[..]);
        bad_ver[0] = 2;
        assert!(decode_frame(bad_ver.freeze()).is_err());

        // Kind inválido
        let mut bad_kind = BytesMut::from(&encode_data_frame(1, 0, b"ok").unwrap()[..]);
        bad_kind[1] = 0x05;
        assert!(decode_frame(bad_kind.freeze()).is_err());

        // Reserved != 0
        let mut bad_res = BytesMut::from(&encode_data_frame(1, 0, b"ok").unwrap()[..]);
        bad_res[2] = 1;
        assert!(decode_frame(bad_res.freeze()).is_err());

        // Len divergente do tamanho real
        let mut bad_len = BytesMut::from(&encode_data_frame(1, 0, b"ok").unwrap()[..]);
        bad_len.put_u8(b'!');
        assert!(decode_frame(bad_len.freeze()).is_err());

        // Gap com tamanho != 8
        assert!(encode_frame(FrameKind::Gap, 1, 0, &[1, 2, 3]).is_err());
    }

    proptest! {
        #[test]
        fn proptest_data_frame_roundtrip_arbitrary_bytes(
            seq in any::<u64>(),
            byte_offset in any::<u64>(),
            payload in proptest::collection::vec(any::<u8>(), 1..=4096)
        ) {
            let encoded = encode_data_frame(seq, byte_offset, &payload).unwrap();
            let decoded = decode_frame(encoded).unwrap();
            prop_assert_eq!(decoded.kind, FrameKind::Data);
            prop_assert_eq!(decoded.seq, seq);
            prop_assert_eq!(decoded.byte_offset, byte_offset);
            prop_assert_eq!(&decoded.payload[..], &payload[..]);
        }

        #[test]
        fn proptest_gap_frame_roundtrip(
            seq in any::<u64>(),
            offset in any::<u64>(),
            dropped in any::<u64>()
        ) {
            let encoded = encode_gap_frame(seq, offset, dropped);
            let decoded = decode_frame(encoded).unwrap();
            prop_assert_eq!(decoded.kind, FrameKind::Gap);
            prop_assert_eq!(decoded.seq, seq);
            prop_assert_eq!(decoded.byte_offset, offset);
            prop_assert_eq!(decode_gap_payload(&decoded).unwrap(), dropped);
        }
    }
}
