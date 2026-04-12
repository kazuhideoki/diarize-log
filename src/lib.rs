pub mod adapters;
pub mod cli;
pub mod config;
pub mod ports;

pub use cli::{
    CliAction, CliArgumentError, CliConfig, CliError, DebugOutputError, SpeakerCliError,
    SpeakerCommand, SpeakerCommandResult, TRANSCRIPTION_MODEL, parse_cli_args, render_help,
    run_cli, run_speaker_command, write_debug_transcript,
};
pub use ports::{
    AudioClipper, AudioClipperError, CaptureStore, CaptureStoreError, ChunkingStrategy,
    DiarizedTranscript, RecordedAudio, Recorder, RecorderError, RecordingSession, ResponseFormat,
    SpeakerStore, SpeakerStoreError, Transcriber, TranscriberError, TranscriptSegment,
    TranscriptionRequest,
};

pub(crate) fn debug_log(debug_enabled: bool, message: &str) {
    if debug_enabled {
        eprintln!("[debug] {message}");
    }
}
