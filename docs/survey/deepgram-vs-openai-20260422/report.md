# Deepgram と gpt-4o-transcribe-diarize の比較調査

作成日: 2026-04-22

## 目的

`Deepgram` の diarization 付き文字起こし結果が、既存の `gpt-4o-transcribe-diarize` と比べて実用上どちらが良いかを確認する。

今回の比較では、次の 2 点を分けて評価した。

- 文字起こし本文の自然さ
- diarization の妥当性

## 入力データ

- 元音声: `~/diarize-log-storage/storage/runs/20260421T150358_264+0900/audios`
- OpenAI の保存結果: `docs/survey/deepgram-vs-openai-20260422/openai_raw/`
- Deepgram の保存結果: `docs/survey/deepgram-vs-openai-20260422/deepgram_raw/`
- pyannote の参照結果: `docs/survey/deepgram-vs-openai-20260422/data/pyannote/`

比較対象の capture は `capture-000001.wav` から `capture-000011.wav` の 11 件。

## 前提

- 実際の会話参加者は最大 3 人
- 通常は 2 人中心で、後半に 3 人目が少し入る
- `capture-000002.wav` は実音声確認の結果 2 人
- `capture-000008.wav` は実音声確認と pyannote 結果の両方から 3 人が出ていると見てよい

## 実施条件

Deepgram には次の条件で送信した。

- model: `nova-3`
- language: `ja`
- diarize: `true`
- utterances: `true`
- punctuate: `true`

Deepgram の生レスポンスは `docs/survey/deepgram-vs-openai-20260422/deepgram_raw/` 配下の JSON に保存した。

## 結論

今回の 11 件では、単純に「どちらが全面的に上」とは言い切れない。

- 文字起こし本文は Deepgram の方が良い
- diarization は、OpenAI は過分割、Deepgram は過少分割の傾向が強い
- 2 人場面では Deepgram が自然に見えるケースが多い
- 3 人目が入る場面では OpenAI の方が話者追加を拾いやすい

実務上の見方としては次の整理が妥当だった。

- ASR 本文を重視するなら Deepgram が優勢
- 話者数の過大推定を嫌うなら Deepgram が優勢
- 3 人目の混入を取りこぼしたくないなら OpenAI が優勢
- diarization を後段で再クラスタリングする前提なら OpenAI の方が伸びしろがある

## 11 件の話者数比較

実際の会話参加者は最大 3 人なので、`4` 以上は過分割とみなせる。

| capture | OpenAI | Deepgram | 所見 |
| --- | ---: | ---: | --- |
| capture-000001 | 4 | 2 | OpenAI は過分割 |
| capture-000002 | 4 | 2 | OpenAI は過分割。Deepgram は 2 人に収まる |
| capture-000003 | 4 | 2 | OpenAI は過分割 |
| capture-000004 | 4 | 1 | OpenAI は過分割。Deepgram は統合しすぎ |
| capture-000005 | 5 | 2 | OpenAI は過分割 |
| capture-000006 | 6 | 2 | OpenAI は強い過分割 |
| capture-000007 | 3 | 3 | 両者とも話者数は自然 |
| capture-000008 | 6 | 1 | OpenAI は過分割。Deepgram は 3 人を潰している |
| capture-000009 | 4 | 2 | OpenAI は過分割 |
| capture-000010 | 5 | 2 | OpenAI は過分割 |
| capture-000011 | 2 | 1 | Deepgram は統合しすぎの可能性あり |

集計すると次の傾向だった。

- OpenAI は 11 件中 9 件で `4` 人以上を出した
- Deepgram は 11 件中 1 件だけ `3` 人を出した
- Deepgram は 11 件中 3 件で `1` 人に潰した

このため、OpenAI は `over-segmentation`、Deepgram は `under-segmentation` と整理するのが最も自然だった。

## ASR 本文の所見

本文品質は Deepgram が明確に良かった。

OpenAI は一部 capture で、日本語会話なのに英語やローマ字へ大きく崩れるケースがあった。

特に差が大きかったのは次の 3 件。

- `capture-000004`: OpenAI は `O_C_P_P_ proxy dev...` のように英語化して崩れたが、Deepgram は `ocppプロキシ...googleクラウド...firestore...` と日本語主体で維持できていた
- `capture-000008`: OpenAI は `KG18 shall we update` の反復に崩れたが、Deepgram は駐車スペースや営業時間の話として読めていた
- `capture-000010`: OpenAI は説明が英語寄りに崩れたが、Deepgram は `エンドポイント` `プロキシ` `Firestore` 周辺を日本語で維持できていた

したがって、文字起こし本文だけを見るなら Deepgram が優勢と判断した。

## diarization の詳細

### capture-000002

参照:

- OpenAI: `docs/survey/deepgram-vs-openai-20260422/openai_raw/capture-000002.json`
- Deepgram: `docs/survey/deepgram-vs-openai-20260422/deepgram_raw/capture-000002.json`
- pyannote: `docs/survey/deepgram-vs-openai-20260422/data/pyannote/capture-000002.json`

前提:

- 実音声確認の結果、この capture は 2 人
- pyannote も 2 話者

観察:

- OpenAI は `4` ラベルに分割した
- Deepgram は `2` ラベルに収めた
- OpenAI は話者切替を細かく拾うが、同じ人物を別ラベルへ裂いている
- Deepgram は切替がやや粗いが、2 人会話としては自然だった

判断:

- diarization の目的を「正しい人数で同一人物を安定して保つこと」と置くなら Deepgram の方が良い
- OpenAI は切替点の感度は高いが、2 人を 4 人にしてしまっているので過分割

補足:

- OpenAI は実質的に `A` と `C` が主ラベルで、`B` と `kazuhide_oki` は小さな破片だった
- Deepgram は `0` と `1` の 2 ラベルで収まっていた

### capture-000008

参照:

- OpenAI: `docs/survey/deepgram-vs-openai-20260422/openai_raw/capture-000008.json`
- Deepgram: `docs/survey/deepgram-vs-openai-20260422/deepgram_raw/capture-000008.json`
- pyannote: `docs/survey/deepgram-vs-openai-20260422/data/pyannote/capture-000008.json`

前提:

- 実音声確認と pyannote の両方から、3 人が出ていると見てよい

観察:

- OpenAI は `6` ラベルに分割した
- Deepgram は `1` ラベルに潰した
- OpenAI の 6 ラベルのうち、実質的に主ラベルは `A` `kazuhide_oki` `C` の 3 本だった
- `B` `D` `E` は相槌や断片に引っ張られたノイズ寄りだった
- Deepgram は 3 人会話を分離できず、1 人に統合してしまった

pyannote との対応をざっくり見ると、OpenAI 側は次の 3 本の主ラベルが成立していた。

- `A` -> `SPEAKER_02`
- `kazuhide_oki` -> `SPEAKER_00`
- `C` -> `SPEAKER_01`

判断:

- 3 人目が存在する場面では OpenAI の方が明確に良い
- Deepgram はここで 3 人目を見落としている
- ただし OpenAI もそのままではラベル数が多すぎるので、後段で `3` クラスタ程度へ畳む前提が望ましい

## 総合評価

今回のデータに対する実務的な評価は次の通り。

- 文字起こし本文を主目的にするなら Deepgram を優先したい
- diarization をそのまま使うなら、2 人中心の区間は Deepgram の方が扱いやすい
- 3 人目の検出を重視するなら OpenAI の方が見落としにくい
- ただし OpenAI はそのままだとラベルが増えすぎるため、後処理なしでは使いづらい

最終的には、用途ごとに次のように整理できる。

- `ASR の自然さ` を優先: Deepgram
- `話者数の暴走を避けたい` を優先: Deepgram
- `3 人目の存在検出` を優先: OpenAI
- `後段で再クラスタリング可能` なら: OpenAI の diarization を圧縮して使う余地がある

## 保存ファイル

- `openai_raw/*.json`: OpenAI diarize の保存結果
- `deepgram_raw/*.json`: Deepgram diarize の保存結果
- `data/pyannote/*.json`: pyannote の参照結果
- `report.md`: この要約

## 今後の調査候補

- 11 件すべてについて、短い断片ラベルを無視した再集計を作る
- OpenAI のラベルを後処理で `2-3` 話者へ畳んだ場合の改善幅を見る
- Deepgram の utterance をさらに細粒度化できる設定があるかを別途調べる
- 3 人目が出る区間だけを切り出して、話者検出の再比較を行う
