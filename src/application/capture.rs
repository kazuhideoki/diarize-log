use crate::ports::{
    CaptureStore, CaptureStoreError, ChunkingStrategy, DiarizedTranscript, KnownSpeakerSample,
    Recorder, RecorderError, RecordingSession, ResponseFormat, Transcriber, TranscriberError,
    TranscriptionRequest,
};
use std::fmt;
use std::io::Write;
use std::time::Duration;

pub const TRANSCRIPTION_MODEL: &str = "gpt-4o-transcribe-diarize";

/// 連続録音ユースケースの設定です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureConfig {
    pub recording_duration: Duration,
    pub capture_duration: Duration,
    pub capture_overlap: Duration,
    pub response_format: ResponseFormat,
    pub transcription_model: &'static str,
    pub chunking_strategy: ChunkingStrategy,
}

impl CaptureConfig {
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

#[derive(Debug)]
pub enum CaptureError {
    Record(RecorderError),
    Transcribe(TranscriberError),
    Store(CaptureStoreError),
    Write(std::io::Error),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Record(error) => write!(f, "recording failed: {error}"),
            Self::Transcribe(error) => write!(f, "transcription failed: {error}"),
            Self::Store(error) => write!(f, "capture persistence failed: {error}"),
            Self::Write(error) => write!(f, "stderr write failed: {error}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// 連続録音と文字起こしを実行します。
pub fn run_capture<R, T, S, L>(
    config: &CaptureConfig,
    speaker_samples: &[KnownSpeakerSample],
    recorder: &mut R,
    transcriber: &mut T,
    capture_store: &mut S,
    stderr: &mut L,
) -> Result<Vec<DiarizedTranscript>, CaptureError>
where
    R: Recorder,
    T: Transcriber,
    S: CaptureStore,
    L: Write,
{
    info_log(stderr, "recording started").map_err(CaptureError::Write)?;
    let mut session = Some(recorder.start_recording().map_err(CaptureError::Record)?);
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
            .map_err(CaptureError::Record)?;

        let audio = session
            .as_mut()
            .expect("recording session must exist until the final capture is copied")
            .capture_wav(window.start_offset, window.duration)
            .map_err(CaptureError::Record)?;
        capture_store
            .persist_audio(window.capture_index, &audio)
            .map_err(CaptureError::Store)?;
        if is_last_window {
            drop(session.take());
            info_log(stderr, "recording finished").map_err(CaptureError::Write)?;
        }
        info_log(
            stderr,
            &format!(
                "transcription request sent for capture {}",
                window.capture_index
            ),
        )
        .map_err(CaptureError::Write)?;
        let transcript = transcriber
            .transcribe(TranscriptionRequest {
                audio: &audio,
                speaker_samples,
                model: config.transcription_model,
                response_format: config.response_format,
                chunking_strategy: config.chunking_strategy,
            })
            .map_err(CaptureError::Transcribe)?;
        info_log(
            stderr,
            &format!(
                "transcription response received for capture {}",
                window.capture_index
            ),
        )
        .map_err(CaptureError::Write)?;
        capture_store
            .persist_transcript(
                window.capture_index,
                duration_to_millis(window.start_offset),
                &transcript,
            )
            .map_err(CaptureError::Store)?;
        transcripts.push(transcript);
    }

    Ok(transcripts)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{RecordedAudio, RecordingSession, TranscriptSegment};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CapturedRequest {
        wav_bytes: Vec<u8>,
        content_type: &'static str,
        speaker_samples: Vec<KnownSpeakerSample>,
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

    impl Drop for FakeRecordingSession {
        fn drop(&mut self) {
            self.observation.borrow_mut().dropped_session_count += 1;
        }
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
                speaker_samples: request.speaker_samples.to_vec(),
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

    fn sample_known_speaker() -> KnownSpeakerSample {
        KnownSpeakerSample {
            speaker_name: "suzuki".to_string(),
            audio: RecordedAudio {
                wav_bytes: vec![0x10, 0x20, 0x30],
                content_type: "audio/wav",
            },
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
        let config = CaptureConfig::new(
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

        run_capture(
            &config,
            &[],
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
                    speaker_samples: Vec::new(),
                    model: TRANSCRIPTION_MODEL,
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
                CapturedRequest {
                    wav_bytes: audio2.wav_bytes,
                    content_type: "audio/wav",
                    speaker_samples: Vec::new(),
                    model: TRANSCRIPTION_MODEL,
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
                CapturedRequest {
                    wav_bytes: audio3.wav_bytes,
                    content_type: "audio/wav",
                    speaker_samples: Vec::new(),
                    model: TRANSCRIPTION_MODEL,
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
            ]
        );
    }

    #[test]
    /// 指定した既知話者サンプルを各 capture の文字起こしリクエストへ添付する。
    fn attaches_known_speaker_samples_to_each_transcription_request() {
        let config = CaptureConfig::new(
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
        let speaker_sample = sample_known_speaker();

        run_capture(
            &config,
            std::slice::from_ref(&speaker_sample),
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(
            transcriber
                .observed_requests
                .borrow()
                .iter()
                .map(|request| request.speaker_samples.clone())
                .collect::<Vec<_>>(),
            vec![vec![speaker_sample.clone()], vec![speaker_sample],]
        );
    }

    #[test]
    /// 文字起こし結果を capture 順にまとめて返す。
    fn returns_transcription_results_to_caller() {
        let config = CaptureConfig::new(
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

        let returned = run_capture(
            &config,
            &[],
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
        let config = CaptureConfig::new(
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

        run_capture(
            &config,
            &[],
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
        let config = CaptureConfig::new(
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

        let error = run_capture(
            &config,
            &[],
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
        )
        .unwrap_err();

        assert!(matches!(error, CaptureError::Transcribe(_)));
        assert_eq!(*capture_store.observed_audios.borrow(), vec![(1, audio)]);
        assert!(capture_store.observed_transcripts.borrow().is_empty());
    }

    #[test]
    /// 通常ログとして録音開始と capture ごとの API 送受信を標準エラーへ順序通りに出力する。
    fn writes_normal_operation_logs_to_stderr() {
        let config = CaptureConfig::new(
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

        run_capture(
            &config,
            &[],
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
        let config = CaptureConfig::new(
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

        run_capture(
            &config,
            &[],
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(*transcriber.observed_drop_counts.borrow(), vec![0, 1]);
    }
}
