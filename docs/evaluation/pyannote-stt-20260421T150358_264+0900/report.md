# pyannote STT orchestration evaluation

- run_dir: `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900`
- reference: `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/merged.jsonl`
- pyannote_stt_absolute_segments: `docs/evaluation/pyannote-stt-20260421T150358_264+0900/pyannote_stt_absolute_segments.jsonl`
- baseline_separated_absolute_segments: `docs/evaluation/separated-20260421T150358_264+0900/separated_absolute_segments.jsonl`
- pyannote_model: `precision-2`
- pyannote_transcription_model: `faster-whisper-large-v3-turbo`
- maxSpeakers: `unset`
- speaker_identification: `enabled`

## summary

| item | reference merged | separated baseline | pyannote STT |
| --- | ---: | ---: | ---: |
| segments | 584 | 331 | 536 |
| speakers | 7 | 4 | 5 |
| text chars | 10106 | 4419 | 9502 |

- reference speakers: `@, A, B, C, D, E, kazuhide_oki`
- separated baseline speakers: `UNKNOWN, hibiki_tsuboi, kazuhide_oki, kosuke_takahara`
- pyannote STT speakers: `SPEAKER_01, UNKNOWN, hibiki_tsuboi, kazuhide_oki, kosuke_takahara`

## per capture

| capture | duration_ms | diarization_segments | transcript_segments | text_chars | speakers |
| --- | ---: | ---: | ---: | ---: | --- |
| capture-000001 | 146421 | 63 | 39 | 859 | `kazuhide_oki, kosuke_takahara` |
| capture-000002 | 145472 | 79 | 57 | 908 | `kazuhide_oki, kosuke_takahara` |
| capture-000003 | 144000 | 72 | 59 | 1086 | `kazuhide_oki, kosuke_takahara` |
| capture-000004 | 144021 | 71 | 54 | 732 | `kazuhide_oki, kosuke_takahara` |
| capture-000005 | 144000 | 73 | 54 | 806 | `UNKNOWN, hibiki_tsuboi, kazuhide_oki, kosuke_takahara` |
| capture-000006 | 144000 | 62 | 39 | 971 | `hibiki_tsuboi, kazuhide_oki, kosuke_takahara` |
| capture-000007 | 146581 | 78 | 54 | 1078 | `kazuhide_oki, kosuke_takahara` |
| capture-000008 | 144000 | 91 | 54 | 888 | `hibiki_tsuboi, kazuhide_oki, kosuke_takahara` |
| capture-000009 | 144128 | 74 | 52 | 856 | `UNKNOWN, hibiki_tsuboi, kazuhide_oki, kosuke_takahara` |
| capture-000010 | 144000 | 87 | 50 | 960 | `UNKNOWN, kosuke_takahara` |
| capture-000011 | 71893 | 30 | 24 | 358 | `SPEAKER_01, UNKNOWN` |

## note

- `pyannote_stt_absolute_segments.jsonl` is capture-start adjusted, but it is not passed through the Rust overlap merger.
- pyannote STT uses `turnLevelTranscription`; word-level output remains available in `pyannote_raw/*.json`.
- If `--speakers-dir` is provided, anonymous pyannote speaker labels are mapped with the same SpeechBrain embedding rule used by the separated evaluation harness.
