pub mod adapters;
pub mod application;
pub mod cli;
pub mod config;
pub mod domain;

pub use adapters::{LineLogger, LogSource};
pub use application::{
    AudioClipper, AudioClipperError, CaptureConfig, CaptureError, CaptureRunResult,
    CaptureSessionMetadata, CaptureStore, CaptureStoreError, CaptureTranscriptionFailure,
    ChunkingStrategy, DebugOutputError, InterruptMonitor, Logger, MixedCaptureError,
    MixedCaptureRunResult, MixedCaptureSessionMetadata, MixedCaptureSourceOutcome,
    MixedCaptureSourceSettings, MixedCaptureSourceStatus, MixedCaptureStore, MixedSourceRun,
    Recorder, RecorderError, RecordingSession, RecordingWaitOutcome, ResponseFormat,
    SourceMergedSegments, SpeakerCommand, SpeakerCommandResult, SpeakerLabel, SpeakerStore,
    SpeakerStoreError, SpeakerUseCaseError, TRANSCRIPTION_MODEL, Transcriber, TranscriberError,
    TranscriptionLanguage, TranscriptionRequest, finalize_mixed_capture, merge_source_segments,
    run_capture, run_capture_with_interrupt_monitor, run_mixed_capture, run_speaker_command,
    write_debug_transcript,
};
pub use cli::{AudioSource, CliAction, CliArgumentError, SpeakerCliCommand, parse_cli_args};
pub use domain::{
    CaptureMerger, CapturePolicy, CaptureRange, CapturedTranscript, DiarizedTranscript,
    KnownSpeakerSample, MergeAuditEntry, MergeAuditOutcome, MergeBatch, MergeOverlapRangeSnapshot,
    MergeRejectReason, MergeSkipReason, MergedTranscriptSegment, RecordedAudio,
    SourcedTranscriptSegment, TranscriptMergePolicy, TranscriptSegment, TranscriptSource,
};
