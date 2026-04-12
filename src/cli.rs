use crate::ports::{
    CaptureStore, CaptureStoreError, ChunkingStrategy, DiarizedTranscript, Recorder, RecorderError,
    ResponseFormat, Transcriber, TranscriberError, TranscriptionRequest,
};
use std::fmt;
use std::io::Write;
use std::time::Duration;

pub const TRANSCRIPTION_MODEL: &str = "gpt-4o-transcribe-diarize";

/// CLI PoC の固定設定です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliConfig {
    pub recording_duration: Duration,
    pub response_format: ResponseFormat,
    pub transcription_model: &'static str,
    pub chunking_strategy: ChunkingStrategy,
}

impl CliConfig {
    pub fn new(recording_duration: Duration) -> Self {
        Self {
            recording_duration,
            response_format: ResponseFormat::DiarizedJson,
            transcription_model: TRANSCRIPTION_MODEL,
            chunking_strategy: ChunkingStrategy::Auto,
        }
    }
}

#[derive(Debug)]
pub enum DebugOutputError {
    Serialize(serde_json::Error),
    Write(std::io::Error),
}

impl fmt::Display for DebugOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialize(source) => {
                write!(f, "failed to serialize debug stdout: {source}")
            }
            Self::Write(source) => write!(f, "failed to write debug stdout: {source}"),
        }
    }
}

impl std::error::Error for DebugOutputError {}

#[derive(Debug)]
pub enum CliError {
    Record(RecorderError),
    Transcribe(TranscriberError),
    Store(CaptureStoreError),
    Write(std::io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Record(error) => write!(f, "recording failed: {error}"),
            Self::Transcribe(error) => write!(f, "transcription failed: {error}"),
            Self::Store(error) => write!(f, "capture persistence failed: {error}"),
            Self::Write(error) => write!(f, "stderr write failed: {error}"),
        }
    }
}

impl std::error::Error for CliError {}

/// CLI PoC のオーケストレーション入口です。
pub fn run_cli<R, T, S, L>(
    config: &CliConfig,
    recorder: &mut R,
    transcriber: &mut T,
    capture_store: &mut S,
    stderr: &mut L,
) -> Result<DiarizedTranscript, CliError>
where
    R: Recorder,
    T: Transcriber,
    S: CaptureStore,
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
    capture_store
        .persist_capture(1, &audio, &transcript)
        .map_err(CliError::Store)?;

    Ok(transcript)
}

/// debug 有効時だけ pretty JSON を出力します。
pub fn write_debug_transcript<W>(
    debug_enabled: bool,
    output: &mut W,
    transcript: &DiarizedTranscript,
) -> Result<(), DebugOutputError>
where
    W: Write,
{
    if !debug_enabled {
        return Ok(());
    }

    serde_json::to_writer_pretty(&mut *output, transcript).map_err(DebugOutputError::Serialize)?;
    output.write_all(b"\n").map_err(DebugOutputError::Write)?;

    Ok(())
}

fn info_log<W>(output: &mut W, message: &str) -> Result<(), std::io::Error>
where
    W: Write,
{
    writeln!(output, "{message}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{RecordedAudio, TranscriptSegment};
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

    struct FakeCaptureStore {
        observed_capture: RefCell<Option<(u64, RecordedAudio, DiarizedTranscript)>>,
    }

    impl CaptureStore for FakeCaptureStore {
        fn persist_capture(
            &mut self,
            capture_index: u64,
            audio: &RecordedAudio,
            transcript: &DiarizedTranscript,
        ) -> Result<(), CaptureStoreError> {
            *self.observed_capture.borrow_mut() =
                Some((capture_index, audio.clone(), transcript.clone()));
            Ok(())
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
        let config = CliConfig::new(Duration::from_secs(30));
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
        let mut capture_store = FakeCaptureStore {
            observed_capture: RefCell::new(None),
        };
        let mut stderr = Vec::new();

        run_cli(
            &config,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(
            *recorder.observed_duration.borrow(),
            Some(Duration::from_secs(30))
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
    /// 文字起こし結果を呼び出し元へ返す。
    fn returns_transcription_result_to_caller() {
        let config = CliConfig::new(Duration::from_secs(30));
        let transcript = sample_transcript();
        let mut recorder = FakeRecorder {
            observed_duration: RefCell::new(None),
            audio: sample_audio(),
        };
        let mut transcriber = FakeTranscriber {
            observed_request: RefCell::new(None),
            response: transcript.clone(),
        };
        let mut capture_store = FakeCaptureStore {
            observed_capture: RefCell::new(None),
        };
        let mut stderr = Vec::new();

        let returned = run_cli(
            &config,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(returned, transcript);
    }

    #[test]
    /// 録音音声と文字起こし結果を capture store へ連番 1 で保存する。
    fn persists_recorded_audio_and_transcription_result_via_capture_store() {
        let config = CliConfig::new(Duration::from_secs(30));
        let audio = sample_audio();
        let transcript = sample_transcript();
        let mut recorder = FakeRecorder {
            observed_duration: RefCell::new(None),
            audio: audio.clone(),
        };
        let mut transcriber = FakeTranscriber {
            observed_request: RefCell::new(None),
            response: transcript.clone(),
        };
        let observed_capture = RefCell::new(None);
        let mut capture_store = FakeCaptureStore { observed_capture };
        let mut stderr = Vec::new();

        run_cli(
            &config,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(
            *capture_store.observed_capture.borrow(),
            Some((1, audio, transcript))
        );
    }

    #[test]
    /// 通常ログとして録音開始と終了および API の送受信を標準エラーへ順序通りに出力する。
    fn writes_normal_operation_logs_to_stderr() {
        let config = CliConfig::new(Duration::from_secs(30));
        let transcript = sample_transcript();
        let mut recorder = FakeRecorder {
            observed_duration: RefCell::new(None),
            audio: sample_audio(),
        };
        let mut transcriber = FakeTranscriber {
            observed_request: RefCell::new(None),
            response: transcript,
        };
        let mut capture_store = FakeCaptureStore {
            observed_capture: RefCell::new(None),
        };
        let mut stderr = Vec::new();

        run_cli(
            &config,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
        )
        .unwrap();

        let printed_logs = String::from_utf8(stderr).unwrap();
        assert_eq!(
            printed_logs,
            "recording started\nrecording finished\ntranscription request sent\ntranscription response received\n"
        );
    }

    #[test]
    /// debug 無効時は標準出力へ何も書かず、有効時だけ pretty JSON を出力する。
    fn writes_debug_transcript_only_when_debug_enabled() {
        let transcript = sample_transcript();
        let mut disabled_output = Vec::new();
        let mut enabled_output = Vec::new();

        write_debug_transcript(false, &mut disabled_output, &transcript).unwrap();
        write_debug_transcript(true, &mut enabled_output, &transcript).unwrap();

        assert!(disabled_output.is_empty());
        assert_eq!(
            String::from_utf8(enabled_output).unwrap(),
            serde_json::to_string_pretty(&transcript).unwrap() + "\n"
        );
    }
}
