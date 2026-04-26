# normalized transcript comparison

- run_dir: `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900`
- separated_dir: `docs/evaluation/separated-20260421T150358_264+0900`
- pyannote_stt_dir: `docs/evaluation/pyannote-stt-20260421T150358_264+0900`
- normalization: `NFKC`, collapse whitespace, remove spaces between Japanese characters, remove spaces around Japanese punctuation
- note: `reference merged` is an existing pipeline output, not a human ground truth. Similarity is a rough regression/comparison signal.

## summary

| item | reference merged | separated baseline | pyannote STT |
| --- | ---: | ---: | ---: |
| normalized chars | 9989 | 4438 | 6316 |

| comparison | similarity |
| --- | ---: |
| separated vs reference | 0.251 |
| pyannote STT vs reference | 0.323 |
| pyannote STT vs separated | 0.473 |

## initial findings

- pyannote STT keeps 63.2% of the normalized reference text volume, while separated baseline keeps 44.4%.
- pyannote STT is closer to reference merged by normalized edit similarity: `0.323` vs separated baseline `0.251`.
- pyannote STT still leaves tokenization artifacts around numbers and ASCII terms, so a downstream formatter would be needed before using the transcript as user-facing text.
- `capture-000011` is the main exception in this comparison; separated baseline is closer to reference merged than pyannote STT on the rough similarity metric.

## per capture

| capture | reference chars | separated chars | pyannote STT chars | separated vs ref | pyannote vs ref | pyannote vs separated |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| capture-000001 | 745 | 445 | 557 | 0.393 | 0.502 | 0.522 |
| capture-000002 | 677 | 491 | 606 | 0.492 | 0.545 | 0.513 |
| capture-000003 | 701 | 556 | 729 | 0.575 | 0.645 | 0.578 |
| capture-000004 | 725 | 439 | 549 | 0.265 | 0.346 | 0.430 |
| capture-000005 | 1379 | 382 | 544 | 0.086 | 0.126 | 0.436 |
| capture-000006 | 1582 | 438 | 604 | 0.164 | 0.235 | 0.513 |
| capture-000007 | 892 | 445 | 689 | 0.337 | 0.500 | 0.456 |
| capture-000008 | 1167 | 430 | 594 | 0.195 | 0.266 | 0.542 |
| capture-000009 | 740 | 313 | 562 | 0.269 | 0.430 | 0.368 |
| capture-000010 | 1296 | 315 | 636 | 0.135 | 0.183 | 0.358 |
| capture-000011 | 85 | 184 | 246 | 0.310 | 0.228 | 0.435 |
