# Repository Guidelines

## Architecture

- `src/adapters/`: 外部依存や I/O を伴う具象実装を分離する。
  - 例: `cpal` による録音、`reqwest` による API 呼び出し。
- `src/application/`: ユースケース単位の orchestration を置く。
- `src/application/ports/`: 外部境界とのやり取りを分離する。
  - `ports/` は境界定義に集中し、業務モデルは必要に応じて `domain/` を参照する。
- `src/application/usecase/`: コマンドの種類が増えた場合でも、CLI のサブコマンドごとではなく「何をするユースケースか」で分割する。
- `src/bootstrap/`: `src/main.rs` から委譲される composition root の実体を置く。
  - CLI 解釈後の設定解決、signal 初期化、use case への配線を担う。
- `src/config/`: 環境変数や `.env` ファイルの読み込みはここで一元管理する。
  - 設定の解決はエントリーポイントで一度だけ行い、解決済みの値を後続の処理へ渡す。
- `src/domain/`: 外部 I/O に依存しない業務モデルと業務ルールを置く。
- `src/cli.rs`: CLI の引数解釈と表示文言を置き、ユースケースの実行そのものは `application/` を呼ぶ。
- `src/lib.rs`: library crate の公開境界として扱う。
- `src/main.rs`: コマンド分岐や adapter の組み立てを行うエントリーポイントとして扱う。
  - `main.rs` は composition root として扱い、route/dispatch と設定解決に責務を限定する。
- `tests/`: 統合テストが必要になったら `tests/` を追加する。
- `build.rs`: 通常の業務ロジックではなくビルド実行環境の補助として扱う。
  - 現在は `ScreenCaptureKit` の Swift runtime をテスト/実行バイナリから解決するための `rpath` 付与だけを責務にする。
  - 手動実行は不要で、`cargo check` / `cargo build` / `cargo test` 実行時に Cargo から自動実行される前提で扱う。

## Documentation

- ドメイン用語の正しい意味と使い分け [docs/domain-words.md](docs/domain-words.md)
  - 参照タイミング: `domain/` の型・関数・doc comment、`application/` から見える公開用語、新しい概念名・別名・略語を追加または変更する前に参照する。
  - 変更タイミング: 中心概念の追加、既存語の意味やスコープ変更、用語統一の命名変更、禁止語や注意語の明文化をしたときに同じ変更内で更新する。

## Commands

- `cargo check`
- `cargo clippy -- -D warnings`
- `cargo fmt -- --check`
- `cargo test`

## Style

- `rustfmt` と `clippy` の警告は残さない。
- 不要な `Option` とフォールバックを避ける。必要な場合は理由を説明する。
- エラー型は明示的に定義し、安易に `anyhow` に逃がさない。
- 公開 API には必要に応じて doc comment を付ける。なるべく意図や背景(Why)を重視して記述

## Tests

- 単体テストは対象コードの近くに置き、統合テストが必要になったら `tests/` を追加する。
- テスト名は仕様が伝わる名前にし、`test_` / `_test` は使わない。
- すべてのテストに、日本語の doc comment で仕様を書く。
- public 関数や複雑な関数にも、背景や概要を説明する doc comment を加える

## Development Phase

- 自分自身向け、かつ開発中であるため、後方互換を保つ必要は無い
