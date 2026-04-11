# Repository Guidelines

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
