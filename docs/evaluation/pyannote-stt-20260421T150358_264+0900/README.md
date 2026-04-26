# pyannote STT orchestration evaluation

対象:

- input audios: `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/audios`
- reference merged: `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/merged.jsonl`
- output dir: `docs/evaluation/pyannote-stt-20260421T150358_264+0900`
- baseline separated: `docs/evaluation/separated-20260421T150358_264+0900`

実行:

```bash
PYANNOTE_API_KEY=... .venv/bin/python scripts/evaluate_pyannote_stt_existing_wavs.py \
  --run-dir /Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900 \
  --speakers-dir /Users/kazuhideoki/diarize-log-storage/storage/speakers \
  --baseline-separated-dir docs/evaluation/separated-20260421T150358_264+0900 \
  --output-dir docs/evaluation/pyannote-stt-20260421T150358_264+0900
```

生成物:

- `captures/*.json`: capture ごとの pyannote STT transcript
- `pyannote_raw/*.json`: pyannote job result
- `pyannote_stt_absolute_segments.jsonl`: capture_start_ms を足した pyannote STT segment
- `report.md`: 既存 `merged.jsonl` および任意の separated baseline との件数・話者・文字数比較
