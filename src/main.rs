use diarize_log::adapters::{
    CpalRecorder, FileSystemCaptureStore, FileSystemMergedTranscriptStore, FileSystemSpeakerStore,
    HoundAudioClipper, OpenAiTranscriber, ScreenCaptureKitApplicationRecorder,
};
use diarize_log::config::{Config, DEFAULT_DOTENV_PATH};
use diarize_log::{
    AudioSource, CaptureConfig, CliAction, KnownSpeakerSample, Recorder, SpeakerCommandResult,
    SpeakerLabel, SpeakerStore, TranscriptSource, merge_source_segments, parse_cli_args,
    run_capture, run_speaker_command, write_debug_transcript,
};
use std::io::{self};
use std::path::Path;
use std::process::ExitCode;
use std::thread;

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
        CliAction::Run {
            speaker_samples,
            audio_source,
        } => match audio_source {
            AudioSource::Microphone => run_capture_command(
                &runtime_config,
                &speaker_samples,
                SpeakerLabel::KeepOriginal,
                CpalRecorder::new(runtime_config.debug_enabled),
            ),
            AudioSource::Application { bundle_id } => run_capture_command(
                &runtime_config,
                &speaker_samples,
                SpeakerLabel::KeepOriginal,
                ScreenCaptureKitApplicationRecorder::new(bundle_id, runtime_config.debug_enabled),
            ),
            AudioSource::Mixed {
                bundle_id,
                microphone_speaker,
            } => run_mixed_capture_command(
                &runtime_config,
                &speaker_samples,
                bundle_id,
                microphone_speaker,
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
    speaker_label: SpeakerLabel,
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
    let config = CaptureConfig {
        merge_policy: runtime_config.transcript_merge_policy.clone(),
        ..config
    };
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
        &speaker_label,
        &mut recorder,
        &mut transcriber,
        &mut capture_store,
        &mut stderr,
    ) {
        Ok(result) => {
            if let Err(error) = write_debug_transcript(
                runtime_config.debug_enabled,
                &mut stdout,
                &result.transcripts,
            ) {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }

            if result.completed_without_failures() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run_mixed_capture_command(
    runtime_config: &Config,
    speaker_sample_names: &[String],
    bundle_id: String,
    microphone_speaker: String,
) -> ExitCode {
    let session_dir = match FileSystemCaptureStore::create_session_dir(&runtime_config.storage_root)
    {
        Ok(session_dir) => session_dir,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let known_speakers = match load_known_speaker_samples(
        &FileSystemSpeakerStore::new(&runtime_config.storage_root),
        speaker_sample_names,
    ) {
        Ok(samples) => samples,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let microphone_config = runtime_config.clone();
    let application_config = runtime_config.clone();
    let microphone_session_dir = session_dir.clone();
    let application_session_dir = session_dir.clone();
    let application_bundle_id = bundle_id;
    let app_speaker_samples = known_speakers.clone();

    let microphone_handle = thread::spawn(move || {
        let mut recorder = CpalRecorder::new(microphone_config.debug_enabled);
        let mut capture_store = FileSystemCaptureStore::new_for_source(
            &microphone_session_dir,
            TranscriptSource::Microphone,
        )
        .map_err(|error| error.to_string())?;
        run_capture_pipeline(
            &microphone_config,
            &[],
            &SpeakerLabel::Fixed(microphone_speaker),
            &mut recorder,
            &mut capture_store,
        )
    });
    let application_handle = thread::spawn(move || {
        let mut recorder = ScreenCaptureKitApplicationRecorder::new(
            application_bundle_id,
            application_config.debug_enabled,
        );
        let mut capture_store = FileSystemCaptureStore::new_for_source(
            &application_session_dir,
            TranscriptSource::Application,
        )
        .map_err(|error| error.to_string())?;
        run_capture_pipeline(
            &application_config,
            &app_speaker_samples,
            &SpeakerLabel::KeepOriginal,
            &mut recorder,
            &mut capture_store,
        )
    });

    let microphone_result = match microphone_handle.join() {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
        Err(_) => {
            eprintln!("microphone capture thread panicked");
            return ExitCode::FAILURE;
        }
    };
    let application_result = match application_handle.join() {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
        Err(_) => {
            eprintln!("application capture thread panicked");
            return ExitCode::FAILURE;
        }
    };

    let final_segments = merge_source_segments(&[
        (
            TranscriptSource::Microphone,
            microphone_result.merged_segments.clone(),
        ),
        (
            TranscriptSource::Application,
            application_result.merged_segments.clone(),
        ),
    ]);
    let mut final_store = match FileSystemMergedTranscriptStore::new(&session_dir) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = final_store.persist_segments(&final_segments) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }

    let mut stdout = io::stdout();
    let mut debug_transcripts = microphone_result.transcripts.clone();
    debug_transcripts.extend(application_result.transcripts.clone());
    if let Err(error) = write_debug_transcript(
        runtime_config.debug_enabled,
        &mut stdout,
        &debug_transcripts,
    ) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }

    if microphone_result.completed_without_failures()
        && application_result.completed_without_failures()
    {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_capture_pipeline<R, S>(
    runtime_config: &Config,
    speaker_samples: &[KnownSpeakerSample],
    speaker_label: &SpeakerLabel,
    recorder: &mut R,
    capture_store: &mut S,
) -> Result<diarize_log::CaptureRunResult, String>
where
    R: Recorder,
    S: diarize_log::CaptureStore,
{
    let config = CaptureConfig::new(
        runtime_config.recording_duration,
        runtime_config.capture_duration,
        runtime_config.capture_overlap,
    );
    let config = CaptureConfig {
        merge_policy: runtime_config.transcript_merge_policy.clone(),
        ..config
    };
    let mut transcriber = OpenAiTranscriber::new(
        runtime_config.openai_api_key.clone(),
        runtime_config.debug_enabled,
    )
    .map_err(|error| error.to_string())?;
    let mut stderr = io::stderr();

    run_capture(
        &config,
        speaker_samples,
        speaker_label,
        recorder,
        &mut transcriber,
        capture_store,
        &mut stderr,
    )
    .map_err(|error| error.to_string())
}
