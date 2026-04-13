use diarize_log::adapters::{
    CpalRecorder, FileSystemCaptureStore, FileSystemSpeakerStore, HoundAudioClipper,
    OpenAiTranscriber, ScreenCaptureKitApplicationRecorder,
};
use diarize_log::config::{Config, DEFAULT_DOTENV_PATH};
use diarize_log::{
    AudioSource, CaptureConfig, CliAction, KnownSpeakerSample, Recorder, SpeakerCommandResult,
    SpeakerStore, parse_cli_args, run_capture, run_speaker_command, write_debug_transcript,
};
use std::io::{self};
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let action = match parse_cli_args(std::env::args_os()) {
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

    match action {
<<<<<<< HEAD
        CliAction::Run {
            speaker_samples,
            audio_source,
        } => match audio_source {
            AudioSource::Microphone => run_capture_command(
                &runtime_config,
=======
        CliAction::Run { speaker_samples } => {
            let config = CaptureConfig::new(
                runtime_config.recording_duration,
                runtime_config.capture_duration,
                runtime_config.capture_overlap,
            );
            let config = CaptureConfig {
                merge_policy: runtime_config.transcript_merge_policy.clone(),
                ..config
            };
            let mut recorder = CpalRecorder::new(runtime_config.debug_enabled);
            let mut transcriber = match OpenAiTranscriber::new(
                runtime_config.openai_api_key,
                runtime_config.debug_enabled,
            ) {
                Ok(transcriber) => transcriber,
                Err(error) => {
                    eprintln!("{error}");
                    return ExitCode::FAILURE;
                }
            };
            let mut stderr = io::stderr();
            let mut stdout = io::stdout();
            let mut capture_store = match FileSystemCaptureStore::new(&runtime_config.storage_root)
            {
                Ok(store) => store,
                Err(error) => {
                    eprintln!("{error}");
                    return ExitCode::FAILURE;
                }
            };
            let speaker_samples = match load_known_speaker_samples(
                &FileSystemSpeakerStore::new(&runtime_config.storage_root),
>>>>>>> main
                &speaker_samples,
                CpalRecorder::new(runtime_config.debug_enabled),
            ),
            AudioSource::Application { bundle_id } => run_capture_command(
                &runtime_config,
                &speaker_samples,
                ScreenCaptureKitApplicationRecorder::new(bundle_id, runtime_config.debug_enabled),
            ),
        },
        CliAction::Speaker(command) => {
            let clipper = HoundAudioClipper;
            let mut speaker_store = FileSystemSpeakerStore::new(&runtime_config.storage_root);

            match run_speaker_command(
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
            }
        }
        CliAction::PrintOutput(_) => unreachable!("print output is handled before config load"),
    }
}

fn load_known_speaker_samples<S>(
    speaker_store: &S,
    speaker_names: &[String],
) -> Result<Vec<KnownSpeakerSample>, diarize_log::SpeakerStoreError>
where
    S: SpeakerStore,
{
    speaker_names
        .iter()
        .map(|speaker_name| speaker_store.read_sample(speaker_name))
        .collect()
}

fn run_capture_command<R>(
    runtime_config: &Config,
    speaker_sample_names: &[String],
    mut recorder: R,
) -> ExitCode
where
    R: Recorder,
{
    let config = CaptureConfig::new(
        runtime_config.recording_duration,
        runtime_config.capture_duration,
        runtime_config.capture_overlap,
    );
    let mut transcriber = match OpenAiTranscriber::new(
        runtime_config.openai_api_key.clone(),
        runtime_config.debug_enabled,
    ) {
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
    let speaker_samples = match load_known_speaker_samples(
        &FileSystemSpeakerStore::new(&runtime_config.storage_root),
        speaker_sample_names,
    ) {
        Ok(samples) => samples,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    match run_capture(
        &config,
        &speaker_samples,
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
