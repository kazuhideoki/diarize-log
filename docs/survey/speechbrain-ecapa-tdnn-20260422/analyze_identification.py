#!/usr/bin/env python3
"""pyannote diarization segments against known speakers with SpeechBrain ECAPA-TDNN."""

from __future__ import annotations

import argparse
import csv
import json
import math
from collections import defaultdict
from pathlib import Path

import soundfile as sf
import torch
import torchaudio.functional as F
from speechbrain.inference.speaker import EncoderClassifier


TARGET_SAMPLE_RATE = 16000


def parse_named_path(value: str) -> tuple[str, Path]:
    if "=" not in value:
        raise argparse.ArgumentTypeError(
            f"expected NAME=PATH or CAPTURE=PATH format, got: {value}"
        )
    name, raw_path = value.split("=", 1)
    if not name:
        raise argparse.ArgumentTypeError(f"name cannot be empty: {value}")
    path = Path(raw_path).expanduser().resolve()
    if not path.exists():
        raise argparse.ArgumentTypeError(f"path does not exist: {path}")
    return name, path


def load_audio_mono(path: Path) -> torch.Tensor:
    data, sample_rate = sf.read(path, always_2d=True)
    waveform = torch.from_numpy(data.T).float()
    if waveform.shape[0] > 1:
        waveform = waveform.mean(dim=0, keepdim=True)
    if sample_rate != TARGET_SAMPLE_RATE:
        waveform = F.resample(waveform, sample_rate, TARGET_SAMPLE_RATE)
    return waveform


def read_segments(path: Path) -> list[dict[str, float | str]]:
    payload = json.loads(path.read_text())
    diarization = payload.get("diarization")
    if not isinstance(diarization, list):
        raise ValueError(f"invalid diarization payload: {path}")
    return diarization


def encode_embedding(
    classifier: EncoderClassifier,
    waveform: torch.Tensor,
) -> torch.Tensor:
    with torch.no_grad():
        embedding = classifier.encode_batch(waveform).squeeze(0).squeeze(0)
    return torch.nn.functional.normalize(embedding, dim=0)


def cosine_similarity(left: torch.Tensor, right: torch.Tensor) -> float:
    return float(torch.dot(left, right).item())


def top_two(scores: dict[str, float]) -> tuple[tuple[str, float], tuple[str, float] | None]:
    ordered = sorted(scores.items(), key=lambda item: item[1], reverse=True)
    first = ordered[0]
    second = ordered[1] if len(ordered) > 1 else None
    return first, second


def weighted_average(vectors: list[torch.Tensor], weights: list[float]) -> torch.Tensor:
    stacked = torch.stack(vectors)
    weight_tensor = torch.tensor(weights, dtype=stacked.dtype).unsqueeze(1)
    averaged = (stacked * weight_tensor).sum(dim=0) / weight_tensor.sum()
    return torch.nn.functional.normalize(averaged, dim=0)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    parser.add_argument("--speaker", action="append", required=True, type=parse_named_path)
    parser.add_argument("--audio", action="append", required=True, type=parse_named_path)
    parser.add_argument("--diarization", action="append", required=True, type=parse_named_path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--cache-dir", type=Path, required=True)
    parser.add_argument(
        "--unknown-threshold",
        type=float,
        default=None,
        help="Mark speaker as UNKNOWN when top score is below this value.",
    )
    parser.add_argument(
        "--min-margin",
        type=float,
        default=None,
        help="Mark speaker as UNKNOWN when (top1 - top2) is below this value.",
    )
    parser.add_argument(
        "--min-analysis-duration-sec",
        type=float,
        default=0.8,
        help="Expand very short segments to at least this duration for embedding analysis.",
    )
    return parser


def main() -> None:
    args = build_parser().parse_args()

    output_dir: Path = args.output_dir.resolve()
    cache_dir: Path = args.cache_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    cache_dir.mkdir(parents=True, exist_ok=True)

    speakers = dict(args.speaker)
    audios = dict(args.audio)
    diarizations = dict(args.diarization)

    missing_audio = sorted(set(diarizations) - set(audios))
    missing_diarization = sorted(set(audios) - set(diarizations))
    if missing_audio:
        raise SystemExit(f"missing audio for captures: {', '.join(missing_audio)}")
    if missing_diarization:
        raise SystemExit(
            f"missing diarization for captures: {', '.join(missing_diarization)}"
        )

    classifier = EncoderClassifier.from_hparams(
        source="speechbrain/spkrec-ecapa-voxceleb",
        savedir=str(cache_dir / "spkrec-ecapa-voxceleb"),
    )

    reference_embeddings: dict[str, torch.Tensor] = {}
    reference_summary: dict[str, dict[str, object]] = {}
    for speaker_name, speaker_path in speakers.items():
        waveform = load_audio_mono(speaker_path)
        embedding = encode_embedding(classifier, waveform)
        reference_embeddings[speaker_name] = embedding
        reference_summary[speaker_name] = {
            "path": str(speaker_path),
            "duration_sec": round(waveform.shape[1] / TARGET_SAMPLE_RATE, 3),
        }

    reference_similarity = {
        left: {
            right: round(cosine_similarity(left_embedding, right_embedding), 6)
            for right, right_embedding in reference_embeddings.items()
        }
        for left, left_embedding in reference_embeddings.items()
    }

    summary: dict[str, object] = {
        "model": "speechbrain/spkrec-ecapa-voxceleb",
        "target_sample_rate": TARGET_SAMPLE_RATE,
        "unknown_threshold": args.unknown_threshold,
        "min_margin": args.min_margin,
        "reference_speakers": reference_summary,
        "reference_similarity": reference_similarity,
        "captures": {},
    }

    for capture_name in sorted(audios):
        audio_path = audios[capture_name]
        diarization_path = diarizations[capture_name]
        waveform = load_audio_mono(audio_path)
        full_duration = waveform.shape[1] / TARGET_SAMPLE_RATE
        segments = read_segments(diarization_path)

        segment_rows: list[dict[str, object]] = []
        speaker_vectors: dict[str, list[torch.Tensor]] = defaultdict(list)
        speaker_weights: dict[str, list[float]] = defaultdict(list)

        for index, segment in enumerate(segments):
            start = max(0.0, float(segment["start"]))
            end = min(full_duration, float(segment["end"]))
            if end <= start:
                continue

            start_frame = max(0, math.floor(start * TARGET_SAMPLE_RATE))
            end_frame = min(waveform.shape[1], math.ceil(end * TARGET_SAMPLE_RATE))
            min_analysis_frames = math.ceil(
                args.min_analysis_duration_sec * TARGET_SAMPLE_RATE
            )
            analysis_start_frame = start_frame
            analysis_end_frame = end_frame
            if end_frame - start_frame < min_analysis_frames:
                deficit = min_analysis_frames - (end_frame - start_frame)
                left_extra = deficit // 2
                right_extra = deficit - left_extra
                analysis_start_frame = max(0, start_frame - left_extra)
                analysis_end_frame = min(waveform.shape[1], end_frame + right_extra)
                remaining = min_analysis_frames - (
                    analysis_end_frame - analysis_start_frame
                )
                if remaining > 0:
                    analysis_start_frame = max(0, analysis_start_frame - remaining)
                    analysis_end_frame = min(
                        waveform.shape[1], analysis_end_frame + remaining
                    )

            segment_waveform = waveform[:, analysis_start_frame:analysis_end_frame]
            if segment_waveform.shape[1] == 0:
                continue
            if segment_waveform.shape[1] < min_analysis_frames:
                padded = torch.zeros(
                    (segment_waveform.shape[0], min_analysis_frames),
                    dtype=segment_waveform.dtype,
                )
                padded[:, : segment_waveform.shape[1]] = segment_waveform
                segment_waveform = padded

            embedding = encode_embedding(classifier, segment_waveform)
            scores = {
                speaker_name: round(
                    cosine_similarity(embedding, reference_embedding), 6
                )
                for speaker_name, reference_embedding in reference_embeddings.items()
            }
            best, second = top_two(scores)
            second_name = second[0] if second is not None else None
            second_score = second[1] if second is not None else None
            margin = None if second_score is None else round(best[1] - second_score, 6)

            pyannote_speaker = str(segment["speaker"])
            duration_sec = round(end - start, 3)
            speaker_vectors[pyannote_speaker].append(embedding)
            speaker_weights[pyannote_speaker].append(end - start)

            segment_rows.append(
                {
                    "segment_index": index,
                    "pyannote_speaker": pyannote_speaker,
                    "start": round(start, 3),
                    "end": round(end, 3),
                    "duration_sec": duration_sec,
                    "analysis_start": round(
                        analysis_start_frame / TARGET_SAMPLE_RATE, 3
                    ),
                    "analysis_end": round(
                        analysis_end_frame / TARGET_SAMPLE_RATE, 3
                    ),
                    "analysis_duration_sec": round(
                        segment_waveform.shape[1] / TARGET_SAMPLE_RATE, 3
                    ),
                    "segment_best_speaker": best[0],
                    "segment_best_score": best[1],
                    "segment_second_speaker": second_name,
                    "segment_second_score": second_score,
                    "segment_margin": margin,
                    "scores": scores,
                }
            )

        pyannote_summaries: list[dict[str, object]] = []
        speaker_assignments: dict[str, dict[str, object]] = {}
        for pyannote_speaker in sorted(speaker_vectors):
            aggregate_embedding = weighted_average(
                speaker_vectors[pyannote_speaker], speaker_weights[pyannote_speaker]
            )
            aggregate_scores = {
                speaker_name: round(
                    cosine_similarity(aggregate_embedding, reference_embedding), 6
                )
                for speaker_name, reference_embedding in reference_embeddings.items()
            }
            best, second = top_two(aggregate_scores)
            second_name = second[0] if second is not None else None
            second_score = second[1] if second is not None else None
            margin = None if second_score is None else round(best[1] - second_score, 6)

            assigned_speaker = best[0]
            unknown_reasons: list[str] = []
            if args.unknown_threshold is not None and best[1] < args.unknown_threshold:
                assigned_speaker = "UNKNOWN"
                unknown_reasons.append(
                    f"top score {best[1]:.6f} < threshold {args.unknown_threshold:.6f}"
                )
            if args.min_margin is not None and second_score is not None and best[1] - second_score < args.min_margin:
                assigned_speaker = "UNKNOWN"
                unknown_reasons.append(
                    f"margin {best[1] - second_score:.6f} < min_margin {args.min_margin:.6f}"
                )

            total_duration = round(sum(speaker_weights[pyannote_speaker]), 3)
            assignment = {
                "pyannote_speaker": pyannote_speaker,
                "assigned_speaker": assigned_speaker,
                "cluster_best_speaker": best[0],
                "cluster_best_score": best[1],
                "cluster_second_speaker": second_name,
                "cluster_second_score": second_score,
                "cluster_margin": margin,
                "total_duration_sec": total_duration,
                "segment_count": len(speaker_vectors[pyannote_speaker]),
                "scores": aggregate_scores,
                "unknown_reasons": unknown_reasons,
            }
            pyannote_summaries.append(assignment)
            speaker_assignments[pyannote_speaker] = assignment

        labeled_segments = []
        for row in segment_rows:
            assignment = speaker_assignments[row["pyannote_speaker"]]
            labeled_segments.append(
                {
                    **row,
                    "assigned_speaker": assignment["assigned_speaker"],
                    "cluster_best_speaker": assignment["cluster_best_speaker"],
                    "cluster_best_score": assignment["cluster_best_score"],
                    "cluster_second_speaker": assignment["cluster_second_speaker"],
                    "cluster_second_score": assignment["cluster_second_score"],
                    "cluster_margin": assignment["cluster_margin"],
                }
            )

        capture_payload = {
            "capture": capture_name,
            "audio_path": str(audio_path),
            "diarization_path": str(diarization_path),
            "audio_duration_sec": round(full_duration, 3),
            "segment_count": len(labeled_segments),
            "pyannote_speakers": pyannote_summaries,
            "segments": labeled_segments,
        }
        summary["captures"][capture_name] = {
            "audio_path": str(audio_path),
            "diarization_path": str(diarization_path),
            "audio_duration_sec": round(full_duration, 3),
            "segment_count": len(labeled_segments),
            "pyannote_speakers": pyannote_summaries,
        }

        (output_dir / f"{capture_name}.json").write_text(
            json.dumps(capture_payload, ensure_ascii=False, indent=2) + "\n"
        )

        with (output_dir / f"{capture_name}.csv").open("w", newline="") as csv_file:
            fieldnames = [
                "segment_index",
                "pyannote_speaker",
                "assigned_speaker",
                "cluster_best_speaker",
                "cluster_best_score",
                "cluster_second_speaker",
                "cluster_second_score",
                "cluster_margin",
                "start",
                "end",
                "duration_sec",
                "analysis_start",
                "analysis_end",
                "analysis_duration_sec",
                "segment_best_speaker",
                "segment_best_score",
                "segment_second_speaker",
                "segment_second_score",
                "segment_margin",
                *sorted(reference_embeddings),
            ]
            writer = csv.DictWriter(csv_file, fieldnames=fieldnames)
            writer.writeheader()
            for row in labeled_segments:
                writer.writerow(
                    {
                        "segment_index": row["segment_index"],
                        "pyannote_speaker": row["pyannote_speaker"],
                        "assigned_speaker": row["assigned_speaker"],
                        "cluster_best_speaker": row["cluster_best_speaker"],
                        "cluster_best_score": row["cluster_best_score"],
                        "cluster_second_speaker": row["cluster_second_speaker"],
                        "cluster_second_score": row["cluster_second_score"],
                        "cluster_margin": row["cluster_margin"],
                        "start": row["start"],
                        "end": row["end"],
                        "duration_sec": row["duration_sec"],
                        "analysis_start": row["analysis_start"],
                        "analysis_end": row["analysis_end"],
                        "analysis_duration_sec": row["analysis_duration_sec"],
                        "segment_best_speaker": row["segment_best_speaker"],
                        "segment_best_score": row["segment_best_score"],
                        "segment_second_speaker": row["segment_second_speaker"],
                        "segment_second_score": row["segment_second_score"],
                        "segment_margin": row["segment_margin"],
                        **row["scores"],
                    }
                )

    (output_dir / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
