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
pub use speech::{DiarizedTranscript, KnownSpeakerSample, TranscriptSegment};
