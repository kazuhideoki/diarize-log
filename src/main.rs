use diarize_log::adapters::{
    CpalRecorder, FileSystemCaptureStore, FileSystemSpeakerStore, HoundAudioClipper,
    OpenAiTranscriber,
};
use diarize_log::config::{Config, DEFAULT_DOTENV_PATH};
use diarize_log::{
<<<<<<< HEAD
    CliAction, CliConfig, parse_cli_args, run_cli, run_speaker_command, write_debug_transcript,
=======
    CliAction, CliConfig, SpeakerCommandResult, parse_cli_args, render_help, run_cli,
    run_speaker_command, write_debug_transcript,
>>>>>>> main
};
use std::io::{self};
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    match parse_cli_args(std::env::args_os()) {
        Ok(CliAction::PrintOutput(message)) => {
            print!("{message}");
            return ExitCode::SUCCESS;
        }
        Ok(CliAction::Speaker(command)) => {
            let runtime_config = match Config::from_dotenv_path(Path::new(DEFAULT_DOTENV_PATH)) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("{error}");
                    return ExitCode::FAILURE;
                }
            };
            let clipper = HoundAudioClipper;
            let mut speaker_store = FileSystemSpeakerStore::new(&runtime_config.storage_root);

            return match run_speaker_command(
                &command,
                runtime_config.speaker_sample_duration,
                &clipper,
                &mut speaker_store,
            ) {
                Ok(SpeakerCommandResult::Updated) => ExitCode::SUCCESS,
                Ok(SpeakerCommandResult::ListedSpeakers(speaker_names)) => {
                    for speaker_name in speaker_names {
                        println!("{speaker_name}");
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            };
        }
        Ok(CliAction::Run) => {}
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    }

    let runtime_config = match Config::from_dotenv_path(Path::new(DEFAULT_DOTENV_PATH)) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let config = CliConfig::new(
        runtime_config.recording_duration,
        runtime_config.capture_duration,
        runtime_config.capture_overlap,
    );
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
        Ok(transcripts) => {
            if let Err(error) =
                write_debug_transcript(runtime_config.debug_enabled, &mut stdout, &transcripts)
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
