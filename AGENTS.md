# Repository Guidelines

## Architecture

- config.rs: 環境変数や .env ファイルの読み込みはここで一元管理する。
  - 設定の解決はエントリーポイントで一度だけ行い、解決済みの値を後続の処理へ渡す。
- application/: ユースケース単位の orchestration を置く。
  - CLI や外部 I/O の詳細は持ち込まず、`ports/` 越しに必要な境界だけへ依存する。
  - コマンドの種類が増えた場合でも、CLI のサブコマンドごとではなく「何をするユースケースか」で分割する。
- 外部境界とのやり取りは `ports/` に分離する。
  - `ports/` は application が必要とする入出力境界として扱う。
- 外部依存や I/O を伴う具象実装は `adapters/` に分離する。
  - 例: `cpal` による録音、`reqwest` による API 呼び出し。
- CLI の引数解釈と表示文言は `cli.rs` に置き、ユースケースの実行そのものは `application/` を呼ぶ。
- コマンド分岐や adapter の組み立てはエントリーポイントで行う。
  - `main.rs` は composition root として扱い、route/dispatch と設定解決に責務を限定する。

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
