//! Captura de sinais fatais/terminação (`SIGINT`, `SIGTERM`, `SIGHUP`, `SIGQUIT`) via `signal-hook`.

use std::sync::{Arc, Once};
use std::thread;

use arc_swap::ArcSwapOption;
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;

use crate::error::{PtyError, PtyResult};
use crate::guard::panic::notify_emergency_fd;

type SignalCallback = dyn Fn(i32) + Send + Sync + 'static;

static SIGNAL_CALLBACK: std::sync::OnceLock<ArcSwapOption<Box<SignalCallback>>> =
    std::sync::OnceLock::new();
static SIGNAL_THREAD_ONCE: Once = Once::new();

fn callback_slot() -> &'static ArcSwapOption<Box<SignalCallback>> {
    SIGNAL_CALLBACK.get_or_init(ArcSwapOption::empty)
}

/// Instala a thread `mt-pty-sig` para escutar sinais de encerramento e registra o callback.
///
/// # Errors
/// Retorna [`PtyError::Io`] se o registro de sinais do sistema operacional falhar.
pub fn install_signal_handler<F>(on_signal: F) -> PtyResult<()>
where
    F: Fn(i32) + Send + Sync + 'static,
{
    let boxed: Box<SignalCallback> = Box::new(on_signal);
    callback_slot().store(Some(Arc::new(boxed)));

    let mut init_err: Option<std::io::Error> = None;
    SIGNAL_THREAD_ONCE.call_once(|| match Signals::new([SIGINT, SIGTERM, SIGHUP, SIGQUIT]) {
        Ok(mut signals) => {
            let _ = thread::Builder::new()
                .name("mt-pty-sig".to_string())
                .stack_size(256 * 1024)
                .spawn(move || {
                    for sig in signals.forever() {
                        notify_emergency_fd();
                        if let Some(cb) = callback_slot().load_full() {
                            cb(sig);
                        }
                    }
                });
        }
        Err(e) => {
            init_err = Some(e);
        }
    });

    if let Some(e) = init_err {
        return Err(PtyError::Io(e));
    }
    Ok(())
}
