# Repository Guidelines

## Architecture

- config.rs: 環境変数や .env ファイルの読み込みはここで一元管理する。
  - 設定の解決はエントリーポイントで一度だけ行い、解決済みの値を後続の処理へ渡す。
- domain/: 外部 I/O に依存しない業務モデルと業務ルールを置く。
  - `domain/` は `ports/` や `application/` に依存しない。
- application/: ユースケース単位の orchestration を置く。
  - CLI や外部 I/O の詳細は持ち込まず、`domain/` と `ports/` 越しに必要な境界だけへ依存する。
  - コマンドの種類が増えた場合でも、CLI のサブコマンドごとではなく「何をするユースケースか」で分割する。
- 外部境界とのやり取りは `ports/` に分離する。
  - `ports/` は application が必要とする入出力境界として扱う。
  - `ports/` は境界定義に集中し、業務モデルは必要に応じて `domain/` を参照する。
- 外部依存や I/O を伴う具象実装は `adapters/` に分離する。
  - 例: `cpal` による録音、`reqwest` による API 呼び出し。
- CLI の引数解釈と表示文言は `cli.rs` に置き、ユースケースの実行そのものは `application/` を呼ぶ。
- コマンド分岐や adapter の組み立てはエントリーポイントで行う。
  - `main.rs` は composition root として扱い、route/dispatch と設定解決に責務を限定する。
- `build.rs` は通常の業務ロジックではなくビルド実行環境の補助として扱う。
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
- 公開 API には必要に応じて doc comment を付ける。

## Tests

- 単体テストは対象コードの近くに置き、統合テストが必要になったら `tests/` を追加する。
- テスト名は仕様が伝わる名前にし、`test_` / `_test` は使わない。
- すべてのテストに、日本語の doc comment で仕様を書く。
- public 関数や複雑な関数にも、背景や概要を説明する doc comment を加える

## Development Phase

- 自分自身向け、かつ開発中であるため、後方互換を保つ必要は無い
