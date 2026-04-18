use std::io;

/// ユースケースが診断ログを記録するための出力境界です。
pub trait Logger {
    /// 通常の進行状況を表す info ログを記録します。
    fn info(&self, message: &str) -> io::Result<()>;

    /// 詳細診断向けの debug ログを記録します。
    fn debug(&self, message: &str) -> io::Result<()>;
}
