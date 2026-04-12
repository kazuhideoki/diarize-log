use crate::domain::RecordedAudio;
use serde::Serialize;

/// 既知話者として転写 API に添付する参照サンプルです。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownSpeakerSample {
    pub speaker_name: String,
    pub audio: RecordedAudio,
}

/// 話者分離された文字起こし結果です。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiarizedTranscript {
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
}

/// 話者単位のセグメントです。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptSegment {
    pub speaker: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}
