use diarize_log::config::{Config, DEFAULT_DOTENV_PATH};
use diarize_log::{CliConfig, CpalRecorder, OpenAiTranscriber, run_cli};
use std::io;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let runtime_config = match Config::from_dotenv_path(Path::new(DEFAULT_DOTENV_PATH)) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let config = CliConfig::default();
    let mut recorder = CpalRecorder::new(runtime_config.debug_enabled);
    let mut transcriber =
        match OpenAiTranscriber::new(runtime_config.openai_api_key, runtime_config.debug_enabled) {
            Ok(transcriber) => transcriber,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();

    match run_cli(
        &config,
        &mut recorder,
        &mut transcriber,
        &mut stdout,
        &mut stderr,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
