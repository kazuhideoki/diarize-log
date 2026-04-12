use crate::ports::{
    CaptureStore, CaptureStoreError, ChunkingStrategy, DiarizedTranscript, Recorder, RecorderError,
    RecordingSession, ResponseFormat, Transcriber, TranscriberError, TranscriptionRequest,
};
use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::time::Duration;

pub const TRANSCRIPTION_MODEL: &str = "gpt-4o-transcribe-diarize";

/// CLI 起動時の振る舞いです。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Run,
    ShowHelp,
}

/// CLI 引数の解釈失敗です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliArgumentError {
    UnexpectedArgument { argument: OsString },
}

impl fmt::Display for CliArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedArgument { argument } => {
                write!(f, "unexpected argument: {}", argument.to_string_lossy())
            }
        }
    }
}

impl std::error::Error for CliArgumentError {}

/// CLI PoC の固定設定です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliConfig {
    pub recording_duration: Duration,
    pub capture_duration: Duration,
    pub capture_overlap: Duration,
    pub response_format: ResponseFormat,
    pub transcription_model: &'static str,
    pub chunking_strategy: ChunkingStrategy,
}

impl CliConfig {
    pub fn new(
        recording_duration: Duration,
        capture_duration: Duration,
        capture_overlap: Duration,
    ) -> Self {
        Self {
            recording_duration,
            capture_duration,
            capture_overlap,
            response_format: ResponseFormat::DiarizedJson,
            transcription_model: TRANSCRIPTION_MODEL,
            chunking_strategy: ChunkingStrategy::Auto,
        }
    }
}

/// CLI 引数を解釈します。
pub fn parse_cli_args<I>(args: I) -> Result<CliAction, CliArgumentError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut parsed_action = CliAction::Run;

    for argument in args.into_iter().skip(1) {
        if argument == "--help" {
            parsed_action = CliAction::ShowHelp;
            continue;
        }

        return Err(CliArgumentError::UnexpectedArgument { argument });
    }

    Ok(parsed_action)
}

/// `--help` で表示する usage 文です。
pub fn render_help(program_name: &str) -> String {
    format!(
        "Usage: {program_name} [--help]\n\nRecords audio, requests diarized transcription, and stores the capture.\n\nOptions:\n  --help    Show this help message and exit.\n"
    )
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
) -> Result<Vec<DiarizedTranscript>, CliError>
where
    R: Recorder,
    T: Transcriber,
    S: CaptureStore,
    L: Write,
{
    info_log(stderr, "recording started").map_err(CliError::Write)?;
    let mut session = Some(recorder.start_recording().map_err(CliError::Record)?);
    let windows = build_capture_windows(
        config.recording_duration,
        config.capture_duration,
        config.capture_overlap,
    );
    let mut transcripts = Vec::with_capacity(windows.len());

    for window in windows {
        let is_last_window = window.end_offset() == config.recording_duration;
        session
            .as_mut()
            .expect("recording session must exist until the final capture is copied")
            .wait_until(window.end_offset())
            .map_err(CliError::Record)?;

        let audio = session
            .as_mut()
            .expect("recording session must exist until the final capture is copied")
            .capture_wav(window.start_offset, window.duration)
            .map_err(CliError::Record)?;
        capture_store
            .persist_audio(window.capture_index, &audio)
            .map_err(CliError::Store)?;
        if is_last_window {
            drop(session.take());
            info_log(stderr, "recording finished").map_err(CliError::Write)?;
        }
        info_log(
            stderr,
            &format!(
                "transcription request sent for capture {}",
                window.capture_index
            ),
        )
        .map_err(CliError::Write)?;
        let transcript = transcriber
            .transcribe(TranscriptionRequest {
                audio: &audio,
                model: config.transcription_model,
                response_format: config.response_format,
                chunking_strategy: config.chunking_strategy,
            })
            .map_err(CliError::Transcribe)?;
        info_log(
            stderr,
            &format!(
                "transcription response received for capture {}",
                window.capture_index
            ),
        )
        .map_err(CliError::Write)?;
        capture_store
            .persist_transcript(
                window.capture_index,
                duration_to_millis(window.start_offset),
                &transcript,
            )
            .map_err(CliError::Store)?;
        transcripts.push(transcript);
    }

    Ok(transcripts)
}

/// debug 有効時だけ pretty JSON を出力します。
pub fn write_debug_transcript<W>(
    debug_enabled: bool,
    output: &mut W,
    transcripts: &[DiarizedTranscript],
) -> Result<(), DebugOutputError>
where
    W: Write,
{
    if !debug_enabled {
        return Ok(());
    }

    for transcript in transcripts {
        serde_json::to_writer_pretty(&mut *output, transcript)
            .map_err(DebugOutputError::Serialize)?;
        output.write_all(b"\n").map_err(DebugOutputError::Write)?;
    }

    Ok(())
}

fn info_log<W>(output: &mut W, message: &str) -> Result<(), std::io::Error>
where
    W: Write,
{
    writeln!(output, "{message}")
}

fn duration_to_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).expect("capture duration in millis must fit into u64")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureWindow {
    capture_index: u64,
    start_offset: Duration,
    duration: Duration,
}

impl CaptureWindow {
    fn end_offset(&self) -> Duration {
        self.start_offset + self.duration
    }
}

fn build_capture_windows(
    recording_duration: Duration,
    capture_duration: Duration,
    capture_overlap: Duration,
) -> Vec<CaptureWindow> {
    let stride = capture_duration
        .checked_sub(capture_overlap)
        .expect("capture overlap must be smaller than capture duration");
    let mut windows = Vec::new();
    let mut capture_index = 1_u64;
    let mut start_offset = Duration::ZERO;

    while start_offset < recording_duration {
        let duration = (recording_duration - start_offset).min(capture_duration);
        windows.push(CaptureWindow {
            capture_index,
            start_offset,
            duration,
        });

        if duration < capture_duration {
            break;
        }

        start_offset += stride;
        capture_index += 1;
    }

    windows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{RecordedAudio, TranscriptSegment};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    #[test]
    /// 引数が無いときは通常実行モードを返す。
    fn returns_run_action_when_no_flags_are_given() {
        let action = parse_cli_args([OsString::from("diarize-log")]).unwrap();

        assert_eq!(action, CliAction::Run);
    }

    #[test]
    /// `--help` を受け取ると help 表示モードを返す。
    fn returns_show_help_action_when_help_flag_is_given() {
        let action =
            parse_cli_args([OsString::from("diarize-log"), OsString::from("--help")]).unwrap();

        assert_eq!(action, CliAction::ShowHelp);
    }

    #[test]
    /// 未知の引数を受け取ると失敗する。
    fn rejects_unknown_argument() {
        let error = parse_cli_args([OsString::from("diarize-log"), OsString::from("--verbose")])
            .unwrap_err();

        assert_eq!(
            error,
            CliArgumentError::UnexpectedArgument {
                argument: OsString::from("--verbose"),
            }
        );
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CapturedRequest {
        wav_bytes: Vec<u8>,
        content_type: &'static str,
        model: &'static str,
        response_format: ResponseFormat,
        chunking_strategy: ChunkingStrategy,
    }

    #[derive(Debug, Default)]
    struct RecordingObservation {
        start_call_count: u64,
        waited_until: Vec<Duration>,
        captured_windows: Vec<(Duration, Duration)>,
        dropped_session_count: u64,
    }

    struct FakeRecordingSession {
        observation: Rc<RefCell<RecordingObservation>>,
        audios: VecDeque<RecordedAudio>,
    }

    impl RecordingSession for FakeRecordingSession {
        fn wait_until(&mut self, duration: Duration) -> Result<(), RecorderError> {
            self.observation.borrow_mut().waited_until.push(duration);
            Ok(())
        }

        fn capture_wav(
            &mut self,
            start_offset: Duration,
            duration: Duration,
        ) -> Result<RecordedAudio, RecorderError> {
            self.observation
                .borrow_mut()
                .captured_windows
                .push((start_offset, duration));
            self.audios
                .pop_front()
                .ok_or_else(|| RecorderError::EncodeWav("missing fake audio".to_string()))
        }
    }

    impl Drop for FakeRecordingSession {
        fn drop(&mut self) {
            self.observation.borrow_mut().dropped_session_count += 1;
        }
    }

    struct FakeRecorder {
        observation: Rc<RefCell<RecordingObservation>>,
        session: Option<FakeRecordingSession>,
    }

    impl Recorder for FakeRecorder {
        type Session = FakeRecordingSession;

        fn start_recording(&mut self) -> Result<Self::Session, RecorderError> {
            self.observation.borrow_mut().start_call_count += 1;
            self.session
                .take()
                .ok_or_else(|| RecorderError::BuildStream("missing fake session".to_string()))
        }
    }

    struct FakeTranscriber {
        observed_requests: RefCell<Vec<CapturedRequest>>,
        observed_drop_counts: RefCell<Vec<u64>>,
        recording_observation: Option<Rc<RefCell<RecordingObservation>>>,
        responses: VecDeque<DiarizedTranscript>,
    }

    impl Transcriber for FakeTranscriber {
        fn transcribe(
            &mut self,
            request: TranscriptionRequest<'_>,
        ) -> Result<DiarizedTranscript, TranscriberError> {
            let dropped_count = self
                .recording_observation
                .as_ref()
                .map(|observation| observation.borrow().dropped_session_count)
                .unwrap_or_default();
            self.observed_drop_counts.borrow_mut().push(dropped_count);
            self.observed_requests.borrow_mut().push(CapturedRequest {
                wav_bytes: request.audio.wav_bytes.clone(),
                content_type: request.audio.content_type,
                model: request.model,
                response_format: request.response_format,
                chunking_strategy: request.chunking_strategy,
            });
            self.responses
                .pop_front()
                .ok_or_else(|| TranscriberError::ParseResponseBody {
                    source: "missing fake transcript".to_string(),
                    body: String::new(),
                })
        }
    }

    struct FakeCaptureStore {
        observed_audios: RefCell<Vec<(u64, RecordedAudio)>>,
        observed_transcripts: RefCell<Vec<(u64, u64, DiarizedTranscript)>>,
    }

    impl CaptureStore for FakeCaptureStore {
        fn persist_audio(
            &mut self,
            capture_index: u64,
            audio: &RecordedAudio,
        ) -> Result<(), CaptureStoreError> {
            self.observed_audios
                .borrow_mut()
                .push((capture_index, audio.clone()));
            Ok(())
        }

        fn persist_transcript(
            &mut self,
            capture_index: u64,
            capture_start_ms: u64,
            transcript: &DiarizedTranscript,
        ) -> Result<(), CaptureStoreError> {
            self.observed_transcripts.borrow_mut().push((
                capture_index,
                capture_start_ms,
                transcript.clone(),
            ));
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
    /// overlap を保ちながら capture 窓を最後の端数まで計画する。
    fn builds_overlapping_capture_windows_until_recording_ends() {
        let windows = build_capture_windows(
            Duration::from_secs(360),
            Duration::from_secs(180),
            Duration::from_secs(15),
        );

        assert_eq!(
            windows,
            vec![
                CaptureWindow {
                    capture_index: 1,
                    start_offset: Duration::from_secs(0),
                    duration: Duration::from_secs(180),
                },
                CaptureWindow {
                    capture_index: 2,
                    start_offset: Duration::from_secs(165),
                    duration: Duration::from_secs(180),
                },
                CaptureWindow {
                    capture_index: 3,
                    start_offset: Duration::from_secs(330),
                    duration: Duration::from_secs(30),
                },
            ]
        );
    }

    #[test]
    /// 継続録音を一度だけ開始し、capture ごとに待機と切り出しを行う。
    fn records_incremental_captures_with_overlap_and_requests_transcription() {
        let config = CliConfig::new(
            Duration::from_secs(360),
            Duration::from_secs(180),
            Duration::from_secs(15),
        );
        let audio1 = sample_audio();
        let audio2 = RecordedAudio {
            wav_bytes: vec![0x01, 0x02],
            content_type: "audio/wav",
        };
        let audio3 = RecordedAudio {
            wav_bytes: vec![0x03, 0x04],
            content_type: "audio/wav",
        };
        let transcript = sample_transcript();
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession {
                observation: Rc::clone(&observation),
                audios: VecDeque::from(vec![audio1.clone(), audio2.clone(), audio3.clone()]),
            }),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            responses: VecDeque::from(vec![
                transcript.clone(),
                transcript.clone(),
                transcript.clone(),
            ]),
        };
        let mut capture_store = FakeCaptureStore {
            observed_audios: RefCell::new(Vec::new()),
            observed_transcripts: RefCell::new(Vec::new()),
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

        assert_eq!(observation.borrow().start_call_count, 1);
        assert_eq!(
            observation.borrow().waited_until,
            vec![
                Duration::from_secs(180),
                Duration::from_secs(345),
                Duration::from_secs(360),
            ]
        );
        assert_eq!(
            observation.borrow().captured_windows,
            vec![
                (Duration::from_secs(0), Duration::from_secs(180)),
                (Duration::from_secs(165), Duration::from_secs(180)),
                (Duration::from_secs(330), Duration::from_secs(30)),
            ]
        );
        assert_eq!(
            *transcriber.observed_requests.borrow(),
            vec![
                CapturedRequest {
                    wav_bytes: audio1.wav_bytes,
                    content_type: "audio/wav",
                    model: TRANSCRIPTION_MODEL,
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
                CapturedRequest {
                    wav_bytes: audio2.wav_bytes,
                    content_type: "audio/wav",
                    model: TRANSCRIPTION_MODEL,
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
                CapturedRequest {
                    wav_bytes: audio3.wav_bytes,
                    content_type: "audio/wav",
                    model: TRANSCRIPTION_MODEL,
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
            ]
        );
    }

    #[test]
    /// 文字起こし結果を capture 順にまとめて返す。
    fn returns_transcription_results_to_caller() {
        let config = CliConfig::new(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        let transcript1 = sample_transcript();
        let transcript2 = DiarizedTranscript {
            text: "別の capture".to_string(),
            segments: vec![TranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 0,
                end_ms: 500,
                text: "別の capture".to_string(),
            }],
        };
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession {
                observation: Rc::clone(&observation),
                audios: VecDeque::from(vec![sample_audio(), sample_audio()]),
            }),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            responses: VecDeque::from(vec![transcript1.clone(), transcript2.clone()]),
        };
        let mut capture_store = FakeCaptureStore {
            observed_audios: RefCell::new(Vec::new()),
            observed_transcripts: RefCell::new(Vec::new()),
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

        assert_eq!(returned, vec![transcript1, transcript2]);
    }

    #[test]
    /// capture store へ各 capture の wav と transcript を開始オフセット付きで保存する。
    fn persists_each_capture_via_capture_store() {
        let config = CliConfig::new(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        let audio1 = sample_audio();
        let audio2 = RecordedAudio {
            wav_bytes: vec![0x05, 0x06],
            content_type: "audio/wav",
        };
        let transcript1 = sample_transcript();
        let transcript2 = DiarizedTranscript {
            text: "二つ目".to_string(),
            segments: vec![TranscriptSegment {
                speaker: "spk_1".to_string(),
                start_ms: 0,
                end_ms: 600,
                text: "二つ目".to_string(),
            }],
        };
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession {
                observation: Rc::clone(&observation),
                audios: VecDeque::from(vec![audio1.clone(), audio2.clone()]),
            }),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            responses: VecDeque::from(vec![transcript1.clone(), transcript2.clone()]),
        };
        let mut capture_store = FakeCaptureStore {
            observed_audios: RefCell::new(Vec::new()),
            observed_transcripts: RefCell::new(Vec::new()),
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
            *capture_store.observed_audios.borrow(),
            vec![(1, audio1), (2, audio2)]
        );
        assert_eq!(
            *capture_store.observed_transcripts.borrow(),
            vec![(1, 0, transcript1), (2, 20_000, transcript2)]
        );
    }

    #[test]
    /// transcript が失敗しても切り出し済みの wav は先に保存する。
    fn persists_audio_before_transcription_succeeds() {
        let config = CliConfig::new(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        let audio = sample_audio();
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession {
                observation: Rc::clone(&observation),
                audios: VecDeque::from(vec![audio.clone(), sample_audio()]),
            }),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            responses: VecDeque::new(),
        };
        let mut capture_store = FakeCaptureStore {
            observed_audios: RefCell::new(Vec::new()),
            observed_transcripts: RefCell::new(Vec::new()),
        };
        let mut stderr = Vec::new();

        let error = run_cli(
            &config,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
        )
        .unwrap_err();

        assert!(matches!(error, CliError::Transcribe(_)));
        assert_eq!(*capture_store.observed_audios.borrow(), vec![(1, audio)]);
        assert!(capture_store.observed_transcripts.borrow().is_empty());
    }

    #[test]
    /// 通常ログとして録音開始と capture ごとの API 送受信を標準エラーへ順序通りに出力する。
    fn writes_normal_operation_logs_to_stderr() {
        let config = CliConfig::new(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession {
                observation: Rc::clone(&observation),
                audios: VecDeque::from(vec![sample_audio(), sample_audio()]),
            }),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            responses: VecDeque::from(vec![sample_transcript(), sample_transcript()]),
        };
        let mut capture_store = FakeCaptureStore {
            observed_audios: RefCell::new(Vec::new()),
            observed_transcripts: RefCell::new(Vec::new()),
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
            "recording started\ntranscription request sent for capture 1\ntranscription response received for capture 1\nrecording finished\ntranscription request sent for capture 2\ntranscription response received for capture 2\n"
        );
    }

    #[test]
    /// debug 無効時は標準出力へ何も書かず、有効時だけ pretty JSON を capture ごとに出力する。
    fn writes_debug_transcript_only_when_debug_enabled() {
        let transcripts = vec![sample_transcript(), sample_transcript()];
        let mut disabled_output = Vec::new();
        let mut enabled_output = Vec::new();

        write_debug_transcript(false, &mut disabled_output, &transcripts).unwrap();
        write_debug_transcript(true, &mut enabled_output, &transcripts).unwrap();

        assert!(disabled_output.is_empty());
        assert_eq!(
            String::from_utf8(enabled_output).unwrap(),
            transcripts
                .iter()
                .map(|transcript| serde_json::to_string_pretty(transcript).unwrap() + "\n")
                .collect::<String>()
        );
    }

    #[test]
    /// 最終 capture を切り出したら最後の文字起こし前に録音 session を破棄する。
    fn drops_recording_session_before_transcribing_final_capture() {
        let config = CliConfig::new(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession {
                observation: Rc::clone(&observation),
                audios: VecDeque::from(vec![sample_audio(), sample_audio()]),
            }),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            responses: VecDeque::from(vec![sample_transcript(), sample_transcript()]),
        };
        let mut capture_store = FakeCaptureStore {
            observed_audios: RefCell::new(Vec::new()),
            observed_transcripts: RefCell::new(Vec::new()),
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

        assert_eq!(*transcriber.observed_drop_counts.borrow(), vec![0, 1]);
    }
}
