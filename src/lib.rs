pub mod adapters;
pub mod application;
pub mod cli;
pub mod config;
pub mod domain;

pub use application::{
    AudioClipper, AudioClipperError, CaptureConfig, CaptureError, CaptureRunResult,
    CaptureSessionMetadata, CaptureStore, CaptureStoreError, CaptureTranscriptionFailure,
    ChunkingStrategy, DebugOutputError, MixedCaptureError, MixedCaptureRunResult,
    MixedCaptureSessionMetadata, MixedCaptureSourceOutcome, MixedCaptureSourceSettings,
    MixedCaptureSourceStatus, MixedCaptureStore, MixedSourceRun, Recorder, RecorderError,
    RecordingSession, ResponseFormat, SourceMergedSegments, SpeakerCommand, SpeakerCommandResult,
    SpeakerLabel, SpeakerStore, SpeakerStoreError, SpeakerUseCaseError, TRANSCRIPTION_MODEL,
    Transcriber, TranscriberError, TranscriptionRequest, finalize_mixed_capture,
    merge_source_segments, run_capture, run_mixed_capture, run_speaker_command,
    write_debug_transcript,
};
pub use cli::{AudioSource, CliAction, CliArgumentError, parse_cli_args};
pub use domain::{
    CaptureMerger, CapturePolicy, CaptureRange, CapturedTranscript, DiarizedTranscript,
<<<<<<< HEAD
    KnownSpeakerSample, MergeAuditEntry, MergeAuditOutcome, MergeBatch, MergeRejectReason,
    MergeSkipReason, MergeWindowSnapshot, MergedTranscriptSegment, RecordedAudio,
    SourcedTranscriptSegment, TranscriptMergePolicy, TranscriptSegment, TranscriptSource,
=======
    KnownSpeakerSample, MergeAuditEntry, MergeAuditOutcome, MergeBatch, MergeOverlapRangeSnapshot,
    MergeRejectReason, MergeSkipReason, MergedTranscriptSegment, RecordedAudio,
    TranscriptMergePolicy, TranscriptSegment,
>>>>>>> main
};

pub(crate) fn debug_log(debug_enabled: bool, message: &str) {
    if debug_enabled {
        eprintln!("[debug] {message}");
    }
}
