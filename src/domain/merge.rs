//! このファイルは、時間が少し重なりながら届く `capture`(一定時間ごとに切り出した文字起こしのまとまり) を、
//! 同じ内容を二重に残さず、1 本の流れとしてつなぎ直すための処理をまとめています。
//!
//! 流れは次の順です。
//! 1. 新しい `capture` を受け取ったら、中の `segment`(話者・時刻・本文を持つ発話の区切り) を
//!    絶対時刻つきの形へ直します。
//! 2. すでに持っている前回ぶんの末尾と、今回の先頭のうち、時間が重なっている範囲だけを取り出します。
//! 3. 取り出した文字列は、空白や一部の記号を外した形で比べます。こうすることで、軽い表記ゆれに引きずられにくくします。
//! 4. そのうえで `overlap`(前後の `capture` で同じ内容として重なっている部分) があるかを調べ、
//!    十分に近いと判断できたときだけ、前回ぶんの末尾と今回の先頭を削ってつなぎ直します。
//! 5. もう新しい `capture` と重ならない古い `segment` は、その時点で確定済みとして返します。
//! 6. 最後に残った未確定ぶんは、`finish` でまとめて返します。

use crate::domain::{DiarizedTranscript, TranscriptSource};
use serde::Serialize;
use std::collections::HashSet;

/// current 先頭の取り違えは capture 境界で起きやすいため、小さな trim を試して補正します。
const MAX_CURRENT_PREFIX_TRIM_CHARS: usize = 10;
const TRIM_ALIGNMENT_RATIO_STEP: f64 = 0.01;
const TRIM_TRIGRAM_SIMILARITY_STEP: f64 = 0.01;
const MAX_ALIGNMENT_RATIO_WITH_PREFIX_TRIM: f64 = 0.90;
const MAX_TRIGRAM_SIMILARITY_WITH_PREFIX_TRIM: f64 = 0.65;

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

/// source 情報つきの最終統合 segment です。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourcedTranscriptSegment {
    pub source: TranscriptSource,
    pub speaker: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

impl SourcedTranscriptSegment {
    /// source と merge 済み segment から最終統合 segment を作ります。
    pub fn from_merged(source: TranscriptSource, segment: &MergedTranscriptSegment) -> Self {
        Self {
            source,
            speaker: segment.speaker.clone(),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: segment.text.clone(),
        }
    }

    /// source の絶対開始時刻を加味して最終統合 segment を作ります。
    pub fn from_merged_with_offset(
        source: TranscriptSource,
        started_at_unix_ms: u64,
        segment: &MergedTranscriptSegment,
    ) -> Self {
        Self {
            source,
            speaker: segment.speaker.clone(),
            start_ms: started_at_unix_ms + segment.start_ms,
            end_ms: started_at_unix_ms + segment.end_ms,
            text: segment.text.clone(),
        }
    }
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

/// merge 判定に使った overlap range の要約です。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergeOverlapRangeSnapshot {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub normalized_char_count: usize,
}

/// capture 間 merge の監査ログです。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MergeAuditEntry {
    pub capture_index: u64,
    pub previous_overlap_range: MergeOverlapRangeSnapshot,
    pub current_overlap_range: MergeOverlapRangeSnapshot,
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
        current_prefix_trim_chars: usize,
        overlap_text_source: MergeOverlapTextSource,
    },
    Rejected {
        overlap_chars: usize,
        alignment_ratio: f64,
        trigram_similarity: f64,
        current_prefix_trim_chars: usize,
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
    NoOverlapRange,
    InsufficientNormalizedChars,
}

/// accepted 時に overlap 本文をどちらの overlap range から採ったかを表します。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeOverlapTextSource {
    PreviousOverlapRange,
    CurrentOverlapRange,
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
        let mut current_segments = capture.segments;

        let mut audit_entries = Vec::new();
        if let Some(evaluation) = evaluate_overlap(
            &overlap_tail,
            capture.capture_index,
            capture.capture_start_ms,
            capture.capture_end_ms,
            &current_segments,
            &self.policy,
        ) {
            if let Some(splice_plan) = evaluation.splice_plan {
                overlap_tail = splice_plan.previous_segments;
                current_segments = splice_plan.current_segments;
            }
            audit_entries.push(evaluation.audit_entry);
        }

        self.pending_segments = merge_sorted_segments(overlap_tail, current_segments);

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
    splice_plan: Option<OverlapSplicePlan>,
    audit_entry: MergeAuditEntry,
}

#[derive(Debug, Clone, PartialEq)]
struct OverlapSplicePlan {
    previous_segments: Vec<MergedTranscriptSegment>,
    current_segments: Vec<MergedTranscriptSegment>,
}

#[derive(Debug, Clone, Copy)]
struct OverlapSpliceContext<'a> {
    previous_tail: &'a [MergedTranscriptSegment],
    current_segments: &'a [MergedTranscriptSegment],
    current_head: &'a [MergedTranscriptSegment],
    previous_overlap_range: &'a NormalizedOverlapRange,
    current_overlap_range: &'a NormalizedOverlapRange,
    overlap_range_end: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct OverlapCandidate {
    overlap_chars: usize,
    alignment_ratio: f64,
    trigram_similarity: f64,
    current_prefix_trim_chars: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NormalizedCharPosition {
    segment_index: usize,
    char_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedOverlapRange {
    chars: Vec<char>,
    positions: Vec<NormalizedCharPosition>,
    text_chars: Vec<char>,
    normalized_text_indexes: Vec<usize>,
    text: String,
    start_ms: u64,
    end_ms: u64,
}

impl NormalizedOverlapRange {
    fn snapshot(&self) -> MergeOverlapRangeSnapshot {
        MergeOverlapRangeSnapshot {
            start_ms: self.start_ms,
            end_ms: self.end_ms,
            text: self.text.clone(),
            normalized_char_count: self.chars.len(),
        }
    }

    fn raw_text_from_normalized_index_to_range_end(&self, start_normalized_index: usize) -> String {
        let start_text_index = self.normalized_text_indexes[start_normalized_index];
        self.text_chars[start_text_index..].iter().collect()
    }

    fn raw_text_from_normalized_range(
        &self,
        start_normalized_index: usize,
        normalized_len: usize,
    ) -> String {
        let start_text_index = self.normalized_text_indexes[start_normalized_index];
        let end_text_index =
            self.text_index_after_normalized_range(start_normalized_index, normalized_len);
        self.text_chars[start_text_index..end_text_index]
            .iter()
            .collect()
    }

    fn raw_text_after_normalized_range(
        &self,
        start_normalized_index: usize,
        normalized_len: usize,
    ) -> String {
        let end_text_index =
            self.text_index_after_normalized_range(start_normalized_index, normalized_len);
        self.text_chars[end_text_index..].iter().collect()
    }

    fn text_index_after_normalized_range(
        &self,
        start_normalized_index: usize,
        normalized_len: usize,
    ) -> usize {
        let last_normalized_index = start_normalized_index + normalized_len - 1;
        let mut end_text_index = self.normalized_text_indexes[last_normalized_index] + 1;
        while end_text_index < self.text_chars.len()
            && normalize_character(self.text_chars[end_text_index]).is_none()
        {
            end_text_index += 1;
        }

        end_text_index
    }
}

fn evaluate_overlap(
    previous_tail: &[MergedTranscriptSegment],
    capture_index: u64,
    capture_start_ms: u64,
    capture_end_ms: u64,
    current_segments: &[MergedTranscriptSegment],
    policy: &TranscriptMergePolicy,
) -> Option<OverlapEvaluation> {
    if previous_tail.is_empty() {
        return None;
    }

    let overlap_range_end = previous_tail
        .iter()
        .map(|segment| segment.end_ms)
        .max()
        .unwrap_or(capture_start_ms)
        .min(capture_end_ms);
    let current_head = current_segments
        .iter()
        .filter(|segment| segment.start_ms < overlap_range_end)
        .cloned()
        .collect::<Vec<_>>();
    if current_head.is_empty() {
        return Some(OverlapEvaluation {
            splice_plan: None,
            audit_entry: MergeAuditEntry {
                capture_index,
                previous_overlap_range: MergeOverlapRangeSnapshot {
                    start_ms: capture_start_ms,
                    end_ms: capture_start_ms,
                    text: String::new(),
                    normalized_char_count: 0,
                },
                current_overlap_range: MergeOverlapRangeSnapshot {
                    start_ms: capture_start_ms,
                    end_ms: capture_start_ms,
                    text: String::new(),
                    normalized_char_count: 0,
                },
                outcome: MergeAuditOutcome::Skipped {
                    reason: MergeSkipReason::NoOverlapRange,
                    previous_normalized_chars: 0,
                    current_normalized_chars: 0,
                    required_min_overlap_chars: policy.min_overlap_chars,
                },
            },
        });
    }

    let overlap_range_start = capture_start_ms;
    let previous_overlap_range =
        build_normalized_overlap_range(previous_tail, overlap_range_start, overlap_range_end);
    let current_overlap_range =
        build_normalized_overlap_range(&current_head, overlap_range_start, overlap_range_end);
    if previous_overlap_range.chars.len() < policy.min_overlap_chars
        || current_overlap_range.chars.len() < policy.min_overlap_chars
    {
        return Some(OverlapEvaluation {
            splice_plan: None,
            audit_entry: MergeAuditEntry {
                capture_index,
                previous_overlap_range: previous_overlap_range.snapshot(),
                current_overlap_range: current_overlap_range.snapshot(),
                outcome: MergeAuditOutcome::Skipped {
                    reason: MergeSkipReason::InsufficientNormalizedChars,
                    previous_normalized_chars: previous_overlap_range.chars.len(),
                    current_normalized_chars: current_overlap_range.chars.len(),
                    required_min_overlap_chars: policy.min_overlap_chars,
                },
            },
        });
    }

    Some(
        match find_best_overlap_candidate(&previous_overlap_range, &current_overlap_range, policy) {
            Ok(candidate) => {
                let overlap_text_source = choose_overlap_text_source(
                    &previous_overlap_range,
                    &current_overlap_range,
                    candidate,
                    current_head.len(),
                );
                OverlapEvaluation {
                    splice_plan: Some(build_overlap_splice_plan(
                        OverlapSpliceContext {
                            previous_tail,
                            current_segments,
                            current_head: &current_head,
                            previous_overlap_range: &previous_overlap_range,
                            current_overlap_range: &current_overlap_range,
                            overlap_range_end,
                        },
                        candidate,
                        overlap_text_source,
                    )),
                    audit_entry: MergeAuditEntry {
                        capture_index,
                        previous_overlap_range: previous_overlap_range.snapshot(),
                        current_overlap_range: current_overlap_range.snapshot(),
                        outcome: MergeAuditOutcome::Accepted {
                            overlap_chars: candidate.overlap_chars,
                            alignment_ratio: candidate.alignment_ratio,
                            trigram_similarity: candidate.trigram_similarity,
                            current_prefix_trim_chars: candidate.current_prefix_trim_chars,
                            overlap_text_source,
                        },
                    },
                }
            }
            Err((candidate, reason)) => OverlapEvaluation {
                splice_plan: None,
                audit_entry: MergeAuditEntry {
                    capture_index,
                    previous_overlap_range: previous_overlap_range.snapshot(),
                    current_overlap_range: current_overlap_range.snapshot(),
                    outcome: MergeAuditOutcome::Rejected {
                        overlap_chars: candidate.overlap_chars,
                        alignment_ratio: candidate.alignment_ratio,
                        trigram_similarity: candidate.trigram_similarity,
                        current_prefix_trim_chars: candidate.current_prefix_trim_chars,
                        reason,
                    },
                },
            },
        },
    )
}

fn build_normalized_overlap_range(
    segments: &[MergedTranscriptSegment],
    overlap_start_ms: u64,
    overlap_end_ms: u64,
) -> NormalizedOverlapRange {
    let mut chars = Vec::new();
    let mut positions = Vec::new();
    let mut text_chars = Vec::new();
    let mut normalized_text_indexes = Vec::new();
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
            text_chars.push(character);
            if let Some(normalized) = normalize_character(character) {
                chars.push(normalized);
                positions.push(NormalizedCharPosition {
                    segment_index,
                    char_offset,
                });
                normalized_text_indexes.push(text_chars.len() - 1);
            }
        }
    }

    NormalizedOverlapRange {
        chars,
        positions,
        text_chars,
        normalized_text_indexes,
        text,
        start_ms: overlap_start_ms,
        end_ms: overlap_end_ms,
    }
}

/// capture 境界では current 先頭に取り違えノイズが乗りやすい一方、
/// overlap 本文そのものは previous の方が句読点まで含めて自然なことがあります。
/// そのため exact match が取れた候補だけは raw text の情報量を見て採用元を選びます。
fn choose_overlap_text_source(
    previous_overlap_range: &NormalizedOverlapRange,
    current_overlap_range: &NormalizedOverlapRange,
    candidate: OverlapCandidate,
    current_head_segment_count: usize,
) -> MergeOverlapTextSource {
    // overlap 本文が複数 current segment に跨る場合は、current 側の speaker/time 境界を
    // そのまま残す必要があります。previous 側の raw text を跨って差し込むと 1 本化が必要になり、
    // diarization の意味を壊すため、このケースでは current 本文を優先します。
    if current_head_segment_count > 1 {
        return MergeOverlapTextSource::CurrentOverlapRange;
    }

    if candidate.alignment_ratio < 1.0 {
        return MergeOverlapTextSource::CurrentOverlapRange;
    }

    let previous_start = previous_overlap_range.chars.len() - candidate.overlap_chars;
    let current_start = candidate.current_prefix_trim_chars;
    let previous_overlap_text =
        previous_overlap_range.raw_text_from_normalized_index_to_range_end(previous_start);
    let current_overlap_text = current_overlap_range
        .raw_text_from_normalized_range(current_start, candidate.overlap_chars);

    let previous_ignored_count = ignored_character_count(&previous_overlap_text);
    let current_ignored_count = ignored_character_count(&current_overlap_text);
    if current_ignored_count > previous_ignored_count {
        return MergeOverlapTextSource::CurrentOverlapRange;
    }
    if current_ignored_count < previous_ignored_count {
        return MergeOverlapTextSource::PreviousOverlapRange;
    }

    if current_overlap_text.chars().count() >= previous_overlap_text.chars().count() {
        MergeOverlapTextSource::CurrentOverlapRange
    } else {
        MergeOverlapTextSource::PreviousOverlapRange
    }
}

fn ignored_character_count(text: &str) -> usize {
    text.chars()
        .filter(|character| normalize_character(*character).is_none())
        .count()
}

fn build_overlap_splice_plan(
    context: OverlapSpliceContext<'_>,
    candidate: OverlapCandidate,
    overlap_text_source: MergeOverlapTextSource,
) -> OverlapSplicePlan {
    let previous_overlap_start =
        context.previous_overlap_range.chars.len() - candidate.overlap_chars;
    let current_overlap_start = candidate.current_prefix_trim_chars;
    let previous_position = context.previous_overlap_range.positions[previous_overlap_start];
    let previous_trim_from = TrimFromChar {
        segment_index: previous_position.segment_index,
        char_offset: previous_position.char_offset,
    };
    let current_position = context.current_overlap_range.positions[current_overlap_start];
    let current_trim_from = TrimFromChar {
        segment_index: current_position.segment_index,
        char_offset: current_position.char_offset,
    };
    let boundary_start_ms = char_boundary_ms(
        &context.previous_tail[previous_trim_from.segment_index],
        previous_trim_from.char_offset,
    );
    let boundary_speaker = context.current_head
        [context.current_overlap_range.positions[current_overlap_start].segment_index]
        .speaker
        .clone();
    let previous_overlap_text = context
        .previous_overlap_range
        .raw_text_from_normalized_index_to_range_end(previous_overlap_start);
    let current_overlap_text = context
        .current_overlap_range
        .raw_text_from_normalized_range(current_overlap_start, candidate.overlap_chars);

    if context.current_head.len() > 1 {
        return OverlapSplicePlan {
            previous_segments: keep_segments_before_char(context.previous_tail, previous_trim_from),
            current_segments: keep_segments_after_char(context.current_segments, current_trim_from),
        };
    }

    let current_suffix_text = context
        .current_overlap_range
        .raw_text_after_normalized_range(current_overlap_start, candidate.overlap_chars);
    let overlap_text = match overlap_text_source {
        MergeOverlapTextSource::PreviousOverlapRange => previous_overlap_text,
        MergeOverlapTextSource::CurrentOverlapRange => current_overlap_text,
    };
    // 共有時間窓は 1 本の synthetic segment に置き換え、
    // previous/current のどちらを採っても後続 segment と素直に連結できるようにする。
    let boundary_segment = MergedTranscriptSegment {
        speaker: boundary_speaker,
        start_ms: boundary_start_ms,
        end_ms: context.overlap_range_end,
        text: format!("{overlap_text}{current_suffix_text}"),
    };

    let mut merged_current_segments = vec![boundary_segment];
    for segment in keep_segments_after_ms(context.current_segments, context.overlap_range_end) {
        push_or_merge_adjacent_segment(&mut merged_current_segments, segment);
    }

    OverlapSplicePlan {
        previous_segments: keep_segments_before_char(context.previous_tail, previous_trim_from),
        current_segments: merged_current_segments,
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

fn char_boundary_ms(segment: &MergedTranscriptSegment, char_offset: usize) -> u64 {
    let character_count = segment.text.chars().count();
    if character_count == 0 || char_offset == 0 {
        return segment.start_ms;
    }
    if char_offset >= character_count {
        return segment.end_ms;
    }

    let original_duration = segment.end_ms.saturating_sub(segment.start_ms);
    let scaled_duration = ((original_duration as u128 * char_offset as u128)
        + (character_count as u128 / 2))
        / character_count as u128;
    let scaled_duration =
        u64::try_from(scaled_duration).expect("scaled segment duration must fit into u64");

    segment
        .start_ms
        .saturating_add(scaled_duration)
        .min(segment.end_ms)
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
    previous: &NormalizedOverlapRange,
    current: &NormalizedOverlapRange,
    policy: &TranscriptMergePolicy,
) -> Result<OverlapCandidate, (OverlapCandidate, MergeRejectReason)> {
    let max_trim = MAX_CURRENT_PREFIX_TRIM_CHARS.min(current.chars.len());
    let mut best_accepted = None;
    let mut best_rejected = None;

    for current_prefix_trim_chars in 0..=max_trim {
        let max_overlap = previous.chars.len().min(
            current
                .chars
                .len()
                .saturating_sub(current_prefix_trim_chars),
        );
        if max_overlap < policy.min_overlap_chars {
            continue;
        }

        // trim を深くするほど誤マージの余地が増えるため、
        // まず overlap 長を最大化しつつ、その中でより exact な候補を優先する。
        for overlap_len in (policy.min_overlap_chars..=max_overlap).rev() {
            let previous_slice = &previous.chars[previous.chars.len() - overlap_len..];
            let current_slice =
                &current.chars[current_prefix_trim_chars..current_prefix_trim_chars + overlap_len];
            let alignment_ratio = alignment_ratio(previous_slice, current_slice);
            let trigram_similarity = trigram_similarity(previous_slice, current_slice);
            let candidate = OverlapCandidate {
                overlap_chars: overlap_len,
                alignment_ratio,
                trigram_similarity,
                current_prefix_trim_chars,
            };

            match reject_reason(candidate, policy) {
                None => {
                    if should_replace_accepted_candidate(best_accepted, candidate) {
                        best_accepted = Some(candidate);
                    }
                }
                Some(reason) => {
                    if should_replace_rejected_candidate(best_rejected, candidate, policy) {
                        best_rejected = Some((candidate, reason));
                    }
                }
            }
        }
    }

    if let Some(candidate) = best_accepted {
        return Ok(candidate);
    }

    Err(best_rejected.expect("rejected candidate must exist when overlap evaluation runs"))
}

fn reject_reason(
    candidate: OverlapCandidate,
    policy: &TranscriptMergePolicy,
) -> Option<MergeRejectReason> {
    let alignment_failed = candidate.alignment_ratio
        < required_alignment_ratio(policy, candidate.current_prefix_trim_chars);
    let trigram_failed = candidate.trigram_similarity
        < required_trigram_similarity(policy, candidate.current_prefix_trim_chars);

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
    if challenger.trigram_similarity > current_candidate.trigram_similarity {
        return true;
    }
    if challenger.trigram_similarity < current_candidate.trigram_similarity {
        return false;
    }

    challenger.current_prefix_trim_chars < current_candidate.current_prefix_trim_chars
}

fn should_replace_accepted_candidate(
    current: Option<OverlapCandidate>,
    challenger: OverlapCandidate,
) -> bool {
    let Some(current_candidate) = current else {
        return true;
    };

    // accepted 候補では、少し長い fuzzy overlap よりも exact overlap を優先します。
    // 長さだけで選ぶと固有本文を食い込ませた候補が勝ちやすく、境界で削ってはいけない文字まで
    // 落とすためです。exact 同士、または fuzzy 同士の比較にだけ従来の長さ優先を使います。
    let current_is_exact = is_exact_overlap_candidate(current_candidate);
    let challenger_is_exact = is_exact_overlap_candidate(challenger);
    if challenger_is_exact != current_is_exact {
        return challenger_is_exact;
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
    if challenger.trigram_similarity > current_candidate.trigram_similarity {
        return true;
    }
    if challenger.trigram_similarity < current_candidate.trigram_similarity {
        return false;
    }

    challenger.current_prefix_trim_chars < current_candidate.current_prefix_trim_chars
}

fn is_exact_overlap_candidate(candidate: OverlapCandidate) -> bool {
    candidate.alignment_ratio == 1.0 && candidate.trigram_similarity == 1.0
}

fn overlap_deficit(candidate: OverlapCandidate, policy: &TranscriptMergePolicy) -> f64 {
    metric_deficit(
        candidate.alignment_ratio,
        required_alignment_ratio(policy, candidate.current_prefix_trim_chars),
    ) + metric_deficit(
        candidate.trigram_similarity,
        required_trigram_similarity(policy, candidate.current_prefix_trim_chars),
    )
}

fn metric_deficit(actual: f64, required: f64) -> f64 {
    if actual >= required {
        0.0
    } else {
        required - actual
    }
}

fn required_alignment_ratio(
    policy: &TranscriptMergePolicy,
    current_prefix_trim_chars: usize,
) -> f64 {
    (policy.min_alignment_ratio + current_prefix_trim_chars as f64 * TRIM_ALIGNMENT_RATIO_STEP)
        .min(MAX_ALIGNMENT_RATIO_WITH_PREFIX_TRIM)
}

fn required_trigram_similarity(
    policy: &TranscriptMergePolicy,
    current_prefix_trim_chars: usize,
) -> f64 {
    (policy.min_trigram_similarity
        + current_prefix_trim_chars as f64 * TRIM_TRIGRAM_SIMILARITY_STEP)
        .min(MAX_TRIGRAM_SIMILARITY_WITH_PREFIX_TRIM)
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

fn keep_segments_before_char(
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

fn keep_segments_after_char(
    segments: &[MergedTranscriptSegment],
    keep_from: TrimFromChar,
) -> Vec<MergedTranscriptSegment> {
    let mut kept = Vec::new();
    if let Some(segment) =
        trim_segment_suffix_from_char(&segments[keep_from.segment_index], keep_from.char_offset)
    {
        kept.push(segment);
    }
    kept.extend_from_slice(&segments[keep_from.segment_index + 1..]);

    kept
}

fn keep_segments_after_ms(
    segments: &[MergedTranscriptSegment],
    keep_from_ms: u64,
) -> Vec<MergedTranscriptSegment> {
    let mut kept = Vec::new();

    for segment in segments {
        if segment.end_ms <= keep_from_ms {
            continue;
        }
        if segment.start_ms >= keep_from_ms {
            kept.push(segment.clone());
            continue;
        }
        if let Some(trimmed) = trim_segment_suffix_from_ms(segment, keep_from_ms) {
            kept.push(trimmed);
        }
    }

    kept
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

    let kept_char_count = trim_char_offset;
    let mut trimmed_end_ms = char_boundary_ms(segment, trim_char_offset);
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

fn trim_segment_suffix_from_char(
    segment: &MergedTranscriptSegment,
    keep_char_offset: usize,
) -> Option<MergedTranscriptSegment> {
    if keep_char_offset == 0 {
        return Some(segment.clone());
    }

    let characters = segment.text.chars().collect::<Vec<_>>();
    if keep_char_offset >= characters.len() {
        return None;
    }

    let kept_text = characters[keep_char_offset..].iter().collect::<String>();
    if kept_text.is_empty() {
        return None;
    }

    let mut trimmed_start_ms = char_boundary_ms(segment, keep_char_offset);
    if trimmed_start_ms >= segment.end_ms {
        trimmed_start_ms = segment.end_ms.saturating_sub(1).max(segment.start_ms);
    }

    Some(MergedTranscriptSegment {
        speaker: segment.speaker.clone(),
        start_ms: trimmed_start_ms.max(segment.start_ms),
        end_ms: segment.end_ms,
        text: kept_text,
    })
}

fn trim_segment_suffix_from_ms(
    segment: &MergedTranscriptSegment,
    keep_from_ms: u64,
) -> Option<MergedTranscriptSegment> {
    if keep_from_ms <= segment.start_ms {
        return Some(segment.clone());
    }
    if keep_from_ms >= segment.end_ms {
        return None;
    }

    let characters = segment.text.chars().collect::<Vec<_>>();
    if characters.is_empty() {
        return None;
    }

    let duration_ms = segment.end_ms.saturating_sub(segment.start_ms);
    let start_char_offset = if duration_ms == 0 {
        0
    } else {
        proportional_char_ceil(
            characters.len(),
            keep_from_ms.saturating_sub(segment.start_ms),
            duration_ms,
        )
        .min(characters.len())
    };
    if start_char_offset >= characters.len() {
        return None;
    }

    Some(MergedTranscriptSegment {
        speaker: segment.speaker.clone(),
        start_ms: keep_from_ms,
        end_ms: segment.end_ms,
        text: characters[start_char_offset..].iter().collect(),
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

fn push_or_merge_adjacent_segment(
    segments: &mut Vec<MergedTranscriptSegment>,
    segment: MergedTranscriptSegment,
) {
    if let Some(last) = segments.last_mut()
        && last.speaker == segment.speaker
        && last.end_ms == segment.start_ms
    {
        last.end_ms = segment.end_ms;
        last.text.push_str(&segment.text);
        return;
    }

    segments.push(segment);
}

#[cfg(test)]
mod tests {
    use super::{
        CaptureMerger, CapturedTranscript, MergeAuditOutcome, MergeOverlapTextSource,
        MergeSkipReason, MergedTranscriptSegment, OverlapCandidate, TranscriptMergePolicy,
        should_replace_accepted_candidate,
    };

    fn joined_text(segments: &[MergedTranscriptSegment]) -> String {
        segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<String>()
    }

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
            second_batch.audit_entries[0].previous_overlap_range.text,
            "EFGHIJKLMNOP"
        );
        assert_eq!(
            second_batch.audit_entries[0].current_overlap_range.text,
            "EFGHIJKLMNOP"
        );
        assert_eq!(
            second_batch.audit_entries[0].outcome,
            MergeAuditOutcome::Accepted {
                overlap_chars: 12,
                alignment_ratio: 1.0,
                trigram_similarity: 1.0,
                current_prefix_trim_chars: 0,
                overlap_text_source: MergeOverlapTextSource::CurrentOverlapRange,
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

    #[test]
    /// current 冒頭の余計な文字を飛ばせば前回の overlap を句読点つきでつなぎ直せる。
    fn merges_overlap_after_trimming_current_prefix_noise() {
        let mut merger = CaptureMerger::new(TranscriptMergePolicy::recommended());
        let first = CapturedTranscript {
            capture_index: 1,
            capture_start_ms: 0,
            capture_end_ms: 10_000,
            segments: vec![
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 0,
                    end_ms: 5_700,
                    text: "大敵規模で地球環境の保護、CO2削減などが叫ばれていますが、".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 6_050,
                    end_ms: 9_250,
                    text: "いずれもまだまだ進んでいないのが現状です。".to_string(),
                },
            ],
        };
        let second = CapturedTranscript {
            capture_index: 2,
            capture_start_ms: 5_000,
            capture_end_ms: 15_000,
            segments: vec![
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 5_000,
                    end_ms: 9_250,
                    text: "いていますがいずれもまだまだ進んでいないのが現状です".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 10_750,
                    end_ms: 14_800,
                    text: "当社ではFA機器の開発・製造を自".to_string(),
                },
            ],
        };

        assert!(merger.push_capture(first).finalized_segments.is_empty());
        assert!(merger.push_capture(second).finalized_segments.is_empty());

        assert_eq!(
            joined_text(&merger.finish()),
            "大敵規模で地球環境の保護、CO2削減などが叫ばれていますが、いずれもまだまだ進んでいないのが現状です。当社ではFA機器の開発・製造を自"
        );
    }

    #[test]
    /// current 冒頭のノイズを除けば重複境界の句読点を保ったまま文を継続できる。
    fn merges_overlap_with_current_punctuation_after_prefix_trim() {
        let mut merger = CaptureMerger::new(TranscriptMergePolicy::recommended());
        let first = CapturedTranscript {
            capture_index: 1,
            capture_start_ms: 10_000,
            capture_end_ms: 20_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 10_000,
                end_ms: 19_900,
                text: "外記事情報安全守護神田明神お守りはご自身で大切にお持ちくださいステッカー"
                    .to_string(),
            }],
        };
        let second = CapturedTranscript {
            capture_index: 2,
            capture_start_ms: 15_000,
            capture_end_ms: 25_000,
            segments: vec![
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 15_000,
                    end_ms: 18_250,
                    text: "妙人　大文字はご自身で大切にお持ちください。".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 19_250,
                    end_ms: 19_900,
                    text: "ステッカー".to_string(),
                },
            ],
        };

        assert!(merger.push_capture(first).finalized_segments.is_empty());
        assert!(merger.push_capture(second).finalized_segments.is_empty());

        assert_eq!(
            joined_text(&merger.finish()),
            "外記事情報安全守護神田明神お守りはご自身で大切にお持ちください。ステッカー"
        );
    }

    #[test]
    /// overlap 本文が exact match までそろう trim を選び、前段の正しい漢字列を維持する。
    fn merges_overlap_by_selecting_more_natural_boundary_candidate() {
        let mut merger = CaptureMerger::new(TranscriptMergePolicy::recommended());
        let first = CapturedTranscript {
            capture_index: 1,
            capture_start_ms: 10_000,
            capture_end_ms: 20_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 10_600,
                end_ms: 19_550,
                text:
                    "当社では、FA機器の開発・製造を自社独自の合理化技術により省力化・見える化する"
                        .to_string(),
            }],
        };
        let second = CapturedTranscript {
            capture_index: 2,
            capture_start_ms: 15_000,
            capture_end_ms: 25_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 15_000,
                end_ms: 19_750,
                text: "医者独自の合理化技術により省力化・見える化する".to_string(),
            }],
        };

        assert!(merger.push_capture(first).finalized_segments.is_empty());
        assert!(merger.push_capture(second).finalized_segments.is_empty());

        assert_eq!(
            joined_text(&merger.finish()),
            "当社では、FA機器の開発・製造を自社独自の合理化技術により省力化・見える化する"
        );
    }

    #[test]
    /// prefix trim を試しても共有本文の類似が弱い組み合わせは merge しない。
    fn keeps_rejected_overlap_when_similarity_stays_too_low_after_prefix_trim() {
        let mut merger = CaptureMerger::new(TranscriptMergePolicy::recommended());
        let first = CapturedTranscript {
            capture_index: 1,
            capture_start_ms: 5_000,
            capture_end_ms: 15_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 10_800,
                end_ms: 14_850,
                text: "IT情報安全守護寒だ".to_string(),
            }],
        };
        let second = CapturedTranscript {
            capture_index: 2,
            capture_start_ms: 10_000,
            capture_end_ms: 20_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 10_000,
                end_ms: 19_900,
                text: "外記事情報安全守護神田明神お守りはご".to_string(),
            }],
        };

        assert!(merger.push_capture(first).finalized_segments.is_empty());
        assert!(merger.push_capture(second).finalized_segments.is_empty());

        assert_eq!(
            joined_text(&merger.finish()),
            "外記事情報安全守護神田明神お守りはごIT情報安全守護寒だ"
        );
    }

    #[test]
    /// overlap 内に複数 current segment がある場合でも speaker 境界を潰さずに残す。
    fn keeps_current_segment_boundaries_for_multi_segment_overlap() {
        let mut merger = CaptureMerger::new(TranscriptMergePolicy::recommended());
        let first = CapturedTranscript {
            capture_index: 1,
            capture_start_ms: 0,
            capture_end_ms: 6_000,
            segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 0,
                end_ms: 6_000,
                text: "qrstuvwxyzabcdABCDEFGHJKLMNO".to_string(),
            }],
        };
        let second = CapturedTranscript {
            capture_index: 2,
            capture_start_ms: 3_000,
            capture_end_ms: 9_000,
            segments: vec![
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 3_000,
                    end_ms: 5_000,
                    text: "XABCDEFGH".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_1".to_string(),
                    start_ms: 5_000,
                    end_ms: 6_000,
                    text: "JKLMNO".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_1".to_string(),
                    start_ms: 6_000,
                    end_ms: 7_500,
                    text: "PQR".to_string(),
                },
            ],
        };

        assert!(merger.push_capture(first).finalized_segments.is_empty());
        let second_batch = merger.push_capture(second);

        assert_eq!(
            second_batch.audit_entries[0].outcome,
            MergeAuditOutcome::Accepted {
                overlap_chars: 14,
                alignment_ratio: 1.0,
                trigram_similarity: 1.0,
                current_prefix_trim_chars: 1,
                overlap_text_source: MergeOverlapTextSource::CurrentOverlapRange,
            }
        );
        assert_eq!(
            merger.finish(),
            vec![
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 0,
                    end_ms: 3_000,
                    text: "qrstuvwxyzabcd".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 3_222,
                    end_ms: 5_000,
                    text: "ABCDEFGH".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_1".to_string(),
                    start_ms: 5_000,
                    end_ms: 6_000,
                    text: "JKLMNO".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_1".to_string(),
                    start_ms: 6_000,
                    end_ms: 7_500,
                    text: "PQR".to_string(),
                },
            ]
        );
    }

    #[test]
    /// accepted 候補では少し長い fuzzy overlap より短い exact overlap を優先する。
    fn prefers_exact_overlap_candidate_over_longer_fuzzy_candidate() {
        let longer_fuzzy = OverlapCandidate {
            overlap_chars: 15,
            alignment_ratio: 0.93,
            trigram_similarity: 0.85,
            current_prefix_trim_chars: 0,
        };
        let shorter_exact = OverlapCandidate {
            overlap_chars: 14,
            alignment_ratio: 1.0,
            trigram_similarity: 1.0,
            current_prefix_trim_chars: 1,
        };

        assert!(should_replace_accepted_candidate(
            Some(longer_fuzzy),
            shorter_exact
        ));
        assert!(!should_replace_accepted_candidate(
            Some(shorter_exact),
            longer_fuzzy
        ));
    }
}
