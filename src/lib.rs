pub mod adapters;
pub mod config;

use reqwest::StatusCode;
use serde::Serialize;
use std::fmt;
use std::io::Write;
use std::time::Duration;

pub use adapters::{CpalRecorder, OpenAiTranscriber};

pub const DEFAULT_RECORDING_DURATION: Duration = Duration::from_secs(30);
pub const TRANSCRIPTION_MODEL: &str = "gpt-4o-transcribe-diarize";
const TRANSCRIPTIONS_ENDPOINT: &str = "https://api.openai.com/v1/audio/transcriptions";

/// CLI PoC の固定設定です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliConfig {
    pub recording_duration: Duration,
    pub response_format: ResponseFormat,
    pub transcription_model: &'static str,
    pub chunking_strategy: ChunkingStrategy,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            recording_duration: DEFAULT_RECORDING_DURATION,
            response_format: ResponseFormat::DiarizedJson,
            transcription_model: TRANSCRIPTION_MODEL,
            chunking_strategy: ChunkingStrategy::Auto,
        }
    }
}

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

/// 録音した WAV 音声です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedAudio {
    pub wav_bytes: Vec<u8>,
    pub content_type: &'static str,
}

/// 話者分離文字起こしリクエストです。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionRequest<'a> {
    pub audio: &'a RecordedAudio,
    pub model: &'static str,
    pub response_format: ResponseFormat,
    pub chunking_strategy: ChunkingStrategy,
}

/// 話者分離された文字起こし結果です。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiarizedTranscript {
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
}

/// 話者単位のセグメントです。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptSegment {
    pub speaker: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// 録音処理を抽象化します。
pub trait Recorder {
    fn record_wav(&mut self, duration: Duration) -> Result<RecordedAudio, RecorderError>;
}

/// OpenAI への文字起こし送信を抽象化します。
pub trait Transcriber {
    fn transcribe(
        &mut self,
        request: TranscriptionRequest<'_>,
    ) -> Result<DiarizedTranscript, TranscriberError>;
}

#[derive(Debug)]
pub enum RecorderError {
    NoInputDevice,
    DefaultInputConfig(cpal::DefaultStreamConfigError),
    BuildStream(cpal::BuildStreamError),
    PlayStream(cpal::PlayStreamError),
    UnsupportedSampleFormat(cpal::SampleFormat),
    CallbackStream(String),
    SampleBufferPoisoned,
    EncodeWav(hound::Error),
}

impl fmt::Display for RecorderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputDevice => f.write_str("default input device is not available"),
            Self::DefaultInputConfig(source) => {
                write!(f, "failed to read default input config: {source}")
            }
            Self::BuildStream(source) => write!(f, "failed to build input stream: {source}"),
            Self::PlayStream(source) => write!(f, "failed to start input stream: {source}"),
            Self::UnsupportedSampleFormat(sample_format) => {
                write!(f, "unsupported input sample format: {sample_format:?}")
            }
            Self::CallbackStream(message) => write!(f, "stream callback failed: {message}"),
            Self::SampleBufferPoisoned => f.write_str("sample buffer was poisoned"),
            Self::EncodeWav(source) => write!(f, "failed to encode wav: {source}"),
        }
    }
}

impl std::error::Error for RecorderError {}

#[derive(Debug)]
pub enum TranscriberError {
    BuildHttpClient(reqwest::Error),
    InvalidMimeType(reqwest::Error),
    SendRequest(reqwest::Error),
    ReadResponseBody(reqwest::Error),
    ApiError {
        status: StatusCode,
        body: String,
    },
    ParseResponseBody {
        source: serde_json::Error,
        body: String,
    },
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
            Self::ApiError { status, body } => {
                write!(f, "transcription api returned {status}: {body}")
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

#[derive(Debug)]
pub enum CliError {
    Record(RecorderError),
    Transcribe(TranscriberError),
    Serialize(serde_json::Error),
    Write(std::io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Record(error) => write!(f, "recording failed: {error}"),
            Self::Transcribe(error) => write!(f, "transcription failed: {error}"),
            Self::Serialize(error) => write!(f, "output serialization failed: {error}"),
            Self::Write(error) => write!(f, "stdout write failed: {error}"),
        }
    }
}

impl std::error::Error for CliError {}

/// CLI PoC のオーケストレーション入口です。
pub fn run_cli<R, T, W, L>(
    config: &CliConfig,
    recorder: &mut R,
    transcriber: &mut T,
    stdout: &mut W,
    stderr: &mut L,
) -> Result<(), CliError>
where
    R: Recorder,
    T: Transcriber,
    W: Write,
    L: Write,
{
    info_log(stderr, "recording started").map_err(CliError::Write)?;
    let audio = recorder
        .record_wav(config.recording_duration)
        .map_err(CliError::Record)?;
    info_log(stderr, "recording finished").map_err(CliError::Write)?;
    info_log(stderr, "transcription request sent").map_err(CliError::Write)?;
    let transcript = transcriber
        .transcribe(TranscriptionRequest {
            audio: &audio,
            model: config.transcription_model,
            response_format: config.response_format,
            chunking_strategy: config.chunking_strategy,
        })
        .map_err(CliError::Transcribe)?;
    info_log(stderr, "transcription response received").map_err(CliError::Write)?;

    serde_json::to_writer_pretty(&mut *stdout, &transcript).map_err(CliError::Serialize)?;
    stdout.write_all(b"\n").map_err(CliError::Write)?;

    Ok(())
}

fn info_log<W>(output: &mut W, message: &str) -> Result<(), std::io::Error>
where
    W: Write,
{
    writeln!(output, "{message}")
}

fn debug_log(debug_enabled: bool, message: &str) {
    if debug_enabled {
        eprintln!("[debug] {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeRecorder {
        observed_duration: RefCell<Option<Duration>>,
        audio: RecordedAudio,
    }

    impl Recorder for FakeRecorder {
        fn record_wav(&mut self, duration: Duration) -> Result<RecordedAudio, RecorderError> {
            *self.observed_duration.borrow_mut() = Some(duration);
            Ok(self.audio.clone())
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CapturedRequest {
        wav_bytes: Vec<u8>,
        content_type: &'static str,
        model: &'static str,
        response_format: ResponseFormat,
        chunking_strategy: ChunkingStrategy,
    }

    struct FakeTranscriber {
        observed_request: RefCell<Option<CapturedRequest>>,
        response: DiarizedTranscript,
    }

    impl Transcriber for FakeTranscriber {
        fn transcribe(
            &mut self,
            request: TranscriptionRequest<'_>,
        ) -> Result<DiarizedTranscript, TranscriberError> {
            *self.observed_request.borrow_mut() = Some(CapturedRequest {
                wav_bytes: request.audio.wav_bytes.clone(),
                content_type: request.audio.content_type,
                model: request.model,
                response_format: request.response_format,
                chunking_strategy: request.chunking_strategy,
            });
            Ok(self.response.clone())
        }
    }

    fn sample_audio() -> RecordedAudio {
        RecordedAudio {
            wav_bytes: vec![0x52, 0x49, 0x46, 0x46],
            content_type: "audio/wav",
        }
    }

    fn sample_transcript() -> DiarizedTranscript {
        DiarizedTranscript {
            text: "こんにちは 今日はよろしくお願いします".to_string(),
            segments: vec![
                TranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 0,
                    end_ms: 900,
                    text: "こんにちは".to_string(),
                },
                TranscriptSegment {
                    speaker: "spk_1".to_string(),
                    start_ms: 950,
                    end_ms: 2_300,
                    text: "今日はよろしくお願いします".to_string(),
                },
            ],
        }
    }

    #[test]
    /// 30秒録音し diarized_json と auto chunking で文字起こしを要求する。
    fn records_for_30_seconds_and_requests_diarized_transcription() {
        let config = CliConfig::default();
        let expected_audio = sample_audio();
        let expected_transcript = sample_transcript();
        let observed_duration = RefCell::new(None);
        let observed_request = RefCell::new(None);
        let mut recorder = FakeRecorder {
            observed_duration,
            audio: expected_audio.clone(),
        };
        let mut transcriber = FakeTranscriber {
            observed_request,
            response: expected_transcript,
        };
        let mut output = Vec::new();
        let mut stderr = Vec::new();

        run_cli(
            &config,
            &mut recorder,
            &mut transcriber,
            &mut output,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(
            *recorder.observed_duration.borrow(),
            Some(DEFAULT_RECORDING_DURATION)
        );
        assert_eq!(
            *transcriber.observed_request.borrow(),
            Some(CapturedRequest {
                wav_bytes: expected_audio.wav_bytes,
                content_type: "audio/wav",
                model: TRANSCRIPTION_MODEL,
                response_format: ResponseFormat::DiarizedJson,
                chunking_strategy: ChunkingStrategy::Auto,
            })
        );
    }

    #[test]
    /// 文字起こし結果を pretty JSON で標準出力に書き出す。
    fn writes_transcription_result_to_stdout_as_pretty_json() {
        let config = CliConfig::default();
        let transcript = sample_transcript();
        let mut recorder = FakeRecorder {
            observed_duration: RefCell::new(None),
            audio: sample_audio(),
        };
        let mut transcriber = FakeTranscriber {
            observed_request: RefCell::new(None),
            response: transcript.clone(),
        };
        let mut output = Vec::new();
        let mut stderr = Vec::new();

        run_cli(
            &config,
            &mut recorder,
            &mut transcriber,
            &mut output,
            &mut stderr,
        )
        .unwrap();

        let printed = String::from_utf8(output).unwrap();
        let expected = serde_json::to_string_pretty(&transcript).unwrap() + "\n";
        assert_eq!(printed, expected);
    }

    #[test]
    /// 通常ログとして録音開始と終了および API の送受信を標準エラーへ順序通りに出力する。
    fn writes_normal_operation_logs_to_stderr() {
        let config = CliConfig::default();
        let transcript = sample_transcript();
        let mut recorder = FakeRecorder {
            observed_duration: RefCell::new(None),
            audio: sample_audio(),
        };
        let mut transcriber = FakeTranscriber {
            observed_request: RefCell::new(None),
            response: transcript,
        };
        let mut output = Vec::new();
        let mut stderr = Vec::new();

        run_cli(
            &config,
            &mut recorder,
            &mut transcriber,
            &mut output,
            &mut stderr,
        )
        .unwrap();

        let printed_logs = String::from_utf8(stderr).unwrap();
        assert_eq!(
            printed_logs,
            "recording started\nrecording finished\ntranscription request sent\ntranscription response received\n"
        );
    }
}
