/// 実行中に外部から中断要求が入ったかを照会する境界です。
pub trait InterruptMonitor {
    fn is_interrupt_requested(&self) -> bool;
}

/// 録音待機の結果です。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingWaitOutcome {
    ReachedTarget,
    Interrupted,
}
