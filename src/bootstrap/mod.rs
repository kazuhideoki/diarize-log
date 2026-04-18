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
