use diarize_log::config::{Config, DEFAULT_DOTENV_PATH};
use diarize_log::{
    CliConfig, CpalRecorder, FileSystemCaptureStore, OpenAiTranscriber, run_cli,
    write_debug_transcript,
};
use std::io::{self};
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
    let mut stderr = io::stderr();
    let mut stdout = io::stdout();
    let mut capture_store = match FileSystemCaptureStore::new(&runtime_config.storage_root) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    match run_cli(
        &config,
        &mut recorder,
        &mut transcriber,
        &mut capture_store,
        &mut stderr,
    ) {
        Ok(transcript) => {
            if let Err(error) =
                write_debug_transcript(runtime_config.debug_enabled, &mut stdout, &transcript)
            {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
