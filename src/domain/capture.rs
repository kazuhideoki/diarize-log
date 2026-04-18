use std::time::Duration;

/// 無音分割を有効化する前に capture のどこまで録るかを表す比率です。
///
/// 冒頭の短い間で細かく request を切りすぎないよう、capture の 80% までは
/// hard limit 到達以外の境界を作らない固定ルールにします。
pub const SILENCE_REQUEST_ARM_AFTER_RATIO: f64 = 0.8;

/// capture の切り出し方を表す業務ルールです。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturePolicy {
    pub recording_duration: Duration,
    pub capture_duration: Duration,
    pub capture_overlap: Duration,
}

impl CapturePolicy {
    /// overlap を考慮した capture の切り出し計画を返します。
    pub fn capture_ranges(&self) -> Vec<CaptureRange> {
        let stride = self
            .capture_duration
            .checked_sub(self.capture_overlap)
            .expect("capture overlap must be smaller than capture duration");
        let mut capture_ranges = Vec::new();
        let mut capture_index = 1_u64;
        let mut start_offset = Duration::ZERO;

        while start_offset < self.recording_duration {
            let duration = (self.recording_duration - start_offset).min(self.capture_duration);
            capture_ranges.push(CaptureRange {
                capture_index,
                start_offset,
                duration,
            });

            if duration < self.capture_duration {
                break;
            }

            start_offset += stride;
            capture_index += 1;
        }

        capture_ranges
    }
}

/// 無音を使った request 境界の切り方です。
#[derive(Debug, Clone, PartialEq)]
pub struct SilenceRequestPolicy {
    /// この dBFS 以下を「無音窓」とみなします。
    pub silence_threshold_dbfs: f64,
    /// arm 後の通常時に必要な無音長です。
    pub silence_min_duration: Duration,
    /// capture 終端に近づいたときの最小無音長です。
    pub tail_silence_min_duration: Duration,
}

impl SilenceRequestPolicy {
    /// 推奨の初期値を返します。
    pub fn recommended() -> Self {
        Self {
            silence_threshold_dbfs: -42.0,
            silence_min_duration: Duration::from_millis(700),
            tail_silence_min_duration: Duration::from_millis(250),
        }
    }

    /// elapsed 時点で無音分割を許可してよいかを返します。
    pub fn is_silence_split_armed(
        &self,
        elapsed: Duration,
        max_capture_duration: Duration,
    ) -> bool {
        elapsed >= arm_after_duration(max_capture_duration)
    }

    /// elapsed 時点で要求する連続無音長を返します。
    ///
    /// capture の終端に近づくほど必要無音長を短くして、
    /// なるべく無音位置で request を確定できるようにします。
    pub fn required_silence_duration(
        &self,
        elapsed: Duration,
        max_capture_duration: Duration,
    ) -> Duration {
        let arm_after = arm_after_duration(max_capture_duration);
        if elapsed <= arm_after {
            return self.silence_min_duration;
        }

        let arm_after_millis = arm_after.as_millis() as f64;
        let max_capture_millis = max_capture_duration.as_millis() as f64;
        let elapsed_millis = elapsed.as_millis() as f64;
        let progress = ((elapsed_millis - arm_after_millis)
            / (max_capture_millis - arm_after_millis))
            .clamp(0.0, 1.0);
        let silence_min_millis = self.silence_min_duration.as_millis() as f64;
        let tail_silence_min_millis = self.tail_silence_min_duration.as_millis() as f64;

        // 通常時の無音長から末尾時の無音長へ線形補間して、
        // capture 終端に近づくほど短い沈黙でも境界にしやすくします。
        Duration::from_millis(
            (silence_min_millis + (tail_silence_min_millis - silence_min_millis) * progress).round()
                as u64,
        )
    }
}

/// 1 capture の確定理由です。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureBoundaryReason {
    Silence,
    MaxDuration,
    Interrupted,
}

/// 録音中セッションが返した、次の capture 境界です。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureBoundary {
    pub duration: Duration,
    pub reason: CaptureBoundaryReason,
}

/// 連続録音から 1 回の capture で扱う区間です。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureRange {
    pub capture_index: u64,
    pub start_offset: Duration,
    pub duration: Duration,
}

impl CaptureRange {
    /// capture の終了位置を返します。
    pub fn end_offset(&self) -> Duration {
        self.start_offset + self.duration
    }
}

fn arm_after_duration(max_capture_duration: Duration) -> Duration {
    Duration::from_millis(
        (max_capture_duration.as_millis() as f64 * SILENCE_REQUEST_ARM_AFTER_RATIO).round() as u64,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        CapturePolicy, CaptureRange, SILENCE_REQUEST_ARM_AFTER_RATIO, SilenceRequestPolicy,
    };
    use std::time::Duration;

    #[test]
    /// overlap を保ちながら capture 範囲を最後の端数まで計画する。
    fn builds_overlapping_capture_ranges_until_recording_ends() {
        let policy = CapturePolicy {
            recording_duration: Duration::from_secs(360),
            capture_duration: Duration::from_secs(180),
            capture_overlap: Duration::from_secs(15),
        };

        assert_eq!(
            policy.capture_ranges(),
            vec![
                CaptureRange {
                    capture_index: 1,
                    start_offset: Duration::from_secs(0),
                    duration: Duration::from_secs(180),
                },
                CaptureRange {
                    capture_index: 2,
                    start_offset: Duration::from_secs(165),
                    duration: Duration::from_secs(180),
                },
                CaptureRange {
                    capture_index: 3,
                    start_offset: Duration::from_secs(330),
                    duration: Duration::from_secs(30),
                },
            ]
        );
    }

    #[test]
    /// 無音分割は capture の 80% を超えるまで有効化しない。
    fn arms_silence_split_only_after_fixed_ratio() {
        let policy = SilenceRequestPolicy::recommended();
        let max_capture_duration = Duration::from_secs(30);
        let arm_after = Duration::from_millis(
            (max_capture_duration.as_millis() as f64 * SILENCE_REQUEST_ARM_AFTER_RATIO).round()
                as u64,
        );

        assert!(!policy.is_silence_split_armed(
            arm_after.saturating_sub(Duration::from_millis(1)),
            max_capture_duration,
        ));
        assert!(policy.is_silence_split_armed(arm_after, max_capture_duration));
    }

    #[test]
    /// 要求無音長は capture 終端に近づくほど tail 側の短い値へ下がる。
    fn shortens_required_silence_toward_capture_tail() {
        let policy = SilenceRequestPolicy::recommended();
        let max_capture_duration = Duration::from_secs(30);

        assert_eq!(
            policy.required_silence_duration(Duration::from_secs(24), max_capture_duration),
            Duration::from_millis(700)
        );
        assert_eq!(
            policy.required_silence_duration(Duration::from_secs(30), max_capture_duration),
            Duration::from_millis(250)
        );
        assert_eq!(
            policy.required_silence_duration(Duration::from_secs(27), max_capture_duration),
            Duration::from_millis(475)
        );
    }
}
