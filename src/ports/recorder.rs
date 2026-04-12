use std::fmt;
use std::time::Duration;

/// 録音した WAV 音声です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedAudio {
    pub wav_bytes: Vec<u8>,
    pub content_type: &'static str,
}

/// 録音処理を抽象化します。
pub trait Recorder {
    fn record_wav(&mut self, duration: Duration) -> Result<RecordedAudio, RecorderError>;
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
        }
    }
}

impl std::error::Error for RecorderError {}
