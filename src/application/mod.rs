pub mod ports;
mod usecase;

pub use ports::{
    AudioClipper, AudioClipperError, CaptureSessionMetadata, CaptureStore, CaptureStoreError,
    ChunkingStrategy, InterruptMonitor, Logger, MixedCaptureSessionMetadata,
    MixedCaptureSourceOutcome, MixedCaptureSourceSettings, MixedCaptureSourceStatus,
    MixedCaptureStore, Recorder, RecorderError, RecordingSession, RecordingWaitOutcome,
    ResponseFormat, SpeakerStore, SpeakerStoreError, Transcriber, TranscriberError,
    TranscriptionLanguage, TranscriptionRequest,
};
pub use usecase::{
    CaptureConfig, CaptureError, CaptureRunResult, CaptureTranscriptionFailure, DebugOutputError,
    MixedCaptureError, MixedCaptureRunResult, MixedSourceRun, SourceMergedSegments, SpeakerCommand,
    SpeakerCommandResult, SpeakerLabel, SpeakerUseCaseError, TRANSCRIPTION_MODEL,
    finalize_mixed_capture, merge_source_segments, run_capture, run_capture_with_interrupt_monitor,
    run_mixed_capture, run_speaker_command, write_debug_transcript,
};
