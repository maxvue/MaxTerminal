//! Ring buffer de replay por PTY (`ReplayRing`) e validação estrita de `OutputCursor`.
//!
//! Armazena frames originais imutáveis até o limite de bytes configurado (padrão `1 MiB`),
//! expulsando sempre frames inteiros pela frente e calculando `Gap` exato quando ocorre truncamento.

use std::collections::VecDeque;

use bytes::Bytes;

use crate::error::{PtyError, PtyResult};
use crate::types::{OutputCursor, U64Decimal};

/// Frame original imutável retido no `ReplayRing`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFrame {
    /// Número de sequência do frame (`>= 1`).
    pub seq: u64,
    /// Offset acumulado de bytes no stream bruto antes deste frame.
    pub byte_offset: u64,
    /// Carga útil bruta do frame.
    pub payload: Bytes,
}

impl StoredFrame {
    /// Retorna o offset absoluto imediatamente após o término deste frame (`byte_offset + payload.len()`).
    #[must_use]
    pub fn end_offset(&self) -> u64 {
        self.byte_offset
            .saturating_add(u64::try_from(self.payload.len()).unwrap_or(u64::MAX))
    }
}

/// Plano calculado ao preparar um `attach` no `ReplayRing`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayPlan {
    /// Cursor a partir do qual o replay efetivamente se inicia.
    pub replay_from: OutputCursor,
    /// Cursor a partir do qual os dados ao vivo (`Data`) começam após o replay.
    pub live_from: OutputCursor,
    /// Indica se parte do histórico solicitado já havia sido expulsa do ring (`Gap`).
    pub truncated: bool,
    /// Tupla `(first_retained_seq, first_retained_offset, dropped_bytes)` quando `truncated == true`.
    pub gap: Option<(u64, u64, u64)>,
    /// Sequência contígua de frames retidos que devem ser enviados como `Replay`.
    pub frames: Vec<StoredFrame>,
}

/// Alias para `ReplayPlan` representando o snapshot imutável de replay no momento do `attach`.
pub type ReplaySnapshot = ReplayPlan;

/// Ring buffer circular em memória que retém os últimos `capacity_bytes` de frames de um PTY.
#[derive(Debug)]
pub struct ReplayRing {
    capacity_bytes: usize,
    retained_bytes: usize,
    frames: VecDeque<StoredFrame>,
    next_seq: u64,
    total_bytes: u64,
}

impl ReplayRing {
    /// Cria um novo `ReplayRing` com capacidade máxima em bytes (`capacity_bytes`).
    #[must_use]
    pub const fn new(capacity_bytes: usize) -> Self {
        Self {
            capacity_bytes,
            retained_bytes: 0,
            frames: VecDeque::new(),
            next_seq: 1,
            total_bytes: 0,
        }
    }

    /// Registra um novo frame bruto no ring, expulsando frames antigos inteiros se necessário.
    pub fn push_frame(&mut self, payload: Bytes) -> StoredFrame {
        let seq = self.next_seq;
        let byte_offset = self.total_bytes;
        let payload_len = payload.len();

        self.next_seq = self.next_seq.saturating_add(1);
        self.total_bytes = self
            .total_bytes
            .saturating_add(u64::try_from(payload_len).unwrap_or(u64::MAX));

        let stored = StoredFrame {
            seq,
            byte_offset,
            payload,
        };

        if self.capacity_bytes == 0 || payload_len > self.capacity_bytes {
            self.frames.clear();
            self.retained_bytes = 0;
            return stored;
        }

        while self.retained_bytes.saturating_add(payload_len) > self.capacity_bytes {
            if let Some(evicted) = self.frames.pop_front() {
                self.retained_bytes = self.retained_bytes.saturating_sub(evicted.payload.len());
            } else {
                break;
            }
        }

        self.retained_bytes = self.retained_bytes.saturating_add(payload_len);
        self.frames.push_back(stored.clone());
        stored
    }

    /// Valida o cursor solicitado e constrói o `ReplayPlan` com `Gap` (se truncado) e frames retidos.
    #[allow(clippy::too_many_lines)]
    pub fn prepare_attach(&self, requested: Option<OutputCursor>) -> PtyResult<ReplayPlan> {
        let req = requested.unwrap_or(OutputCursor {
            next_seq: U64Decimal(1),
            byte_offset: U64Decimal(0),
        });
        let req_seq = req.next_seq.0;
        let req_offset = req.byte_offset.0;

        if req_seq == 0 {
            return Err(PtyError::CursorInvalid(
                "next_seq do cursor deve ser >= 1".into(),
            ));
        }
        if req_seq > self.next_seq {
            return Err(PtyError::CursorInvalid(format!(
                "next_seq do cursor ({req_seq}) está no futuro (atual: {})",
                self.next_seq
            )));
        }
        if req_offset > self.total_bytes {
            return Err(PtyError::CursorInvalid(format!(
                "byte_offset do cursor ({req_offset}) excede total_bytes ({})",
                self.total_bytes
            )));
        }
        if req_seq == 1 && req_offset != 0 {
            return Err(PtyError::CursorInvalid(format!(
                "next_seq=1 exige byte_offset=0, recebido {req_offset}"
            )));
        }

        let live_from = OutputCursor {
            next_seq: U64Decimal(self.next_seq),
            byte_offset: U64Decimal(self.total_bytes),
        };

        if req_seq == self.next_seq {
            if req_offset != self.total_bytes {
                return Err(PtyError::CursorInvalid(format!(
                    "Cursor no topo (next_seq={req_seq}) exige byte_offset={}, recebido {req_offset}",
                    self.total_bytes
                )));
            }
            return Ok(ReplayPlan {
                replay_from: live_from,
                live_from,
                truncated: false,
                gap: None,
                frames: Vec::new(),
            });
        }

        if let Some(first) = self.frames.front() {
            if req_seq >= first.seq {
                let idx = usize::try_from(req_seq - first.seq).unwrap_or(0);
                let Some(target) = self.frames.get(idx) else {
                    return Err(PtyError::CursorInvalid(format!(
                        "Frame seq={req_seq} não encontrado no ring"
                    )));
                };
                if target.byte_offset != req_offset {
                    return Err(PtyError::CursorInvalid(format!(
                        "byte_offset ({req_offset}) desalinhado para seq={req_seq} (esperado {})",
                        target.byte_offset
                    )));
                }
                let frames: Vec<StoredFrame> = self.frames.iter().skip(idx).cloned().collect();
                let replay_from = OutputCursor {
                    next_seq: U64Decimal(req_seq),
                    byte_offset: U64Decimal(req_offset),
                };
                Ok(ReplayPlan {
                    replay_from,
                    live_from,
                    truncated: false,
                    gap: None,
                    frames,
                })
            } else {
                if req_offset >= first.byte_offset {
                    return Err(PtyError::CursorInvalid(format!(
                        "byte_offset ({req_offset}) inconsistente com seq truncado {req_seq} (primeiro retido em offset {})",
                        first.byte_offset
                    )));
                }
                let dropped_bytes = first.byte_offset - req_offset;
                let replay_from = OutputCursor {
                    next_seq: U64Decimal(first.seq),
                    byte_offset: U64Decimal(first.byte_offset),
                };
                let frames: Vec<StoredFrame> = self.frames.iter().cloned().collect();
                Ok(ReplayPlan {
                    replay_from,
                    live_from,
                    truncated: true,
                    gap: Some((first.seq, first.byte_offset, dropped_bytes)),
                    frames,
                })
            }
        } else {
            if req_offset >= self.total_bytes {
                return Err(PtyError::CursorInvalid(format!(
                    "byte_offset ({req_offset}) inconsistente para seq={req_seq} com ring esvaziado (total_bytes={})",
                    self.total_bytes
                )));
            }
            let dropped_bytes = self.total_bytes - req_offset;
            Ok(ReplayPlan {
                replay_from: live_from,
                live_from,
                truncated: true,
                gap: Some((self.next_seq, self.total_bytes, dropped_bytes)),
                frames: Vec::new(),
            })
        }
    }

    /// Retorna o próximo número de sequência a ser atribuído (`>= 1`).
    #[must_use]
    pub const fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Retorna o último número de sequência produzido (`0` se nenhum frame foi emitido).
    #[must_use]
    pub const fn last_seq(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }

    /// Retorna o total acumulado de bytes já produzidos pelo PTY.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Retorna a quantidade de bytes atualmente retidos no ring buffer.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Retorna a capacidade máxima configurada em bytes.
    #[must_use]
    pub const fn capacity_bytes(&self) -> usize {
        self.capacity_bytes
    }

    /// Retorna a quantidade de frames atualmente retidos no ring buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Verifica se não há frames retidos no momento.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn evicts_whole_frames_and_computes_exact_gap_and_replay_plan() {
        let mut ring = ReplayRing::new(10);

        let f1 = ring.push_frame(Bytes::from_static(b"aaaa")); // seq 1, offset 0..4
        let f2 = ring.push_frame(Bytes::from_static(b"bbbb")); // seq 2, offset 4..8
        assert_eq!(f1.seq, 1);
        assert_eq!(f1.byte_offset, 0);
        assert_eq!(f2.seq, 2);
        assert_eq!(f2.byte_offset, 4);
        assert_eq!(ring.retained_bytes(), 8);
        assert_eq!(ring.len(), 2);

        // Inserir 4 bytes ("cccc") excede 10 (8 + 4 = 12 > 10) -> expulsa f1 inteiro
        let f3 = ring.push_frame(Bytes::from_static(b"cccc")); // seq 3, offset 8..12
        assert_eq!(f3.seq, 3);
        assert_eq!(f3.byte_offset, 8);
        assert_eq!(ring.retained_bytes(), 8);
        assert_eq!(ring.len(), 2);

        // Attach desde o início (None) deve sinalizar truncamento de 4 bytes (f1) e entregar f2, f3
        let plan_origin = ring.prepare_attach(None).unwrap();
        assert!(plan_origin.truncated);
        assert_eq!(plan_origin.gap, Some((2, 4, 4)));
        assert_eq!(plan_origin.replay_from.next_seq.0, 2);
        assert_eq!(plan_origin.replay_from.byte_offset.0, 4);
        assert_eq!(plan_origin.live_from.next_seq.0, 4);
        assert_eq!(plan_origin.live_from.byte_offset.0, 12);
        assert_eq!(plan_origin.frames.len(), 2);
        assert_eq!(plan_origin.frames[0].seq, 2);
        assert_eq!(plan_origin.frames[1].seq, 3);

        // Attach a partir de seq=2, offset=4 (ainda retido): sem truncamento
        let plan_retained = ring
            .prepare_attach(Some(OutputCursor {
                next_seq: U64Decimal(2),
                byte_offset: U64Decimal(4),
            }))
            .unwrap();
        assert!(!plan_retained.truncated);
        assert!(plan_retained.gap.is_none());
        assert_eq!(plan_retained.frames.len(), 2);

        // Attach no topo (seq=4, offset=12): vazio e sem truncamento
        let plan_head = ring
            .prepare_attach(Some(OutputCursor {
                next_seq: U64Decimal(4),
                byte_offset: U64Decimal(12),
            }))
            .unwrap();
        assert!(!plan_head.truncated);
        assert_eq!(plan_head.frames.len(), 0);
    }

    #[test]
    fn rejects_invalid_or_misaligned_cursors() {
        let mut ring = ReplayRing::new(100);
        ring.push_frame(Bytes::from_static(b"hello")); // seq 1, offset 0..5
        ring.push_frame(Bytes::from_static(b"world")); // seq 2, offset 5..10

        // seq = 0 inválido
        assert_eq!(
            ring.prepare_attach(Some(OutputCursor {
                next_seq: U64Decimal(0),
                byte_offset: U64Decimal(0),
            }))
            .unwrap_err()
            .code(),
            "pty.cursor_invalid"
        );

        // seq no futuro (4 > 3)
        assert_eq!(
            ring.prepare_attach(Some(OutputCursor {
                next_seq: U64Decimal(4),
                byte_offset: U64Decimal(10),
            }))
            .unwrap_err()
            .code(),
            "pty.cursor_invalid"
        );

        // byte_offset desalinhado para seq=2 (esperado 5, passado 3)
        assert_eq!(
            ring.prepare_attach(Some(OutputCursor {
                next_seq: U64Decimal(2),
                byte_offset: U64Decimal(3),
            }))
            .unwrap_err()
            .code(),
            "pty.cursor_invalid"
        );

        // byte_offset no topo incorreto
        assert_eq!(
            ring.prepare_attach(Some(OutputCursor {
                next_seq: U64Decimal(3),
                byte_offset: U64Decimal(9),
            }))
            .unwrap_err()
            .code(),
            "pty.cursor_invalid"
        );
    }
}
