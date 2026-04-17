use crate::domain::{
    DiarizedTranscript, MergeAuditEntry, MergedTranscriptSegment, RecordedAudio,
    SourcedTranscriptSegment, TranscriptMergePolicy, TranscriptSource,
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
    pub response_format: String,
    pub chunking_strategy: String,
    pub merge_policy: TranscriptMergePolicy,
}

/// mixed capture session 全体の保存メタデータです。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MixedCaptureSessionMetadata {
    pub mode: String,
    pub application_bundle_id: String,
    pub microphone_speaker: String,
    pub source_settings: Vec<MixedCaptureSourceSettings>,
    pub source_outcomes: Vec<MixedCaptureSourceOutcome>,
}

/// mixed capture における source 単位の設定要約です。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MixedCaptureSourceSettings {
    pub source: TranscriptSource,
    pub recording_duration_ms: u64,
    pub capture_duration_ms: u64,
    pub capture_overlap_ms: u64,
    pub transcription_model: String,
    pub response_format: String,
    pub chunking_strategy: String,
    pub merge_policy: TranscriptMergePolicy,
    pub fixed_speaker: Option<String>,
}

/// mixed capture における source 単位の終了状態です。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MixedCaptureSourceOutcome {
    pub source: TranscriptSource,
    pub started_at_unix_ms: u64,
    pub status: MixedCaptureSourceStatus,
    pub transcription_failure_count: usize,
    pub error_message: Option<String>,
}

/// mixed capture の source 完了状態です。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MixedCaptureSourceStatus {
    Succeeded,
    PartialFailure,
    Failed,
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

/// mixed capture session 全体の保存先を抽象化します。
pub trait MixedCaptureStore {
    fn persist_mixed_session_metadata(
        &mut self,
        metadata: &MixedCaptureSessionMetadata,
    ) -> Result<(), CaptureStoreError>;

    fn persist_final_segments(
        &mut self,
        segments: &[SourcedTranscriptSegment],
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
