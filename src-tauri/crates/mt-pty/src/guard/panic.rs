//! Hook de pânico encadeado que sinaliza o canal de emergência do watchdog sem alocar nem usar locks.

use std::os::fd::{BorrowedFd, OwnedFd};
use std::sync::{Arc, Once, OnceLock};

use arc_swap::ArcSwapOption;
use rustix::io::{dup, write};

use crate::error::{PtyError, PtyResult};

static EMERGENCY_FD: OnceLock<ArcSwapOption<OwnedFd>> = OnceLock::new();
static PANIC_HOOK_ONCE: Once = Once::new();

fn emergency_slot() -> &'static ArcSwapOption<OwnedFd> {
    EMERGENCY_FD.get_or_init(ArcSwapOption::empty)
}

/// Dispara 1 byte (`b"!"`) no descritor de emergência do watchdog de forma lock-free.
pub fn notify_emergency_fd() {
    if let Some(fd) = emergency_slot().load_full() {
        let _ = write(&*fd, b"!");
    }
}

/// Registra (ou atualiza) o descritor de emergência do watchdog e instala o panic hook encadeado.
///
/// # Errors
/// Retorna [`PtyError::Io`] caso a duplicação (`dup`) do descritor falhe.
pub fn install_panic_hook(emergency_fd: BorrowedFd<'_>) -> PtyResult<()> {
    let duplicated = dup(emergency_fd)
        .map_err(|e| PtyError::Io(std::io::Error::from_raw_os_error(e.raw_os_error())))?;
    emergency_slot().store(Some(Arc::new(duplicated)));

    PANIC_HOOK_ONCE.call_once(|| {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            notify_emergency_fd();
            prev_hook(panic_info);
        }));
    });

    Ok(())
}
