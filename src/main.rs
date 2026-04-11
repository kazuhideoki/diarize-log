use diarize_log::{CliConfig, CpalRecorder, OpenAiTranscriber, run_cli};
use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let config = CliConfig::default();
    let mut recorder = CpalRecorder;
    let mut transcriber = match OpenAiTranscriber::from_env() {
        Ok(transcriber) => transcriber,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut stdout = io::stdout();

    match run_cli(&config, &mut recorder, &mut transcriber, &mut stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
