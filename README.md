# diarize-log

音声を継続録音して話者分離文字起こしする CLI です。
OpenAI の gpt-4o-transcribe-diarize 利用が前提となっています。
既定では transcription language に `ja` を指定し、`DIARIZE_LOG_TRANSCRIPTION_LANGUAGE` 環境変数で `ja`、`en`、`auto` に上書きできます。`auto` を指定した場合は API へ `language` を送らず、自動判定に委ねます。

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
