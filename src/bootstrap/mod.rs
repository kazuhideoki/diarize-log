mod commands;
mod signal;

use diarize_log::config::{Config, DEFAULT_DOTENV_PATH};
use diarize_log::{CliAction, LineLogger, LogSource, parse_cli_args};
use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

/// バイナリの composition root です。
///
/// CLI 解釈、設定解決、signal 初期化、command 配線だけを担い、
/// 実処理本体は `commands` へ委譲します。
pub(crate) fn run<I>(args: I) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    let action = match parse_cli_args(args) {
        Ok(action) => action,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    if let CliAction::PrintOutput(message) = action {
        print!("{message}");
        return ExitCode::SUCCESS;
    }

    let runtime_config = match Config::from_dotenv_path(Path::new(DEFAULT_DOTENV_PATH)) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let root_logger = LineLogger::stderr(runtime_config.debug_enabled);

    match action {
        CliAction::Run {
            speaker_samples,
            audio_source,
        } => {
            let system_logger = root_logger
                .with_source(LogSource::System)
                .with_component("signal");
            let interrupt_state = match signal::SignalInterruptState::install(system_logger) {
                Ok(state) => state,
                Err(error) => {
                    eprintln!("{error}");
                    return ExitCode::FAILURE;
                }
            };
            commands::run_capture_action(
                &runtime_config,
                &speaker_samples,
                audio_source,
                interrupt_state,
                &root_logger,
            )
        }
        CliAction::Speaker(command) => commands::run_speaker_action(&runtime_config, command),
        CliAction::PrintOutput(_) => unreachable!("print output is handled before config load"),
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    use std::ffi::OsString;
    use std::process::ExitCode;
    use std::sync::{Mutex, OnceLock};

    #[test]
    /// `--help` は config 解決前に成功終了する。
    fn returns_success_for_help_output() {
        let exit_code = run([OsString::from("diarize-log"), OsString::from("--help")]);

        assert_eq!(exit_code, ExitCode::SUCCESS);
    }

    #[test]
    /// `--version` は config 解決前に成功終了する。
    fn returns_success_for_version_output() {
        let exit_code = run([OsString::from("diarize-log"), OsString::from("--version")]);

        assert_eq!(exit_code, ExitCode::SUCCESS);
    }

    #[test]
    /// 不正な引数は config 解決前に失敗終了する。
    fn returns_failure_for_invalid_cli_argument() {
        let exit_code = run([OsString::from("diarize-log"), OsString::from("--verbose")]);

        assert_eq!(exit_code, ExitCode::FAILURE);
    }

    #[test]
    /// 通常実行経路で config 解決に失敗した場合は失敗終了する。
    fn returns_failure_for_run_action_when_config_is_invalid() {
        let _guard = env_lock().lock().unwrap();
        let original = std::env::var_os("OPENAI_API_KEY");
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "");
        }

        let exit_code = run([OsString::from("diarize-log")]);

        restore_env_var("OPENAI_API_KEY", original);
        assert_eq!(exit_code, ExitCode::FAILURE);
    }

    #[test]
    /// speaker 経路で config 解決に失敗した場合も失敗終了する。
    fn returns_failure_for_speaker_action_when_config_is_invalid() {
        let _guard = env_lock().lock().unwrap();
        let original = std::env::var_os("OPENAI_API_KEY");
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "");
        }

        let exit_code = run([
            OsString::from("diarize-log"),
            OsString::from("speaker"),
            OsString::from("list"),
        ]);

        restore_env_var("OPENAI_API_KEY", original);
        assert_eq!(exit_code, ExitCode::FAILURE);
    }

    fn restore_env_var(name: &str, original: Option<std::ffi::OsString>) {
        match original {
            Some(value) => unsafe {
                std::env::set_var(name, value);
            },
            None => unsafe {
                std::env::remove_var(name);
            },
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }
}
