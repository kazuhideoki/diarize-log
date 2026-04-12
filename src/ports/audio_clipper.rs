use crate::ports::RecordedAudio;
use std::fmt;
use std::path::Path;
use std::time::Duration;

/// WAV 音声から任意区間を切り出します。
pub trait AudioClipper {
    fn clip_wav_segment(
        &self,
        wav_path: &Path,
        start_offset: Duration,
        duration: Duration,
    ) -> Result<RecordedAudio, AudioClipperError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioClipperError {
    ReadSource(String),
    InvalidRange {
        requested_start_ms: u64,
        requested_duration_ms: u64,
        available_duration_ms: u64,
    },
    EncodeClip(String),
}

impl fmt::Display for AudioClipperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadSource(source) => write!(f, "failed to read source wav: {source}"),
            Self::InvalidRange {
                requested_start_ms,
                requested_duration_ms,
                available_duration_ms,
            } => write!(
                f,
                "requested sample exceeds source wav: requested_start_ms={requested_start_ms} requested_duration_ms={requested_duration_ms} available_duration_ms={available_duration_ms}"
            ),
            Self::EncodeClip(source) => write!(f, "failed to encode clipped wav: {source}"),
        }
    }
}

impl std::error::Error for AudioClipperError {}
