mod capture;
mod mixed;
mod speaker;

pub use capture::{
    CaptureConfig, CaptureError, CaptureRunResult, CaptureTranscriptionFailure, DebugOutputError,
    SpeakerLabel, TRANSCRIPTION_MODEL, run_capture, run_capture_with_interrupt_monitor,
    write_debug_transcript,
};
pub use mixed::{
    MixedCaptureError, MixedCaptureRunResult, MixedSourceRun, SourceMergedSegments,
    finalize_mixed_capture, merge_source_segments, run_mixed_capture,
};
pub use speaker::{SpeakerCommand, SpeakerCommandResult, SpeakerUseCaseError, run_speaker_command};
