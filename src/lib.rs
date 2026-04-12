pub mod adapters;
pub mod application;
pub mod cli;
pub mod config;
pub mod domain;
pub mod ports;

pub use application::{
    CaptureConfig, CaptureError, DebugOutputError, SpeakerCommand, SpeakerCommandResult,
    SpeakerUseCaseError, TRANSCRIPTION_MODEL, run_capture, run_speaker_command,
    write_debug_transcript,
};
pub use cli::{CliAction, CliArgumentError, parse_cli_args};
pub use domain::{
    CapturePolicy, CaptureRange, DiarizedTranscript, KnownSpeakerSample, RecordedAudio,
    TranscriptSegment,
};
pub use ports::{
    AudioClipper, AudioClipperError, CaptureStore, CaptureStoreError, ChunkingStrategy, Recorder,
    RecorderError, RecordingSession, ResponseFormat, SpeakerStore, SpeakerStoreError, Transcriber,
    TranscriberError, TranscriptionRequest,
};

pub(crate) fn debug_log(debug_enabled: bool, message: &str) {
    if debug_enabled {
        eprintln!("[debug] {message}");
    }
}
