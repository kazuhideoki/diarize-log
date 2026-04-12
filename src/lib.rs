pub mod adapters;
pub mod application;
pub mod cli;
pub mod config;
pub mod domain;

pub use application::{
    AudioClipper, AudioClipperError, CaptureConfig, CaptureError, CaptureStore, CaptureStoreError,
    ChunkingStrategy, DebugOutputError, Recorder, RecorderError, RecordingSession, ResponseFormat,
    SpeakerCommand, SpeakerCommandResult, SpeakerStore, SpeakerStoreError, SpeakerUseCaseError,
    TRANSCRIPTION_MODEL, Transcriber, TranscriberError, TranscriptionRequest, run_capture,
    run_speaker_command, write_debug_transcript,
};
pub use cli::{CliAction, CliArgumentError, parse_cli_args};
pub use domain::{
    CapturePolicy, CaptureRange, DiarizedTranscript, KnownSpeakerSample, RecordedAudio,
    TranscriptSegment,
};

pub(crate) fn debug_log(debug_enabled: bool, message: &str) {
    if debug_enabled {
        eprintln!("[debug] {message}");
    }
}
