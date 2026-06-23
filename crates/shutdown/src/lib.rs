use synapto_interface::sync::mpsc;
use std::error::Error;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

pub type FatalError = Box<dyn Error + Send + Sync + 'static>;

#[derive(Debug)]
pub struct ShutdownResult(pub Result<(), FatalError>);
synapto_interface::register_channel_name!(ShutdownResult, "shutdown_result");

static SHUTDOWN_TX: OnceLock<mpsc::UnboundedSender<ShutdownResult>> = OnceLock::new();
pub static IS_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Initializes the shutdown mechanism, sets a global panic hook, and returns a receiver for the orchestrator.
pub fn init() -> mpsc::UnboundedReceiver<ShutdownResult> {
    let (tx, rx) = mpsc::unbounded_channel();
    SHUTDOWN_TX
        .set(tx)
        .unwrap_or_else(|e| panic!("Shutdown mechanism must be initialized only once: {:?}", e));

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

        trigger_fatal(format!("Panic: {}{}", msg, location));
    }));

    rx
}

/// Checks if the application is currently shutting down.
pub fn is_shutting_down() -> bool {
    IS_SHUTTING_DOWN.load(Ordering::Relaxed)
}

/// Triggers a fatal shutdown with an error.
pub fn trigger_fatal<E: Into<FatalError>>(err: E) {
    IS_SHUTTING_DOWN.store(true, Ordering::Relaxed);
    if let Some(tx) = SHUTDOWN_TX.get() {
        tx.send(ShutdownResult(Err(err.into())))
            .inspect_err(|e| tracing::trace!("Channel send failed: {:?}", e))
            .ok();
    }
}

/// Triggers a graceful shutdown.
pub fn trigger_graceful() {
    IS_SHUTTING_DOWN.store(true, Ordering::Relaxed);
    if let Some(tx) = SHUTDOWN_TX.get() {
        tx.send(ShutdownResult(Ok(())))
            .inspect_err(|e| tracing::trace!("Channel send failed: {:?}", e))
            .ok();
    }
}
