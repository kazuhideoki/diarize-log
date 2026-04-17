use crate::application::ports::{
    CaptureSessionMetadata, CaptureStore, CaptureStoreError, ChunkingStrategy, InterruptMonitor,
    Recorder, RecorderError, RecordingSession, RecordingWaitOutcome, ResponseFormat, Transcriber,
    TranscriberError, TranscriptionRequest,
};
use crate::domain::{
    CaptureMerger, CapturePolicy, CaptureRange, CapturedTranscript, DiarizedTranscript,
    KnownSpeakerSample, MergedTranscriptSegment, RecordedAudio, TranscriptMergePolicy,
    TranscriptSegment,
};
use std::fmt;
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const TRANSCRIPTION_MODEL: &str = "gpt-4o-transcribe-diarize";

/// 連続録音から capture を切り出して文字起こしするユースケースの設定です。
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureConfig {
    pub capture_policy: CapturePolicy,
    pub merge_policy: TranscriptMergePolicy,
    pub response_format: ResponseFormat,
    pub transcription_model: &'static str,
    pub transcription_language: String,
    pub chunking_strategy: ChunkingStrategy,
}

/// capture 実行時の話者ラベル適用方針です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakerLabel {
    KeepOriginal,
    Fixed(String),
}

impl CaptureConfig {
    pub fn new(
        recording_duration: Duration,
        capture_duration: Duration,
        capture_overlap: Duration,
        transcription_language: String,
    ) -> Self {
        Self {
            capture_policy: CapturePolicy {
                recording_duration,
                capture_duration,
                capture_overlap,
            },
            merge_policy: TranscriptMergePolicy::recommended(),
            response_format: ResponseFormat::DiarizedJson,
            transcription_model: TRANSCRIPTION_MODEL,
            transcription_language,
            chunking_strategy: ChunkingStrategy::Auto,
        }
    }
}

/// capture run の成功・部分失敗をまとめた結果です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRunResult {
    pub started_at_unix_ms: u64,
    pub transcripts: Vec<DiarizedTranscript>,
    pub merged_segments: Vec<MergedTranscriptSegment>,
    pub transcription_failures: Vec<CaptureTranscriptionFailure>,
}

impl CaptureRunResult {
    /// すべての capture が文字起こし成功で完了したかを返します。
    pub fn completed_without_failures(&self) -> bool {
        self.transcription_failures.is_empty()
    }
}

/// 継続実行した capture のうち文字起こしに失敗した 1 件の記録です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureTranscriptionFailure {
    pub capture_index: u64,
    pub capture_start_ms: u64,
    pub message: String,
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

struct NoopInterruptMonitor;

impl InterruptMonitor for NoopInterruptMonitor {
    fn is_interrupt_requested(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingCaptureAudio {
    range: CaptureRange,
    audio: RecordedAudio,
}

/// 連続録音と文字起こしを実行します。
pub fn run_capture<R, T, S, L>(
    config: &CaptureConfig,
    speaker_samples: &[KnownSpeakerSample],
    speaker_label: &SpeakerLabel,
    recorder: &mut R,
    transcriber: &mut T,
    capture_store: &mut S,
    stderr: &mut L,
) -> Result<CaptureRunResult, CaptureError>
where
    R: Recorder,
    T: Transcriber,
    S: CaptureStore,
    L: Write,
{
    run_capture_with_interrupt_monitor(
        config,
        speaker_samples,
        speaker_label,
        recorder,
        transcriber,
        capture_store,
        stderr,
        &NoopInterruptMonitor,
    )
}

/// 連続録音と文字起こしを実行し、中断要求が来たら録音済みぶんだけを処理して終了します。
#[allow(clippy::too_many_arguments)]
pub fn run_capture_with_interrupt_monitor<R, T, S, L>(
    config: &CaptureConfig,
    speaker_samples: &[KnownSpeakerSample],
    speaker_label: &SpeakerLabel,
    recorder: &mut R,
    transcriber: &mut T,
    capture_store: &mut S,
    stderr: &mut L,
    interrupt_monitor: &dyn InterruptMonitor,
) -> Result<CaptureRunResult, CaptureError>
where
    R: Recorder,
    T: Transcriber,
    S: CaptureStore,
    L: Write,
{
    run_capture_with_clock(
        config,
        speaker_samples,
        speaker_label,
        recorder,
        transcriber,
        capture_store,
        stderr,
        interrupt_monitor,
        current_unix_ms,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_capture_with_clock<R, T, S, L, C>(
    config: &CaptureConfig,
    speaker_samples: &[KnownSpeakerSample],
    speaker_label: &SpeakerLabel,
    recorder: &mut R,
    transcriber: &mut T,
    capture_store: &mut S,
    stderr: &mut L,
    interrupt_monitor: &dyn InterruptMonitor,
    current_unix_ms: C,
) -> Result<CaptureRunResult, CaptureError>
where
    R: Recorder,
    T: Transcriber,
    S: CaptureStore,
    L: Write,
    C: Fn() -> u64,
{
    capture_store
        .persist_session_metadata(&CaptureSessionMetadata {
            recording_duration_ms: duration_to_millis(config.capture_policy.recording_duration),
            capture_duration_ms: duration_to_millis(config.capture_policy.capture_duration),
            capture_overlap_ms: duration_to_millis(config.capture_policy.capture_overlap),
            transcription_model: config.transcription_model.to_string(),
            transcription_language: config.transcription_language.clone(),
            response_format: config.response_format.as_api_value().to_string(),
            chunking_strategy: config.chunking_strategy.as_api_value().to_string(),
            merge_policy: config.merge_policy.clone(),
        })
        .map_err(CaptureError::Store)?;
    info_log(stderr, "recording started").map_err(CaptureError::Write)?;
    let mut session = Some(recorder.start_recording().map_err(CaptureError::Record)?);
    let started_at_unix_ms = current_unix_ms();
    let capture_ranges = config.capture_policy.capture_ranges();
    let capture_count = capture_ranges.len();
    let mut capture_merger = CaptureMerger::new(config.merge_policy.clone());
    let mut transcripts = Vec::with_capacity(capture_ranges.len());
    let mut merged_segments = Vec::new();
    let mut transcription_failures = Vec::new();

    for (capture_position, capture_range) in capture_ranges.into_iter().enumerate() {
        let is_last_capture = capture_position + 1 == capture_count;
        let wait_outcome = session
            .as_mut()
            .expect("recording session must exist until the final capture is copied")
            .wait_until(capture_range.end_offset(), interrupt_monitor)
            .map_err(CaptureError::Record)?;

        if wait_outcome == RecordingWaitOutcome::Interrupted {
            info_log(stderr, "interrupt received, finalizing recorded audio")
                .map_err(CaptureError::Write)?;
            let available_duration = session
                .as_mut()
                .expect("recording session must exist until interrupted captures are copied")
                .recorded_duration()
                .map_err(CaptureError::Record)?;
            let interrupted_policy = CapturePolicy {
                recording_duration: available_duration,
                capture_duration: config.capture_policy.capture_duration,
                capture_overlap: config.capture_policy.capture_overlap,
            };
            let pending_ranges = interrupted_policy
                .capture_ranges()
                .into_iter()
                .skip(capture_position)
                .collect::<Vec<_>>();
            let pending_audios = capture_pending_audios(
                session
                    .as_mut()
                    .expect("recording session must exist until interrupted captures are copied"),
                capture_store,
                pending_ranges,
            )?;
            drop(session.take());
            info_log(stderr, "recording finished").map_err(CaptureError::Write)?;
            for pending in pending_audios {
                process_capture_audio(
                    pending.range,
                    pending.audio,
                    config,
                    speaker_samples,
                    speaker_label,
                    transcriber,
                    capture_store,
                    stderr,
                    &mut capture_merger,
                    &mut transcripts,
                    &mut merged_segments,
                    &mut transcription_failures,
                )?;
            }
            break;
        }
        let audio = capture_audio(
            session
                .as_mut()
                .expect("recording session must exist until the final capture is copied"),
            capture_store,
            capture_range,
        )?;
        if is_last_capture {
            drop(session.take());
            info_log(stderr, "recording finished").map_err(CaptureError::Write)?;
        }
        process_capture_audio(
            capture_range,
            audio,
            config,
            speaker_samples,
<<<<<<< HEAD
            speaker_label,
            transcriber,
            capture_store,
=======
            model: config.transcription_model,
            language: config.transcription_language.as_str(),
            response_format: config.response_format,
            chunking_strategy: config.chunking_strategy,
        }) {
            Ok(transcript) => transcript,
            Err(error) => {
                if !is_recoverable_transcription_error(&error) {
                    return Err(CaptureError::Transcribe(error));
                }
                info_log(
                    stderr,
                    &format!(
                        "transcription failed for capture {}, continuing: {error}",
                        capture_range.capture_index
                    ),
                )
                .map_err(CaptureError::Write)?;
                transcription_failures.push(CaptureTranscriptionFailure {
                    capture_index: capture_range.capture_index,
                    capture_start_ms,
                    message: error.to_string(),
                });
                continue;
            }
        };
        let transcript = apply_speaker_label(transcript, speaker_label);
        info_log(
>>>>>>> main
            stderr,
            &mut capture_merger,
            &mut transcripts,
            &mut merged_segments,
            &mut transcription_failures,
        )?;
    }
    let tail_segments = capture_merger.finish();
    capture_store
        .persist_merged_segments(&tail_segments)
        .map_err(CaptureError::Store)?;
    merged_segments.extend(tail_segments);

    if !transcription_failures.is_empty() {
        info_log(
            stderr,
            &format!(
                "capture run completed with partial transcription failures: succeeded={} failed={} total={}",
                transcripts.len(),
                transcription_failures.len(),
                transcripts.len() + transcription_failures.len()
            ),
        )
        .map_err(CaptureError::Write)?;
    }

    Ok(CaptureRunResult {
        started_at_unix_ms,
        transcripts,
        merged_segments,
        transcription_failures,
    })
}

fn capture_pending_audios<SN, ST>(
    session: &mut SN,
    capture_store: &mut ST,
    pending_ranges: Vec<CaptureRange>,
) -> Result<Vec<PendingCaptureAudio>, CaptureError>
where
    SN: RecordingSession,
    ST: CaptureStore,
{
    pending_ranges
        .into_iter()
        .map(|range| {
            let audio = capture_audio(session, capture_store, range)?;
            Ok(PendingCaptureAudio { range, audio })
        })
        .collect()
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

#[allow(clippy::too_many_arguments)]
fn process_capture_audio<T, S, L>(
    capture_range: CaptureRange,
    audio: RecordedAudio,
    config: &CaptureConfig,
    speaker_samples: &[KnownSpeakerSample],
    speaker_label: &SpeakerLabel,
    transcriber: &mut T,
    capture_store: &mut S,
    stderr: &mut L,
    capture_merger: &mut CaptureMerger,
    transcripts: &mut Vec<DiarizedTranscript>,
    merged_segments: &mut Vec<MergedTranscriptSegment>,
    transcription_failures: &mut Vec<CaptureTranscriptionFailure>,
) -> Result<(), CaptureError>
where
    T: Transcriber,
    S: CaptureStore,
    L: Write,
{
    info_log(
        stderr,
        &format!(
            "transcription request sent for capture {}",
            capture_range.capture_index
        ),
    )
    .map_err(CaptureError::Write)?;
    let capture_start_ms = duration_to_millis(capture_range.start_offset);
    let capture_end_ms = duration_to_millis(capture_range.end_offset());
    let transcript = match transcriber.transcribe(TranscriptionRequest {
        audio: &audio,
        speaker_samples,
        model: config.transcription_model,
        response_format: config.response_format,
        chunking_strategy: config.chunking_strategy,
    }) {
        Ok(transcript) => transcript,
        Err(error) => {
            if !is_recoverable_transcription_error(&error) {
                return Err(CaptureError::Transcribe(error));
            }
            info_log(
                stderr,
                &format!(
                    "transcription failed for capture {}, continuing: {error}",
                    capture_range.capture_index
                ),
            )
            .map_err(CaptureError::Write)?;
            transcription_failures.push(CaptureTranscriptionFailure {
                capture_index: capture_range.capture_index,
                capture_start_ms,
                message: error.to_string(),
            });
            return Ok(());
        }
    };
    let transcript = apply_speaker_label(transcript, speaker_label);
    info_log(
        stderr,
        &format!(
            "transcription response received for capture {}",
            capture_range.capture_index
        ),
    )
    .map_err(CaptureError::Write)?;
    capture_store
        .persist_transcript(capture_range.capture_index, capture_start_ms, &transcript)
        .map_err(CaptureError::Store)?;
    let merge_batch = capture_merger.push_capture(CapturedTranscript::from_relative(
        capture_range.capture_index,
        capture_start_ms,
        capture_end_ms,
        &transcript,
    ));
    capture_store
        .persist_merge_audit_entries(&merge_batch.audit_entries)
        .map_err(CaptureError::Store)?;
    capture_store
        .persist_merged_segments(&merge_batch.finalized_segments)
        .map_err(CaptureError::Store)?;
    merged_segments.extend(merge_batch.finalized_segments);
    transcripts.push(transcript);
    Ok(())
}

fn is_recoverable_transcription_error(error: &TranscriberError) -> bool {
    matches!(error, TranscriberError::SendRequest(_))
}

fn apply_speaker_label(
    transcript: DiarizedTranscript,
    speaker_label: &SpeakerLabel,
) -> DiarizedTranscript {
    match speaker_label {
        SpeakerLabel::KeepOriginal => transcript,
        SpeakerLabel::Fixed(speaker_name) => DiarizedTranscript {
            text: transcript.text,
            segments: transcript
                .segments
                .into_iter()
                .map(|segment| TranscriptSegment {
                    speaker: speaker_name.clone(),
                    start_ms: segment.start_ms,
                    end_ms: segment.end_ms,
                    text: segment.text,
                })
                .collect(),
        },
    }
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

fn current_unix_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after Unix epoch")
            .as_millis(),
    )
    .expect("unix timestamp in milliseconds must fit into u64")
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
    use crate::application::ports::{CaptureSessionMetadata, RecordingWaitOutcome};
    use crate::domain::{
        MergeAuditEntry, MergeAuditOutcome, MergeOverlapRangeSnapshot, MergedTranscriptSegment,
        RecordedAudio, TranscriptSegment,
    };
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    const TEST_TRANSCRIPTION_LANGUAGE: &str = "ja";

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CapturedRequest {
        wav_bytes: Vec<u8>,
        content_type: &'static str,
        speaker_samples: Vec<KnownSpeakerSample>,
        model: &'static str,
        language: String,
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
        recorded_duration: Duration,
    }

    impl FakeRecordingSession {
        fn new(observation: Rc<RefCell<RecordingObservation>>, audios: Vec<RecordedAudio>) -> Self {
            Self {
                observation,
                audios: VecDeque::from(audios),
                wait_outcomes: VecDeque::new(),
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
                recorded_duration,
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
        outcomes: VecDeque<Result<DiarizedTranscript, crate::application::ports::TranscriberError>>,
    }

    impl Transcriber for FakeTranscriber {
        fn transcribe(
            &mut self,
            request: TranscriptionRequest<'_>,
        ) -> Result<DiarizedTranscript, crate::application::ports::TranscriberError> {
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
                language: request.language.to_string(),
                response_format: request.response_format,
                chunking_strategy: request.chunking_strategy,
            });
            match self.outcomes.pop_front() {
                Some(Ok(transcript)) => Ok(transcript),
                Some(Err(error)) => Err(error),
                None => Err(
                    crate::application::ports::TranscriberError::ParseResponseBody {
                        source: "missing fake transcript".to_string(),
                        body: String::new(),
                    },
                ),
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
            TEST_TRANSCRIPTION_LANGUAGE.to_string(),
        )
    }

    #[test]
    /// run 結果には recorder.start_recording の直後に取得した録音開始 Unix 時刻を含める。
    fn returns_recording_started_unix_time_in_result() {
        let config = CaptureConfig::new(
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
        let mut stderr = Vec::new();

        let result = run_capture_with_clock(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
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
        let mut stderr = Vec::new();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
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
                    language: TEST_TRANSCRIPTION_LANGUAGE.to_string(),
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
                CapturedRequest {
                    wav_bytes: audio2.wav_bytes,
                    content_type: "audio/wav",
                    speaker_samples: Vec::new(),
                    model: TRANSCRIPTION_MODEL,
                    language: TEST_TRANSCRIPTION_LANGUAGE.to_string(),
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
                CapturedRequest {
                    wav_bytes: audio3.wav_bytes,
                    content_type: "audio/wav",
                    speaker_samples: Vec::new(),
                    model: TRANSCRIPTION_MODEL,
                    language: TEST_TRANSCRIPTION_LANGUAGE.to_string(),
                    response_format: ResponseFormat::DiarizedJson,
                    chunking_strategy: ChunkingStrategy::Auto,
                },
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
        let mut stderr = Vec::new();
        let speaker_sample = sample_known_speaker();

        run_capture(
            &config,
            std::slice::from_ref(&speaker_sample),
            &SpeakerLabel::KeepOriginal,
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
    /// 設定した transcription language を各 capture の文字起こしリクエストへ渡す。
    fn passes_transcription_language_to_each_request() {
        let mut config = capture_config(
            Duration::from_secs(40),
            Duration::from_secs(30),
            Duration::from_secs(10),
        );
        config.transcription_language = "en".to_string();
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
            outcomes: VecDeque::from(vec![Ok(sample_transcript()), Ok(sample_transcript())]),
        };
        let mut capture_store = FakeCaptureStore::new();
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
            transcriber
                .observed_requests
                .borrow()
                .iter()
                .map(|request| request.language.clone())
                .collect::<Vec<_>>(),
            vec!["en".to_string(), "en".to_string()]
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
        let mut stderr = Vec::new();

        let returned = run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
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
        let mut stderr = Vec::new();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
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
        let mut stderr = Vec::new();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
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
                Err(crate::application::ports::TranscriberError::SendRequest(
                    "simulated timeout".to_string(),
                )),
                Ok(success_transcript.clone()),
            ]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let mut stderr = Vec::new();

        let result = run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
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
            String::from_utf8(stderr).unwrap(),
            concat!(
                "recording started\n",
                "transcription request sent for capture 1\n",
                "transcription failed for capture 1, continuing: failed to send transcription request: simulated timeout\n",
                "recording finished\n",
                "transcription request sent for capture 2\n",
                "transcription response received for capture 2\n",
                "capture run completed with partial transcription failures: succeeded=1 failed=1 total=2\n"
            )
        );
    }

    #[test]
    /// 待機中に中断要求が来たら、録音済みぶんだけ切り出して session を閉じてから文字起こしする。
    fn finalizes_recorded_audio_when_interrupted_while_waiting_for_next_capture() {
        let config = CaptureConfig::new(
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
        let mut stderr = Vec::new();

        let result = run_capture_with_interrupt_monitor(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
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
            String::from_utf8(stderr).unwrap(),
            concat!(
                "recording started\n",
                "transcription request sent for capture 1\n",
                "transcription response received for capture 1\n",
                "interrupt received, finalizing recorded audio\n",
                "recording finished\n",
                "transcription request sent for capture 2\n",
                "transcription response received for capture 2\n"
            )
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
            outcomes: VecDeque::from(vec![Err(
                crate::application::ports::TranscriberError::ParseResponseBody {
                    source: "invalid json".to_string(),
                    body: "{".to_string(),
                },
            )]),
        };
        let mut capture_store = FakeCaptureStore::new();
        let mut stderr = Vec::new();

        let error = run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            CaptureError::Transcribe(
                crate::application::ports::TranscriberError::ParseResponseBody { .. }
            )
        ));
        assert_eq!(*capture_store.observed_audios.borrow(), vec![(1, audio1)]);
        assert!(capture_store.observed_transcripts.borrow().is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "recording started\ntranscription request sent for capture 1\n"
        );
    }

    #[test]
    /// 通常ログとして録音開始と capture ごとの API 送受信を標準エラーへ順序通りに出力する。
    fn writes_normal_operation_logs_to_stderr() {
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
        let mut stderr = Vec::new();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
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
        let mut stderr = Vec::new();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(
            *capture_store.observed_session_metadata.borrow(),
            vec![CaptureSessionMetadata {
                recording_duration_ms: 40_000,
                capture_duration_ms: 30_000,
                capture_overlap_ms: 18_000,
                transcription_model: TRANSCRIPTION_MODEL.to_string(),
                transcription_language: TEST_TRANSCRIPTION_LANGUAGE.to_string(),
                response_format: "diarized_json".to_string(),
                chunking_strategy: "auto".to_string(),
                merge_policy: TranscriptMergePolicy::recommended(),
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
        let mut stderr = Vec::new();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
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
        let mut stderr = Vec::new();

        run_capture(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
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
        let config = CaptureConfig::new(
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
        let mut stderr = Vec::new();

        let result = run_capture(
            &config,
            &[],
            &SpeakerLabel::Fixed("me".to_string()),
            &mut recorder,
            &mut transcriber,
            &mut capture_store,
            &mut stderr,
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
    }
}
