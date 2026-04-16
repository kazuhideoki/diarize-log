pub mod adapters;
pub mod application;
pub mod cli;
pub mod config;
pub mod domain;

pub use application::{
    AudioClipper, AudioClipperError, CaptureConfig, CaptureError, CaptureSessionMetadata,
    CaptureStore, CaptureStoreError, ChunkingStrategy, DebugOutputError, Recorder, RecorderError,
    RecordingSession, ResponseFormat, SpeakerCommand, SpeakerCommandResult, SpeakerStore,
    SpeakerStoreError, SpeakerUseCaseError, TRANSCRIPTION_MODEL, Transcriber, TranscriberError,
    TranscriptionRequest, run_capture, run_speaker_command, write_debug_transcript,
};
pub use cli::{AudioSource, CliAction, CliArgumentError, parse_cli_args};
pub use domain::{
    CaptureMerger, CapturePolicy, CaptureRange, CapturedTranscript, DiarizedTranscript,
    KnownSpeakerSample, MergeAuditEntry, MergeAuditOutcome, MergeBatch, MergeRejectReason,
    MergeSkipReason, MergeWindowSnapshot, MergedTranscriptSegment, RecordedAudio,
    TranscriptMergePolicy, TranscriptSegment,
};

pub(crate) fn debug_log(debug_enabled: bool, message: &str) {
    if debug_enabled {
        eprintln!("[debug] {message}");
    }
}
