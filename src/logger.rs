use std::io::{self, Write};
use std::sync::{Arc, Mutex};

/// 実行時ログを識別する出力元です。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSource {
    Microphone,
    Application,
    System,
}

impl LogSource {
    /// ログへ出力する固定ラベルを返します。
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::Application => "application",
            Self::System => "system",
        }
    }
}

#[derive(Clone)]
pub struct Logger {
    debug_enabled: bool,
    sink: Arc<Mutex<Box<dyn Write + Send>>>,
    labels: Vec<String>,
}

impl Logger {
    /// 共通ログ設定を持つ root logger を生成します。
    pub fn new<W>(sink: W, debug_enabled: bool) -> Self
    where
        W: Write + Send + 'static,
    {
        Self {
            debug_enabled,
            sink: Arc::new(Mutex::new(Box::new(sink))),
            labels: Vec::new(),
        }
    }

    /// 標準エラー出力へ書く root logger を生成します。
    pub fn stderr(debug_enabled: bool) -> Self {
        Self::new(io::stderr(), debug_enabled)
    }

    /// source ラベルを追加した派生 logger を返します。
    pub fn with_source(&self, source: LogSource) -> Self {
        self.with_label(source.as_label())
    }

    /// component ラベルを追加した派生 logger を返します。
    pub fn with_component(&self, component: impl Into<String>) -> Self {
        self.with_label(component.into())
    }

    /// info レベルのログを出力します。
    pub fn info(&self, message: &str) -> io::Result<()> {
        self.write("info", message)
    }

    /// debug 有効時だけ debug ログを出力します。
    pub fn debug(&self, message: &str) -> io::Result<()> {
        if !self.debug_enabled {
            return Ok(());
        }

        self.write("debug", message)
    }

    fn with_label(&self, label: impl Into<String>) -> Self {
        let mut labels = self.labels.clone();
        labels.push(label.into());

        Self {
            debug_enabled: self.debug_enabled,
            sink: Arc::clone(&self.sink),
            labels,
        }
    }

    fn write(&self, level: &str, message: &str) -> io::Result<()> {
        let mut guard = self
            .sink
            .lock()
            .expect("logger sink mutex must not be poisoned");

        write!(guard, "[{level}]")?;
        for label in &self.labels {
            write!(guard, " [{label}]")?;
        }
        writeln!(guard, " {message}")
    }
}

#[cfg(test)]
mod tests {
    use super::{LogSource, Logger};
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct SharedBuffer {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl SharedBuffer {
        fn new() -> Self {
            Self::default()
        }

        fn contents(&self) -> String {
            String::from_utf8(self.bytes.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    /// info ログは level, source, component, message の順で共通書式にそろえる。
    fn writes_info_log_with_common_format() {
        let buffer = SharedBuffer::new();
        let logger = Logger::new(buffer.clone(), false)
            .with_source(LogSource::Microphone)
            .with_component("capture");

        logger.info("recording started").unwrap();

        assert_eq!(
            buffer.contents(),
            "[info] [microphone] [capture] recording started\n"
        );
    }

    #[test]
    /// debug 無効時は debug ログを出力しない。
    fn skips_debug_log_when_debug_is_disabled() {
        let buffer = SharedBuffer::new();
        let logger = Logger::new(buffer.clone(), false)
            .with_source(LogSource::Application)
            .with_component("transcriber");

        logger.debug("sending request").unwrap();

        assert_eq!(buffer.contents(), "");
    }

    #[test]
    /// 派生 logger は元の label を保ったまま追加の component を積み増せる。
    fn appends_labels_when_building_child_logger() {
        let buffer = SharedBuffer::new();
        let root = Logger::new(buffer.clone(), true).with_source(LogSource::System);
        let child = root.with_component("signal");

        root.info("root message").unwrap();
        child.debug("interrupt received").unwrap();

        assert_eq!(
            buffer.contents(),
            "[info] [system] root message\n[debug] [system] [signal] interrupt received\n"
        );
    }
}
