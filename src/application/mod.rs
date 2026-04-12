pub mod ports;
mod usecase;

pub use ports::{
    AudioClipper, AudioClipperError, CaptureStore, CaptureStoreError, ChunkingStrategy, Recorder,
    RecorderError, RecordingSession, ResponseFormat, SpeakerStore, SpeakerStoreError, Transcriber,
    TranscriberError, TranscriptionRequest,
};
pub use usecase::{
    CaptureConfig, CaptureError, DebugOutputError, SpeakerCommand, SpeakerCommandResult,
    SpeakerUseCaseError, TRANSCRIPTION_MODEL, run_capture, run_speaker_command,
    write_debug_transcript,
};
