use crate::domain::{DiarizedTranscript, MergedTranscriptSegment, RecordedAudio};
use std::fmt;

/// 文字起こし結果の保存先を抽象化します。
pub trait CaptureStore {
    fn persist_audio(
        &mut self,
        capture_index: u64,
        audio: &RecordedAudio,
    ) -> Result<(), CaptureStoreError>;

    fn persist_transcript(
        &mut self,
        capture_index: u64,
        capture_start_ms: u64,
        transcript: &DiarizedTranscript,
    ) -> Result<(), CaptureStoreError>;

    fn persist_merged_segments(
        &mut self,
        segments: &[MergedTranscriptSegment],
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
    OpenMerged(String),
    WriteMerged(String),
    SerializeMerged(String),
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
            Self::OpenMerged(source) => write!(f, "failed to open merged log file: {source}"),
            Self::WriteMerged(source) => write!(f, "failed to append merged log file: {source}"),
            Self::SerializeMerged(source) => {
                write!(f, "failed to serialize merged log entry: {source}")
            }
        }
    }
}

impl std::error::Error for CaptureStoreError {}
