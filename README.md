# diarize-log

`diarize-log` は、macOS 上でマイク音声や特定アプリケーションの音声を継続録音し、OpenAI の文字起こし API で transcript をローカル保存する CLI です。

## できること

- マイク音声の継続録音と文字起こし
- 特定アプリケーションの音声だけを bundle ID 指定で取得して文字起こし
- マイク音声とアプリ音声を同時に録り、時系列で統合した transcript を生成
- 既知話者のサンプル音声を登録し、文字起こし時に添付して話者推定を補助
- 無音状態を検知して自然な切れ目で文字起こしを区切り、長い録音でも扱いやすくする
- 録音した WAV、capture ごとの transcript、統合済み transcript、metadata をローカルへ保存

## 前提

- 通常は OpenAI `gpt-4o-transcribe-diarize` を利用します
- `--microphone-only-speaker` / `--application-only-speaker` で単一話者を明示した source は、話者分離せず `gpt-4o-transcribe` で文字起こしします
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
cargo run -- --microphone-only-speaker me # マイク入力が単一話者だと確定している場合
cargo run -- --audio-source application --application-bundle-id com.brave.Browser --application-only-speaker guest # アプリ音声が単一話者だと確定している場合
cargo run -- --audio-source mixed --application-bundle-id com.brave.Browser --microphone-only-speaker me -s suzuki # アプリ音声とマイク音声を混ぜ、マイク側が単一話者だと確定している場合
cargo run -- --audio-source mixed --application-bundle-id com.brave.Browser --microphone-only-speaker me --application-only-speaker guest # mixed の両 source がそれぞれ単一話者だと確定している場合
```

- `--microphone-only-speaker` / `--application-only-speaker` は、対象 source がその 1 人だけだと確定している場合に指定します。指定された source は話者分離せず、後から指定名を機械的に付与します。
- `mixed` では `--microphone-only-speaker` が必須です。`-s` / `--speaker-sample` でアプリ側の既知話者サンプルを添付できます。
- `-s` / `--speaker-sample` は最大 4 回まで指定できます。
