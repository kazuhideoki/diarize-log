# `pyannote` OSS 話者分離の技術検証

作成日: 2026-04-22

## 対象

- 目的: `pyannote` OSS の話者分離を、既存の pyannote 有料版結果と比較する。
- 入力データ:
  - `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/audios/capture-000002.wav`
  - `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/audios/capture-000008.wav`
- 比較対象:
  - `docs/survey/deepgram-vs-openai-20260422/data/pyannote/capture-000002.json`
  - `docs/survey/deepgram-vs-openai-20260422/data/pyannote/capture-000008.json`

## 実施条件

- Python 環境: ローカル `.venv`
- ライブラリ: `pyannote.audio==4.0.4`
- 実行した OSS モデル: `pyannote-community/speaker-diarization-community-1`
- 実行方式: CPU ローカル推論

## 注意

- 公式の `pyannote/speaker-diarization-community-1` は Hugging Face の gated model で、この環境にはトークンが無いため直接は実行できなかった。
- 今回の実測は、同系モデルを配布している `pyannote-community/speaker-diarization-community-1` を使った。
- そのため、これは **公式 gated 配布をそのまま叩いた結果ではなく、community mirror 経由の OSS 実行結果** である。

## 結論

- `capture-000002` はかなり近かった。話者数は有料版と同じ 2 人で、speech activity の union IoU も `0.8816` だった。
- `capture-000008` は default 推定だと 2 話者に潰れ、有料版の 3 話者構成を再現できなかった。
- ただし `capture-000008` は `num_speakers=3` を与えると、話者割り当ての一致度がかなり改善した。
- この 2 件だけを見る限り、**OSS 側の弱点は speech activity よりも話者数推定とクラスタリング側に出ている**。

## default 実行の比較

| capture | OSS 話者数 | 有料版話者数 | OSS segment 数 | 有料版 segment 数 | speech union IoU | 所見 |
| --- | --- | --- | --- | --- | --- | --- |
| capture-000002 | 2 | 2 | 77 | 88 | 0.8816 | 近い。OSS の方がやや粗くまとまる。 |
| capture-000008 | 2 | 3 | 68 | 100 | 0.8583 | 3 人目を落として 2 話者へ寄った。 |

## `capture-000008` の追加確認

- default の `best_one_to_one_speaker_overlap_seconds`: `61.910`
- `num_speakers=3` 指定時の `best_one_to_one_speaker_overlap_seconds`: `94.613`
- `num_speakers=3` では話者数は有料版と同じ 3 になり、誰がどこを話しているかの対応づけはかなり改善した。
- 一方で speech activity の union IoU は `0.8583` のままで、改善したのは主に話者クラスタリング側だった。

## 所見

- `capture-000002` のような 2 人会話では、OSS でも有料版にかなり近い結果が出る。
- `capture-000008` のように 3 話者目が短く混ざるケースでは、default の話者数推定は弱い。
- 実運用で話者数の上限や期待人数を事前に与えられる場面なら、OSS 側の実用性は上がる可能性が高い。
- 一方で完全自動で人数不明のまま流す場合は、話者数推定の揺れを前提に評価する必要がある。

## 保存ファイル

- `raw/capture-000002.json`: OSS default 実行結果
- `raw/capture-000008.json`: OSS default 実行結果
- `raw/capture-000008.num_speakers_3.json`: `num_speakers=3` の追加実験結果
- `comparison.json`: default 実行の集計結果
- `report.md`: この要約
