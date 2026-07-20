use std::error::Error;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use synapto_interface::sync::mpsc;

pub type FatalError = Box<dyn Error + Send + Sync + 'static>;

#[derive(Debug)]
pub struct ShutdownResult(pub Result<(), FatalError>);

// Replaced OnceLock with Mutex<Option<..>> to allow the shutdown mechanism
// to be safely re-initialized when running multiple tests sequentially
// in the same OS process (e.g., `cargo test --test-threads=1`).
static SHUTDOWN_TX: Mutex<Option<mpsc::UnboundedSender<ShutdownResult>>> = Mutex::new(None);
static IS_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

fn shutdown_tx_lock()
-> std::sync::MutexGuard<'static, Option<mpsc::UnboundedSender<ShutdownResult>>> {
    match SHUTDOWN_TX.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Initializes the shutdown mechanism, sets a global panic hook, and returns a receiver for the orchestrator.
pub fn init() -> mpsc::UnboundedReceiver<ShutdownResult> {
    let (tx, rx) = mpsc::unbounded_channel();
    *shutdown_tx_lock() = Some(tx);
    IS_SHUTTING_DOWN.store(false, Ordering::Relaxed);

    std::panic::set_hook(Box::new(|panic_info| {
        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };
        let location = panic_info
            .location()
            .map(|l| format!(" at {}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        let backtrace = std::backtrace::Backtrace::capture();

        let has_tracing = tracing::dispatcher::has_been_set();

        if has_tracing {
            tracing::error!("Panic: {}{}\nBacktrace:\n{}", msg, location, backtrace);
        } else {
            eprintln!("Panic: {}{}\nBacktrace:\n{}", msg, location, backtrace);
        }

        trigger_fatal(format!(
            "Panic: {}{}\nBacktrace:\n{}",
            msg, location, backtrace
        ));
    }));

    rx
}

/// Checks if the shutdown mechanism has been initialized.
pub fn is_initialized() -> bool {
    shutdown_tx_lock()
        .as_ref()
        .map_or(false, |tx| !tx.is_closed())
}

/// Checks if the application is currently shutting down.
pub fn is_shutting_down() -> bool {
    IS_SHUTTING_DOWN.load(Ordering::Relaxed)
}

/// Triggers a fatal shutdown with an error.
pub fn trigger_fatal<E: Into<FatalError>>(err: E) {
    IS_SHUTTING_DOWN.store(true, Ordering::Relaxed);
    if let Some(tx) = shutdown_tx_lock().as_ref() {
        tx.send(ShutdownResult(Err(err.into())))
            .inspect_err(|e| tracing::trace!("Channel send failed: {:?}", e))
            .ok();
    }
}

/// Triggers a graceful shutdown.
pub fn trigger_graceful() {
    IS_SHUTTING_DOWN.store(true, Ordering::Relaxed);
    if let Some(tx) = shutdown_tx_lock().as_ref() {
        tx.send(ShutdownResult(Ok(())))
            .inspect_err(|e| tracing::trace!("Channel send failed: {:?}", e))
            .ok();
    }
}
