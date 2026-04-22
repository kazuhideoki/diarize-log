# シンプル文字起こし調査: OpenAI `gpt-4o-transcribe` vs Deepgram `nova-3`

作成日: 2026-04-22

## 対象

- 目的: diarize なしのシンプル文字起こし精度を比較する。
- 入力データ: `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/audios` にある 11 件の WAV。
- あわせて、各ベンダーの既存 diarize 結果とも比較した。

## 実施条件

- OpenAI simple: model=`gpt-4o-transcribe`, language=`ja`, response_format=`json`
- Deepgram simple: model=`nova-3`, language=`ja`, diarize=`false`, utterances=`false`, punctuate=`true`, smart_format=`true`
- OpenAI diarize 参照: `/Users/kazuhideoki/diarize-log-storage/storage/runs/20260421T150358_264+0900/captures`
- Deepgram diarize 参照: `/Users/kazuhideoki/diarize-log-storage/deepgram-survey/20260422T102700+0900/raw`

## 結論

- 今回の 11 件では、**non-diarize の総合精度は OpenAI の方が高かった**。
- 差が大きかったのは `capture-000004`, `capture-000005`, `capture-000006`, `capture-000008`, `capture-000010` で、技術用語や会話の筋を OpenAI の方が保てていた。
- **Deepgram の simple と diarize は本文がほぼ同じ**だった。11 件中 10 件は完全一致、正規化後は 11 件すべて一致で、今回の条件では diarize の有無が本文品質にほとんど影響していない。
- **OpenAI diarize は本文品質では最も弱い場面が多かった**。一方で `gpt-4o-transcribe` にすると、その崩れが大きく改善した。
- 例外として、**`capture-000011` では OpenAI simple が不安定**で、再試行しても末尾の短い断片しか返らなかった。この 1 件だけは Deepgram の方が安全だった。

## capture ごとの判定

| capture | simple 勝者 | OpenAI simple と OpenAI diarize の比較 | Deepgram simple と Deepgram diarize の比較 | 所見 |
| --- | --- | --- | --- | --- |
| capture-000001 | OpenAI やや優勢 | improved | same | どちらも読めるが、OpenAI の方が少し自然。 |
| capture-000002 | 引き分け | improved | same | どちらも実用範囲で大差は小さい。 |
| capture-000003 | OpenAI やや優勢 | improved | same | デプロイ周辺の用語は OpenAI の方が読みやすい。 |
| capture-000004 | OpenAI 明確優勢 | improved_large | same | OpenAI simple で、OpenAI diarize の英語崩れが解消された。 |
| capture-000005 | OpenAI 明確優勢 | improved_large | same | OpenAI simple は会話内容を回復できたが、OpenAI diarize はかなり崩れていた。 |
| capture-000006 | OpenAI 明確優勢 | improved_large | same | OpenAI simple の方がかなり一貫している。 |
| capture-000007 | Deepgram やや優勢 | improved | same | Deepgram の方が冒頭の流れが自然。OpenAI は出だしが少しノイジー。 |
| capture-000008 | OpenAI 明確優勢 | improved_large | same | OpenAI simple は駐車スペース運用の会話として読める形まで回復した。 |
| capture-000009 | OpenAI 明確優勢 | improved | same | OpenAI simple の方が会話の筋が追いやすい。 |
| capture-000010 | OpenAI 明確優勢 | improved_large | same | endpoint / proxy / Firestore 周辺は OpenAI simple が最も良かった。 |
| capture-000011 | Deepgram 明確優勢 | worse_unstable | same | OpenAI simple は再試行しても短い末尾断片しか返らず不安定だった。 |

## 代表例

- `capture-000004`: OpenAI diarize は英語断片へ崩れたが、OpenAI simple は OCPP / DNS / Firestore の話として読める形になった。
- `capture-000008`: OpenAI diarize はほぼ読めないが、OpenAI simple は駐車スペースの営業時間調整の会話として読めた。
- `capture-000010`: OpenAI simple は endpoint / proxy / Firestore の文脈を、OpenAI diarize より大きく改善し、Deepgram simple よりもやや良かった。
- `capture-000011`: OpenAI simple は不安定で、`見れますね。` や `見れます。ありがとうございました。` のような短い断片しか返らない再試行があった。

## `capture-000011` の再試行メモ

- language=`ja`, response_format=`json` -> `はい、よろしくお願いします。ありがとうございました。`
- language 省略, response_format=`json` -> `見れますね。`
- language=`ja`, response_format=`text` -> `見れます。ありがとうございました。`

## 保存ファイル

- `openai_raw/*.json`: OpenAI non-diarize の生レスポンス
- `deepgram_raw/*.json`: Deepgram non-diarize の生レスポンス
- `comparison.json`: 11 件ぶんの本文、判定、簡易メトリクス
- `report.md`: この要約
