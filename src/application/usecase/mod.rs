mod capture;
mod speaker;

pub use capture::{
    CaptureConfig, CaptureError, CaptureRunResult, CaptureTranscriptionFailure, DebugOutputError,
    SpeakerLabel, TRANSCRIPTION_MODEL, merge_source_segments, run_capture, write_debug_transcript,
};
pub use speaker::{SpeakerCommand, SpeakerCommandResult, SpeakerUseCaseError, run_speaker_command};
