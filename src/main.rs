use diarize_log::adapters::{
    CpalRecorder, FileSystemCaptureStore, FileSystemMergedTranscriptStore, FileSystemSpeakerStore,
    HoundAudioClipper, OpenAiTranscriber, ScreenCaptureKitApplicationRecorder,
};
use diarize_log::config::{Config, DEFAULT_DOTENV_PATH};
use diarize_log::{
    AudioSource, CaptureConfig, ChunkingStrategy, CliAction, InterruptMonitor, KnownSpeakerSample,
    LogSource, Logger, MixedCaptureSessionMetadata, MixedCaptureSourceSettings, Recorder,
    ResponseFormat, SpeakerCommandResult, SpeakerLabel, SpeakerStore, TranscriptSource,
    parse_cli_args, run_capture_with_interrupt_monitor, run_mixed_capture, run_speaker_command,
    write_debug_transcript,
};
use std::io::{self};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Default)]
struct SignalInterruptState {
    phase: AtomicU8,
}

impl InterruptMonitor for SignalInterruptState {
    fn is_interrupt_requested(&self) -> bool {
        self.phase.load(Ordering::SeqCst) != 0
    }
}

impl SignalInterruptState {
    fn install(logger: Logger) -> Result<Arc<Self>, ctrlc::Error> {
        let state = Arc::new(Self::default());
        let handler_state = Arc::clone(&state);
        ctrlc::set_handler(move || {
            if handler_state
                .phase
                .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                let _ = logger.info(
                    "interrupt received, stopping after flushing the recorded audio; press Ctrl+C again to abort immediately",
                );
            } else {
                let _ = logger.info("interrupt received again, aborting immediately");
                std::process::exit(130);
            }
        })?;
        Ok(state)
    }
}

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
    let root_logger = Logger::stderr(runtime_config.debug_enabled);

    match action {
        CliAction::Run {
            speaker_samples,
            audio_source,
        } => {
            let system_logger = root_logger
                .with_source(LogSource::System)
                .with_component("signal");
            let interrupt_state = match SignalInterruptState::install(system_logger) {
                Ok(state) => state,
                Err(error) => {
                    eprintln!("{error}");
                    return ExitCode::FAILURE;
                }
            };

            match audio_source {
                AudioSource::Microphone => run_capture_command(
                    &runtime_config,
                    &speaker_samples,
                    SpeakerLabel::KeepOriginal,
                    &root_logger.with_source(LogSource::Microphone),
                    interrupt_state.as_ref(),
                    CpalRecorder::new(
                        root_logger
                            .with_source(LogSource::Microphone)
                            .with_component("recorder"),
                    ),
                ),
                AudioSource::Application { bundle_id } => run_capture_command(
                    &runtime_config,
                    &speaker_samples,
                    SpeakerLabel::KeepOriginal,
                    &root_logger.with_source(LogSource::Application),
                    interrupt_state.as_ref(),
                    ScreenCaptureKitApplicationRecorder::new(
                        bundle_id,
                        root_logger
                            .with_source(LogSource::Application)
                            .with_component("recorder"),
                    ),
                ),
                AudioSource::Mixed {
                    bundle_id,
                    microphone_speaker,
                } => run_mixed_capture_command(
                    &runtime_config,
                    &speaker_samples,
                    bundle_id,
                    microphone_speaker,
                    interrupt_state,
                    &root_logger,
                ),
            }
        }
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
    source_logger: &Logger,
    interrupt_monitor: &dyn InterruptMonitor,
    mut recorder: R,
) -> ExitCode
where
    R: Recorder,
{
    let config = CaptureConfig::new(
        runtime_config.recording_duration,
        runtime_config.capture_duration,
        runtime_config.capture_overlap,
        runtime_config.transcription_language.clone(),
    );
    let config = CaptureConfig {
        merge_policy: runtime_config.transcript_merge_policy.clone(),
        ..config
    };
    let mut transcriber = match OpenAiTranscriber::new(
        runtime_config.openai_api_key.clone(),
        source_logger.with_component("transcriber"),
    ) {
        Ok(transcriber) => transcriber,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
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

    match run_capture_with_interrupt_monitor(
        &config,
        &speaker_samples,
        &speaker_label,
        &source_logger.with_component("capture"),
        &mut recorder,
        &mut transcriber,
        &mut capture_store,
        interrupt_monitor,
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
    interrupt_state: Arc<SignalInterruptState>,
    root_logger: &Logger,
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
    let microphone_interrupt_state = Arc::clone(&interrupt_state);
    let application_interrupt_state = Arc::clone(&interrupt_state);
    let microphone_logger = root_logger.with_source(LogSource::Microphone);
    let application_logger = root_logger.with_source(LogSource::Application);
    let mut final_store = match FileSystemMergedTranscriptStore::new(&session_dir) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let metadata =
        build_mixed_capture_metadata(runtime_config, &application_bundle_id, &microphone_speaker);
    let mixed_result = run_mixed_capture(
        &mut final_store,
        metadata,
        move || {
            let mut recorder = CpalRecorder::new(microphone_logger.with_component("recorder"));
            let mut capture_store = FileSystemCaptureStore::new_for_source(
                &microphone_session_dir,
                TranscriptSource::Microphone,
            )
            .map_err(|error| error.to_string())?;
            run_capture_pipeline(
                &microphone_config,
                &[],
                &SpeakerLabel::Fixed(microphone_speaker),
                &microphone_logger,
                microphone_interrupt_state.as_ref(),
                &mut recorder,
                &mut capture_store,
            )
        },
        move || {
            let mut recorder = ScreenCaptureKitApplicationRecorder::new(
                application_bundle_id,
                application_logger.with_component("recorder"),
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
                &application_logger,
                application_interrupt_state.as_ref(),
                &mut recorder,
                &mut capture_store,
            )
        },
    );

    let mut stdout = io::stdout();
    match mixed_result {
        Ok(result) => {
            if let Err(error) = write_debug_transcript(
                runtime_config.debug_enabled,
                &mut stdout,
                &result.debug_transcripts,
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

fn build_mixed_capture_metadata(
    runtime_config: &Config,
    application_bundle_id: &str,
    microphone_speaker: &str,
) -> MixedCaptureSessionMetadata {
    let capture_config = CaptureConfig::new(
        runtime_config.recording_duration,
        runtime_config.capture_duration,
        runtime_config.capture_overlap,
        runtime_config.transcription_language.clone(),
    );
    let capture_config = CaptureConfig {
        merge_policy: runtime_config.transcript_merge_policy.clone(),
        ..capture_config
    };

    MixedCaptureSessionMetadata {
        mode: "mixed".to_string(),
        application_bundle_id: application_bundle_id.to_string(),
        microphone_speaker: microphone_speaker.to_string(),
        source_settings: vec![
            MixedCaptureSourceSettings {
                source: TranscriptSource::Microphone,
                recording_duration_ms: duration_to_millis(
                    capture_config.capture_policy.recording_duration,
                ),
                capture_duration_ms: duration_to_millis(
                    capture_config.capture_policy.capture_duration,
                ),
                capture_overlap_ms: duration_to_millis(
                    capture_config.capture_policy.capture_overlap,
                ),
                transcription_model: capture_config.transcription_model.to_string(),
                transcription_language: capture_config.transcription_language.to_string(),
                response_format: response_format_value(capture_config.response_format).to_string(),
                chunking_strategy: chunking_strategy_value(capture_config.chunking_strategy)
                    .to_string(),
                merge_policy: capture_config.merge_policy.clone(),
                fixed_speaker: Some(microphone_speaker.to_string()),
            },
            MixedCaptureSourceSettings {
                source: TranscriptSource::Application,
                recording_duration_ms: duration_to_millis(
                    capture_config.capture_policy.recording_duration,
                ),
                capture_duration_ms: duration_to_millis(
                    capture_config.capture_policy.capture_duration,
                ),
                capture_overlap_ms: duration_to_millis(
                    capture_config.capture_policy.capture_overlap,
                ),
                transcription_model: capture_config.transcription_model.to_string(),
                transcription_language: capture_config.transcription_language.to_string(),
                response_format: response_format_value(capture_config.response_format).to_string(),
                chunking_strategy: chunking_strategy_value(capture_config.chunking_strategy)
                    .to_string(),
                merge_policy: capture_config.merge_policy,
                fixed_speaker: None,
            },
        ],
        source_outcomes: Vec::new(),
    }
}

fn run_capture_pipeline<R, S>(
    runtime_config: &Config,
    speaker_samples: &[KnownSpeakerSample],
    speaker_label: &SpeakerLabel,
    source_logger: &Logger,
    interrupt_monitor: &dyn InterruptMonitor,
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
        runtime_config.transcription_language.clone(),
    );
    let config = CaptureConfig {
        merge_policy: runtime_config.transcript_merge_policy.clone(),
        ..config
    };
    let mut transcriber = OpenAiTranscriber::new(
        runtime_config.openai_api_key.clone(),
        source_logger.with_component("transcriber"),
    )
    .map_err(|error| error.to_string())?;

    run_capture_with_interrupt_monitor(
        &config,
        speaker_samples,
        speaker_label,
        &source_logger.with_component("capture"),
        recorder,
        &mut transcriber,
        capture_store,
        interrupt_monitor,
    )
    .map_err(|error| error.to_string())
}

fn duration_to_millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).expect("duration in millis must fit into u64")
}

fn response_format_value(format: ResponseFormat) -> &'static str {
    match format {
        ResponseFormat::DiarizedJson => "diarized_json",
    }
}

fn chunking_strategy_value(strategy: ChunkingStrategy) -> &'static str {
    match strategy {
        ChunkingStrategy::Auto => "auto",
    }
}
