//! Ponto de extensão `OutputTap` (somente leitura) para observação de bytes brutos (Fase 05 — parser OSC 133).

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use mt_core::ids::PtyId;
use parking_lot::{Condvar, Mutex, RwLock};

use crate::types::ExitInfo;

thread_local! {
    /// Identificador do tap atualmente em execução na thread corrente, usado para evitar
    /// deadlock caso o próprio callback tente remover seu `TapHandle`.
    static CURRENT_TAP_IN_CALLBACK: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Tap somente leitura do output bruto do PTY (consumido pela Fase 05).
///
/// Chamado na thread de leitura, por frame original coalescido, **antes** do envio ao `OutputSink`.
/// Nunca é chamado durante `Replay`.
pub trait OutputTap: Send + Sync + 'static {
    /// Recebe os bytes brutos originais de um frame `Data`.
    fn on_output(&self, pty_id: PtyId, seq: u64, bytes: &[u8]);

    /// Notificado uma única vez após o último callback de bytes e cleanup confirmado do PTY.
    fn on_exit(&self, _pty_id: PtyId, _exit: &ExitInfo) {}
}

struct TapEntry {
    id: u64,
    active: AtomicBool,
    in_flight: AtomicUsize,
    wait_mutex: Mutex<()>,
    wait_cv: Condvar,
    tap: Arc<dyn OutputTap>,
}

impl TapEntry {
    fn new(id: u64, tap: Arc<dyn OutputTap>) -> Self {
        Self {
            id,
            active: AtomicBool::new(true),
            in_flight: AtomicUsize::new(0),
            wait_mutex: Mutex::new(()),
            wait_cv: Condvar::new(),
            tap,
        }
    }
}

/// Registro concorrente de `OutputTap` com snapshot lock-free durante callbacks e barreira de remoção.
#[derive(Default)]
pub struct TapRegistry {
    next_id: AtomicU64,
    entries: RwLock<Vec<Arc<TapEntry>>>,
}

impl TapRegistry {
    /// Cria um novo registro de taps vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra um novo `OutputTap` e retorna seu `TapHandle` RAII.
    #[must_use]
    pub fn register(self: &Arc<Self>, tap: Arc<dyn OutputTap>) -> TapHandle {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let entry = Arc::new(TapEntry::new(id, tap));
        self.entries.write().push(Arc::clone(&entry));
        TapHandle {
            registry: Arc::clone(self),
            entry: Some(entry),
        }
    }

    /// Despacha um frame de saída original para todos os taps ativos fora do lock do registro.
    pub fn dispatch_output(&self, pty_id: PtyId, seq: u64, bytes: &[u8]) {
        let snapshot = self.entries.read().clone();
        for entry in snapshot {
            if !entry.active.load(Ordering::Acquire) {
                continue;
            }
            entry.in_flight.fetch_add(1, Ordering::AcqRel);
            if entry.active.load(Ordering::Acquire) {
                let prev = CURRENT_TAP_IN_CALLBACK.with(|c| c.replace(Some(entry.id)));
                entry.tap.on_output(pty_id, seq, bytes);
                CURRENT_TAP_IN_CALLBACK.with(|c| c.set(prev));
            }
            if entry.in_flight.fetch_sub(1, Ordering::AcqRel) == 1 {
                let _guard = entry.wait_mutex.lock();
                entry.wait_cv.notify_all();
            }
        }
    }

    /// Despacha o evento de saída final do PTY para todos os taps ativos.
    pub fn dispatch_exit(&self, pty_id: PtyId, exit: &ExitInfo) {
        let snapshot = self.entries.read().clone();
        for entry in snapshot {
            if !entry.active.load(Ordering::Acquire) {
                continue;
            }
            entry.in_flight.fetch_add(1, Ordering::AcqRel);
            if entry.active.load(Ordering::Acquire) {
                let prev = CURRENT_TAP_IN_CALLBACK.with(|c| c.replace(Some(entry.id)));
                entry.tap.on_exit(pty_id, exit);
                CURRENT_TAP_IN_CALLBACK.with(|c| c.set(prev));
            }
            if entry.in_flight.fetch_sub(1, Ordering::AcqRel) == 1 {
                let _guard = entry.wait_mutex.lock();
                entry.wait_cv.notify_all();
            }
        }
    }

    fn unregister(&self, entry: &Arc<TapEntry>) {
        entry.active.store(false, Ordering::Release);
        self.entries.write().retain(|e| e.id != entry.id);

        let is_self_callback = CURRENT_TAP_IN_CALLBACK.with(|c| c.get() == Some(entry.id));
        if !is_self_callback {
            let mut guard = entry.wait_mutex.lock();
            while entry.in_flight.load(Ordering::Acquire) > 0 {
                entry.wait_cv.wait(&mut guard);
            }
        }
    }
}

/// Handle RAII que remove o `OutputTap` ao ser descartado e aguarda callbacks em andamento.
pub struct TapHandle {
    registry: Arc<TapRegistry>,
    entry: Option<Arc<TapEntry>>,
}

impl Drop for TapHandle {
    fn drop(&mut self) {
        if let Some(entry) = self.entry.take() {
            self.registry.unregister(&entry);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    struct CountingTap {
        count: AtomicUsize,
    }

    impl OutputTap for CountingTap {
        fn on_output(&self, _pty_id: PtyId, _seq: u64, _bytes: &[u8]) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn registers_dispatches_and_stops_after_drop() {
        let reg = Arc::new(TapRegistry::new());
        let tap = Arc::new(CountingTap {
            count: AtomicUsize::new(0),
        });
        let handle = reg.register(tap.clone());
        let pty_id = PtyId::new();

        reg.dispatch_output(pty_id, 1, b"abc");
        assert_eq!(tap.count.load(Ordering::SeqCst), 1);

        drop(handle);
        reg.dispatch_output(pty_id, 2, b"def");
        assert_eq!(tap.count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn drop_waits_for_in_flight_callback_and_prevents_subsequent_calls() {
        struct BlockingTap {
            entered: Arc<Barrier>,
            release: Arc<Barrier>,
            calls: AtomicUsize,
        }

        impl OutputTap for BlockingTap {
            fn on_output(&self, _pty_id: PtyId, _seq: u64, _bytes: &[u8]) {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.entered.wait();
                self.release.wait();
            }
        }

        let reg = Arc::new(TapRegistry::new());
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let tap = Arc::new(BlockingTap {
            entered: entered.clone(),
            release: release.clone(),
            calls: AtomicUsize::new(0),
        });

        let handle = reg.register(tap.clone());
        let reg_reader = reg.clone();
        let pty_id = PtyId::new();

        let reader_thread = std::thread::spawn(move || {
            reg_reader.dispatch_output(pty_id, 1, b"chunk1");
        });

        entered.wait();

        let dropped = Arc::new(AtomicBool::new(false));
        let dropped_clone = dropped.clone();
        let drop_thread = std::thread::spawn(move || {
            drop(handle);
            dropped_clone.store(true, Ordering::SeqCst);
        });

        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(
            !dropped.load(Ordering::SeqCst),
            "drop(handle) deve aguardar o término do callback em andamento"
        );

        release.wait();
        reader_thread.join().unwrap();
        drop_thread.join().unwrap();
        assert!(dropped.load(Ordering::SeqCst));

        reg.dispatch_output(pty_id, 2, b"chunk2");
        assert_eq!(tap.calls.load(Ordering::SeqCst), 1);
    }
}
