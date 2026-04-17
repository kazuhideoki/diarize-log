use crate::domain::{DiarizedTranscript, KnownSpeakerSample, RecordedAudio};
use std::fmt;

/// 文字起こし API に送るレスポンス形式です。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseFormat {
    DiarizedJson,
}

impl ResponseFormat {
    pub(crate) fn as_api_value(self) -> &'static str {
        match self {
            Self::DiarizedJson => "diarized_json",
        }
    }
}

/// 文字起こし API に送るチャンク戦略です。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkingStrategy {
    Auto,
}

impl ChunkingStrategy {
    pub(crate) fn as_api_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
        }
    }
}

/// 文字起こし対象の言語指定です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptionLanguage {
    Auto,
    Fixed(String),
}

impl TranscriptionLanguage {
    /// API に送る language 値を返します。自動判定時はフィールド自体を省略します。
    pub fn as_api_value(&self) -> Option<&str> {
        match self {
            Self::Auto => None,
            Self::Fixed(language) => Some(language.as_str()),
        }
    }
}

impl fmt::Display for TranscriptionLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Fixed(language) => f.write_str(language),
        }
    }
}

/// 話者分離文字起こしリクエストです。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionRequest<'a> {
    pub audio: &'a RecordedAudio,
    pub speaker_samples: &'a [KnownSpeakerSample],
    pub model: &'static str,
    pub language: Option<&'a str>,
    pub response_format: ResponseFormat,
    pub chunking_strategy: ChunkingStrategy,
}

/// OpenAI への文字起こし送信を抽象化します。
pub trait Transcriber {
    fn transcribe(
        &mut self,
        request: TranscriptionRequest<'_>,
    ) -> Result<DiarizedTranscript, TranscriberError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriberError {
    BuildHttpClient(String),
    InvalidMimeType(String),
    SendRequest(String),
    ReadResponseBody(String),
    ApiError { status_code: u16, body: String },
    ParseResponseBody { source: String, body: String },
}

impl fmt::Display for TranscriberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuildHttpClient(source) => write!(f, "failed to build http client: {source}"),
            Self::InvalidMimeType(source) => write!(f, "invalid audio mime type: {source}"),
            Self::SendRequest(source) => {
                write!(f, "failed to send transcription request: {source}")
            }
            Self::ReadResponseBody(source) => {
                write!(f, "failed to read transcription response: {source}")
            }
            Self::ApiError { status_code, body } => {
                write!(f, "transcription api returned {status_code}: {body}")
            }
            Self::ParseResponseBody { source, body } => {
                write!(
                    f,
                    "failed to parse transcription response: {source}; body: {body}"
                )
            }
        }
    }
}

impl std::error::Error for TranscriberError {}
