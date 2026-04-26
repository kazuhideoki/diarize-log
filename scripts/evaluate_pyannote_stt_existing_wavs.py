#!/usr/bin/env python3
"""Evaluate pyannote STT orchestration on an existing run audio directory.

This script mirrors scripts/evaluate_separated_existing_wavs.py output shape so
the result can be compared with docs/evaluation/separated-* runs.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import time
import wave
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import requests
import torch
import torchaudio
from speechbrain.inference.speaker import EncoderClassifier

PYANNOTE_MODEL = "precision-2"
PYANNOTE_STT_MODEL = "faster-whisper-large-v3-turbo"
PYANNOTE_MEDIA_ENDPOINT = "https://api.pyannote.ai/v1/media/input"
PYANNOTE_DIARIZE_ENDPOINT = "https://api.pyannote.ai/v1/diarize"
PYANNOTE_JOBS_ENDPOINT = "https://api.pyannote.ai/v1/jobs"
SPEAKER_MODEL = "speechbrain/spkrec-ecapa-voxceleb"

IDENTIFICATION_MIN_SCORE = 0.50
IDENTIFICATION_MIN_MARGIN = 0.15
IDENTIFICATION_MIN_SEGMENT_MS = 300


@dataclass(frozen=True)
class TimedSpeakerSegment:
    speaker: str
    start_ms: int
    end_ms: int


@dataclass(frozen=True)
class TranscriptionSegment:
    speaker: str
    start_ms: int
    end_ms: int
    text: str


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--speakers-dir", type=Path)
    parser.add_argument("--baseline-separated-dir", type=Path)
    parser.add_argument("--max-speakers", type=int)
    parser.add_argument("--transcription-model", default=PYANNOTE_STT_MODEL)
    parser.add_argument("--dotenv", type=Path, default=Path(".env"))
    args = parser.parse_args()

    load_dotenv(args.dotenv)
    pyannote_api_key = require_env("PYANNOTE_API_KEY")

    audios_dir = args.run_dir / "audios"
    captures_dir = args.run_dir / "captures"
    reference_merged_path = args.run_dir / "merged.jsonl"
    args.output_dir.mkdir(parents=True, exist_ok=True)
    (args.output_dir / "captures").mkdir(exist_ok=True)
    (args.output_dir / "pyannote_raw").mkdir(exist_ok=True)

    known_embeddings = load_known_embeddings(args.speakers_dir) if args.speakers_dir else {}
    classifier = EncoderClassifier.from_hparams(source=SPEAKER_MODEL) if known_embeddings else None

    all_absolute_segments: list[dict[str, Any]] = []
    capture_summaries: list[dict[str, Any]] = []
    for wav_path in sorted(audios_dir.glob("capture-*.wav")):
        capture_id = wav_path.stem
        print(f"processing {capture_id}", flush=True)
        capture_start_ms = read_capture_start_ms(captures_dir / f"{capture_id}.json")
        duration_ms = wav_duration_ms(wav_path)
        raw_path = args.output_dir / "pyannote_raw" / f"{capture_id}.json"
        if raw_path.exists():
            payload = json.loads(raw_path.read_text())
        else:
            payload = run_pyannote_stt(
                wav_path=wav_path,
                api_key=pyannote_api_key,
                max_speakers=args.max_speakers,
                transcription_model=args.transcription_model,
                media_id=f"diarize-log-pyannote-stt-eval-{capture_id}-{int(time.time() * 1000)}",
            )
            raw_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")

        diarization_segments = parse_timed_speaker_segments(payload, "exclusiveDiarization")
        transcript_segments = parse_transcription_segments(payload)
        speaker_names = identify_speakers(
            wav_path=wav_path,
            segments=transcript_segments,
            classifier=classifier,
            known_embeddings=known_embeddings,
        )
        named_segments: list[TranscriptionSegment] = []
        for segment in transcript_segments:
            text = segment.text.strip()
            if not text:
                continue
            named_segments.append(
                TranscriptionSegment(
                    speaker=speaker_names.get(segment.speaker, segment.speaker),
                    start_ms=segment.start_ms,
                    end_ms=segment.end_ms,
                    text=text,
                )
            )

        capture_output = {
            "capture_start_ms": capture_start_ms,
            "text": " ".join(segment.text for segment in named_segments),
            "segments": [segment_to_json(segment) for segment in named_segments],
        }
        (args.output_dir / "captures" / f"{capture_id}.json").write_text(
            json.dumps(capture_output, ensure_ascii=False, indent=2) + "\n"
        )

        for segment in named_segments:
            all_absolute_segments.append(
                {
                    "speaker": segment.speaker,
                    "start_ms": capture_start_ms + segment.start_ms,
                    "end_ms": capture_start_ms + segment.end_ms,
                    "text": segment.text,
                }
            )

        capture_summaries.append(
            {
                "capture": capture_id,
                "duration_ms": duration_ms,
                "diarization_segments": len(diarization_segments),
                "transcript_segments": len(named_segments),
                "speakers": sorted({segment.speaker for segment in named_segments}),
                "text_chars": sum(len(segment.text) for segment in named_segments),
            }
        )

    absolute_path = args.output_dir / "pyannote_stt_absolute_segments.jsonl"
    with absolute_path.open("w") as handle:
        for segment in sorted(all_absolute_segments, key=lambda item: (item["start_ms"], item["end_ms"])):
            handle.write(json.dumps(segment, ensure_ascii=False) + "\n")

    reference_segments = read_jsonl(reference_merged_path)
    baseline_segments = read_baseline_segments(args.baseline_separated_dir)
    write_report(
        output_path=args.output_dir / "report.md",
        run_dir=args.run_dir,
        reference_path=reference_merged_path,
        pyannote_stt_path=absolute_path,
        baseline_path=baseline_segments[0],
        capture_summaries=capture_summaries,
        reference_segments=reference_segments,
        pyannote_segments=all_absolute_segments,
        baseline_segments=baseline_segments[1],
        max_speakers=args.max_speakers,
        transcription_model=args.transcription_model,
        speaker_identification_enabled=bool(known_embeddings),
    )
    write_readme(args.output_dir, args.run_dir, args.speakers_dir, args.baseline_separated_dir)
    return 0


def load_dotenv(path: Path) -> None:
    if not path.exists():
        return
    for raw_line in path.read_text().splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        os.environ.setdefault(key.strip(), value.strip().strip('"').strip("'"))


def require_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise SystemExit(f"{name} is required")
    return value


def run_pyannote_stt(
    wav_path: Path,
    api_key: str,
    max_speakers: int | None,
    transcription_model: str,
    media_id: str,
) -> dict[str, Any]:
    media_url = f"media://{media_id}"
    upload_response = requests.post(
        PYANNOTE_MEDIA_ENDPOINT,
        headers={"Authorization": f"Bearer {api_key}"},
        json={"url": media_url},
        timeout=60,
    )
    upload_response.raise_for_status()
    upload_url = upload_response.json()["url"]
    with wav_path.open("rb") as wav_file:
        put_response = requests.put(
            upload_url,
            data=wav_file,
            headers={"Content-Type": "application/octet-stream"},
            timeout=300,
        )
    put_response.raise_for_status()

    job_payload: dict[str, Any] = {
        "url": media_url,
        "model": PYANNOTE_MODEL,
        "exclusive": True,
        "transcription": True,
        "transcriptionConfig": {"model": transcription_model},
    }
    if max_speakers is not None:
        job_payload["maxSpeakers"] = max_speakers
    job_response = requests.post(
        PYANNOTE_DIARIZE_ENDPOINT,
        headers={"Authorization": f"Bearer {api_key}"},
        json=job_payload,
        timeout=60,
    )
    job_response.raise_for_status()
    job_id = job_response.json()["jobId"]

    deadline = time.time() + 20 * 60
    while time.time() < deadline:
        poll_response = requests.get(
            f"{PYANNOTE_JOBS_ENDPOINT}/{job_id}",
            headers={"Authorization": f"Bearer {api_key}"},
            timeout=60,
        )
        poll_response.raise_for_status()
        payload = poll_response.json()
        if payload["status"] == "succeeded":
            return payload
        if payload["status"] in {"failed", "canceled", "cancelled"}:
            raise RuntimeError(f"pyannote STT job failed: {payload}")
        time.sleep(2)
    raise TimeoutError(f"pyannote STT job timed out: {job_id}")


def parse_timed_speaker_segments(payload: dict[str, Any], key: str) -> list[TimedSpeakerSegment]:
    output = payload.get("output") or {}
    raw_segments = output.get(key) or output.get("diarization") or []
    return [
        TimedSpeakerSegment(
            speaker=item["speaker"],
            start_ms=round(float(item["start"]) * 1000),
            end_ms=round(float(item["end"]) * 1000),
        )
        for item in raw_segments
        if float(item["end"]) > float(item["start"])
    ]


def parse_transcription_segments(payload: dict[str, Any]) -> list[TranscriptionSegment]:
    output = payload.get("output") or {}
    if "turnLevelTranscription" not in output:
        raise RuntimeError(f"pyannote STT output does not include turnLevelTranscription: {payload}")
    raw_segments = output["turnLevelTranscription"] or []
    return [
        TranscriptionSegment(
            speaker=item["speaker"],
            start_ms=round(float(item["start"]) * 1000),
            end_ms=round(float(item["end"]) * 1000),
            text=item.get("text", ""),
        )
        for item in raw_segments
        if float(item["end"]) > float(item["start"])
    ]


def segment_to_json(segment: TranscriptionSegment) -> dict[str, Any]:
    return {
        "speaker": segment.speaker,
        "start_ms": segment.start_ms,
        "end_ms": segment.end_ms,
        "text": segment.text,
    }


def load_known_embeddings(speakers_dir: Path) -> dict[str, list[float]]:
    embeddings = {}
    for path in sorted(speakers_dir.glob("*.embedding.json")):
        payload = json.loads(path.read_text())
        embeddings[payload["speaker_name"]] = payload["vector"]
    return embeddings


def identify_speakers(
    wav_path: Path,
    segments: list[TranscriptionSegment],
    classifier: EncoderClassifier | None,
    known_embeddings: dict[str, list[float]],
) -> dict[str, str]:
    if not known_embeddings or classifier is None:
        return {}
    signal, _sample_rate = load_mono_16k(wav_path)
    grouped: dict[str, list[tuple[list[float], int]]] = {}
    for segment in segments:
        duration_ms = segment.end_ms - segment.start_ms
        if duration_ms < IDENTIFICATION_MIN_SEGMENT_MS:
            continue
        start = round(segment.start_ms * 16)
        end = max(start + 1, round(segment.end_ms * 16))
        chunk = signal[:, start:end]
        with torch.no_grad():
            vector = classifier.encode_batch(chunk).squeeze().detach().cpu().tolist()
        grouped.setdefault(segment.speaker, []).append((vector, duration_ms))

    names = {}
    for anonymous_speaker, weighted_vectors in grouped.items():
        vector = weighted_average(weighted_vectors)
        best = identify_vector(vector, known_embeddings)
        names[anonymous_speaker] = best or "UNKNOWN"
    return names


def identify_vector(vector: list[float], known_embeddings: dict[str, list[float]]) -> str | None:
    scores = sorted(
        ((name, cosine_similarity(vector, known_vector)) for name, known_vector in known_embeddings.items()),
        key=lambda item: item[1],
        reverse=True,
    )
    if not scores:
        return None
    best_name, best_score = scores[0]
    second_score = scores[1][1] if len(scores) > 1 else 0.0
    if best_score >= IDENTIFICATION_MIN_SCORE and best_score - second_score >= IDENTIFICATION_MIN_MARGIN:
        return best_name
    return None


def weighted_average(weighted_vectors: list[tuple[list[float], int]]) -> list[float]:
    total = sum(weight for _, weight in weighted_vectors)
    dims = len(weighted_vectors[0][0])
    output = [0.0] * dims
    for vector, weight in weighted_vectors:
        for index, value in enumerate(vector):
            output[index] += value * weight
    return [value / total for value in output]


def cosine_similarity(left: list[float], right: list[float]) -> float:
    if len(left) != len(right) or not left:
        return 0.0
    dot = sum(a * b for a, b in zip(left, right))
    left_norm = math.sqrt(sum(value * value for value in left))
    right_norm = math.sqrt(sum(value * value for value in right))
    if left_norm == 0.0 or right_norm == 0.0:
        return 0.0
    return dot / (left_norm * right_norm)


def load_mono_16k(wav_path: Path) -> tuple[torch.Tensor, int]:
    signal, sample_rate = torchaudio.load(wav_path)
    if signal.shape[0] > 1:
        signal = signal.mean(dim=0, keepdim=True)
    if sample_rate != 16000:
        signal = torchaudio.functional.resample(signal, sample_rate, 16000)
        sample_rate = 16000
    return signal, sample_rate


def wav_duration_ms(wav_path: Path) -> int:
    with wave.open(str(wav_path), "rb") as reader:
        return round(reader.getnframes() * 1000 / reader.getframerate())


def read_capture_start_ms(path: Path) -> int:
    return int(json.loads(path.read_text())["capture_start_ms"])


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def read_baseline_segments(baseline_dir: Path | None) -> tuple[Path | None, list[dict[str, Any]]]:
    if baseline_dir is None:
        return None, []
    path = baseline_dir / "separated_absolute_segments.jsonl"
    if not path.exists():
        raise SystemExit(f"baseline separated segments not found: {path}")
    return path, read_jsonl(path)


def write_report(
    output_path: Path,
    run_dir: Path,
    reference_path: Path,
    pyannote_stt_path: Path,
    baseline_path: Path | None,
    capture_summaries: list[dict[str, Any]],
    reference_segments: list[dict[str, Any]],
    pyannote_segments: list[dict[str, Any]],
    baseline_segments: list[dict[str, Any]],
    max_speakers: int | None,
    transcription_model: str,
    speaker_identification_enabled: bool,
) -> None:
    reference_speakers = sorted({segment["speaker"] for segment in reference_segments})
    pyannote_speakers = sorted({segment["speaker"] for segment in pyannote_segments})
    baseline_speakers = sorted({segment["speaker"] for segment in baseline_segments})
    lines = [
        "# pyannote STT orchestration evaluation",
        "",
        f"- run_dir: `{run_dir}`",
        f"- reference: `{reference_path}`",
        f"- pyannote_stt_absolute_segments: `{pyannote_stt_path}`",
        f"- baseline_separated_absolute_segments: `{baseline_path or 'unset'}`",
        f"- pyannote_model: `{PYANNOTE_MODEL}`",
        f"- pyannote_transcription_model: `{transcription_model}`",
        f"- maxSpeakers: `{max_speakers if max_speakers is not None else 'unset'}`",
        f"- speaker_identification: `{'enabled' if speaker_identification_enabled else 'disabled'}`",
        "",
        "## summary",
        "",
    ]
    if baseline_segments:
        lines.extend(
            [
                "| item | reference merged | separated baseline | pyannote STT |",
                "| --- | ---: | ---: | ---: |",
                f"| segments | {len(reference_segments)} | {len(baseline_segments)} | {len(pyannote_segments)} |",
                f"| speakers | {len(reference_speakers)} | {len(baseline_speakers)} | {len(pyannote_speakers)} |",
                f"| text chars | {text_chars(reference_segments)} | {text_chars(baseline_segments)} | {text_chars(pyannote_segments)} |",
                "",
                f"- reference speakers: `{', '.join(reference_speakers)}`",
                f"- separated baseline speakers: `{', '.join(baseline_speakers)}`",
                f"- pyannote STT speakers: `{', '.join(pyannote_speakers)}`",
            ]
        )
    else:
        lines.extend(
            [
                "| item | reference merged | pyannote STT |",
                "| --- | ---: | ---: |",
                f"| segments | {len(reference_segments)} | {len(pyannote_segments)} |",
                f"| speakers | {len(reference_speakers)} | {len(pyannote_speakers)} |",
                f"| text chars | {text_chars(reference_segments)} | {text_chars(pyannote_segments)} |",
                "",
                f"- reference speakers: `{', '.join(reference_speakers)}`",
                f"- pyannote STT speakers: `{', '.join(pyannote_speakers)}`",
            ]
        )
    lines.extend(
        [
            "",
            "## per capture",
            "",
            "| capture | duration_ms | diarization_segments | transcript_segments | text_chars | speakers |",
            "| --- | ---: | ---: | ---: | ---: | --- |",
        ]
    )
    for item in capture_summaries:
        lines.append(
            f"| {item['capture']} | {item['duration_ms']} | {item['diarization_segments']} | "
            f"{item['transcript_segments']} | {item['text_chars']} | `{', '.join(item['speakers'])}` |"
        )
    lines.extend(
        [
            "",
            "## note",
            "",
            "- `pyannote_stt_absolute_segments.jsonl` is capture-start adjusted, but it is not passed through the Rust overlap merger.",
            "- pyannote STT uses `turnLevelTranscription`; word-level output remains available in `pyannote_raw/*.json`.",
            "- If `--speakers-dir` is provided, anonymous pyannote speaker labels are mapped with the same SpeechBrain embedding rule used by the separated evaluation harness.",
        ]
    )
    output_path.write_text("\n".join(lines) + "\n")


def text_chars(segments: list[dict[str, Any]]) -> int:
    return sum(len(segment["text"]) for segment in segments)


def write_readme(
    output_dir: Path,
    run_dir: Path,
    speakers_dir: Path | None,
    baseline_separated_dir: Path | None,
) -> None:
    lines = [
        "# pyannote STT orchestration evaluation",
        "",
        "対象:",
        "",
        f"- input audios: `{run_dir / 'audios'}`",
        f"- reference merged: `{run_dir / 'merged.jsonl'}`",
        f"- output dir: `{output_dir}`",
    ]
    if baseline_separated_dir:
        lines.append(f"- baseline separated: `{baseline_separated_dir}`")
    lines.extend(
        [
            "",
            "実行:",
            "",
            "```bash",
            "PYANNOTE_API_KEY=... .venv/bin/python scripts/evaluate_pyannote_stt_existing_wavs.py \\",
            f"  --run-dir {run_dir} \\",
        ]
    )
    if speakers_dir:
        lines.append(f"  --speakers-dir {speakers_dir} \\")
    if baseline_separated_dir:
        lines.append(f"  --baseline-separated-dir {baseline_separated_dir} \\")
    lines.extend(
        [
            f"  --output-dir {output_dir}",
            "```",
            "",
            "生成物:",
            "",
            "- `captures/*.json`: capture ごとの pyannote STT transcript",
            "- `pyannote_raw/*.json`: pyannote job result",
            "- `pyannote_stt_absolute_segments.jsonl`: capture_start_ms を足した pyannote STT segment",
            "- `report.md`: 既存 `merged.jsonl` および任意の separated baseline との件数・話者・文字数比較",
        ]
    )
    (output_dir / "README.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    raise SystemExit(main())
