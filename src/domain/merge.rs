use crate::domain::DiarizedTranscript;
use serde::Serialize;
use std::collections::HashSet;

/// capture 間の重複判定に使う閾値です。
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptMergePolicy {
    /// 重複確定に必要な正規化後の最小一致文字数です。
    ///
    /// 推奨値の `10` は、短い相づちや定型句だけで誤って merge しにくくしつつ、
    /// 通常のフレーズ重複は拾いやすい下限として選んでいます。
    pub min_overlap_chars: usize,
    /// suffix/prefix 整列を重複とみなす最小一致率です。
    ///
    /// 推奨値の `0.80` は、capture 境界のずれや軽い表記揺れは許容しつつ、
    /// 別の発話を似ているだけで畳まないための保守的な基準です。
    pub min_alignment_ratio: f64,
    /// 編集距離判定を補強する文字 trigram 類似度の下限です。
    ///
    /// 推奨値の `0.55` は、編集距離だけでは拾いきれない局所的な並びの近さを確認しつつ、
    /// 語尾違いや軽い切れ方の差で落としすぎない補強値として置いています。
    pub min_trigram_similarity: f64,
}

impl TranscriptMergePolicy {
    /// 推奨の初期値を返します。
    pub fn recommended() -> Self {
        Self {
            min_overlap_chars: 10,
            min_alignment_ratio: 0.80,
            min_trigram_similarity: 0.55,
        }
    }
}

/// capture の相対時刻を絶対時刻へ変換した segment です。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergedTranscriptSegment {
    pub speaker: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// merge 対象の capture です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedTranscript {
    pub capture_start_ms: u64,
    pub capture_end_ms: u64,
    pub segments: Vec<MergedTranscriptSegment>,
}

impl CapturedTranscript {
    /// capture と文字起こしから merge 用の絶対時刻 segment 列を作ります。
    pub fn from_relative(
        capture_start_ms: u64,
        capture_end_ms: u64,
        transcript: &DiarizedTranscript,
    ) -> Self {
        let mut segments = transcript
            .segments
            .iter()
            .map(|segment| MergedTranscriptSegment {
                speaker: segment.speaker.clone(),
                start_ms: capture_start_ms + segment.start_ms,
                end_ms: capture_start_ms + segment.end_ms,
                text: segment.text.clone(),
            })
            .collect::<Vec<_>>();
        segments.sort_by_key(|segment| (segment.start_ms, segment.end_ms));

        Self {
            capture_start_ms,
            capture_end_ms,
            segments,
        }
    }
}

/// 1 capture 追加時に確定した merged segment 群です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeBatch {
    pub finalized_segments: Vec<MergedTranscriptSegment>,
}

/// capture を順次投入して merged segment を確定させます。
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureMerger {
    policy: TranscriptMergePolicy,
    pending_segments: Vec<MergedTranscriptSegment>,
}

impl CaptureMerger {
    /// merge state を初期化します。
    pub fn new(policy: TranscriptMergePolicy) -> Self {
        Self {
            policy,
            pending_segments: Vec::new(),
        }
    }

    /// 新しい capture を取り込み、今回確定した segment を返します。
    pub fn push_capture(&mut self, capture: CapturedTranscript) -> MergeBatch {
        if self.pending_segments.is_empty() {
            self.pending_segments = capture.segments;
            return MergeBatch {
                finalized_segments: Vec::new(),
            };
        }

        let finalized_count = self
            .pending_segments
            .iter()
            .take_while(|segment| segment.end_ms <= capture.capture_start_ms)
            .count();
        let finalized_segments = self.pending_segments[..finalized_count].to_vec();
        let mut overlap_tail = self.pending_segments[finalized_count..].to_vec();

        if let Some(trim_from) = find_overlap_trim_index(&overlap_tail, &capture, &self.policy) {
            overlap_tail = trim_segments_from_char(&overlap_tail, trim_from);
        }

        self.pending_segments = merge_sorted_segments(overlap_tail, capture.segments);

        MergeBatch { finalized_segments }
    }

    /// 保留中の segment を最後に確定させます。
    pub fn finish(self) -> Vec<MergedTranscriptSegment> {
        self.pending_segments
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrimFromChar {
    segment_index: usize,
    char_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NormalizedCharPosition {
    segment_index: usize,
    char_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedWindow {
    chars: Vec<char>,
    positions: Vec<NormalizedCharPosition>,
}

fn find_overlap_trim_index(
    previous_tail: &[MergedTranscriptSegment],
    current_capture: &CapturedTranscript,
    policy: &TranscriptMergePolicy,
) -> Option<TrimFromChar> {
    if previous_tail.is_empty() {
        return None;
    }

    let overlap_window_end = previous_tail
        .iter()
        .map(|segment| segment.end_ms)
        .max()
        .unwrap_or(current_capture.capture_start_ms)
        .min(current_capture.capture_end_ms);
    let current_head = current_capture
        .segments
        .iter()
        .filter(|segment| segment.start_ms < overlap_window_end)
        .cloned()
        .collect::<Vec<_>>();
    if current_head.is_empty() {
        return None;
    }

    let overlap_window_start = current_capture.capture_start_ms;
    let previous_window =
        build_normalized_window(previous_tail, overlap_window_start, overlap_window_end);
    let current_window =
        build_normalized_window(&current_head, overlap_window_start, overlap_window_end);
    if previous_window.chars.len() < policy.min_overlap_chars
        || current_window.chars.len() < policy.min_overlap_chars
    {
        return None;
    }

    let overlap_len = find_best_overlap_length(&previous_window, &current_window, policy)?;
    let trim_position = previous_window.positions[previous_window.chars.len() - overlap_len];

    Some(TrimFromChar {
        segment_index: trim_position.segment_index,
        char_offset: trim_position.char_offset,
    })
}

fn build_normalized_window(
    segments: &[MergedTranscriptSegment],
    overlap_start_ms: u64,
    overlap_end_ms: u64,
) -> NormalizedWindow {
    let mut chars = Vec::new();
    let mut positions = Vec::new();

    for (segment_index, segment) in segments.iter().enumerate() {
        let Some((start_char_offset, end_char_offset)) =
            overlap_char_bounds(segment, overlap_start_ms, overlap_end_ms)
        else {
            continue;
        };

        for (char_offset, character) in segment
            .text
            .chars()
            .enumerate()
            .skip(start_char_offset)
            .take(end_char_offset - start_char_offset)
        {
            if let Some(normalized) = normalize_character(character) {
                chars.push(normalized);
                positions.push(NormalizedCharPosition {
                    segment_index,
                    char_offset,
                });
            }
        }
    }

    NormalizedWindow { chars, positions }
}

/// long turn 全文をそのまま比較すると、共有時間外の固有テキストまで重複扱いして削ることがあります。
/// それを避けるため、この関数は共有時間に当たる部分だけを文字数比で text 上へ写像します。
fn overlap_char_bounds(
    segment: &MergedTranscriptSegment,
    overlap_start_ms: u64,
    overlap_end_ms: u64,
) -> Option<(usize, usize)> {
    let clipped_start_ms = segment.start_ms.max(overlap_start_ms);
    let clipped_end_ms = segment.end_ms.min(overlap_end_ms);
    if clipped_start_ms >= clipped_end_ms {
        return None;
    }

    let character_count = segment.text.chars().count();
    if character_count == 0 {
        return None;
    }

    let duration_ms = segment.end_ms.saturating_sub(segment.start_ms);
    if duration_ms == 0 {
        return Some((0, character_count));
    }

    let start_offset_ms = clipped_start_ms.saturating_sub(segment.start_ms);
    let end_offset_ms = clipped_end_ms.saturating_sub(segment.start_ms);
    let start_char_offset = proportional_char_floor(character_count, start_offset_ms, duration_ms);
    let end_char_offset =
        proportional_char_ceil(character_count, end_offset_ms, duration_ms).min(character_count);

    if start_char_offset >= end_char_offset {
        return None;
    }

    Some((start_char_offset, end_char_offset))
}

fn proportional_char_floor(character_count: usize, offset_ms: u64, duration_ms: u64) -> usize {
    let scaled = (character_count as u128 * offset_ms as u128) / duration_ms as u128;
    usize::try_from(scaled).expect("scaled floor char offset must fit into usize")
}

fn proportional_char_ceil(character_count: usize, offset_ms: u64, duration_ms: u64) -> usize {
    let numerator = character_count as u128 * offset_ms as u128;
    let scaled = numerator.div_ceil(duration_ms as u128);
    usize::try_from(scaled).expect("scaled ceil char offset must fit into usize")
}

fn normalize_character(character: char) -> Option<char> {
    if character.is_whitespace() || is_ignored_punctuation(character) {
        return None;
    }

    Some(character)
}

fn is_ignored_punctuation(character: char) -> bool {
    character.is_ascii_punctuation()
        || matches!(
            character,
            '、' | '。'
                | '，'
                | '．'
                | '・'
                | '！'
                | '？'
                | '「'
                | '」'
                | '『'
                | '』'
                | '（'
                | '）'
                | '【'
                | '】'
                | '［'
                | '］'
                | '〔'
                | '〕'
                | '〈'
                | '〉'
                | '《'
                | '》'
                | '…'
        )
}

fn find_best_overlap_length(
    previous: &NormalizedWindow,
    current: &NormalizedWindow,
    policy: &TranscriptMergePolicy,
) -> Option<usize> {
    let max_overlap = previous.chars.len().min(current.chars.len());

    for overlap_len in (policy.min_overlap_chars..=max_overlap).rev() {
        let previous_slice = &previous.chars[previous.chars.len() - overlap_len..];
        let current_slice = &current.chars[..overlap_len];
        let alignment_ratio = alignment_ratio(previous_slice, current_slice);
        if alignment_ratio < policy.min_alignment_ratio {
            continue;
        }

        let trigram_similarity = trigram_similarity(previous_slice, current_slice);
        if trigram_similarity < policy.min_trigram_similarity {
            continue;
        }

        return Some(overlap_len);
    }

    None
}

fn alignment_ratio(left: &[char], right: &[char]) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }

    let max_len = left.len().max(right.len());
    if max_len == 0 {
        return 1.0;
    }

    let distance = levenshtein_distance(left, right);
    1.0 - (distance as f64 / max_len as f64)
}

fn levenshtein_distance(left: &[char], right: &[char]) -> usize {
    if left.is_empty() {
        return right.len();
    }
    if right.is_empty() {
        return left.len();
    }

    let mut previous_row = (0..=right.len()).collect::<Vec<_>>();
    let mut current_row = vec![0; right.len() + 1];

    for (left_index, left_char) in left.iter().enumerate() {
        current_row[0] = left_index + 1;

        for (right_index, right_char) in right.iter().enumerate() {
            let substitution_cost = usize::from(left_char != right_char);
            current_row[right_index + 1] = (previous_row[right_index + 1] + 1)
                .min(current_row[right_index] + 1)
                .min(previous_row[right_index] + substitution_cost);
        }

        previous_row.clone_from(&current_row);
    }

    previous_row[right.len()]
}

fn trigram_similarity(left: &[char], right: &[char]) -> f64 {
    let left_trigrams = trigrams(left);
    let right_trigrams = trigrams(right);

    if left_trigrams.is_empty() && right_trigrams.is_empty() {
        return 1.0;
    }

    let intersection_count = left_trigrams.intersection(&right_trigrams).count();
    let union_count = left_trigrams.union(&right_trigrams).count();
    intersection_count as f64 / union_count as f64
}

fn trigrams(characters: &[char]) -> HashSet<[char; 3]> {
    if characters.len() < 3 {
        return HashSet::new();
    }

    characters
        .windows(3)
        .map(|window| [window[0], window[1], window[2]])
        .collect()
}

fn trim_segments_from_char(
    segments: &[MergedTranscriptSegment],
    trim_from: TrimFromChar,
) -> Vec<MergedTranscriptSegment> {
    let mut trimmed = segments[..trim_from.segment_index].to_vec();
    if let Some(segment) =
        trim_segment_prefix(&segments[trim_from.segment_index], trim_from.char_offset)
    {
        trimmed.push(segment);
    }

    trimmed
}

fn trim_segment_prefix(
    segment: &MergedTranscriptSegment,
    trim_char_offset: usize,
) -> Option<MergedTranscriptSegment> {
    if trim_char_offset == 0 {
        return None;
    }

    let characters = segment.text.chars().collect::<Vec<_>>();
    if trim_char_offset >= characters.len() {
        return Some(segment.clone());
    }

    let kept_text = characters[..trim_char_offset].iter().collect::<String>();
    if kept_text.is_empty() {
        return None;
    }

    let original_char_count = characters.len();
    let kept_char_count = trim_char_offset;
    let original_duration = segment.end_ms.saturating_sub(segment.start_ms);
    let scaled_duration = ((original_duration as u128 * kept_char_count as u128)
        + (original_char_count as u128 / 2))
        / original_char_count as u128;
    let scaled_duration =
        u64::try_from(scaled_duration).expect("scaled segment duration must fit into u64");
    let mut trimmed_end_ms = segment.start_ms.saturating_add(scaled_duration);
    if kept_char_count > 0 && trimmed_end_ms <= segment.start_ms {
        trimmed_end_ms = (segment.start_ms + 1).min(segment.end_ms);
    }

    Some(MergedTranscriptSegment {
        speaker: segment.speaker.clone(),
        start_ms: segment.start_ms,
        end_ms: trimmed_end_ms.min(segment.end_ms),
        text: kept_text,
    })
}

fn merge_sorted_segments(
    left: Vec<MergedTranscriptSegment>,
    right: Vec<MergedTranscriptSegment>,
) -> Vec<MergedTranscriptSegment> {
    let mut merged = Vec::with_capacity(left.len() + right.len());
    let mut left_iter = left.into_iter().peekable();
    let mut right_iter = right.into_iter().peekable();

    loop {
        match (left_iter.peek(), right_iter.peek()) {
            (Some(left_segment), Some(right_segment)) => {
                if (left_segment.start_ms, left_segment.end_ms)
                    <= (right_segment.start_ms, right_segment.end_ms)
                {
                    merged.push(
                        left_iter
                            .next()
                            .expect("left iterator must yield after peek"),
                    );
                } else {
                    merged.push(
                        right_iter
                            .next()
                            .expect("right iterator must yield after peek"),
                    );
                }
            }
            (Some(_), None) => {
                merged.extend(left_iter);
                break;
            }
            (None, Some(_)) => {
                merged.extend(right_iter);
                break;
            }
            (None, None) => break,
        }
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::{
        CaptureMerger, CapturedTranscript, MergedTranscriptSegment, TranscriptMergePolicy,
    };

    #[test]
    /// 重複する overlap を見つけたら後続 capture 側を残して 1 回に畳む。
    fn merges_overlapping_duplicate_tail_and_head() {
        let mut merger = CaptureMerger::new(TranscriptMergePolicy::recommended());
        let first = CapturedTranscript {
            capture_start_ms: 0,
            capture_end_ms: 18_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 10_000,
                end_ms: 18_000,
                text: "ABCDEFGHIJKLMNOP".to_string(),
            }],
        };
        let second = CapturedTranscript {
            capture_start_ms: 12_000,
            capture_end_ms: 27_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 12_000,
                end_ms: 20_000,
                text: "EFGHIJKLMNOPQRST".to_string(),
            }],
        };

        assert!(merger.push_capture(first).finalized_segments.is_empty());
        assert!(merger.push_capture(second).finalized_segments.is_empty());
        assert_eq!(
            merger.finish(),
            vec![
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 10_000,
                    end_ms: 12_000,
                    text: "ABCD".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 12_000,
                    end_ms: 20_000,
                    text: "EFGHIJKLMNOPQRST".to_string(),
                },
            ]
        );
    }

    #[test]
    /// 一致文字数が足りない短い相づちは重複扱いせず両方を残す。
    fn keeps_short_acknowledgements_separate() {
        let mut merger = CaptureMerger::new(TranscriptMergePolicy::recommended());
        let first = CapturedTranscript {
            capture_start_ms: 0,
            capture_end_ms: 15_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 13_000,
                end_ms: 14_000,
                text: "はい".to_string(),
            }],
        };
        let second = CapturedTranscript {
            capture_start_ms: 12_000,
            capture_end_ms: 27_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_1".to_string(),
                start_ms: 12_200,
                end_ms: 13_000,
                text: "はい".to_string(),
            }],
        };

        assert!(merger.push_capture(first).finalized_segments.is_empty());
        assert!(merger.push_capture(second).finalized_segments.is_empty());
        assert_eq!(
            merger.finish(),
            vec![
                MergedTranscriptSegment {
                    speaker: "spk_1".to_string(),
                    start_ms: 12_200,
                    end_ms: 13_000,
                    text: "はい".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 13_000,
                    end_ms: 14_000,
                    text: "はい".to_string(),
                },
            ]
        );
    }

    #[test]
    /// overlap 開始をまたぐ長い segment は共有時間窓内の部分だけを重複判定に使う。
    fn keeps_unique_prefix_when_previous_segment_straddles_overlap_start() {
        let mut merger = CaptureMerger::new(TranscriptMergePolicy::recommended());
        let first = CapturedTranscript {
            capture_start_ms: 0,
            capture_end_ms: 15_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 0,
                end_ms: 15_000,
                text: "ABCDEFGHIJKL".to_string(),
            }],
        };
        let second = CapturedTranscript {
            capture_start_ms: 12_000,
            capture_end_ms: 27_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 12_000,
                end_ms: 20_000,
                text: "ABCDEFGHIJKLMN".to_string(),
            }],
        };

        assert!(merger.push_capture(first).finalized_segments.is_empty());
        assert!(merger.push_capture(second).finalized_segments.is_empty());
        assert_eq!(
            merger.finish(),
            vec![
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 0,
                    end_ms: 15_000,
                    text: "ABCDEFGHIJKL".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 12_000,
                    end_ms: 20_000,
                    text: "ABCDEFGHIJKLMN".to_string(),
                },
            ]
        );
    }
}
