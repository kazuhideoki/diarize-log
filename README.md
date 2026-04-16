# diarize-log

音声を継続録音して話者分離文字起こしする CLI です。
OpenAI の gpt-4o-transcribe-diarize 利用が前提となっています。

## Usage

マイク入力を使う場合:

```bash
cargo run --
```

特定アプリケーションの音声を使う場合:

```bash
cargo run -- --audio-source application --application-bundle-id com.brave.Browser
```

`--audio-source application` は `ScreenCaptureKit` を使って対象アプリケーションの音声を取得するため、macOS の「画面収録とシステムオーディオ録音」権限が必要です。`Terminal` や `iTerm` など、この CLI を起動するアプリに権限付与してください。
