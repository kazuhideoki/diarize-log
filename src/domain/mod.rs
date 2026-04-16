mod audio;
mod capture;
mod merge;
mod speech;

pub use audio::RecordedAudio;
pub use capture::{CapturePolicy, CaptureRange};
pub use merge::{
    CaptureMerger, CapturedTranscript, MergeAuditEntry, MergeAuditOutcome, MergeBatch,
    MergeOverlapTextSource, MergeRejectReason, MergeSkipReason, MergeWindowSnapshot,
    MergedTranscriptSegment, TranscriptMergePolicy,
};
pub use speech::{DiarizedTranscript, KnownSpeakerSample, TranscriptSegment};
