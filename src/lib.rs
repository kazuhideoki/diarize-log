pub mod adapters;
pub mod cli;
pub mod config;
pub mod ports;

pub use cli::{
    CliConfig, CliError, DebugOutputError, TRANSCRIPTION_MODEL, run_cli, write_debug_transcript,
};
pub use ports::{
    CaptureStore, CaptureStoreError, ChunkingStrategy, DiarizedTranscript, RecordedAudio, Recorder,
    RecorderError, ResponseFormat, Transcriber, TranscriberError, TranscriptSegment,
    TranscriptionRequest,
};

pub(crate) fn debug_log(debug_enabled: bool, message: &str) {
    if debug_enabled {
        eprintln!("[debug] {message}");
    }
}
