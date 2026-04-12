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
    ReadShareableContent(String),
    ApplicationNotFound {
        bundle_id: String,
    },
    ApplicationDisplayNotFound {
        bundle_id: String,
    },
    BuildStream(String),
    PlayStream(String),
    AddStreamOutput(String),
    StartCapture(String),
    UnsupportedSampleFormat(String),
    CallbackStream(String),
    SampleBufferPoisoned,
    DecodeCapturedAudio(String),
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
            Self::ReadShareableContent(source) => {
                write!(
                    f,
                    "failed to read shareable screen capture content: {source}"
                )
            }
            Self::ApplicationNotFound { bundle_id } => {
                write!(f, "capture target application is not running: {bundle_id}")
            }
            Self::ApplicationDisplayNotFound { bundle_id } => write!(
                f,
                "failed to resolve a display for the target application: {bundle_id}"
            ),
            Self::BuildStream(source) => write!(f, "failed to build input stream: {source}"),
            Self::PlayStream(source) => write!(f, "failed to start input stream: {source}"),
            Self::AddStreamOutput(source) => {
                write!(f, "failed to add screen capture stream output: {source}")
            }
            Self::StartCapture(source) => {
                write!(f, "failed to start screen capture stream: {source}")
            }
            Self::UnsupportedSampleFormat(sample_format) => {
                write!(f, "unsupported input sample format: {sample_format}")
            }
            Self::CallbackStream(message) => write!(f, "stream callback failed: {message}"),
            Self::SampleBufferPoisoned => f.write_str("sample buffer was poisoned"),
            Self::DecodeCapturedAudio(source) => {
                write!(f, "failed to decode captured audio: {source}")
            }
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
