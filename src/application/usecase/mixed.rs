use super::capture::CaptureRunResult;
use crate::application::ports::{
    CaptureStoreError, MixedCaptureSessionMetadata, MixedCaptureSourceOutcome,
    MixedCaptureSourceStatus, MixedCaptureStore,
};
use crate::domain::{MergedTranscriptSegment, SourcedTranscriptSegment, TranscriptSource};
use std::fmt;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

/// mixed capture で source ごとに完了した merge 済み segment 群です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMergedSegments {
    pub source: TranscriptSource,
    pub started_at_unix_ms: u64,
    pub segments: Vec<MergedTranscriptSegment>,
}

/// mixed capture で 1 source 分の実行結果です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixedSourceRun {
    pub source: TranscriptSource,
    pub started_at_unix_ms: u64,
    pub result: Result<CaptureRunResult, String>,
}

/// mixed capture 全体の結果です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixedCaptureRunResult {
    pub final_segments: Vec<SourcedTranscriptSegment>,
    pub source_outcomes: Vec<MixedCaptureSourceOutcome>,
    pub debug_transcripts: Vec<crate::domain::DiarizedTranscript>,
}

impl MixedCaptureRunResult {
    /// すべての source が成功したかを返します。
    pub fn completed_without_failures(&self) -> bool {
        self.source_outcomes
            .iter()
            .all(|outcome| outcome.status == MixedCaptureSourceStatus::Succeeded)
    }
}

/// mixed capture 実行の失敗です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixedCaptureError {
    Store(CaptureStoreError),
    AllSourcesFailed {
        failures: Vec<MixedCaptureSourceFailure>,
    },
}

impl fmt::Display for MixedCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(f, "mixed capture persistence failed: {error}"),
            Self::AllSourcesFailed { failures } => {
                let joined = failures
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "all mixed capture sources failed: {joined}")
            }
        }
    }
}

impl std::error::Error for MixedCaptureError {}

/// mixed capture で source 単位に失敗した情報です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixedCaptureSourceFailure {
    pub source: TranscriptSource,
    pub message: String,
}

impl fmt::Display for MixedCaptureSourceFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.source, self.message)
    }
}

/// source ごとの merge 済み segment 群を絶対時刻ベースで最終統合します。
pub fn merge_source_segments(
    source_segments: &[SourceMergedSegments],
) -> Vec<SourcedTranscriptSegment> {
    let mut merged = source_segments
        .iter()
        .flat_map(|source_segments| {
            source_segments.segments.iter().map(|segment| {
                SourcedTranscriptSegment::from_merged_with_offset(
                    source_segments.source,
                    source_segments.started_at_unix_ms,
                    segment,
                )
            })
        })
        .collect::<Vec<_>>();
    merged.sort_by_key(|segment| {
        (
            segment.start_ms,
            segment.end_ms,
            segment.source.sort_order(),
            segment.speaker.clone(),
            segment.text.clone(),
        )
    });
    merged
}

/// mixed capture の結果を保存し、最終統合結果を返します。
pub fn finalize_mixed_capture<S>(
    store: &mut S,
    mut metadata: MixedCaptureSessionMetadata,
    source_runs: Vec<MixedSourceRun>,
) -> Result<MixedCaptureRunResult, CaptureStoreError>
where
    S: MixedCaptureStore,
{
    let source_outcomes = source_runs
        .iter()
        .map(MixedCaptureSourceOutcome::from_source_run)
        .collect::<Vec<_>>();
    metadata.source_outcomes = source_outcomes.clone();
    store.persist_mixed_session_metadata(&metadata)?;

    let successful_runs = source_runs
        .iter()
        .filter_map(|run| {
            run.result.as_ref().ok().map(|result| SourceMergedSegments {
                source: run.source,
                started_at_unix_ms: result.started_at_unix_ms,
                segments: result.merged_segments.clone(),
            })
        })
        .collect::<Vec<_>>();
    let final_segments = merge_source_segments(&successful_runs);
    store.persist_final_segments(&final_segments)?;

    let debug_transcripts = source_runs
        .into_iter()
        .filter_map(|run| run.result.ok())
        .flat_map(|result| result.transcripts)
        .collect::<Vec<_>>();

    Ok(MixedCaptureRunResult {
        final_segments,
        source_outcomes,
        debug_transcripts,
    })
}

/// microphone/application の capture 実行を並列で起動し、必ず両方回収して結果を保存します。
pub fn run_mixed_capture<S, MF, AF>(
    store: &mut S,
    metadata: MixedCaptureSessionMetadata,
    microphone_capture: MF,
    application_capture: AF,
) -> Result<MixedCaptureRunResult, MixedCaptureError>
where
    S: MixedCaptureStore,
    MF: FnOnce() -> Result<CaptureRunResult, String> + Send + 'static,
    AF: FnOnce() -> Result<CaptureRunResult, String> + Send + 'static,
{
    let microphone_handle = spawn_source_capture(TranscriptSource::Microphone, microphone_capture);
    let application_handle =
        spawn_source_capture(TranscriptSource::Application, application_capture);

    let source_runs = vec![
        join_source_capture(TranscriptSource::Microphone, microphone_handle),
        join_source_capture(TranscriptSource::Application, application_handle),
    ];
    let has_success = source_runs.iter().any(|run| run.result.is_ok());

    let result =
        finalize_mixed_capture(store, metadata, source_runs).map_err(MixedCaptureError::Store)?;
    if has_success {
        Ok(result)
    } else {
        Err(MixedCaptureError::AllSourcesFailed {
            failures: result
                .source_outcomes
                .iter()
                .map(|outcome| MixedCaptureSourceFailure {
                    source: outcome.source,
                    message: outcome
                        .error_message
                        .clone()
                        .unwrap_or_else(|| "unknown source failure".to_string()),
                })
                .collect(),
        })
    }
}

fn spawn_source_capture<F>(
    source: TranscriptSource,
    capture: F,
) -> thread::JoinHandle<MixedSourceRun>
where
    F: FnOnce() -> Result<CaptureRunResult, String> + Send + 'static,
{
    thread::spawn(move || MixedSourceRun {
        source,
        started_at_unix_ms: current_unix_ms(),
        result: capture(),
    })
}

fn join_source_capture(
    source: TranscriptSource,
    handle: thread::JoinHandle<MixedSourceRun>,
) -> MixedSourceRun {
    match handle.join() {
        Ok(run) => run,
        Err(_) => MixedSourceRun {
            source,
            started_at_unix_ms: current_unix_ms(),
            result: Err("capture thread panicked".to_string()),
        },
    }
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

impl MixedCaptureSourceOutcome {
    fn from_source_run(run: &MixedSourceRun) -> Self {
        match &run.result {
            Ok(result) if result.completed_without_failures() => Self {
                source: run.source,
                started_at_unix_ms: result.started_at_unix_ms,
                status: MixedCaptureSourceStatus::Succeeded,
                transcription_failure_count: 0,
                error_message: None,
            },
            Ok(result) => Self {
                source: run.source,
                started_at_unix_ms: result.started_at_unix_ms,
                status: MixedCaptureSourceStatus::PartialFailure,
                transcription_failure_count: result.transcription_failures.len(),
                error_message: None,
            },
            Err(message) => Self {
                source: run.source,
                started_at_unix_ms: run.started_at_unix_ms,
                status: MixedCaptureSourceStatus::Failed,
                transcription_failure_count: 0,
                error_message: Some(message.clone()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::{
        MixedCaptureSessionMetadata, MixedCaptureSourceSettings, MixedCaptureStore,
    };
    use crate::domain::{
        DiarizedTranscript, MergedTranscriptSegment, TranscriptMergePolicy, TranscriptSegment,
        TranscriptSource,
    };
    use std::cell::RefCell;

    #[derive(Debug, Default)]
    struct FakeMixedCaptureStore {
        observed_metadata: RefCell<Vec<MixedCaptureSessionMetadata>>,
        observed_final_segments: RefCell<Vec<SourcedTranscriptSegment>>,
    }

    impl MixedCaptureStore for FakeMixedCaptureStore {
        fn persist_mixed_session_metadata(
            &mut self,
            metadata: &MixedCaptureSessionMetadata,
        ) -> Result<(), CaptureStoreError> {
            self.observed_metadata.borrow_mut().push(metadata.clone());
            Ok(())
        }

        fn persist_final_segments(
            &mut self,
            segments: &[SourcedTranscriptSegment],
        ) -> Result<(), CaptureStoreError> {
            self.observed_final_segments
                .borrow_mut()
                .extend_from_slice(segments);
            Ok(())
        }
    }

    fn sample_metadata() -> MixedCaptureSessionMetadata {
        MixedCaptureSessionMetadata {
            mode: "mixed".to_string(),
            application_bundle_id: "us.zoom.xos".to_string(),
            microphone_speaker: "me".to_string(),
            source_settings: vec![
                MixedCaptureSourceSettings {
                    source: TranscriptSource::Microphone,
                    recording_duration_ms: 30_000,
                    capture_duration_ms: 15_000,
                    capture_overlap_ms: 1_000,
                    transcription_model: "model".to_string(),
                    transcription_language: "ja".to_string(),
                    response_format: "json".to_string(),
                    chunking_strategy: "auto".to_string(),
                    merge_policy: TranscriptMergePolicy::recommended(),
                    fixed_speaker: Some("me".to_string()),
                },
                MixedCaptureSourceSettings {
                    source: TranscriptSource::Application,
                    recording_duration_ms: 30_000,
                    capture_duration_ms: 15_000,
                    capture_overlap_ms: 1_000,
                    transcription_model: "model".to_string(),
                    transcription_language: "ja".to_string(),
                    response_format: "json".to_string(),
                    chunking_strategy: "auto".to_string(),
                    merge_policy: TranscriptMergePolicy::recommended(),
                    fixed_speaker: None,
                },
            ],
            source_outcomes: Vec::new(),
        }
    }

    fn sample_capture_result(
        started_at_unix_ms: u64,
        speaker: &str,
        start_ms: u64,
        end_ms: u64,
        text: &str,
    ) -> CaptureRunResult {
        CaptureRunResult {
            started_at_unix_ms,
            transcripts: vec![DiarizedTranscript {
                text: text.to_string(),
                segments: vec![TranscriptSegment {
                    speaker: speaker.to_string(),
                    start_ms,
                    end_ms,
                    text: text.to_string(),
                }],
            }],
            merged_segments: vec![MergedTranscriptSegment {
                speaker: speaker.to_string(),
                start_ms,
                end_ms,
                text: text.to_string(),
            }],
            transcription_failures: Vec::new(),
        }
    }

    #[test]
    /// source 間統合では source ごとの開始絶対時刻を加味して並べ替える。
    fn merges_source_segments_by_absolute_time() {
        let merged = merge_source_segments(&[
            SourceMergedSegments {
                source: TranscriptSource::Application,
                started_at_unix_ms: 2_000,
                segments: vec![MergedTranscriptSegment {
                    speaker: "alice".to_string(),
                    start_ms: 100,
                    end_ms: 300,
                    text: "app".to_string(),
                }],
            },
            SourceMergedSegments {
                source: TranscriptSource::Microphone,
                started_at_unix_ms: 1_000,
                segments: vec![MergedTranscriptSegment {
                    speaker: "me".to_string(),
                    start_ms: 900,
                    end_ms: 1_200,
                    text: "mic".to_string(),
                }],
            },
        ]);

        assert_eq!(
            merged,
            vec![
                SourcedTranscriptSegment {
                    source: TranscriptSource::Microphone,
                    speaker: "me".to_string(),
                    start_ms: 1_900,
                    end_ms: 2_200,
                    text: "mic".to_string(),
                },
                SourcedTranscriptSegment {
                    source: TranscriptSource::Application,
                    speaker: "alice".to_string(),
                    start_ms: 2_100,
                    end_ms: 2_300,
                    text: "app".to_string(),
                },
            ]
        );
    }

    #[test]
    /// 同時刻の発話は両方残し、同率時は microphone を先にする。
    fn keeps_both_sources_and_uses_deterministic_tie_breaker() {
        let merged = merge_source_segments(&[
            SourceMergedSegments {
                source: TranscriptSource::Application,
                started_at_unix_ms: 1_000,
                segments: vec![MergedTranscriptSegment {
                    speaker: "alice".to_string(),
                    start_ms: 100,
                    end_ms: 200,
                    text: "same".to_string(),
                }],
            },
            SourceMergedSegments {
                source: TranscriptSource::Microphone,
                started_at_unix_ms: 1_000,
                segments: vec![MergedTranscriptSegment {
                    speaker: "me".to_string(),
                    start_ms: 100,
                    end_ms: 200,
                    text: "same".to_string(),
                }],
            },
        ]);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].source, TranscriptSource::Microphone);
        assert_eq!(merged[1].source, TranscriptSource::Application);
    }

    #[test]
    /// 片方の source が失敗しても成功分は final merged と metadata に残す。
    fn persists_partial_results_when_one_source_fails() {
        let mut store = FakeMixedCaptureStore::default();

        let result = finalize_mixed_capture(
            &mut store,
            sample_metadata(),
            vec![
                MixedSourceRun {
                    source: TranscriptSource::Microphone,
                    started_at_unix_ms: 1_000,
                    result: Ok(sample_capture_result(1_000, "me", 0, 100, "hello")),
                },
                MixedSourceRun {
                    source: TranscriptSource::Application,
                    started_at_unix_ms: 1_050,
                    result: Err("capture failed".to_string()),
                },
            ],
        )
        .unwrap();

        assert_eq!(
            result.source_outcomes,
            vec![
                MixedCaptureSourceOutcome {
                    source: TranscriptSource::Microphone,
                    started_at_unix_ms: 1_000,
                    status: MixedCaptureSourceStatus::Succeeded,
                    transcription_failure_count: 0,
                    error_message: None,
                },
                MixedCaptureSourceOutcome {
                    source: TranscriptSource::Application,
                    started_at_unix_ms: 1_050,
                    status: MixedCaptureSourceStatus::Failed,
                    transcription_failure_count: 0,
                    error_message: Some("capture failed".to_string()),
                },
            ]
        );
        assert_eq!(store.observed_metadata.borrow().len(), 1);
        assert_eq!(store.observed_final_segments.borrow().len(), 1);
        assert!(!result.completed_without_failures());
    }

    #[test]
    /// 成功した source の absolute time 基準は thread 開始時刻ではなく録音開始時刻を使う。
    fn uses_recording_started_time_from_capture_result_for_final_merge() {
        let mut store = FakeMixedCaptureStore::default();

        let result = finalize_mixed_capture(
            &mut store,
            sample_metadata(),
            vec![MixedSourceRun {
                source: TranscriptSource::Microphone,
                started_at_unix_ms: 50,
                result: Ok(CaptureRunResult {
                    started_at_unix_ms: 1_000,
                    transcripts: Vec::new(),
                    merged_segments: vec![MergedTranscriptSegment {
                        speaker: "me".to_string(),
                        start_ms: 10,
                        end_ms: 20,
                        text: "hello".to_string(),
                    }],
                    transcription_failures: Vec::new(),
                }),
            }],
        )
        .unwrap();

        assert_eq!(result.final_segments[0].start_ms, 1_010);
        assert_eq!(result.source_outcomes[0].started_at_unix_ms, 1_000);
    }
}
