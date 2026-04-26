use crate::domain::RecordedAudio;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

/// 既知話者として転写 API に添付する参照サンプルです。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownSpeakerSample {
    pub speaker_name: String,
    pub audio: RecordedAudio,
}

/// 既知話者サンプルから抽出済みの speaker embedding です。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnownSpeakerEmbedding {
    pub speaker_name: String,
    pub model: String,
    pub vector: Vec<f32>,
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

/// pyannote が返す匿名話者の発話区間です。
///
/// この段階ではまだ既知話者名を載せず、ASR 用の切り出しと
/// 話者同定の入力として扱います。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiarizationSegment {
    pub anonymous_speaker: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// ASR に送るために、匿名話者区間を結合した処理単位です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechTurn {
    pub anonymous_speaker: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// 話者同定後の既知話者名です。
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerIdentification {
    pub speaker_name: String,
    pub score: f32,
    pub margin: f32,
}

/// ASR 用 turn を作るための業務ルールです。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechTurnPolicy {
    pub merge_gap: Duration,
    pub padding: Duration,
    pub min_turn_duration: Duration,
    pub max_turn_duration: Duration,
}

impl SpeechTurnPolicy {
    /// separated pipeline v1 の推奨値です。
    pub fn recommended() -> Self {
        Self {
            merge_gap: Duration::from_secs(10),
            padding: Duration::from_millis(150),
            min_turn_duration: Duration::from_millis(300),
            max_turn_duration: Duration::from_secs(5 * 60),
        }
    }
}

/// diarization の匿名区間から ASR 用 turn を構築します。
pub fn build_speech_turns(
    segments: &[DiarizationSegment],
    capture_duration_ms: u64,
    policy: &SpeechTurnPolicy,
) -> Vec<SpeechTurn> {
    let mut sorted = segments
        .iter()
        .filter(|segment| segment.end_ms > segment.start_ms)
        .cloned()
        .collect::<Vec<_>>();
    sorted.sort_by_key(|segment| (segment.start_ms, segment.end_ms));

    let mut turns: Vec<SpeechTurn> = Vec::new();
    for segment in sorted {
        let segment_duration_ms = segment.end_ms.saturating_sub(segment.start_ms);
        if segment_duration_ms < duration_to_millis(policy.min_turn_duration) {
            if let Some(previous) = turns.last_mut()
                && previous.anonymous_speaker == segment.anonymous_speaker
            {
                previous.end_ms = previous.end_ms.max(segment.end_ms);
            }
            continue;
        }

        if let Some(previous) = turns.last_mut()
            && previous.anonymous_speaker == segment.anonymous_speaker
            && segment.start_ms.saturating_sub(previous.end_ms)
                <= duration_to_millis(policy.merge_gap)
        {
            previous.end_ms = previous.end_ms.max(segment.end_ms);
            continue;
        }

        turns.push(SpeechTurn {
            anonymous_speaker: segment.anonymous_speaker,
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
        });
    }

    turns
        .into_iter()
        .flat_map(|turn| split_and_pad_turn(turn, capture_duration_ms, policy))
        .collect()
}

/// 匿名話者ごとの総発話時間を返します。
pub fn speaker_durations(segments: &[DiarizationSegment]) -> BTreeMap<String, u64> {
    let mut durations = BTreeMap::new();
    for segment in segments {
        *durations
            .entry(segment.anonymous_speaker.clone())
            .or_insert(0) += segment.end_ms.saturating_sub(segment.start_ms);
    }
    durations
}

fn split_and_pad_turn(
    turn: SpeechTurn,
    capture_duration_ms: u64,
    policy: &SpeechTurnPolicy,
) -> Vec<SpeechTurn> {
    let max_duration_ms = duration_to_millis(policy.max_turn_duration);
    let padding_ms = duration_to_millis(policy.padding);
    let mut output = Vec::new();
    let mut start_ms = turn.start_ms;

    while start_ms < turn.end_ms {
        let end_ms = start_ms.saturating_add(max_duration_ms).min(turn.end_ms);
        output.push(SpeechTurn {
            anonymous_speaker: turn.anonymous_speaker.clone(),
            start_ms: start_ms.saturating_sub(padding_ms),
            end_ms: end_ms.saturating_add(padding_ms).min(capture_duration_ms),
        });
        start_ms = end_ms;
    }

    output
}

fn duration_to_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).expect("duration millis must fit into u64")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 同じ匿名話者の区間は 10 秒以内の gap をまたいで 1 つの ASR turn にまとめる。
    fn merges_same_speaker_segments_across_allowed_gap() {
        let policy = SpeechTurnPolicy::recommended();
        let turns = build_speech_turns(
            &[
                segment("SPEAKER_00", 1_000, 2_000),
                segment("SPEAKER_00", 11_500, 12_500),
            ],
            20_000,
            &policy,
        );

        assert_eq!(turns, vec![turn("SPEAKER_00", 850, 12_650)]);
    }

    #[test]
    /// 違う匿名話者の区間は gap が短くても結合しない。
    fn keeps_different_speakers_separate() {
        let policy = SpeechTurnPolicy::recommended();
        let turns = build_speech_turns(
            &[
                segment("SPEAKER_00", 1_000, 2_000),
                segment("SPEAKER_01", 2_100, 3_000),
            ],
            10_000,
            &policy,
        );

        assert_eq!(
            turns,
            vec![
                turn("SPEAKER_00", 850, 2_150),
                turn("SPEAKER_01", 1_950, 3_150)
            ]
        );
    }

    #[test]
    /// padding は capture の先頭と末尾を超えないように丸める。
    fn clamps_padding_to_capture_bounds() {
        let policy = SpeechTurnPolicy::recommended();
        let turns = build_speech_turns(&[segment("SPEAKER_00", 50, 950)], 1_000, &policy);

        assert_eq!(turns, vec![turn("SPEAKER_00", 0, 1_000)]);
    }

    #[test]
    /// 300ms 未満の短い区間は直前の同一話者 turn に吸収し、単独なら捨てる。
    fn absorbs_or_skips_too_short_segments() {
        let policy = SpeechTurnPolicy::recommended();
        let turns = build_speech_turns(
            &[
                segment("SPEAKER_00", 0, 200),
                segment("SPEAKER_00", 1_000, 1_600),
                segment("SPEAKER_00", 1_700, 1_950),
            ],
            5_000,
            &policy,
        );

        assert_eq!(turns, vec![turn("SPEAKER_00", 850, 2_100)]);
    }

    fn segment(speaker: &str, start_ms: u64, end_ms: u64) -> DiarizationSegment {
        DiarizationSegment {
            anonymous_speaker: speaker.to_string(),
            start_ms,
            end_ms,
        }
    }

    fn turn(speaker: &str, start_ms: u64, end_ms: u64) -> SpeechTurn {
        SpeechTurn {
            anonymous_speaker: speaker.to_string(),
            start_ms,
            end_ms,
        }
    }
}
