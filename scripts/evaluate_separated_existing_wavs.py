#!/usr/bin/env python3
"""Evaluate separated transcription on an existing run audio directory.

This script is intentionally evaluation-only: the production CLI records audio
before transcription, while this harness accepts already persisted capture WAVs.
"""

from __future__ import annotations

import argparse
import base64
import concurrent.futures
import json
import math
import os
import tempfile
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
PYANNOTE_MEDIA_ENDPOINT = "https://api.pyannote.ai/v1/media/input"
PYANNOTE_DIARIZE_ENDPOINT = "https://api.pyannote.ai/v1/diarize"
PYANNOTE_JOBS_ENDPOINT = "https://api.pyannote.ai/v1/jobs"
OPENAI_TRANSCRIPTIONS_ENDPOINT = "https://api.openai.com/v1/audio/transcriptions"
OPENAI_ASR_MODEL = "gpt-4o-transcribe"
SPEAKER_MODEL = "speechbrain/spkrec-ecapa-voxceleb"

MERGE_GAP_MS = 10_000
PADDING_MS = 150
MIN_TURN_MS = 300
MAX_TURN_MS = 5 * 60 * 1_000
IDENTIFICATION_MIN_SCORE = 0.50
IDENTIFICATION_MIN_MARGIN = 0.15
ASR_CONCURRENCY = 2


@dataclass(frozen=True)
class DiarizationSegment:
    speaker: str
    start_ms: int
    end_ms: int


@dataclass(frozen=True)
class SpeechTurn:
    speaker: str
    start_ms: int
    end_ms: int


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--speakers-dir", type=Path, required=True)
    parser.add_argument("--max-speakers", type=int)
    parser.add_argument("--dotenv", type=Path, default=Path(".env"))
    args = parser.parse_args()

    load_dotenv(args.dotenv)
    openai_api_key = require_env("OPENAI_API_KEY")
    pyannote_api_key = require_env("PYANNOTE_API_KEY")

    audios_dir = args.run_dir / "audios"
    captures_dir = args.run_dir / "captures"
    reference_merged_path = args.run_dir / "merged.jsonl"
    args.output_dir.mkdir(parents=True, exist_ok=True)
    (args.output_dir / "captures").mkdir(exist_ok=True)
    (args.output_dir / "pyannote_raw").mkdir(exist_ok=True)

    known_embeddings = load_known_embeddings(args.speakers_dir)
    classifier = EncoderClassifier.from_hparams(source=SPEAKER_MODEL)

    all_absolute_segments: list[dict[str, Any]] = []
    capture_summaries: list[dict[str, Any]] = []
    for wav_path in sorted(audios_dir.glob("capture-*.wav")):
        capture_id = wav_path.stem
        print(f"processing {capture_id}", flush=True)
        capture_start_ms = read_capture_start_ms(captures_dir / f"{capture_id}.json")
        duration_ms = wav_duration_ms(wav_path)
        diarization_payload = run_pyannote(
            wav_path,
            pyannote_api_key,
            max_speakers=args.max_speakers,
            media_id=f"diarize-log-eval-{capture_id}-{int(time.time() * 1000)}",
        )
        (args.output_dir / "pyannote_raw" / f"{capture_id}.json").write_text(
            json.dumps(diarization_payload, ensure_ascii=False, indent=2) + "\n"
        )
        diarization_segments = parse_diarization_segments(diarization_payload)
        turns = build_speech_turns(diarization_segments, duration_ms)
        speaker_names = identify_speakers(
            wav_path=wav_path,
            turns=turns,
            classifier=classifier,
            known_embeddings=known_embeddings,
        )
        texts = transcribe_turns(wav_path, turns, openai_api_key)
        transcript_segments = []
        for turn, text in zip(turns, texts, strict=True):
            text = text.strip()
            if not text:
                continue
            speaker = speaker_names.get(turn.speaker, "UNKNOWN")
            segment = {
                "speaker": speaker,
                "start_ms": turn.start_ms,
                "end_ms": turn.end_ms,
                "text": text,
            }
            transcript_segments.append(segment)
            all_absolute_segments.append(
                {
                    "speaker": speaker,
                    "start_ms": capture_start_ms + turn.start_ms,
                    "end_ms": capture_start_ms + turn.end_ms,
                    "text": text,
                }
            )

        capture_output = {
            "capture_start_ms": capture_start_ms,
            "text": " ".join(segment["text"] for segment in transcript_segments),
            "segments": transcript_segments,
        }
        (args.output_dir / "captures" / f"{capture_id}.json").write_text(
            json.dumps(capture_output, ensure_ascii=False, indent=2) + "\n"
        )
        capture_summaries.append(
            {
                "capture": capture_id,
                "duration_ms": duration_ms,
                "diarization_segments": len(diarization_segments),
                "speech_turns": len(turns),
                "transcript_segments": len(transcript_segments),
                "speakers": sorted({segment["speaker"] for segment in transcript_segments}),
                "text_chars": sum(len(segment["text"]) for segment in transcript_segments),
            }
        )

    absolute_path = args.output_dir / "separated_absolute_segments.jsonl"
    with absolute_path.open("w") as handle:
        for segment in sorted(all_absolute_segments, key=lambda item: (item["start_ms"], item["end_ms"])):
            handle.write(json.dumps(segment, ensure_ascii=False) + "\n")

    reference_segments = read_jsonl(reference_merged_path)
    write_report(
        output_path=args.output_dir / "report.md",
        run_dir=args.run_dir,
        reference_path=reference_merged_path,
        separated_path=absolute_path,
        capture_summaries=capture_summaries,
        reference_segments=reference_segments,
        separated_segments=all_absolute_segments,
        max_speakers=args.max_speakers,
    )
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


def run_pyannote(
    wav_path: Path,
    api_key: str,
    max_speakers: int | None,
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

    deadline = time.time() + 10 * 60
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
            raise RuntimeError(f"pyannote job failed: {payload}")
        time.sleep(2)
    raise TimeoutError(f"pyannote job timed out: {job_id}")


def parse_diarization_segments(payload: dict[str, Any]) -> list[DiarizationSegment]:
    output = payload.get("output") or {}
    raw_segments = output.get("exclusiveDiarization") or output.get("diarization") or []
    return [
        DiarizationSegment(
            speaker=item["speaker"],
            start_ms=round(float(item["start"]) * 1000),
            end_ms=round(float(item["end"]) * 1000),
        )
        for item in raw_segments
        if float(item["end"]) > float(item["start"])
    ]


def build_speech_turns(
    segments: list[DiarizationSegment],
    capture_duration_ms: int,
) -> list[SpeechTurn]:
    turns: list[SpeechTurn] = []
    for segment in sorted(segments, key=lambda item: (item.start_ms, item.end_ms)):
        if segment.end_ms - segment.start_ms < MIN_TURN_MS:
            if turns and turns[-1].speaker == segment.speaker:
                turns[-1] = SpeechTurn(turns[-1].speaker, turns[-1].start_ms, max(turns[-1].end_ms, segment.end_ms))
            continue
        if (
            turns
            and turns[-1].speaker == segment.speaker
            and segment.start_ms - turns[-1].end_ms <= MERGE_GAP_MS
        ):
            turns[-1] = SpeechTurn(turns[-1].speaker, turns[-1].start_ms, max(turns[-1].end_ms, segment.end_ms))
        else:
            turns.append(SpeechTurn(segment.speaker, segment.start_ms, segment.end_ms))

    output: list[SpeechTurn] = []
    for turn in turns:
        start_ms = turn.start_ms
        while start_ms < turn.end_ms:
            end_ms = min(start_ms + MAX_TURN_MS, turn.end_ms)
            output.append(
                SpeechTurn(
                    turn.speaker,
                    max(0, start_ms - PADDING_MS),
                    min(capture_duration_ms, end_ms + PADDING_MS),
                )
            )
            start_ms = end_ms
    return output


def load_known_embeddings(speakers_dir: Path) -> dict[str, list[float]]:
    embeddings = {}
    for path in sorted(speakers_dir.glob("*.embedding.json")):
        payload = json.loads(path.read_text())
        embeddings[payload["speaker_name"]] = payload["vector"]
    return embeddings


def identify_speakers(
    wav_path: Path,
    turns: list[SpeechTurn],
    classifier: EncoderClassifier,
    known_embeddings: dict[str, list[float]],
) -> dict[str, str]:
    if not known_embeddings:
        return {}
    signal, sample_rate = load_mono_16k(wav_path)
    grouped: dict[str, list[tuple[list[float], int]]] = {}
    for turn in turns:
        start = round(turn.start_ms * 16)
        end = max(start + 1, round(turn.end_ms * 16))
        chunk = signal[:, start:end]
        with torch.no_grad():
            vector = classifier.encode_batch(chunk).squeeze().detach().cpu().tolist()
        grouped.setdefault(turn.speaker, []).append((vector, turn.end_ms - turn.start_ms))

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


def transcribe_turns(wav_path: Path, turns: list[SpeechTurn], api_key: str) -> list[str]:
    def transcribe(turn: SpeechTurn) -> str:
        with tempfile.NamedTemporaryFile(suffix=".wav") as clip_file:
            write_wav_clip(wav_path, clip_file.name, turn.start_ms, turn.end_ms)
            with open(clip_file.name, "rb") as audio_file:
                response = requests.post(
                    OPENAI_TRANSCRIPTIONS_ENDPOINT,
                    headers={"Authorization": f"Bearer {api_key}"},
                    data={
                        "model": OPENAI_ASR_MODEL,
                        "response_format": "json",
                        "language": "ja",
                    },
                    files={"file": ("turn.wav", audio_file, "audio/wav")},
                    timeout=300,
                )
        response.raise_for_status()
        return response.json()["text"]

    with concurrent.futures.ThreadPoolExecutor(max_workers=ASR_CONCURRENCY) as executor:
        return list(executor.map(transcribe, turns))


def load_mono_16k(wav_path: Path) -> tuple[torch.Tensor, int]:
    signal, sample_rate = torchaudio.load(wav_path)
    if signal.shape[0] > 1:
        signal = signal.mean(dim=0, keepdim=True)
    if sample_rate != 16000:
        signal = torchaudio.functional.resample(signal, sample_rate, 16000)
        sample_rate = 16000
    return signal, sample_rate


def write_wav_clip(source_path: Path, output_path: str, start_ms: int, end_ms: int) -> None:
    with wave.open(str(source_path), "rb") as reader:
        params = reader.getparams()
        frame_rate = reader.getframerate()
        start_frame = start_ms * frame_rate // 1000
        end_frame = end_ms * frame_rate // 1000
        reader.setpos(start_frame)
        frames = reader.readframes(max(0, end_frame - start_frame))
    with wave.open(output_path, "wb") as writer:
        writer.setparams(params)
        writer.writeframes(frames)


def wav_duration_ms(wav_path: Path) -> int:
    with wave.open(str(wav_path), "rb") as reader:
        return round(reader.getnframes() * 1000 / reader.getframerate())


def read_capture_start_ms(path: Path) -> int:
    return int(json.loads(path.read_text())["capture_start_ms"])


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def write_report(
    output_path: Path,
    run_dir: Path,
    reference_path: Path,
    separated_path: Path,
    capture_summaries: list[dict[str, Any]],
    reference_segments: list[dict[str, Any]],
    separated_segments: list[dict[str, Any]],
    max_speakers: int | None,
) -> None:
    reference_speakers = sorted({segment["speaker"] for segment in reference_segments})
    separated_speakers = sorted({segment["speaker"] for segment in separated_segments})
    lines = [
        "# separated pipeline evaluation",
        "",
        f"- run_dir: `{run_dir}`",
        f"- reference: `{reference_path}`",
        f"- separated_absolute_segments: `{separated_path}`",
        f"- pyannote_model: `{PYANNOTE_MODEL}`",
        f"- openai_asr_model: `{OPENAI_ASR_MODEL}`",
        f"- maxSpeakers: `{max_speakers if max_speakers is not None else 'unset'}`",
        "",
        "## summary",
        "",
        "| item | reference merged | separated raw absolute |",
        "| --- | ---: | ---: |",
        f"| segments | {len(reference_segments)} | {len(separated_segments)} |",
        f"| speakers | {len(reference_speakers)} | {len(separated_speakers)} |",
        f"| text chars | {sum(len(s['text']) for s in reference_segments)} | {sum(len(s['text']) for s in separated_segments)} |",
        "",
        f"- reference speakers: `{', '.join(reference_speakers)}`",
        f"- separated speakers: `{', '.join(separated_speakers)}`",
        "",
        "## per capture",
        "",
        "| capture | duration_ms | diarization_segments | speech_turns | transcript_segments | text_chars | speakers |",
        "| --- | ---: | ---: | ---: | ---: | ---: | --- |",
    ]
    for item in capture_summaries:
        lines.append(
            f"| {item['capture']} | {item['duration_ms']} | {item['diarization_segments']} | "
            f"{item['speech_turns']} | {item['transcript_segments']} | {item['text_chars']} | "
            f"`{', '.join(item['speakers'])}` |"
        )
    lines.extend(
        [
            "",
            "## note",
            "",
            "- `separated_absolute_segments.jsonl` is capture-start adjusted, but it is not passed through the Rust overlap merger.",
            "- Use the per-capture JSON and pyannote raw JSON when judging diarization and ASR quality.",
        ]
    )
    output_path.write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    raise SystemExit(main())
