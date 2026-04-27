use super::{CaptureConfig, CaptureError, CaptureRunResult, SpeakerLabel, duration_to_millis};
use crate::application::ports::{
    CaptureSessionMetadata, CaptureStore, InterruptMonitor, Logger, Recorder, RecordingSession,
    Transcriber,
};
use crate::domain::{
    CaptureBoundaryReason, CaptureMerger, CaptureRange, KnownSpeakerSample, RecordedAudio,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::transcript::process_capture_audio;

/// capture 実行本体です。
///
/// 現在時刻取得を引数で受け取り、録音開始時刻を固定したテストを書けるようにしています。
#[allow(clippy::too_many_arguments)]
pub(super) fn run_capture_with_clock<R, T, S, C>(
    config: &CaptureConfig,
    speaker_samples: &[KnownSpeakerSample],
    speaker_label: &SpeakerLabel,
    logger: &dyn Logger,
    recorder: &mut R,
    transcriber: &mut T,
    capture_store: &mut S,
    interrupt_monitor: &dyn InterruptMonitor,
    current_unix_ms: C,
) -> Result<CaptureRunResult, CaptureError>
where
    R: Recorder,
    T: Transcriber + ?Sized,
    S: CaptureStore,
    C: Fn() -> u64,
{
    capture_store
        .persist_session_metadata(&CaptureSessionMetadata {
            recording_duration_ms: duration_to_millis(config.capture_policy.recording_duration),
            capture_duration_ms: duration_to_millis(config.capture_policy.capture_duration),
            capture_overlap_ms: duration_to_millis(config.capture_policy.capture_overlap),
            capture_silence_threshold_dbfs: config.silence_request_policy.silence_threshold_dbfs,
            capture_silence_min_duration_ms: duration_to_millis(
                config.silence_request_policy.silence_min_duration,
            ),
            capture_tail_silence_min_duration_ms: duration_to_millis(
                config.silence_request_policy.tail_silence_min_duration,
            ),
            transcription_model: config.transcription_model.to_string(),
            transcription_language: config.transcription_language.to_string(),
            response_format: config.response_format.as_api_value().to_string(),
            chunking_strategy: config.chunking_strategy.as_api_value().to_string(),
            merge_policy: config.merge_policy.clone(),
            fixed_speaker: fixed_speaker_name(speaker_label),
        })
        .map_err(CaptureError::Store)?;
    logger
        .info("recording started")
        .map_err(CaptureError::Write)?;
    let mut session = Some(recorder.start_recording().map_err(CaptureError::Record)?);
    let started_at_unix_ms = current_unix_ms();
    let mut capture_merger = CaptureMerger::new(config.merge_policy.clone());
    let mut transcripts = Vec::new();
    let mut merged_segments = Vec::new();
    let mut transcription_failures = Vec::new();
    let mut capture_index = 1_u64;
    let mut capture_start_offset = Duration::ZERO;

    while capture_start_offset < config.capture_policy.recording_duration {
        let boundary = session
            .as_mut()
            .expect("recording session must exist until the final capture is copied")
            .wait_for_capture_boundary(
                capture_start_offset,
                &config.capture_policy,
                &config.silence_request_policy,
                interrupt_monitor,
            )
            .map_err(CaptureError::Record)?;

        if boundary.reason == CaptureBoundaryReason::Interrupted {
            logger
                .info("interrupt received, finalizing recorded audio")
                .map_err(CaptureError::Write)?;
        }

        if boundary.duration.is_zero() {
            drop(session.take());
            logger
                .info("recording finished")
                .map_err(CaptureError::Write)?;
            break;
        }

        let capture_range = CaptureRange {
            capture_index,
            start_offset: capture_start_offset,
            duration: boundary.duration,
        };
        let audio = capture_audio(
            session
                .as_mut()
                .expect("recording session must exist until the final capture is copied"),
            capture_store,
            capture_range,
        )?;
        let capture_end_offset = capture_range.end_offset();
        let reached_recording_end = capture_end_offset >= config.capture_policy.recording_duration;
        let next_overlap_start_offset =
            capture_end_offset.saturating_sub(config.capture_policy.capture_overlap);
        // hard limit で録音終端に達した直後だけは、従来どおり overlap tail capture を 1 本残して
        // 末尾の切れ目を merge で救えるようにします。
        let needs_overlap_tail_capture = boundary.reason == CaptureBoundaryReason::MaxDuration
            && reached_recording_end
            && boundary.duration == config.capture_policy.capture_duration
            && config.capture_policy.capture_overlap > Duration::ZERO
            && next_overlap_start_offset > capture_start_offset
            && next_overlap_start_offset < config.capture_policy.recording_duration;
        let should_finish_recording = boundary.reason == CaptureBoundaryReason::Interrupted
            || (reached_recording_end && !needs_overlap_tail_capture);

        if should_finish_recording {
            drop(session.take());
            logger
                .info("recording finished")
                .map_err(CaptureError::Write)?;
        }

        process_capture_audio(
            capture_range,
            audio,
            config,
            speaker_samples,
            speaker_label,
            logger,
            transcriber,
            capture_store,
            &mut capture_merger,
            &mut transcripts,
            &mut merged_segments,
            &mut transcription_failures,
        )?;

        if should_finish_recording {
            break;
        }

        // 無音境界はその終了位置から次を始め、hard limit 境界だけ既存 overlap を使います。
        capture_start_offset = match boundary.reason {
            CaptureBoundaryReason::Silence => capture_end_offset,
            CaptureBoundaryReason::MaxDuration => next_overlap_start_offset,
            CaptureBoundaryReason::Interrupted => break,
        };
        capture_index += 1;
    }
    let tail_segments = capture_merger.finish();
    capture_store
        .persist_merged_segments(&tail_segments)
        .map_err(CaptureError::Store)?;
    merged_segments.extend(tail_segments);

    if !transcription_failures.is_empty() {
        logger
            .info(&format!(
                "capture run completed with partial transcription failures: succeeded={} failed={} total={}",
                transcripts.len(),
                transcription_failures.len(),
                transcripts.len() + transcription_failures.len()
            ))
            .map_err(CaptureError::Write)?;
    }

    Ok(CaptureRunResult {
        started_at_unix_ms,
        transcripts,
        merged_segments,
        transcription_failures,
    })
}

pub(super) fn current_unix_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after Unix epoch")
            .as_millis(),
    )
    .expect("unix timestamp in milliseconds must fit into u64")
}

fn capture_audio<SN, ST>(
    session: &mut SN,
    capture_store: &mut ST,
    range: CaptureRange,
) -> Result<RecordedAudio, CaptureError>
where
    SN: RecordingSession,
    ST: CaptureStore,
{
    let audio = session
        .capture_wav(range.start_offset, range.duration)
        .map_err(CaptureError::Record)?;
    capture_store
        .persist_audio(range.capture_index, &audio)
        .map_err(CaptureError::Store)?;
    Ok(audio)
}

fn fixed_speaker_name(speaker_label: &SpeakerLabel) -> Option<String> {
    match speaker_label {
        SpeakerLabel::KeepOriginal => None,
        SpeakerLabel::Fixed(speaker_name) => Some(speaker_name.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        CaptureConfig, CaptureError, CaptureRunResult, CaptureTranscriptionFailure,
        NoopInterruptMonitor, SpeakerLabel, TRANSCRIPTION_MODEL, run_capture,
        run_capture_with_interrupt_monitor, write_debug_transcript,
    };
    use super::*;
    use crate::application::ports::{
        CaptureStoreError, ChunkingStrategy, Logger, RecorderError, RecordingWaitOutcome,
        ResponseFormat, TranscriberError, TranscriptionLanguage, TranscriptionRequest,
    };
    use crate::domain::{
        CaptureBoundary, CapturePolicy, DiarizedTranscript, KnownSpeakerSample, MergeAuditEntry,
        MergeAuditOutcome, MergeOverlapRangeSnapshot, MergedTranscriptSegment, RecordedAudio,
        SilenceRequestPolicy, TranscriptMergePolicy, TranscriptSegment,
    };
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::io;
    use std::rc::Rc;

    const TEST_TRANSCRIPTION_LANGUAGE: &str = "ja";

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct LoggedMessage {
        level: &'static str,
        message: String,
    }

    #[derive(Clone, Default)]
    struct SpyLogger {
        messages: Rc<RefCell<Vec<LoggedMessage>>>,
    }

    impl SpyLogger {
        fn entries(&self) -> Vec<LoggedMessage> {
            self.messages.borrow().clone()
        }
    }

    impl Logger for SpyLogger {
        fn info(&self, message: &str) -> io::Result<()> {
            self.messages.borrow_mut().push(LoggedMessage {
                level: "info",
                message: message.to_string(),
            });
            Ok(())
        }

        fn debug(&self, message: &str) -> io::Result<()> {
            self.messages.borrow_mut().push(LoggedMessage {
                level: "debug",
                message: message.to_string(),
            });
            Ok(())
        }
    }

    fn info(message: &str) -> LoggedMessage {
        LoggedMessage {
            level: "info",
            message: message.to_string(),
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CapturedRequest {
        wav_bytes: Vec<u8>,
        content_type: &'static str,
        speaker_samples: Vec<KnownSpeakerSample>,
        model: &'static str,
        language: Option<String>,
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
        wait_outcomes: VecDeque<RecordingWaitOutcome>,
        boundary_outcomes: VecDeque<CaptureBoundary>,
        recorded_duration: Duration,
    }

    impl FakeRecordingSession {
        fn new(observation: Rc<RefCell<RecordingObservation>>, audios: Vec<RecordedAudio>) -> Self {
            Self {
                observation,
                audios: VecDeque::from(audios),
                wait_outcomes: VecDeque::new(),
                boundary_outcomes: VecDeque::new(),
                recorded_duration: Duration::ZERO,
            }
        }

        fn with_wait_outcomes(
            observation: Rc<RefCell<RecordingObservation>>,
            audios: Vec<RecordedAudio>,
            wait_outcomes: Vec<RecordingWaitOutcome>,
            recorded_duration: Duration,
        ) -> Self {
            Self {
                observation,
                audios: VecDeque::from(audios),
                wait_outcomes: VecDeque::from(wait_outcomes),
                boundary_outcomes: VecDeque::new(),
                recorded_duration,
            }
        }

        fn with_capture_boundaries(
            observation: Rc<RefCell<RecordingObservation>>,
            audios: Vec<RecordedAudio>,
            boundary_outcomes: Vec<CaptureBoundary>,
        ) -> Self {
            Self {
                observation,
                audios: VecDeque::from(audios),
                wait_outcomes: VecDeque::new(),
                boundary_outcomes: VecDeque::from(boundary_outcomes),
                recorded_duration: Duration::ZERO,
            }
        }
    }

    impl Drop for FakeRecordingSession {
        fn drop(&mut self) {
            self.observation.borrow_mut().dropped_session_count += 1;
        }
    }

    impl RecordingSession for FakeRecordingSession {
        fn wait_until(
            &mut self,
            duration: Duration,
            _interrupt_monitor: &dyn InterruptMonitor,
        ) -> Result<RecordingWaitOutcome, RecorderError> {
            self.observation.borrow_mut().waited_until.push(duration);
            Ok(self
                .wait_outcomes
                .pop_front()
                .unwrap_or(RecordingWaitOutcome::ReachedTarget))
        }

        fn wait_for_capture_boundary(
            &mut self,
            capture_start_offset: Duration,
            capture_policy: &CapturePolicy,
            _silence_request_policy: &SilenceRequestPolicy,
            _interrupt_monitor: &dyn InterruptMonitor,
        ) -> Result<CaptureBoundary, RecorderError> {
            let max_capture_duration = (capture_policy.recording_duration - capture_start_offset)
                .min(capture_policy.capture_duration);
            self.observation
                .borrow_mut()
                .waited_until
                .push(capture_start_offset + max_capture_duration);
            if let Some(boundary) = self.boundary_outcomes.pop_front() {
                return Ok(boundary);
            }

            match self
                .wait_outcomes
                .pop_front()
                .unwrap_or(RecordingWaitOutcome::ReachedTarget)
            {
                RecordingWaitOutcome::ReachedTarget => Ok(CaptureBoundary {
                    duration: max_capture_duration,
                    reason: CaptureBoundaryReason::MaxDuration,
                }),
                RecordingWaitOutcome::Interrupted => Ok(CaptureBoundary {
                    duration: self.recorded_duration.saturating_sub(capture_start_offset),
                    reason: CaptureBoundaryReason::Interrupted,
                }),
            }
        }

        fn recorded_duration(&mut self) -> Result<Duration, RecorderError> {
            Ok(self.recorded_duration)
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
        outcomes: VecDeque<Result<DiarizedTranscript, TranscriberError>>,
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
                language: request.language.map(ToString::to_string),
                response_format: request.response_format,
                chunking_strategy: request.chunking_strategy,
            });
            match self.outcomes.pop_front() {
                Some(Ok(transcript)) => Ok(transcript),
                Some(Err(error)) => Err(error),
                None => Err(TranscriberError::ParseResponseBody {
                    source: "missing fake transcript".to_string(),
                    body: String::new(),
                }),
            }
        }
    }

    struct FakeCaptureStore {
        observed_session_metadata: RefCell<Vec<CaptureSessionMetadata>>,
        observed_audios: RefCell<Vec<(u64, RecordedAudio)>>,
        observed_transcripts: RefCell<Vec<(u64, u64, DiarizedTranscript)>>,
        observed_merge_audit_entries: RefCell<Vec<MergeAuditEntry>>,
        observed_merged_segments: RefCell<Vec<MergedTranscriptSegment>>,
    }

    impl CaptureStore for FakeCaptureStore {
        fn persist_session_metadata(
            &mut self,
            metadata: &CaptureSessionMetadata,
        ) -> Result<(), CaptureStoreError> {
            self.observed_session_metadata
                .borrow_mut()
                .push(metadata.clone());
            Ok(())
        }

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

        fn persist_merged_segments(
            &mut self,
            segments: &[MergedTranscriptSegment],
        ) -> Result<(), CaptureStoreError> {
            self.observed_merged_segments
                .borrow_mut()
                .extend_from_slice(segments);
            Ok(())
        }

        fn persist_merge_audit_entries(
            &mut self,
            entries: &[MergeAuditEntry],
        ) -> Result<(), CaptureStoreError> {
            self.observed_merge_audit_entries
                .borrow_mut()
                .extend_from_slice(entries);
            Ok(())
        }
    }

    impl FakeCaptureStore {
        fn new() -> Self {
            Self {
                observed_session_metadata: RefCell::new(Vec::new()),
                observed_audios: RefCell::new(Vec::new()),
                observed_transcripts: RefCell::new(Vec::new()),
                observed_merge_audit_entries: RefCell::new(Vec::new()),
                observed_merged_segments: RefCell::new(Vec::new()),
            }
        }
    }

    fn test_capture_logger() -> SpyLogger {
        SpyLogger::default()
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

    fn capture_config(
        recording_duration: Duration,
        capture_duration: Duration,
        capture_overlap: Duration,
    ) -> CaptureConfig {
        CaptureConfig::new(
            recording_duration,
            capture_duration,
            capture_overlap,
            TranscriptionLanguage::Fixed(TEST_TRANSCRIPTION_LANGUAGE.to_string()),
        )
    }

    #[test]
    /// run 結果には recorder.start_recording の直後に取得した録音開始 Unix 時刻を含める。
    fn returns_recording_started_unix_time_in_result() {
        let config = capture_config(
            Duration::from_secs(10),
            Duration::from_secs(10),
            Duration::ZERO,
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(observation, vec![sample_audio()])),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: None,
            outcomes: VecDeque::from(vec![Ok(sample_transcript())]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        let result = run_capture_with_clock(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &NoopInterruptMonitor,
            || 1_234_567,
        )
        .unwrap();

        assert_eq!(result.started_at_unix_ms, 1_234_567);
    }

    #[test]
    /// 継続録音を一度だけ開始し、capture ごとに待機と切り出しを行う。
    fn records_incremental_captures_with_overlap_and_requests_transcription() {
        let config = capture_config(
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
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![audio1.clone(), audio2.clone(), audio3.clone()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![
                Ok(transcript.clone()),
                Ok(transcript.clone()),
                Ok(transcript.clone()),
            ]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
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
                    language: Some(TEST_TRANSCRIPTION_LANGUAGE.to_string()),
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
                CapturedRequest {
                    wav_bytes: audio2.wav_bytes,
                    content_type: "audio/wav",
                    speaker_samples: Vec::new(),
                    model: TRANSCRIPTION_MODEL,
                    language: Some(TEST_TRANSCRIPTION_LANGUAGE.to_string()),
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
                CapturedRequest {
                    wav_bytes: audio3.wav_bytes,
                    content_type: "audio/wav",
                    speaker_samples: Vec::new(),
                    model: TRANSCRIPTION_MODEL,
                    language: Some(TEST_TRANSCRIPTION_LANGUAGE.to_string()),
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
            ]
        );
    }

    #[test]
    /// 無音境界で確定した次 capture は overlap を付けず、その終了位置から次を始める。
    fn starts_next_capture_without_overlap_after_silence_boundary() {
        let config = capture_config(
            Duration::from_secs(330),
            Duration::from_secs(180),
            Duration::from_secs(15),
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::with_capture_boundaries(
                Rc::clone(&observation),
                vec![sample_audio(), sample_audio(), sample_audio()],
                vec![
                    CaptureBoundary {
                        duration: Duration::from_secs(150),
                        reason: CaptureBoundaryReason::Silence,
                    },
                    CaptureBoundary {
                        duration: Duration::from_secs(180),
                        reason: CaptureBoundaryReason::MaxDuration,
                    },
                    CaptureBoundary {
                        duration: Duration::from_secs(15),
                        reason: CaptureBoundaryReason::MaxDuration,
                    },
                ],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![
                Ok(sample_transcript()),
                Ok(sample_transcript()),
                Ok(sample_transcript()),
            ]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(
            observation.borrow().captured_windows,
            vec![
                (Duration::from_secs(0), Duration::from_secs(150)),
                (Duration::from_secs(150), Duration::from_secs(180)),
                (Duration::from_secs(315), Duration::from_secs(15)),
            ]
        );
    }

    #[test]
    /// hard limit 到達で確定した次 capture は既存 overlap ぶんだけ巻き戻して始める。
    fn starts_next_capture_with_overlap_after_max_duration_boundary() {
        let config = capture_config(
            Duration::from_secs(345),
            Duration::from_secs(180),
            Duration::from_secs(15),
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::with_capture_boundaries(
                Rc::clone(&observation),
                vec![sample_audio(), sample_audio(), sample_audio()],
                vec![
                    CaptureBoundary {
                        duration: Duration::from_secs(180),
                        reason: CaptureBoundaryReason::MaxDuration,
                    },
                    CaptureBoundary {
                        duration: Duration::from_secs(180),
                        reason: CaptureBoundaryReason::MaxDuration,
                    },
                    CaptureBoundary {
                        duration: Duration::from_secs(15),
                        reason: CaptureBoundaryReason::MaxDuration,
                    },
                ],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![
                Ok(sample_transcript()),
                Ok(sample_transcript()),
                Ok(sample_transcript()),
            ]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(
            observation.borrow().captured_windows,
            vec![
                (Duration::from_secs(0), Duration::from_secs(180)),
                (Duration::from_secs(165), Duration::from_secs(180)),
                (Duration::from_secs(330), Duration::from_secs(15)),
            ]
        );
    }

    #[test]
    /// 指定した既知話者サンプルを各 capture の文字起こしリクエストへ添付する。
    fn attaches_known_speaker_samples_to_each_transcription_request() {
        let config = capture_config(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![sample_audio(), sample_audio()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![Ok(sample_transcript()), Ok(sample_transcript())]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();
        let speaker_sample = sample_known_speaker();

        run_capture(
            &config,
            std::slice::from_ref(&speaker_sample),
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
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
    /// 固定 transcription language を各 capture の文字起こしリクエストへ渡す。
    fn passes_transcription_language_to_each_request() {
        let mut config = capture_config(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        config.transcription_language = TranscriptionLanguage::Fixed("en".to_string());
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![sample_audio(), sample_audio()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![Ok(sample_transcript()), Ok(sample_transcript())]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(
            transcriber
                .observed_requests
                .borrow()
                .iter()
                .map(|request| request.language.clone())
                .collect::<Vec<_>>(),
            vec![Some("en".to_string()), Some("en".to_string())]
        );
    }

    #[test]
    /// transcription language が auto のときは API リクエストへ language を渡さない。
    fn omits_transcription_language_from_each_request_in_auto_mode() {
        let mut config = capture_config(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        config.transcription_language = TranscriptionLanguage::Auto;
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![sample_audio(), sample_audio()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![Ok(sample_transcript()), Ok(sample_transcript())]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(
            transcriber
                .observed_requests
                .borrow()
                .iter()
                .map(|request| request.language.clone())
                .collect::<Vec<_>>(),
            vec![None, None]
        );
    }

    #[test]
    /// 文字起こし結果を capture 順にまとめて返す。
    fn returns_transcription_results_to_caller() {
        let config = capture_config(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(18),
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
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![sample_audio(), sample_audio()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![Ok(transcript1.clone()), Ok(transcript2.clone())]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        let returned = run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(
            returned,
            CaptureRunResult {
                started_at_unix_ms: returned.started_at_unix_ms,
                transcripts: vec![transcript1, transcript2],
                merged_segments: vec![
                    MergedTranscriptSegment {
                        speaker: "spk_0".to_string(),
                        start_ms: 0,
                        end_ms: 900,
                        text: "こんにちは".to_string(),
                    },
                    MergedTranscriptSegment {
                        speaker: "spk_1".to_string(),
                        start_ms: 950,
                        end_ms: 2_300,
                        text: "今日はよろしくお願いします".to_string(),
                    },
                    MergedTranscriptSegment {
                        speaker: "spk_0".to_string(),
                        start_ms: 12_000,
                        end_ms: 12_500,
                        text: "別の capture".to_string(),
                    },
                ],
                transcription_failures: Vec::new(),
            }
        );
    }

    #[test]
    /// capture store へ各 capture の wav と transcript を開始オフセット付きで保存する。
    fn persists_each_capture_via_capture_store() {
        let config = capture_config(
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
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![audio1.clone(), audio2.clone()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![Ok(transcript1.clone()), Ok(transcript2.clone())]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
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
    /// overlap が重複した発話は merge 後の absolute segment として 1 回だけ保存する。
    fn persists_merged_segments_after_deduplicating_overlap() {
        let config = capture_config(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(18),
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![sample_audio(), sample_audio()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![
                Ok(DiarizedTranscript {
                    text: "ABCDEFGHIJKLMNOP".to_string(),
                    segments: vec![TranscriptSegment {
                        speaker: "spk_0".to_string(),
                        start_ms: 10_000,
                        end_ms: 18_000,
                        text: "ABCDEFGHIJKLMNOP".to_string(),
                    }],
                }),
                Ok(DiarizedTranscript {
                    text: "EFGHIJKLMNOPQRST".to_string(),
                    segments: vec![TranscriptSegment {
                        speaker: "spk_0".to_string(),
                        start_ms: 0,
                        end_ms: 8_000,
                        text: "EFGHIJKLMNOPQRST".to_string(),
                    }],
                }),
            ]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(
            *capture_store.observed_merged_segments.borrow(),
            vec![
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 10_000,
                    end_ms: 12_000,
                    text: "ABCD".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 12_000,
                    end_ms: 20_000,
                    text: "EFGHIJKLMNOPQRST".to_string(),
                },
            ]
        );
    }

    #[test]
    /// send request 失敗は capture 単位で記録して後続 capture の処理を続ける。
    fn continues_after_recoverable_transcription_failure_and_records_it_in_result() {
        let config = capture_config(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        let audio1 = sample_audio();
        let audio2 = RecordedAudio {
            wav_bytes: vec![0x05, 0x06],
            content_type: "audio/wav",
        };
        let success_transcript = sample_transcript();
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![audio1.clone(), audio2.clone()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![
                Err(TranscriberError::SendRequest(
                    "simulated timeout".to_string(),
                )),
                Ok(success_transcript.clone()),
            ]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        let result = run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(
            *capture_store.observed_audios.borrow(),
            vec![(1, audio1), (2, audio2)]
        );
        assert_eq!(
            *capture_store.observed_transcripts.borrow(),
            vec![(2, 20_000, success_transcript.clone())]
        );
        assert_eq!(
            result,
            CaptureRunResult {
                started_at_unix_ms: result.started_at_unix_ms,
                transcripts: vec![success_transcript],
                merged_segments: vec![
                    MergedTranscriptSegment {
                        speaker: "spk_0".to_string(),
                        start_ms: 20_000,
                        end_ms: 20_900,
                        text: "こんにちは".to_string(),
                    },
                    MergedTranscriptSegment {
                        speaker: "spk_1".to_string(),
                        start_ms: 20_950,
                        end_ms: 22_300,
                        text: "今日はよろしくお願いします".to_string(),
                    },
                ],
                transcription_failures: vec![CaptureTranscriptionFailure {
                    capture_index: 1,
                    capture_start_ms: 0,
                    message: "failed to send transcription request: simulated timeout".to_string(),
                }],
            }
        );
        assert_eq!(
            logger.entries(),
            vec![
                info("recording started"),
                info("transcription request sent for capture 1"),
                info(
                    "transcription failed for capture 1, continuing: failed to send transcription request: simulated timeout"
                ),
                info("recording finished"),
                info("transcription request sent for capture 2"),
                info("transcription response received for capture 2"),
                info(
                    "capture run completed with partial transcription failures: succeeded=1 failed=1 total=2"
                ),
            ]
        );
    }

    #[test]
    /// 待機中に中断要求が来たら、録音済みぶんだけ切り出して session を閉じてから文字起こしする。
    fn finalizes_recorded_audio_when_interrupted_while_waiting_for_next_capture() {
        let config = capture_config(
            Duration::from_secs(360),
            Duration::from_secs(180),
            Duration::from_secs(15),
        );
        let audio1 = sample_audio();
        let audio2 = RecordedAudio {
            wav_bytes: vec![0x09, 0x0a],
            content_type: "audio/wav",
        };
        let transcript1 = sample_transcript();
        let transcript2 = DiarizedTranscript {
            text: "途中終了".to_string(),
            segments: vec![TranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 0,
                end_ms: 700,
                text: "途中終了".to_string(),
            }],
        };
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::with_wait_outcomes(
                Rc::clone(&observation),
                vec![audio1.clone(), audio2.clone()],
                vec![
                    RecordingWaitOutcome::ReachedTarget,
                    RecordingWaitOutcome::Interrupted,
                ],
                Duration::from_secs(200),
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![Ok(transcript1.clone()), Ok(transcript2.clone())]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        let result = run_capture_with_interrupt_monitor(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &NoopInterruptMonitor,
        )
        .unwrap();

        assert_eq!(
            observation.borrow().waited_until,
            vec![Duration::from_secs(180), Duration::from_secs(345)]
        );
        assert_eq!(
            observation.borrow().captured_windows,
            vec![
                (Duration::from_secs(0), Duration::from_secs(180)),
                (Duration::from_secs(165), Duration::from_secs(35)),
            ]
        );
        assert_eq!(
            *capture_store.observed_audios.borrow(),
            vec![(1, audio1), (2, audio2)]
        );
        assert_eq!(
            *capture_store.observed_transcripts.borrow(),
            vec![
                (1, 0, transcript1.clone()),
                (2, 165_000, transcript2.clone())
            ]
        );
        assert_eq!(*transcriber.observed_drop_counts.borrow(), vec![0, 1]);
        assert_eq!(
            result,
            CaptureRunResult {
                started_at_unix_ms: result.started_at_unix_ms,
                transcripts: vec![transcript1, transcript2],
                merged_segments: vec![
                    MergedTranscriptSegment {
                        speaker: "spk_0".to_string(),
                        start_ms: 0,
                        end_ms: 900,
                        text: "こんにちは".to_string(),
                    },
                    MergedTranscriptSegment {
                        speaker: "spk_1".to_string(),
                        start_ms: 950,
                        end_ms: 2_300,
                        text: "今日はよろしくお願いします".to_string(),
                    },
                    MergedTranscriptSegment {
                        speaker: "spk_0".to_string(),
                        start_ms: 165_000,
                        end_ms: 165_700,
                        text: "途中終了".to_string(),
                    },
                ],
                transcription_failures: Vec::new(),
            }
        );
        assert_eq!(
            logger.entries(),
            vec![
                info("recording started"),
                info("transcription request sent for capture 1"),
                info("transcription response received for capture 1"),
                info("interrupt received, finalizing recorded audio"),
                info("recording finished"),
                info("transcription request sent for capture 2"),
                info("transcription response received for capture 2"),
            ]
        );
    }

    #[test]
    /// 非回復な transcription 失敗は run 全体の失敗として即時に返す。
    fn stops_on_nonrecoverable_transcription_failure() {
        let config = capture_config(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        let audio1 = sample_audio();
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![audio1.clone(), sample_audio()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![Err(TranscriberError::ParseResponseBody {
                source: "invalid json".to_string(),
                body: "{".to_string(),
            })]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        let error = run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            CaptureError::Transcribe(TranscriberError::ParseResponseBody { .. })
        ));
        assert_eq!(*capture_store.observed_audios.borrow(), vec![(1, audio1)]);
        assert!(capture_store.observed_transcripts.borrow().is_empty());
        assert_eq!(
            logger.entries(),
            vec![
                info("recording started"),
                info("transcription request sent for capture 1"),
            ]
        );
    }

    #[test]
    /// 通常経路では録音開始と capture ごとの送受信イベントを順序どおり記録する。
    fn records_normal_operation_logs_in_order() {
        let config = capture_config(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![sample_audio(), sample_audio()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![Ok(sample_transcript()), Ok(sample_transcript())]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(
            logger.entries(),
            vec![
                info("recording started"),
                info("transcription request sent for capture 1"),
                info("transcription response received for capture 1"),
                info("recording finished"),
                info("transcription request sent for capture 2"),
                info("transcription response received for capture 2"),
            ]
        );
    }

    #[test]
    /// debug 無効時は標準出力へ何も書かず、有効時だけ transcript JSON を capture ごとに出力する。
    fn writes_debug_transcript_only_when_debug_enabled() {
        let transcripts = vec![sample_transcript(), sample_transcript()];
        let mut disabled_output = Vec::new();
        let mut enabled_output = Vec::new();

        write_debug_transcript(false, &mut disabled_output, &transcripts).unwrap();
        write_debug_transcript(true, &mut enabled_output, &transcripts).unwrap();

        assert!(disabled_output.is_empty());
        let parsed = serde_json::Deserializer::from_slice(&enabled_output)
            .into_iter::<serde_json::Value>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(parsed.len(), transcripts.len());
        assert_eq!(
            parsed[0]["text"],
            serde_json::Value::String(transcripts[0].text.clone())
        );
        assert_eq!(
            parsed[1]["text"],
            serde_json::Value::String(transcripts[1].text.clone())
        );
    }

    #[test]
    /// capture 開始前に session metadata を保存し、各 merge 判定の監査ログも保存する。
    fn persists_session_metadata_and_merge_audit_entries() {
        let config = capture_config(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(18),
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![sample_audio(), sample_audio()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![
                Ok(DiarizedTranscript {
                    text: "ABCDEFGHIJKLMNOP".to_string(),
                    segments: vec![TranscriptSegment {
                        speaker: "spk_0".to_string(),
                        start_ms: 10_000,
                        end_ms: 18_000,
                        text: "ABCDEFGHIJKLMNOP".to_string(),
                    }],
                }),
                Ok(DiarizedTranscript {
                    text: "EFGHIJKLMNOPQRST".to_string(),
                    segments: vec![TranscriptSegment {
                        speaker: "spk_0".to_string(),
                        start_ms: 0,
                        end_ms: 8_000,
                        text: "EFGHIJKLMNOPQRST".to_string(),
                    }],
                }),
            ]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(
            *capture_store.observed_session_metadata.borrow(),
            vec![CaptureSessionMetadata {
                recording_duration_ms: 40_000,
                capture_duration_ms: 30_000,
                capture_overlap_ms: 18_000,
                capture_silence_threshold_dbfs: -42.0,
                capture_silence_min_duration_ms: 700,
                capture_tail_silence_min_duration_ms: 250,
                transcription_model: TRANSCRIPTION_MODEL.to_string(),
                transcription_language: TEST_TRANSCRIPTION_LANGUAGE.to_string(),
                response_format: "diarized_json".to_string(),
                chunking_strategy: "auto".to_string(),
                merge_policy: TranscriptMergePolicy::recommended(),
                fixed_speaker: None,
            }]
        );
        assert_eq!(
            *capture_store.observed_merge_audit_entries.borrow(),
            vec![MergeAuditEntry {
                capture_index: 2,
                previous_overlap_range: MergeOverlapRangeSnapshot {
                    start_ms: 12_000,
                    end_ms: 18_000,
                    text: "EFGHIJKLMNOP".to_string(),
                    normalized_char_count: 12,
                },
                current_overlap_range: MergeOverlapRangeSnapshot {
                    start_ms: 12_000,
                    end_ms: 18_000,
                    text: "EFGHIJKLMNOP".to_string(),
                    normalized_char_count: 12,
                },
                outcome: MergeAuditOutcome::Accepted {
                    overlap_chars: 12,
                    alignment_ratio: 1.0,
                    trigram_similarity: 1.0,
                    current_prefix_trim_chars: 0,
                    overlap_text_source: crate::domain::MergeOverlapTextSource::CurrentOverlapRange,
                },
            }]
        );
    }

    #[test]
    /// 最終 capture を切り出したら最後の文字起こし前に録音 session を破棄する。
    fn drops_recording_session_before_transcribing_final_capture() {
        let config = capture_config(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![sample_audio(), sample_audio()],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![Ok(sample_transcript()), Ok(sample_transcript())]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(*transcriber.observed_drop_counts.borrow(), vec![0, 1]);
    }

    #[test]
    /// 録音終端に届く capture の後ろに tail capture が続く場合でも、最後の capture までは session を維持する。
    fn keeps_recording_session_until_tail_capture_is_copied() {
        let config = capture_config(
            Duration::from_secs(20),
            Duration::from_secs(10),
            Duration::from_secs(5),
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(
                Rc::clone(&observation),
                vec![
                    sample_audio(),
                    sample_audio(),
                    sample_audio(),
                    sample_audio(),
                ],
            )),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: Some(Rc::clone(&observation)),
            outcomes: VecDeque::from(vec![
                Ok(sample_transcript()),
                Ok(sample_transcript()),
                Ok(sample_transcript()),
                Ok(sample_transcript()),
            ]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(
            observation.borrow().captured_windows,
            vec![
                (Duration::from_secs(0), Duration::from_secs(10)),
                (Duration::from_secs(5), Duration::from_secs(10)),
                (Duration::from_secs(10), Duration::from_secs(10)),
                (Duration::from_secs(15), Duration::from_secs(5)),
            ]
        );
        assert_eq!(*transcriber.observed_drop_counts.borrow(), vec![0, 0, 0, 1]);
    }

    #[test]
    /// マイク用に固定話者名を指定した場合は転写済み segment の speaker を上書きする。
    fn replaces_segment_speaker_names_when_fixed_speaker_is_configured() {
        let config = capture_config(
            Duration::from_secs(180),
            Duration::from_secs(180),
            Duration::ZERO,
        );
        let observation = Rc::new(RefCell::new(RecordingObservation::default()));
        let mut recorder = FakeRecorder {
            observation: Rc::clone(&observation),
            session: Some(FakeRecordingSession::new(observation, vec![sample_audio()])),
        };
        let mut transcriber = FakeTranscriber {
            observed_requests: RefCell::new(Vec::new()),
            observed_drop_counts: RefCell::new(Vec::new()),
            recording_observation: None,
            outcomes: VecDeque::from(vec![Ok(sample_transcript())]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let logger = test_capture_logger();

        let result = run_capture(
            &config,
            &[],
            &SpeakerLabel::Fixed("me".to_string()),
            &logger,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
        )
        .unwrap();

        assert_eq!(
            result.merged_segments,
            vec![
                MergedTranscriptSegment {
                    speaker: "me".to_string(),
                    start_ms: 0,
                    end_ms: 900,
                    text: "こんにちは".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "me".to_string(),
                    start_ms: 950,
                    end_ms: 2_300,
                    text: "今日はよろしくお願いします".to_string(),
                },
            ]
        );
        assert_eq!(
            capture_store.observed_session_metadata.borrow()[0].fixed_speaker,
            Some("me".to_string())
        );
    }
}
