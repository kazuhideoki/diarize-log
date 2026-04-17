pub mod ports;
mod usecase;

pub use ports::{
    AudioClipper, AudioClipperError, CaptureSessionMetadata, CaptureStore, CaptureStoreError,
    ChunkingStrategy, Recorder, RecorderError, RecordingSession, ResponseFormat, SpeakerStore,
    SpeakerStoreError, Transcriber, TranscriberError, TranscriptionRequest,
};
pub use usecase::{
    CaptureConfig, CaptureError, CaptureRunResult, CaptureTranscriptionFailure, DebugOutputError,
    SpeakerCommand, SpeakerCommandResult, SpeakerLabel, SpeakerUseCaseError, TRANSCRIPTION_MODEL,
    merge_source_segments, run_capture, run_speaker_command, write_debug_transcript,
};
