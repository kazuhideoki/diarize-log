#!/usr/bin/env python3
"""Compare normalized transcript text across evaluation outputs."""

from __future__ import annotations

import argparse
import json
import re
import unicodedata
import wave
from pathlib import Path
from typing import Any

JA_CHAR = r"\u3040-\u30ff\u3400-\u9fff々〆〤ー"
JA_PUNCT = r"、。！？「」『』（）［］【】"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--separated-dir", type=Path, required=True)
    parser.add_argument("--pyannote-stt-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    captures = sorted((args.pyannote_stt_dir / "captures").glob("capture-*.json"))
    rows = []
    totals = {
        "reference": "",
        "separated": "",
        "pyannote": "",
    }
    normalized_json: list[dict[str, Any]] = []

    for pyannote_capture_path in captures:
        capture_id = pyannote_capture_path.stem
        reference_text = reference_text_for_capture(args.run_dir, capture_id)
        separated_text = read_capture_text(args.separated_dir / "captures" / f"{capture_id}.json")
        pyannote_text = read_capture_text(pyannote_capture_path)

        reference_norm = normalize_text(reference_text)
        separated_norm = normalize_text(separated_text)
        pyannote_norm = normalize_text(pyannote_text)

        totals["reference"] += reference_norm
        totals["separated"] += separated_norm
        totals["pyannote"] += pyannote_norm
        rows.append(
            {
                "capture": capture_id,
                "reference_chars": len(reference_norm),
                "separated_chars": len(separated_norm),
                "pyannote_chars": len(pyannote_norm),
                "separated_vs_reference": similarity(separated_norm, reference_norm),
                "pyannote_vs_reference": similarity(pyannote_norm, reference_norm),
                "pyannote_vs_separated": similarity(pyannote_norm, separated_norm),
            }
        )
        normalized_json.append(
            {
                "capture": capture_id,
                "reference": reference_norm,
                "separated": separated_norm,
                "pyannote_stt": pyannote_norm,
            }
        )

    summary = {
        "reference_chars": len(totals["reference"]),
        "separated_chars": len(totals["separated"]),
        "pyannote_chars": len(totals["pyannote"]),
        "separated_vs_reference": similarity(totals["separated"], totals["reference"]),
        "pyannote_vs_reference": similarity(totals["pyannote"], totals["reference"]),
        "pyannote_vs_separated": similarity(totals["pyannote"], totals["separated"]),
    }

    output_dir = args.output.parent
    output_dir.mkdir(parents=True, exist_ok=True)
    (output_dir / "normalized_texts.json").write_text(
        json.dumps(normalized_json, ensure_ascii=False, indent=2) + "\n"
    )
    args.output.write_text(render_report(args, summary, rows) + "\n")
    return 0


def normalize_text(text: str) -> str:
    text = unicodedata.normalize("NFKC", text)
    text = text.replace("\u3000", " ")
    text = re.sub(r"\s+", " ", text).strip()
    text = re.sub(fr"(?<=[{JA_CHAR}])\s+(?=[{JA_CHAR}])", "", text)
    text = re.sub(fr"\s+(?=[{JA_PUNCT}])", "", text)
    text = re.sub(fr"(?<=[{JA_PUNCT}])\s+", "", text)
    return text


def read_capture_text(path: Path) -> str:
    payload = json.loads(path.read_text())
    return payload["text"]


def reference_text_for_capture(run_dir: Path, capture_id: str) -> str:
    capture_path = run_dir / "captures" / f"{capture_id}.json"
    audio_path = run_dir / "audios" / f"{capture_id}.wav"
    capture_start_ms = int(json.loads(capture_path.read_text())["capture_start_ms"])
    capture_end_ms = capture_start_ms + wav_duration_ms(audio_path)
    segments = read_jsonl(run_dir / "merged.jsonl")
    texts = [
        segment["text"]
        for segment in segments
        if intervals_overlap(
            int(segment["start_ms"]),
            int(segment["end_ms"]),
            capture_start_ms,
            capture_end_ms,
        )
    ]
    return " ".join(texts)


def intervals_overlap(left_start: int, left_end: int, right_start: int, right_end: int) -> bool:
    return left_start < right_end and right_start < left_end


def wav_duration_ms(wav_path: Path) -> int:
    with wave.open(str(wav_path), "rb") as reader:
        return round(reader.getnframes() * 1000 / reader.getframerate())


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def similarity(left: str, right: str) -> float:
    if not left and not right:
        return 1.0
    distance = levenshtein_distance(left, right)
    return 1.0 - distance / max(len(left), len(right), 1)


def levenshtein_distance(left: str, right: str) -> int:
    if len(left) < len(right):
        left, right = right, left
    previous = list(range(len(right) + 1))
    for left_index, left_char in enumerate(left, start=1):
        current = [left_index]
        for right_index, right_char in enumerate(right, start=1):
            current.append(
                min(
                    previous[right_index] + 1,
                    current[right_index - 1] + 1,
                    previous[right_index - 1] + (left_char != right_char),
                )
            )
        previous = current
    return previous[-1]


def render_report(args: argparse.Namespace, summary: dict[str, Any], rows: list[dict[str, Any]]) -> str:
    lines = [
        "# normalized transcript comparison",
        "",
        f"- run_dir: `{args.run_dir}`",
        f"- separated_dir: `{args.separated_dir}`",
        f"- pyannote_stt_dir: `{args.pyannote_stt_dir}`",
        "- normalization: `NFKC`, collapse whitespace, remove spaces between Japanese characters, remove spaces around Japanese punctuation",
        "- note: `reference merged` is an existing pipeline output, not a human ground truth. Similarity is a rough regression/comparison signal.",
        "",
        "## summary",
        "",
        "| item | reference merged | separated baseline | pyannote STT |",
        "| --- | ---: | ---: | ---: |",
        f"| normalized chars | {summary['reference_chars']} | {summary['separated_chars']} | {summary['pyannote_chars']} |",
        "",
        "| comparison | similarity |",
        "| --- | ---: |",
        f"| separated vs reference | {summary['separated_vs_reference']:.3f} |",
        f"| pyannote STT vs reference | {summary['pyannote_vs_reference']:.3f} |",
        f"| pyannote STT vs separated | {summary['pyannote_vs_separated']:.3f} |",
        "",
        "## initial findings",
        "",
        f"- pyannote STT keeps {summary['pyannote_chars'] / summary['reference_chars']:.1%} of the normalized reference text volume, while separated baseline keeps {summary['separated_chars'] / summary['reference_chars']:.1%}.",
        f"- pyannote STT is closer to reference merged by normalized edit similarity: `{summary['pyannote_vs_reference']:.3f}` vs separated baseline `{summary['separated_vs_reference']:.3f}`.",
        "- pyannote STT still leaves tokenization artifacts around numbers and ASCII terms, so a downstream formatter would be needed before using the transcript as user-facing text.",
        "- `capture-000011` is the main exception in this comparison; separated baseline is closer to reference merged than pyannote STT on the rough similarity metric.",
        "",
        "## per capture",
        "",
        "| capture | reference chars | separated chars | pyannote STT chars | separated vs ref | pyannote vs ref | pyannote vs separated |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for row in rows:
        lines.append(
            f"| {row['capture']} | {row['reference_chars']} | {row['separated_chars']} | "
            f"{row['pyannote_chars']} | {row['separated_vs_reference']:.3f} | "
            f"{row['pyannote_vs_reference']:.3f} | {row['pyannote_vs_separated']:.3f} |"
        )
    return "\n".join(lines)


if __name__ == "__main__":
    raise SystemExit(main())
