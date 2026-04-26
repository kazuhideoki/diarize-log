mod run;
mod transcript;

use crate::application::ports::{
    CaptureStore, CaptureStoreError, ChunkingStrategy, InterruptMonitor, Logger, Recorder,
    RecorderError, ResponseFormat, Transcriber, TranscriberError, TranscriptionLanguage,
};
use crate::domain::{
    CapturePolicy, DiarizedTranscript, KnownSpeakerSample, MergedTranscriptSegment,
    SilenceRequestPolicy, TranscriptMergePolicy,
};
use std::fmt;
use std::time::Duration;

pub use transcript::{DebugOutputError, write_debug_transcript};

pub const TRANSCRIPTION_MODEL: &str = "gpt-4o-transcribe-diarize";

/// 連続録音から capture を切り出して文字起こしするユースケースの設定です。
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureConfig {
    pub capture_policy: CapturePolicy,
    pub silence_request_policy: SilenceRequestPolicy,
    pub merge_policy: TranscriptMergePolicy,
    pub response_format: ResponseFormat,
    pub transcription_model: &'static str,
    pub transcription_language: TranscriptionLanguage,
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
        transcription_language: TranscriptionLanguage,
    ) -> Self {
        Self {
            capture_policy: CapturePolicy {
                recording_duration,
                capture_duration,
                capture_overlap,
            },
            silence_request_policy: SilenceRequestPolicy::recommended(),
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
            Self::Write(error) => write!(f, "logger write failed: {error}"),
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

/// 連続録音と文字起こしを実行します。
///
/// 中断監視が不要な通常経路です。
/// 実装本体は `run_capture_with_interrupt_monitor` に寄せ、ここでは
/// 中断要求が来ない `NoopInterruptMonitor` を差し込むだけにします。
#[allow(clippy::too_many_arguments)]
pub fn run_capture<R, T, S>(
    config: &CaptureConfig,
    speaker_samples: &[KnownSpeakerSample],
    speaker_label: &SpeakerLabel,
    logger: &dyn Logger,
    recorder: &mut R,
    transcriber: &mut T,
    capture_store: &mut S,
) -> Result<CaptureRunResult, CaptureError>
where
    R: Recorder,
    T: Transcriber + ?Sized,
    S: CaptureStore,
{
    run_capture_with_interrupt_monitor(
        config,
        speaker_samples,
        speaker_label,
        logger,
        recorder,
        transcriber,
        capture_store,
        &NoopInterruptMonitor,
    )
}

/// 連続録音と文字起こしを実行し、中断要求が来たら録音済みぶんだけを処理して終了します。
///
/// 公開APIとして中断監視を差し替えられる経路です。
/// 実処理は `run::run_capture_with_clock` に委譲し、ここでは
/// ユースケース入力に含めたい依存だけを束ねます。
#[allow(clippy::too_many_arguments)]
pub fn run_capture_with_interrupt_monitor<R, T, S>(
    config: &CaptureConfig,
    speaker_samples: &[KnownSpeakerSample],
    speaker_label: &SpeakerLabel,
    logger: &dyn Logger,
    recorder: &mut R,
    transcriber: &mut T,
    capture_store: &mut S,
    interrupt_monitor: &dyn InterruptMonitor,
) -> Result<CaptureRunResult, CaptureError>
where
    R: Recorder,
    T: Transcriber + ?Sized,
    S: CaptureStore,
{
    run::run_capture_with_clock(
        config,
        speaker_samples,
        speaker_label,
        logger,
        recorder,
        transcriber,
        capture_store,
        interrupt_monitor,
        run::current_unix_ms,
    )
}

pub(super) fn duration_to_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).expect("capture duration in millis must fit into u64")
}
