use diarize_log::{InterruptMonitor, LineLogger, Logger};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Default)]
pub(super) struct SignalInterruptState {
    phase: AtomicU8,
}

impl InterruptMonitor for SignalInterruptState {
    fn is_interrupt_requested(&self) -> bool {
        self.phase.load(Ordering::SeqCst) != 0
    }
}

impl SignalInterruptState {
    pub(super) fn install(logger: LineLogger) -> Result<Arc<Self>, ctrlc::Error> {
        let state = Arc::new(Self::default());
        let handler_state = Arc::clone(&state);
        ctrlc::set_handler(move || {
            if handler_state
                .phase
                .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                let _ = logger.info(
                    "interrupt received, stopping after flushing the recorded audio; press Ctrl+C again to abort immediately",
                );
            } else {
                let _ = logger.info("interrupt received again, aborting immediately");
                std::process::exit(130);
            }
        })?;
        Ok(state)
    }
}
