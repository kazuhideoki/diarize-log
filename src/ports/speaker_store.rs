use crate::domain::{KnownSpeakerSample, RecordedAudio};
use std::fmt;

/// 話者サンプル音声の保存先を抽象化します。
pub trait SpeakerStore {
    fn create_sample(
        &mut self,
        speaker_name: &str,
        audio: &RecordedAudio,
    ) -> Result<(), SpeakerStoreError>;

    fn remove_sample(&mut self, speaker_name: &str) -> Result<(), SpeakerStoreError>;

    fn list_samples(&self) -> Result<Vec<String>, SpeakerStoreError>;

    fn read_sample(&self, speaker_name: &str) -> Result<KnownSpeakerSample, SpeakerStoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakerStoreError {
    CreateDirectory(String),
    InvalidSpeakerName { speaker_name: String },
    SpeakerAlreadyExists { speaker_name: String },
    SpeakerNotFound { speaker_name: String },
    WriteSample(String),
    ReadSample(String),
    DeleteSample(String),
    ListSamples(String),
}

impl fmt::Display for SpeakerStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateDirectory(source) => {
                write!(f, "failed to create speaker storage directory: {source}")
            }
            Self::InvalidSpeakerName { speaker_name } => {
                write!(f, "invalid speaker name: {speaker_name}")
            }
            Self::SpeakerAlreadyExists { speaker_name } => {
                write!(f, "speaker sample already exists: {speaker_name}")
            }
            Self::SpeakerNotFound { speaker_name } => {
                write!(f, "speaker sample was not found: {speaker_name}")
            }
            Self::WriteSample(source) => write!(f, "failed to write speaker sample: {source}"),
            Self::ReadSample(source) => write!(f, "failed to read speaker sample: {source}"),
            Self::DeleteSample(source) => write!(f, "failed to delete speaker sample: {source}"),
            Self::ListSamples(source) => write!(f, "failed to list speaker samples: {source}"),
        }
    }
}

impl std::error::Error for SpeakerStoreError {}
