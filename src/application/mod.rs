pub mod ports;
mod usecase;

pub use ports::{
    AudioClipper, AudioClipperError, CaptureSessionMetadata, CaptureStore, CaptureStoreError,
    ChunkingStrategy, InterruptMonitor, MixedCaptureSessionMetadata, MixedCaptureSourceOutcome,
    MixedCaptureSourceSettings, MixedCaptureSourceStatus, MixedCaptureStore, Recorder,
<<<<<<< HEAD
    RecorderError, RecordingSession, ResponseFormat, SpeakerStore, SpeakerStoreError, Transcriber,
    TranscriberError, TranscriptionLanguage, TranscriptionRequest,
=======
    RecorderError, RecordingSession, RecordingWaitOutcome, ResponseFormat, SpeakerStore,
    SpeakerStoreError, Transcriber, TranscriberError, TranscriptionRequest,
>>>>>>> main
};
pub use usecase::{
    CaptureConfig, CaptureError, CaptureRunResult, CaptureTranscriptionFailure, DebugOutputError,
    MixedCaptureError, MixedCaptureRunResult, MixedSourceRun, SourceMergedSegments, SpeakerCommand,
    SpeakerCommandResult, SpeakerLabel, SpeakerUseCaseError, TRANSCRIPTION_MODEL,
    finalize_mixed_capture, merge_source_segments, run_capture, run_capture_with_interrupt_monitor,
    run_mixed_capture, run_speaker_command, write_debug_transcript,
};
