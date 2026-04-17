use std::time::Duration;

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

#[cfg(test)]
mod tests {
    use super::{CapturePolicy, CaptureRange};
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
}
