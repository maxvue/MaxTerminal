//! Abstração de destino de saída binária (`OutputSink`) para transporte de alto volume.

use bytes::Bytes;
use parking_lot::Mutex;

/// Indica que o canal de saída foi fechado ou desconectado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkClosed;

/// Destino dos frames binários de output (alto volume).
pub trait OutputSink: Send + Sync + 'static {
    /// Envia um frame binário codificado. Retorna `Err(SinkClosed)` se o consumidor desconectou.
    fn send(&self, frame: Bytes) -> Result<(), SinkClosed>;
}

/// Implementação de `OutputSink` que descarta todos os frames (para testes e benchmarks).
#[derive(Debug, Default)]
pub struct NullSink;

impl OutputSink for NullSink {
    fn send(&self, _frame: Bytes) -> Result<(), SinkClosed> {
        Ok(())
    }
}

/// Implementação de `OutputSink` em memória que armazena todos os frames recebidos (para testes).
#[derive(Debug, Default)]
pub struct VecSink {
    frames: Mutex<Vec<Bytes>>,
}

impl VecSink {
    /// Cria um novo `VecSink` vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Retira e retorna todos os frames acumulados até o momento.
    #[must_use]
    pub fn take_frames(&self) -> Vec<Bytes> {
        std::mem::take(&mut *self.frames.lock())
    }

    /// Retorna uma cópia de todos os frames acumulados sem esvaziar o buffer.
    #[must_use]
    pub fn frames_snapshot(&self) -> Vec<Bytes> {
        self.frames.lock().clone()
    }

    /// Retorna a quantidade de frames armazenados.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.lock().len()
    }

    /// Verifica se nenhum frame foi recebido.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.lock().is_empty()
    }
}

impl OutputSink for VecSink {
    fn send(&self, frame: Bytes) -> Result<(), SinkClosed> {
        self.frames.lock().push(frame);
        Ok(())
    }
}
