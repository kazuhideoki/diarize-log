# doctor --fix preparation

## 背景

`diarize-log` は macOS の録音権限、ScreenCaptureKit、OpenAI API、pyannote API、Python の speaker embedder など複数の環境依存を持つ。`doctor` はそれらの前提を診断し、`doctor --fix` は安全に機械化できる項目を初期セットアップ用途でも再利用できる形で修正する。

## 現在の足場

- `doctor --fix` を CLI として受け取れる。
- `doctor` は診断項目を `[ok]` / `[fail]` で出力する。
- `doctor --fix` は現時点では実修正を行わず、修正計画を出力する。
- 診断結果には自動修正可能、手動対応、修正不要の区別を持たせている。

## 診断候補

- config: `.env` と環境変数から必須設定を解決できるか。
- OpenAI API key: `OPENAI_API_KEY` が設定されているか。
- pyannote API key: pipeline の選択に関わらず `PYANNOTE_API_KEY` が設定されているか。legacy pipeline では実行時必須ではないが、doctor は環境棚卸しとして確認する。
- storage root: `DIARIZE_LOG_STORAGE_ROOT` が絶対パスで、保存先として使えるか。
- microphone capture: default input device と入力 config を読めるか。
- application capture: bundle ID の対象アプリが起動しており、ScreenCaptureKit の preflight を通るか。
- speaker embedder: separated pipeline で既知話者サンプルを使うとき、`scripts/speaker_embedder.py --check` が成功するか。

## 自動修正候補

- storage root が存在しない場合に directory を作成する。
- `.env` が存在しない場合に、secret を空欄にしたテンプレートを作成する。
- Python speaker embedder 用の venv 作成と依存インストールを行う。ただしインストール元と所要時間が大きいため、実行前に明示表示する。

## 手動対応として残す項目

- `OPENAI_API_KEY` と `PYANNOTE_API_KEY` の値入力。
- macOS のマイク権限、画面収録とシステムオーディオ録音権限の付与。
- application audio capture 対象アプリの起動。
- API の疎通確認で発生する認証、課金、レート制限の問題。

## 次の実装候補

1. `doctor` だけは config load 失敗を即終了せず、設定エラーも診断項目として出す。
2. `DIARIZE_LOG_STORAGE_ROOT` の存在確認を追加し、`doctor --fix` で `std::fs::create_dir_all` を実行する。
3. `.env` テンプレート生成を `doctor --fix` に追加する。
4. speaker embedder 依存のセットアップを専用スクリプトに切り出し、`doctor --fix` から呼べるようにする。
