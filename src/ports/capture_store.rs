use crate::ports::{DiarizedTranscript, RecordedAudio};
use std::fmt;

/// 文字起こし結果の保存先を抽象化します。
pub trait CaptureStore {
    fn persist_capture(
        &mut self,
        capture_index: u64,
        audio: &RecordedAudio,
        transcript: &DiarizedTranscript,
    ) -> Result<(), CaptureStoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureStoreError {
    CreateSession(String),
    ResolveLocalOffset(String),
    FormatSessionName(String),
    WriteAudio(String),
    WriteCapture(String),
    SerializeCapture(String),
    OpenFinal(String),
    WriteFinal(String),
    SerializeFinal(String),
}

impl fmt::Display for CaptureStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateSession(source) => {
                write!(f, "failed to create storage directories: {source}")
            }
            Self::ResolveLocalOffset(source) => {
                write!(f, "failed to resolve local timezone offset: {source}")
            }
            Self::FormatSessionName(source) => {
                write!(f, "failed to format session directory name: {source}")
            }
            Self::WriteAudio(source) => write!(f, "failed to write audio file: {source}"),
            Self::WriteCapture(source) => write!(f, "failed to write capture file: {source}"),
            Self::SerializeCapture(source) => {
                write!(f, "failed to serialize capture file: {source}")
            }
            Self::OpenFinal(source) => write!(f, "failed to open final log file: {source}"),
            Self::WriteFinal(source) => write!(f, "failed to append final log file: {source}"),
            Self::SerializeFinal(source) => {
                write!(f, "failed to serialize final log entry: {source}")
            }
        }
    }
}

impl std::error::Error for CaptureStoreError {}
