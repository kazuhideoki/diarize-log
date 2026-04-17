use crate::domain::{
    DiarizedTranscript, MergeAuditEntry, MergedTranscriptSegment, RecordedAudio,
    TranscriptMergePolicy,
};
use serde::Serialize;
use std::fmt;

/// capture session の保存メタデータです。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CaptureSessionMetadata {
    pub recording_duration_ms: u64,
    pub capture_duration_ms: u64,
    pub capture_overlap_ms: u64,
    pub transcription_model: String,
    pub transcription_language: String,
    pub response_format: String,
    pub chunking_strategy: String,
    pub merge_policy: TranscriptMergePolicy,
}

/// 文字起こし結果の保存先を抽象化します。
pub trait CaptureStore {
    fn persist_session_metadata(
        &mut self,
        metadata: &CaptureSessionMetadata,
    ) -> Result<(), CaptureStoreError>;

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

    fn persist_merge_audit_entries(
        &mut self,
        entries: &[MergeAuditEntry],
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
    WriteMetadata(String),
    SerializeMetadata(String),
    OpenMerged(String),
    WriteMerged(String),
    SerializeMerged(String),
    OpenMergeAudit(String),
    WriteMergeAudit(String),
    SerializeMergeAudit(String),
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
            Self::WriteMetadata(source) => write!(f, "failed to write metadata file: {source}"),
            Self::SerializeMetadata(source) => {
                write!(f, "failed to serialize metadata file: {source}")
            }
            Self::OpenMerged(source) => write!(f, "failed to open merged log file: {source}"),
            Self::WriteMerged(source) => write!(f, "failed to append merged log file: {source}"),
            Self::SerializeMerged(source) => {
                write!(f, "failed to serialize merged log entry: {source}")
            }
            Self::OpenMergeAudit(source) => {
                write!(f, "failed to open merge audit file: {source}")
            }
            Self::WriteMergeAudit(source) => {
                write!(f, "failed to append merge audit file: {source}")
            }
            Self::SerializeMergeAudit(source) => {
                write!(f, "failed to serialize merge audit entry: {source}")
            }
        }
    }
}

impl std::error::Error for CaptureStoreError {}
