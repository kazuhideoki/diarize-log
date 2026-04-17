mod audio;
mod capture;
mod merge;
mod source;
mod speech;

pub use audio::RecordedAudio;
pub use capture::{CapturePolicy, CaptureRange};
pub use merge::{
    CaptureMerger, CapturedTranscript, MergeAuditEntry, MergeAuditOutcome, MergeBatch,
<<<<<<< HEAD
    MergeOverlapTextSource, MergeRejectReason, MergeSkipReason, MergeWindowSnapshot,
    MergedTranscriptSegment, SourcedTranscriptSegment, TranscriptMergePolicy,
=======
    MergeOverlapRangeSnapshot, MergeOverlapTextSource, MergeRejectReason, MergeSkipReason,
    MergedTranscriptSegment, TranscriptMergePolicy,
>>>>>>> main
};
pub use source::TranscriptSource;
pub use speech::{DiarizedTranscript, KnownSpeakerSample, TranscriptSegment};
