# SpeechBrain ECAPA-TDNN による話者同定調査

作成日: 2026-04-22

## 目的

`docs/backlog/transcription-pipeline-direction-20260422.md` で採用候補にした `SpeechBrain ECAPA-TDNN` を使い、`pyannote` の話者分離結果へ既知話者名を付与できるかを確認する。

今回の調査では、次の 2 点を確認した。

- `pyannote` の各発話区間について、3 つの話者音源との近似度を取得できるか
- capture 単位で `SPEAKER_00` などの匿名ラベルを `oki` `takahara` `tsuboi` へ安定して対応付けられるか

## 入力データ

- 元音声
  - `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/audios/capture-000002.wav`
  - `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/audios/capture-000008.wav`
- `pyannote` 話者分離
  - `docs/survey/deepgram-vs-openai-20260422/data/pyannote/capture-000002.json`
  - `docs/survey/deepgram-vs-openai-20260422/data/pyannote/capture-000008.json`
- 既知話者サンプル
  - `oki`: `/Users/kazuhideoki/diarize-log-storage/storage/speakers/kazuhide_oki.wav`
  - `takahara`: `/Users/kazuhideoki/diarize-log-storage/storage/speakers/kosuke_takahara.wav`
  - `tsuboi`: `/Users/kazuhideoki/diarize-log-storage/storage/speakers/hibiki_tsuboi.wav`

## 実施条件

- 埋め込みモデル: `speechbrain/spkrec-ecapa-voxceleb`
- 実行環境: `./.venv-survey`
- 音声は mono 化して `16kHz` に resample してから埋め込み抽出
- 各 `pyannote` セグメントごとに 3 話者とのコサイン類似度を算出
- 名前付けはセグメント単位の top1 ではなく、`pyannote` 話者ラベル単位で duration-weighted 平均埋め込みを作って決定
- `UNKNOWN` 判定ルール
  - top score `>= 0.50`
  - top1 と top2 の差 `>= 0.15`
- 極短セグメントでも埋め込みが計算できるよう、分析時だけ最短 `0.8` 秒まで前後へ広げ、元の start/end は保持

## 結論

- `capture-000002` は `2` 話者として安定して対応付けできた。
  - `SPEAKER_00 -> oki`
  - `SPEAKER_01 -> takahara`
- `capture-000008` は `3` 話者として安定して対応付けできた。
  - `SPEAKER_00 -> oki`
  - `SPEAKER_01 -> takahara`
  - `SPEAKER_02 -> tsuboi`
- クラスタ単位の best score は `0.611678` から `0.715708`、margin は `0.219999` から `0.353221` で、今回の閾値をすべて上回った。
- 参照話者同士の相互類似度は最大でも `0.363146` だったため、今回の capture では既知 3 話者の分離は十分に取れている。

## 話者対応の詳細

### 参照話者どうしの類似度

| speaker | oki | takahara | tsuboi |
| --- | ---: | ---: | ---: |
| oki | 1.000000 | 0.259471 | 0.363146 |
| takahara | 0.259471 | 1.000000 | 0.357558 |
| tsuboi | 0.363146 | 0.357558 | 1.000000 |

### capture-000002

`pyannote` は 88 セグメント、2 話者だった。

| pyannote | assigned | best score | second | second score | margin | total duration |
| --- | --- | ---: | --- | ---: | ---: | ---: |
| `SPEAKER_00` | `oki` | 0.667036 | `tsuboi` | 0.423738 | 0.243298 | 61.86s |
| `SPEAKER_01` | `takahara` | 0.715708 | `tsuboi` | 0.362487 | 0.353221 | 46.48s |

### capture-000008

`pyannote` は 100 セグメント、3 話者だった。

| pyannote | assigned | best score | second | second score | margin | total duration |
| --- | --- | ---: | --- | ---: | ---: | ---: |
| `SPEAKER_00` | `oki` | 0.625461 | `tsuboi` | 0.405462 | 0.219999 | 29.38s |
| `SPEAKER_01` | `takahara` | 0.611678 | `tsuboi` | 0.344542 | 0.267136 | 43.22s |
| `SPEAKER_02` | `tsuboi` | 0.671467 | `oki` | 0.355076 | 0.316391 | 36.64s |

## セグメント単位の所見

セグメント単位で単純に top1 を採ると、短い相槌や重なり発話で揺れやすかった。

- `capture-000002`: 88 セグメント中 27 件で、セグメント単体 top1 とクラスタ割当が不一致
- `capture-000008`: 100 セグメント中 46 件で、セグメント単体 top1 とクラスタ割当が不一致

ただし、長い発話ではかなり安定した。

- `capture-000002`: 1.0 秒以上のセグメントでは一致率 `0.938`、2.0 秒以上では `1.000`
- `capture-000008`: 1.0 秒以上のセグメントでは一致率 `0.780`、2.0 秒以上では `0.941`

このため、`docs/backlog/transcription-pipeline-direction-20260422.md` にある通り、**1 発話ごとの直接命名より、まとまった区間またはクラスタ単位で命名する方が安定する**という結論で良い。

## 保存ファイル

- `docs/survey/speechbrain-ecapa-tdnn-20260422/analyze_identification.py`
  - 今回の再現用スクリプト
- `docs/survey/speechbrain-ecapa-tdnn-20260422/data/summary.json`
  - 参照話者相互の類似度と capture ごとの集約結果
- `docs/survey/speechbrain-ecapa-tdnn-20260422/data/capture-000002.json`
- `docs/survey/speechbrain-ecapa-tdnn-20260422/data/capture-000008.json`
  - 各セグメントの `scores.{oki,takahara,tsuboi}` と、`assigned_speaker` を含む JSON
- `docs/survey/speechbrain-ecapa-tdnn-20260422/data/capture-000002.csv`
- `docs/survey/speechbrain-ecapa-tdnn-20260422/data/capture-000008.csv`
  - セグメントごとの類似度を一覧しやすい CSV

## 判断

今回の 2 capture に限れば、`SpeechBrain ECAPA-TDNN` で `pyannote` の匿名話者ラベルへ人名を載せることは成立した。

特に、

- 2 人 capture では `oki` と `takahara` を明確に分離できた
- 3 人 capture では `tsuboi` を追加しても崩れなかった
- cluster 単位の score が参照話者同士の近さを十分に上回った

ので、当面の方針としては **`pyannote OSS + ECAPA-TDNN` で diarization と speaker identification を分離する構成は妥当** と判断してよい。
