# diarize-log

`diarize-log` は、macOS 上でマイク音声や特定アプリケーションの音声を継続録音し、OpenAI `gpt-4o-transcribe-diarize` で話者分離文字起こししてローカル保存する CLI です。

## できること

- マイク音声の継続録音と話者分離文字起こし
- 特定アプリケーションの音声だけを bundle ID 指定で取得して文字起こし
- マイク音声とアプリ音声を同時に録り、時系列で統合した transcript を生成
- 既知話者のサンプル音声を登録し、文字起こし時に添付して話者推定を補助
- 無音状態を検知して自然な切れ目で文字起こしを区切り、長い録音でも扱いやすくする
- 録音した WAV、capture ごとの transcript、統合済み transcript、metadata をローカルへ保存

## 前提

- OpenAI `gpt-4o-transcribe-diarize` を利用します
- 既定の transcription language は `ja` です
- `DIARIZE_LOG_TRANSCRIPTION_LANGUAGE` 環境変数で `ja`、`en`、`auto` に上書きできます

## Permissions

`--audio-source application` は `ScreenCaptureKit` を使って対象アプリケーションの音声を取得するため、macOS の「画面収録とシステムオーディオ録音」権限が必要です。`Terminal` や `iTerm` など、この CLI を起動するアプリに権限付与してください。

## Usage

### speaker

```bash
cargo run -- speaker add suzuki /absolute/path/to/source.wav 30 # 話者サンプルを登録。指定ファイルの開始30秒地点から
cargo run -- speaker list # 話者サンプル一覧
cargo run -- speaker remove suzuki # サンプル削除
```

### 文字起こし

```bash
cargo run -- # マイク入力
cargo run -- -s suzuki -s sato # マイク入力で、登録済みの話者サンプルを文字起こしに添付する
cargo run -- --audio-source application --application-bundle-id com.brave.Browser # 特定アプリケーションの音声
cargo run -- --audio-source mixed --application-bundle-id com.brave.Browser --microphone-speaker me -s suzuki # アプリ音声とマイク音声を混ぜ、マイク側の話者名を固定する場合
```

- `mixed` では `--microphone-speaker` でマイク側の話者名を指定し、`-s` / `--speaker-sample` でアプリ側の既知話者サンプルを添付できます
- `-s` / `--speaker-sample` は最大 4 回まで指定できます。
