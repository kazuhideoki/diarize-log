mod config;

use crate::config::{Config, ConfigError, DEFAULT_DOTENV_PATH, debug_logging_enabled};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample};
use dotenvy::Error as DotenvError;
use reqwest::StatusCode;
use reqwest::blocking::{Client, multipart};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

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
    fn as_api_value(self) -> &'static str {
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
    fn as_api_value(self) -> &'static str {
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

/// `cpal` を使ってデフォルトマイクから録音します。
#[derive(Debug, Default)]
pub struct CpalRecorder;

impl Recorder for CpalRecorder {
    fn record_wav(&mut self, duration: Duration) -> Result<RecordedAudio, RecorderError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(RecorderError::NoInputDevice)?;
        debug_log(&format!(
            "input device selected: {}",
            device.name().unwrap_or_else(|_| "<unknown>".to_string())
        ));
        let supported_config = device
            .default_input_config()
            .map_err(RecorderError::DefaultInputConfig)?;
        let sample_format = supported_config.sample_format();
        let stream_config: cpal::StreamConfig = supported_config.into();
        let channels = stream_config.channels;
        let sample_rate = stream_config.sample_rate.0;
        debug_log(&format!(
            "recording starts: duration={}s sample_format={sample_format:?} channels={channels} sample_rate={sample_rate}",
            duration.as_secs()
        ));
        let sample_buffer = Arc::new(Mutex::new(Vec::new()));
        let (error_sender, error_receiver) = mpsc::channel();

        let stream = match sample_format {
            cpal::SampleFormat::F32 => build_input_stream::<f32>(
                &device,
                &stream_config,
                Arc::clone(&sample_buffer),
                error_sender.clone(),
            )?,
            cpal::SampleFormat::I16 => build_input_stream::<i16>(
                &device,
                &stream_config,
                Arc::clone(&sample_buffer),
                error_sender.clone(),
            )?,
            cpal::SampleFormat::U16 => build_input_stream::<u16>(
                &device,
                &stream_config,
                Arc::clone(&sample_buffer),
                error_sender.clone(),
            )?,
            other => return Err(RecorderError::UnsupportedSampleFormat(other)),
        };

        stream.play().map_err(RecorderError::PlayStream)?;
        thread::sleep(duration);
        drop(stream);
        debug_log("recording finished");

        if let Ok(callback_error) = error_receiver.try_recv() {
            return Err(callback_error);
        }

        let captured_samples = sample_buffer
            .lock()
            .map_err(|_| RecorderError::SampleBufferPoisoned)?
            .clone();
        debug_log(&format!(
            "captured pcm samples: count={}",
            captured_samples.len()
        ));

        encode_wav(captured_samples, channels, sample_rate)
    }
}

/// OpenAI の話者分離文字起こし API を呼び出します。
#[derive(Debug)]
pub struct OpenAiTranscriber {
    client: Client,
    api_key: String,
}

impl OpenAiTranscriber {
    pub fn from_env() -> Result<Self, TranscriberError> {
        let config = Config::from_dotenv_path(std::path::Path::new(DEFAULT_DOTENV_PATH))
            .map_err(TranscriberError::from)?;
        debug_log(&format!("api key source: {}", config.openai_api_key_source));
        let client = Client::builder()
            .build()
            .map_err(TranscriberError::BuildHttpClient)?;

        Ok(Self {
            client,
            api_key: config.openai_api_key,
        })
    }
}

impl Transcriber for OpenAiTranscriber {
    fn transcribe(
        &mut self,
        request: TranscriptionRequest<'_>,
    ) -> Result<DiarizedTranscript, TranscriberError> {
        debug_log(&format!(
            "sending transcription request: endpoint={TRANSCRIPTIONS_ENDPOINT} model={} response_format={} chunking_strategy={} audio_bytes={}",
            request.model,
            request.response_format.as_api_value(),
            request.chunking_strategy.as_api_value(),
            request.audio.wav_bytes.len()
        ));
        let audio_part = multipart::Part::bytes(request.audio.wav_bytes.clone())
            .file_name("recording.wav")
            .mime_str(request.audio.content_type)
            .map_err(TranscriberError::InvalidMimeType)?;
        let form = multipart::Form::new()
            .part("file", audio_part)
            .text("model", request.model.to_owned())
            .text(
                "response_format",
                request.response_format.as_api_value().to_owned(),
            )
            .text(
                "chunking_strategy",
                request.chunking_strategy.as_api_value().to_owned(),
            );
        let response = self
            .client
            .post(TRANSCRIPTIONS_ENDPOINT)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .map_err(TranscriberError::SendRequest)?;
        let status = response.status();
        debug_log(&format!("transcription response status: {status}"));
        let body = response
            .text()
            .map_err(TranscriberError::ReadResponseBody)?;
        debug_log(&format!("transcription response body bytes={}", body.len()));

        if !status.is_success() {
            return Err(TranscriberError::ApiError { status, body });
        }

        let api_response: ApiDiarizedTranscript = serde_json::from_str(&body)
            .map_err(|source| TranscriberError::ParseResponseBody { source, body })?;
        debug_log(&format!(
            "transcription parsed: text_chars={} segments={}",
            api_response.text.chars().count(),
            api_response.segments.len()
        ));

        Ok(api_response.into_domain())
    }
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
    MissingApiKey,
    ReadDotEnv(DotenvError),
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
            Self::MissingApiKey => f.write_str("OPENAI_API_KEY is not set"),
            Self::ReadDotEnv(source) => write!(f, "failed to read .env: {source}"),
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

impl From<ConfigError> for TranscriberError {
    fn from(value: ConfigError) -> Self {
        match value {
            ConfigError::MissingOpenAiApiKey => Self::MissingApiKey,
            ConfigError::ReadDotEnv(source) => Self::ReadDotEnv(source),
        }
    }
}

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
pub fn run_cli<R, T, W>(
    config: &CliConfig,
    recorder: &mut R,
    transcriber: &mut T,
    output: &mut W,
) -> Result<(), CliError>
where
    R: Recorder,
    T: Transcriber,
    W: Write,
{
    let audio = recorder
        .record_wav(config.recording_duration)
        .map_err(CliError::Record)?;
    let transcript = transcriber
        .transcribe(TranscriptionRequest {
            audio: &audio,
            model: config.transcription_model,
            response_format: config.response_format,
            chunking_strategy: config.chunking_strategy,
        })
        .map_err(CliError::Transcribe)?;

    serde_json::to_writer_pretty(&mut *output, &transcript).map_err(CliError::Serialize)?;
    output.write_all(b"\n").map_err(CliError::Write)?;

    Ok(())
}
fn debug_log(message: &str) {
    if debug_logging_enabled() {
        eprintln!("[debug] {message}");
    }
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_buffer: Arc<Mutex<Vec<i16>>>,
    error_sender: mpsc::Sender<RecorderError>,
) -> Result<cpal::Stream, RecorderError>
where
    T: cpal::SizedSample,
    i16: FromSample<T>,
{
    let callback_error_sender = error_sender.clone();

    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut converted = Vec::with_capacity(data.len());
                for sample in data {
                    converted.push(sample_to_i16(*sample));
                }

                match sample_buffer.lock() {
                    Ok(mut guard) => guard.extend_from_slice(&converted),
                    Err(_) => {
                        let _ = error_sender.send(RecorderError::SampleBufferPoisoned);
                    }
                }
            },
            move |error| {
                let _ =
                    callback_error_sender.send(RecorderError::CallbackStream(error.to_string()));
            },
            None,
        )
        .map_err(RecorderError::BuildStream)
}

fn sample_to_i16<T>(sample: T) -> i16
where
    T: cpal::SizedSample,
    i16: FromSample<T>,
{
    i16::from_sample(sample)
}

fn encode_wav(
    pcm_samples: Vec<i16>,
    channels: u16,
    sample_rate: u32,
) -> Result<RecordedAudio, RecorderError> {
    let mut wav_bytes = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::new(&mut wav_bytes, spec).map_err(RecorderError::EncodeWav)?;

    for sample in pcm_samples {
        writer
            .write_sample(sample)
            .map_err(RecorderError::EncodeWav)?;
    }

    writer.finalize().map_err(RecorderError::EncodeWav)?;

    Ok(RecordedAudio {
        wav_bytes: wav_bytes.into_inner(),
        content_type: "audio/wav",
    })
}

#[derive(Debug, Deserialize)]
struct ApiDiarizedTranscript {
    text: String,
    segments: Vec<ApiTranscriptSegment>,
}

impl ApiDiarizedTranscript {
    fn into_domain(self) -> DiarizedTranscript {
        DiarizedTranscript {
            text: self.text,
            segments: self
                .segments
                .into_iter()
                .map(ApiTranscriptSegment::into_domain)
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiTranscriptSegment {
    speaker: String,
    start: f64,
    end: f64,
    text: String,
}

impl ApiTranscriptSegment {
    fn into_domain(self) -> TranscriptSegment {
        TranscriptSegment {
            speaker: self.speaker,
            start_ms: seconds_to_millis(self.start),
            end_ms: seconds_to_millis(self.end),
            text: self.text,
        }
    }
}

fn seconds_to_millis(seconds: f64) -> u64 {
    (seconds * 1_000.0).round() as u64
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

        run_cli(&config, &mut recorder, &mut transcriber, &mut output).unwrap();

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

        run_cli(&config, &mut recorder, &mut transcriber, &mut output).unwrap();

        let printed = String::from_utf8(output).unwrap();
        let expected = serde_json::to_string_pretty(&transcript).unwrap() + "\n";
        assert_eq!(printed, expected);
    }

    #[test]
    /// API の秒単位セグメントをミリ秒単位の出力モデルへ変換する。
    fn converts_api_segments_from_seconds_to_milliseconds() {
        let api_transcript = ApiDiarizedTranscript {
            text: "hello".to_string(),
            segments: vec![ApiTranscriptSegment {
                speaker: "spk_0".to_string(),
                start: 0.125,
                end: 1.875,
                text: "hello".to_string(),
            }],
        };

        let transcript = api_transcript.into_domain();

        assert_eq!(
            transcript,
            DiarizedTranscript {
                text: "hello".to_string(),
                segments: vec![TranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 125,
                    end_ms: 1_875,
                    text: "hello".to_string(),
                }],
            }
        );
    }
}
