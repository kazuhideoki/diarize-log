# separated pipeline 評価

- 対象 run: `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900`
- 比較対象: `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/merged.jsonl`
- separated 結果: `docs/evaluation/separated-20260421T150358_264+0900/separated_absolute_segments.jsonl`
- pyannote model: `precision-2`
- OpenAI ASR model: `gpt-4o-transcribe`
- maxSpeakers: `unset`

## 概要

| 項目 | 既存 merged | separated raw absolute |
| --- | ---: | ---: |
| segment 数 | 584 | 331 |
| 話者数 | 7 | 4 |
| 文字数 | 10106 | 4419 |

- 既存 merged の話者: `@, A, B, C, D, E, kazuhide_oki`
- separated の話者: `UNKNOWN, hibiki_tsuboi, kazuhide_oki, kosuke_takahara`

## capture ごとの集計

| capture | duration_ms | diarization segment 数 | speech turn 数 | transcript segment 数 | 文字数 | 話者 |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| capture-000001 | 146421 | 63 | 28 | 28 | 443 | `kazuhide_oki, kosuke_takahara` |
| capture-000002 | 145472 | 79 | 44 | 44 | 489 | `kazuhide_oki, kosuke_takahara` |
| capture-000003 | 144000 | 72 | 36 | 36 | 555 | `kazuhide_oki, kosuke_takahara` |
| capture-000004 | 144021 | 71 | 40 | 40 | 436 | `kazuhide_oki, kosuke_takahara` |
| capture-000005 | 144000 | 73 | 39 | 39 | 379 | `UNKNOWN, hibiki_tsuboi, kazuhide_oki, kosuke_takahara` |
| capture-000006 | 144000 | 62 | 23 | 23 | 437 | `hibiki_tsuboi, kazuhide_oki, kosuke_takahara` |
| capture-000007 | 146581 | 78 | 34 | 34 | 443 | `kazuhide_oki, kosuke_takahara` |
| capture-000008 | 144000 | 91 | 35 | 35 | 430 | `hibiki_tsuboi, kazuhide_oki, kosuke_takahara` |
| capture-000009 | 144128 | 74 | 29 | 29 | 310 | `UNKNOWN, hibiki_tsuboi, kazuhide_oki, kosuke_takahara` |
| capture-000010 | 144000 | 87 | 22 | 22 | 313 | `UNKNOWN, kosuke_takahara` |
| capture-000011 | 71893 | 30 | 1 | 1 | 184 | `UNKNOWN` |

## 注意

- `separated_absolute_segments.jsonl` は `capture_start_ms` を加算した比較用 JSONL であり、Rust の overlap merger は通していない。
- diarization と ASR の品質を見るときは、capture ごとの JSON と pyannote raw JSON も合わせて確認する。

## 初期所見

- `capture-000008` では期待していた改善が出ている。既存 capture 出力は冒頭が `KG18 shall we update` のような英語風の反復に崩れていたが、separated 出力では駐車スペースに関する日本語の会話として読める。
- `capture-000008` では `hibiki_tsuboi`, `kazuhide_oki`, `kosuke_takahara` の 3 話者を維持できている。
- separated 出力は既存 merged よりかなり短い。文字数は `4419` 対 `10106`。この評価ファイルは Rust の overlap merger を通していない影響もあるが、差が大きいため、ASR turn 構築と skip/空 text の扱いは追加確認が必要。
- `UNKNOWN` は `capture-000005`, `capture-000009`, `capture-000010`, `capture-000011` に残っている。話者同定の閾値、または capture 単位の embedding 集約方法の調整余地がある。
