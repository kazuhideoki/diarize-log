use crate::domain::RecordedAudio;
use std::fmt;
use std::time::Duration;

/// 録音処理を抽象化します。
pub trait Recorder {
    type Session: RecordingSession;

    fn start_recording(&mut self) -> Result<Self::Session, RecorderError>;
}

/// 録音中セッションから capture 単位で音声を切り出します。
pub trait RecordingSession {
    fn wait_until(&mut self, duration: Duration) -> Result<(), RecorderError>;

    fn capture_wav(
        &mut self,
        start_offset: Duration,
        duration: Duration,
    ) -> Result<RecordedAudio, RecorderError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecorderError {
    NoInputDevice,
    ReadInputConfig(String),
    BuildStream(String),
    PlayStream(String),
    UnsupportedSampleFormat(String),
    CallbackStream(String),
    SampleBufferPoisoned,
    EncodeWav(String),
    CaptureOutOfRange {
        requested_end_ms: u64,
        available_end_ms: u64,
    },
}

impl fmt::Display for RecorderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputDevice => f.write_str("default input device is not available"),
            Self::ReadInputConfig(source) => {
                write!(f, "failed to read default input config: {source}")
            }
            Self::BuildStream(source) => write!(f, "failed to build input stream: {source}"),
            Self::PlayStream(source) => write!(f, "failed to start input stream: {source}"),
            Self::UnsupportedSampleFormat(sample_format) => {
                write!(f, "unsupported input sample format: {sample_format}")
            }
            Self::CallbackStream(message) => write!(f, "stream callback failed: {message}"),
            Self::SampleBufferPoisoned => f.write_str("sample buffer was poisoned"),
            Self::EncodeWav(source) => write!(f, "failed to encode wav: {source}"),
            Self::CaptureOutOfRange {
                requested_end_ms,
                available_end_ms,
            } => write!(
                f,
                "requested capture exceeds recorded audio: requested_end_ms={requested_end_ms} available_end_ms={available_end_ms}"
            ),
        }
    }
}

impl std::error::Error for RecorderError {}
