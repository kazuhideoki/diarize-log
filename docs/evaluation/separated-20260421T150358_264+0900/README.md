# separated pipeline evaluation

対象:

- input audios: `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/audios`
- reference merged: `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/merged.jsonl`
- output dir: `docs/evaluation/separated-20260421T150358_264+0900`

実行:

```bash
PYANNOTE_API_KEY=... .venv/bin/python scripts/evaluate_separated_existing_wavs.py \
  --run-dir /Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900 \
  --speakers-dir /Users/kazuhideoki/diarize-log-storage/storage/speakers \
  --output-dir docs/evaluation/separated-20260421T150358_264+0900
```

生成物:

- `captures/*.json`: capture ごとの separated transcript
- `pyannote_raw/*.json`: pyannote job result
- `separated_absolute_segments.jsonl`: capture_start_ms を足した separated segment
- `report.md`: 既存 `merged.jsonl` との件数・話者・文字数比較

注意:

- `separated_absolute_segments.jsonl` は capture start を足した比較用 JSONL で、Rust の overlap merger は通していない。
- 本評価 harness は既存 wav を評価するための専用 script で、通常 CLI の録音フローとは分けている。
