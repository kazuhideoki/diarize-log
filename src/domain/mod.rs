mod audio;
mod capture;
mod merge;
mod source;
mod speech;

pub use audio::RecordedAudio;
pub use capture::{
    CaptureBoundary, CaptureBoundaryReason, CapturePolicy, CaptureRange,
    SILENCE_REQUEST_ARM_AFTER_RATIO, SilenceRequestPolicy,
};
pub use merge::{
    CaptureMerger, CapturedTranscript, MergeAuditEntry, MergeAuditOutcome, MergeBatch,
    MergeOverlapRangeSnapshot, MergeOverlapTextSource, MergeRejectReason, MergeSkipReason,
    MergedTranscriptSegment, SourcedTranscriptSegment, TranscriptMergePolicy,
};
pub use source::TranscriptSource;
pub use speech::{
    DiarizationSegment, DiarizedTranscript, KnownSpeakerEmbedding, KnownSpeakerSample,
    SpeakerIdentification, SpeechTurn, SpeechTurnPolicy, TranscriptSegment, build_speech_turns,
    speaker_durations,
};
