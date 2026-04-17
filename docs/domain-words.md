# Domain Words

このドキュメントは、このリポジトリで扱う中心的なドメイン語を整理するためのものです。
目的は、同じ概念に対して同じ語を使い、別の概念に同じ語を流用しないようにすることです。

## Core Words

| Word | Meaning | Notes |
| --- | --- | --- |
| `capture` | 連続録音から切り出して、個別に文字起こしと merge にかける処理単位。 | transcript や segment の別名としては使わない。 |
| `capture_range` | 1 capture が担当する時間区間。 | 時間区間を表す語は `range` に寄せる。 |
| `capture_boundary` | 1 capture をどこで確定するかを表す境界。 | `silence` / `max_duration` / `interrupted` のように確定理由を伴う。 |
| `silence_request_policy` | 無音を使って request 境界を作るための業務ルール。 | 無音閾値、必要無音長、tail 側での短縮ルールを含む。 |
| `speaker sample` | 既知話者として文字起こし API に添付する参照音声。 | speaker そのものの別名ではない。話者識別のための音声サンプルを指す。 |
| `diarized_transcript` | 話者分離済みの文字起こし結果全体。 | text 全体と segment 列を含む。 |
| `transcript_segment` | `diarized_transcript` の中にある、話者単位の発話区間。 | `segment` はまずこの意味で使う。 |
| `merged_transcript_segment` | 複数 capture を突き合わせたあとに残る、絶対時刻ベースの segment。 | merge 後の最終出力側の segment を指す。 |
| `overlap_range` | 隣接する capture 間で共有している時間帯。重複判定の対象となる区間。 | `segment` の別名ではない。共有時間帯を表す。 |
| `merge_audit_entry` | 1 回の overlap 判定について保存する監査ログ。 | 判定対象、判定結果、採用理由や棄却理由を残す。 |

## Notes

- `segment` は発話区間の意味に限定する。
- `capture` は録音から切り出した処理単位を指し、文字起こし結果そのものを指さない。
- `capture_boundary` は capture を確定する理由つきの境界であり、単なる時間区間ではない。
- `overlap_range` は capture 間の共有時間帯を指し、発話単位ではない。
- `range` は start/end を持つ時間区間に使う。
- `window` は merge ドメイン語として使わない。
