#!/usr/bin/env python3
"""Generate SpeechBrain ECAPA-TDNN speaker embeddings from JSON stdin."""

import base64
import json
import sys
import tempfile
from pathlib import Path

import torchaudio
from speechbrain.inference.speaker import EncoderClassifier

MODEL_ID = "speechbrain/spkrec-ecapa-voxceleb"


def main() -> int:
    request = json.load(sys.stdin)
    wav_bytes = base64.b64decode(request["wav_base64"])

    with tempfile.TemporaryDirectory() as tmp_dir:
        wav_path = Path(tmp_dir) / "speaker.wav"
        wav_path.write_bytes(wav_bytes)
        signal, sample_rate = torchaudio.load(wav_path)

    if signal.shape[0] > 1:
        signal = signal.mean(dim=0, keepdim=True)
    if sample_rate != 16000:
        signal = torchaudio.functional.resample(signal, sample_rate, 16000)

    classifier = EncoderClassifier.from_hparams(source=MODEL_ID)
    embedding = classifier.encode_batch(signal).squeeze().detach().cpu().tolist()
    json.dump(
        {
            "speaker_name": request["speaker_name"],
            "model": MODEL_ID,
            "vector": embedding,
        },
        sys.stdout,
    )
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
