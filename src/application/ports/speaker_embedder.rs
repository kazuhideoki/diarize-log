use crate::domain::{KnownSpeakerEmbedding, RecordedAudio};
use std::fmt;

/// 話者サンプル音声から speaker embedding を抽出します。
pub trait SpeakerEmbedder {
    fn embed_speaker(
        &self,
        speaker_name: &str,
        audio: &RecordedAudio,
    ) -> Result<KnownSpeakerEmbedding, SpeakerEmbedderError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakerEmbedderError {
    SpawnProcess(String),
    WriteInput(String),
    ReadOutput(String),
    ProcessFailed { status: String, stderr: String },
    ParseOutput(String),
    InvalidOutput(String),
}

impl fmt::Display for SpeakerEmbedderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpawnProcess(source) => write!(f, "failed to spawn speaker embedder: {source}"),
            Self::WriteInput(source) => {
                write!(f, "failed to write speaker embedder input: {source}")
            }
            Self::ReadOutput(source) => {
                write!(f, "failed to read speaker embedder output: {source}")
            }
            Self::ProcessFailed { status, stderr } => {
                write!(f, "speaker embedder failed with {status}: {stderr}")
            }
            Self::ParseOutput(source) => {
                write!(f, "failed to parse speaker embedder output: {source}")
            }
            Self::InvalidOutput(source) => write!(f, "invalid speaker embedder output: {source}"),
        }
    }
}

impl std::error::Error for SpeakerEmbedderError {}
