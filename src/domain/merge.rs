use crate::domain::DiarizedTranscript;
use serde::Serialize;
use std::collections::HashSet;

/// capture 間の重複判定に使う閾値です。
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    pub capture_index: u64,
    pub capture_start_ms: u64,
    pub capture_end_ms: u64,
    pub segments: Vec<MergedTranscriptSegment>,
}

impl CapturedTranscript {
    /// capture と文字起こしから merge 用の絶対時刻 segment 列を作ります。
    pub fn from_relative(
        capture_index: u64,
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
            capture_index,
            capture_start_ms,
            capture_end_ms,
            segments,
        }
    }
}

/// merge 判定に使った overlap 窓の要約です。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergeWindowSnapshot {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub normalized_char_count: usize,
}

/// capture 間 merge の監査ログです。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MergeAuditEntry {
    pub capture_index: u64,
    pub previous_window: MergeWindowSnapshot,
    pub current_window: MergeWindowSnapshot,
    pub outcome: MergeAuditOutcome,
}

/// overlap 判定の結果です。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum MergeAuditOutcome {
    Accepted {
        overlap_chars: usize,
        alignment_ratio: f64,
        trigram_similarity: f64,
    },
    Rejected {
        overlap_chars: usize,
        alignment_ratio: f64,
        trigram_similarity: f64,
        reason: MergeRejectReason,
    },
    Skipped {
        reason: MergeSkipReason,
        previous_normalized_chars: usize,
        current_normalized_chars: usize,
        required_min_overlap_chars: usize,
    },
}

/// overlap が不採用だった理由です。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeRejectReason {
    AlignmentRatioBelowThreshold,
    TrigramSimilarityBelowThreshold,
    AlignmentAndTrigramSimilarityBelowThreshold,
}

/// overlap 判定自体をスキップした理由です。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeSkipReason {
    NoOverlapWindow,
    InsufficientNormalizedChars,
}

/// 1 capture 追加時に確定した merged segment 群です。
#[derive(Debug, Clone, PartialEq)]
pub struct MergeBatch {
    pub finalized_segments: Vec<MergedTranscriptSegment>,
    pub audit_entries: Vec<MergeAuditEntry>,
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
                audit_entries: Vec::new(),
            };
        }

        let finalized_count = self
            .pending_segments
            .iter()
            .take_while(|segment| segment.end_ms <= capture.capture_start_ms)
            .count();
        let finalized_segments = self.pending_segments[..finalized_count].to_vec();
        let mut overlap_tail = self.pending_segments[finalized_count..].to_vec();

        let mut audit_entries = Vec::new();
        if let Some(evaluation) = evaluate_overlap(&overlap_tail, &capture, &self.policy) {
            if let Some(trim_from) = evaluation.trim_from {
                overlap_tail = trim_segments_from_char(&overlap_tail, trim_from);
            }
            audit_entries.push(evaluation.audit_entry);
        }

        self.pending_segments = merge_sorted_segments(overlap_tail, capture.segments);

        MergeBatch {
            finalized_segments,
            audit_entries,
        }
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

#[derive(Debug, Clone, PartialEq)]
struct OverlapEvaluation {
    trim_from: Option<TrimFromChar>,
    audit_entry: MergeAuditEntry,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct OverlapCandidate {
    overlap_chars: usize,
    alignment_ratio: f64,
    trigram_similarity: f64,
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
    text: String,
    start_ms: u64,
    end_ms: u64,
}

impl NormalizedWindow {
    fn snapshot(&self) -> MergeWindowSnapshot {
        MergeWindowSnapshot {
            start_ms: self.start_ms,
            end_ms: self.end_ms,
            text: self.text.clone(),
            normalized_char_count: self.chars.len(),
        }
    }
}

fn evaluate_overlap(
    previous_tail: &[MergedTranscriptSegment],
    current_capture: &CapturedTranscript,
    policy: &TranscriptMergePolicy,
) -> Option<OverlapEvaluation> {
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
        return Some(OverlapEvaluation {
            trim_from: None,
            audit_entry: MergeAuditEntry {
                capture_index: current_capture.capture_index,
                previous_window: MergeWindowSnapshot {
                    start_ms: current_capture.capture_start_ms,
                    end_ms: current_capture.capture_start_ms,
                    text: String::new(),
                    normalized_char_count: 0,
                },
                current_window: MergeWindowSnapshot {
                    start_ms: current_capture.capture_start_ms,
                    end_ms: current_capture.capture_start_ms,
                    text: String::new(),
                    normalized_char_count: 0,
                },
                outcome: MergeAuditOutcome::Skipped {
                    reason: MergeSkipReason::NoOverlapWindow,
                    previous_normalized_chars: 0,
                    current_normalized_chars: 0,
                    required_min_overlap_chars: policy.min_overlap_chars,
                },
            },
        });
    }

    let overlap_window_start = current_capture.capture_start_ms;
    let previous_window =
        build_normalized_window(previous_tail, overlap_window_start, overlap_window_end);
    let current_window =
        build_normalized_window(&current_head, overlap_window_start, overlap_window_end);
    if previous_window.chars.len() < policy.min_overlap_chars
        || current_window.chars.len() < policy.min_overlap_chars
    {
        return Some(OverlapEvaluation {
            trim_from: None,
            audit_entry: MergeAuditEntry {
                capture_index: current_capture.capture_index,
                previous_window: previous_window.snapshot(),
                current_window: current_window.snapshot(),
                outcome: MergeAuditOutcome::Skipped {
                    reason: MergeSkipReason::InsufficientNormalizedChars,
                    previous_normalized_chars: previous_window.chars.len(),
                    current_normalized_chars: current_window.chars.len(),
                    required_min_overlap_chars: policy.min_overlap_chars,
                },
            },
        });
    }

    Some(
        match find_best_overlap_candidate(&previous_window, &current_window, policy) {
            Ok(candidate) => {
                let trim_position = previous_window.positions
                    [previous_window.chars.len() - candidate.overlap_chars];
                OverlapEvaluation {
                    trim_from: Some(TrimFromChar {
                        segment_index: trim_position.segment_index,
                        char_offset: trim_position.char_offset,
                    }),
                    audit_entry: MergeAuditEntry {
                        capture_index: current_capture.capture_index,
                        previous_window: previous_window.snapshot(),
                        current_window: current_window.snapshot(),
                        outcome: MergeAuditOutcome::Accepted {
                            overlap_chars: candidate.overlap_chars,
                            alignment_ratio: candidate.alignment_ratio,
                            trigram_similarity: candidate.trigram_similarity,
                        },
                    },
                }
            }
            Err((candidate, reason)) => OverlapEvaluation {
                trim_from: None,
                audit_entry: MergeAuditEntry {
                    capture_index: current_capture.capture_index,
                    previous_window: previous_window.snapshot(),
                    current_window: current_window.snapshot(),
                    outcome: MergeAuditOutcome::Rejected {
                        overlap_chars: candidate.overlap_chars,
                        alignment_ratio: candidate.alignment_ratio,
                        trigram_similarity: candidate.trigram_similarity,
                        reason,
                    },
                },
            },
        },
    )
}

fn build_normalized_window(
    segments: &[MergedTranscriptSegment],
    overlap_start_ms: u64,
    overlap_end_ms: u64,
) -> NormalizedWindow {
    let mut chars = Vec::new();
    let mut positions = Vec::new();
    let mut text = String::new();

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
            text.push(character);
            if let Some(normalized) = normalize_character(character) {
                chars.push(normalized);
                positions.push(NormalizedCharPosition {
                    segment_index,
                    char_offset,
                });
            }
        }
    }

    NormalizedWindow {
        chars,
        positions,
        text,
        start_ms: overlap_start_ms,
        end_ms: overlap_end_ms,
    }
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

fn find_best_overlap_candidate(
    previous: &NormalizedWindow,
    current: &NormalizedWindow,
    policy: &TranscriptMergePolicy,
) -> Result<OverlapCandidate, (OverlapCandidate, MergeRejectReason)> {
    let max_overlap = previous.chars.len().min(current.chars.len());
    let mut best_rejected = None;

    for overlap_len in (policy.min_overlap_chars..=max_overlap).rev() {
        let previous_slice = &previous.chars[previous.chars.len() - overlap_len..];
        let current_slice = &current.chars[..overlap_len];
        let alignment_ratio = alignment_ratio(previous_slice, current_slice);
        let trigram_similarity = trigram_similarity(previous_slice, current_slice);
        let candidate = OverlapCandidate {
            overlap_chars: overlap_len,
            alignment_ratio,
            trigram_similarity,
        };

        match reject_reason(candidate, policy) {
            None => return Ok(candidate),
            Some(reason) => {
                if should_replace_rejected_candidate(best_rejected, candidate, policy) {
                    best_rejected = Some((candidate, reason));
                }
            }
        }
    }

    Err(best_rejected.expect("rejected candidate must exist when overlap evaluation runs"))
}

fn reject_reason(
    candidate: OverlapCandidate,
    policy: &TranscriptMergePolicy,
) -> Option<MergeRejectReason> {
    let alignment_failed = candidate.alignment_ratio < policy.min_alignment_ratio;
    let trigram_failed = candidate.trigram_similarity < policy.min_trigram_similarity;

    match (alignment_failed, trigram_failed) {
        (false, false) => None,
        (true, false) => Some(MergeRejectReason::AlignmentRatioBelowThreshold),
        (false, true) => Some(MergeRejectReason::TrigramSimilarityBelowThreshold),
        (true, true) => Some(MergeRejectReason::AlignmentAndTrigramSimilarityBelowThreshold),
    }
}

fn should_replace_rejected_candidate(
    current: Option<(OverlapCandidate, MergeRejectReason)>,
    challenger: OverlapCandidate,
    policy: &TranscriptMergePolicy,
) -> bool {
    let Some((current_candidate, _)) = current else {
        return true;
    };

    let current_deficit = overlap_deficit(current_candidate, policy);
    let challenger_deficit = overlap_deficit(challenger, policy);
    if challenger_deficit < current_deficit {
        return true;
    }
    if challenger_deficit > current_deficit {
        return false;
    }
    if challenger.overlap_chars > current_candidate.overlap_chars {
        return true;
    }
    if challenger.overlap_chars < current_candidate.overlap_chars {
        return false;
    }
    if challenger.alignment_ratio > current_candidate.alignment_ratio {
        return true;
    }
    if challenger.alignment_ratio < current_candidate.alignment_ratio {
        return false;
    }

    challenger.trigram_similarity > current_candidate.trigram_similarity
}

fn overlap_deficit(candidate: OverlapCandidate, policy: &TranscriptMergePolicy) -> f64 {
    metric_deficit(candidate.alignment_ratio, policy.min_alignment_ratio)
        + metric_deficit(candidate.trigram_similarity, policy.min_trigram_similarity)
}

fn metric_deficit(actual: f64, required: f64) -> f64 {
    if actual >= required {
        0.0
    } else {
        required - actual
    }
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
        CaptureMerger, CapturedTranscript, MergeAuditOutcome, MergeSkipReason,
        MergedTranscriptSegment, TranscriptMergePolicy,
    };

    #[test]
    /// 重複する overlap を見つけたら後続 capture 側を残して 1 回に畳む。
    fn merges_overlapping_duplicate_tail_and_head() {
        let mut merger = CaptureMerger::new(TranscriptMergePolicy::recommended());
        let first = CapturedTranscript {
            capture_index: 1,
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
            capture_index: 2,
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
        let second_batch = merger.push_capture(second);

        assert!(second_batch.finalized_segments.is_empty());
        assert_eq!(second_batch.audit_entries.len(), 1);
        assert_eq!(second_batch.audit_entries[0].capture_index, 2);
        assert_eq!(
            second_batch.audit_entries[0].previous_window.text,
            "EFGHIJKLMNOP"
        );
        assert_eq!(
            second_batch.audit_entries[0].current_window.text,
            "EFGHIJKLMNOP"
        );
        assert_eq!(
            second_batch.audit_entries[0].outcome,
            MergeAuditOutcome::Accepted {
                overlap_chars: 12,
                alignment_ratio: 1.0,
                trigram_similarity: 1.0,
            }
        );
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
            capture_index: 1,
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
            capture_index: 2,
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
        let second_batch = merger.push_capture(second);

        assert!(second_batch.finalized_segments.is_empty());
        assert_eq!(second_batch.audit_entries.len(), 1);
        assert_eq!(
            second_batch.audit_entries[0].outcome,
            MergeAuditOutcome::Skipped {
                reason: MergeSkipReason::InsufficientNormalizedChars,
                previous_normalized_chars: 2,
                current_normalized_chars: 2,
                required_min_overlap_chars: 10,
            }
        );
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
            capture_index: 1,
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
            capture_index: 2,
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
