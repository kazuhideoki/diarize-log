use super::signal::SignalInterruptState;
use diarize_log::adapters::{
    CpalRecorder, FileSystemCaptureStore, FileSystemMergedTranscriptStore, FileSystemSpeakerStore,
    HoundAudioClipper, OpenAiSimpleTranscriber, OpenAiTranscriber, PythonSpeakerEmbedder,
    ScreenCaptureKitApplicationRecorder, SeparatedTranscriber,
};
use diarize_log::config::{Config, TranscriptionPipeline};
use diarize_log::{
    AudioSource, CaptureConfig, CaptureRunResult, ChunkingStrategy, DebugOutputError,
    KnownSpeakerEmbedding, KnownSpeakerSample, LineLogger, LogSource, MixedCaptureRunResult,
    MixedCaptureSessionMetadata, MixedCaptureSourceSettings, Recorder, ResponseFormat,
    SIMPLE_TRANSCRIPTION_MODEL, SpeakerCommand, SpeakerCommandResult, SpeakerLabel, SpeakerStore,
    Transcriber, TranscriptSource, run_capture_with_interrupt_monitor, run_mixed_capture,
    run_speaker_command, write_debug_transcript,
};
use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Arc;

pub(super) fn run_capture_action(
    runtime_config: &Config,
    speaker_samples: &[String],
    audio_source: AudioSource,
    interrupt_state: Arc<SignalInterruptState>,
    root_logger: &LineLogger,
) -> ExitCode {
    if let Err(error) = ensure_audio_source_preflight_available(
        runtime_config,
        speaker_samples,
        &audio_source,
        root_logger,
    ) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }

    match audio_source {
        AudioSource::Microphone { only_speaker } => run_capture_command(
            runtime_config,
            speaker_samples,
            speaker_label_from_only_speaker(only_speaker),
            &root_logger.with_source(LogSource::Microphone),
            interrupt_state.as_ref(),
            CpalRecorder::new(
                root_logger
                    .with_source(LogSource::Microphone)
                    .with_component("recorder"),
            ),
        ),
        AudioSource::Application {
            bundle_id,
            only_speaker,
        } => {
            let application_logger = root_logger.with_source(LogSource::Application);
            run_capture_command(
                runtime_config,
                speaker_samples,
                speaker_label_from_only_speaker(only_speaker),
                &application_logger,
                interrupt_state.as_ref(),
                ScreenCaptureKitApplicationRecorder::new(
                    bundle_id,
                    application_logger.with_component("recorder"),
                ),
            )
        }
        AudioSource::Mixed {
            bundle_id,
            microphone_only_speaker,
            application_only_speaker,
        } => run_mixed_capture_command(
            runtime_config,
            speaker_samples,
            bundle_id,
            microphone_only_speaker,
            application_only_speaker,
            interrupt_state,
            root_logger,
        ),
    }
}

pub(super) fn run_speaker_action(runtime_config: &Config, command: SpeakerCommand) -> ExitCode {
    let clipper = HoundAudioClipper;
    let embedder = PythonSpeakerEmbedder::default();
    let mut speaker_store = FileSystemSpeakerStore::new(&runtime_config.storage_root);

    match run_speaker_command(
        &command,
        runtime_config.speaker_sample_duration,
        &clipper,
        &embedder,
        &mut speaker_store,
    ) {
        Ok(result) => {
            let mut stdout = io::stdout();
            match complete_speaker_command(&mut stdout, result) {
                Ok(exit_code) => exit_code,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

pub(super) fn run_doctor_action(
    runtime_config: &Config,
    speaker_samples: &[String],
    audio_source: AudioSource,
    root_logger: &LineLogger,
    fix: bool,
) -> ExitCode {
    let report = build_doctor_report(runtime_config, speaker_samples, &audio_source, root_logger);
    let mut stdout = io::stdout();
    match complete_doctor_command(&mut stdout, &report, fix) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
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
    source_logger: &LineLogger,
    interrupt_monitor: &dyn diarize_log::InterruptMonitor,
    mut recorder: R,
) -> ExitCode
where
    R: Recorder,
{
    let config = capture_config_from_runtime_config(runtime_config, &speaker_label);
    let mut stdout = io::stdout();
    let mut capture_store = match FileSystemCaptureStore::new(&runtime_config.storage_root) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut transcriber = match build_transcriber(
        runtime_config,
        speaker_sample_names,
        &speaker_label,
        source_logger.with_component("transcriber"),
    ) {
        Ok(transcriber) => transcriber,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let speaker_samples = match &speaker_label {
        SpeakerLabel::KeepOriginal => match load_known_speaker_samples(
            &FileSystemSpeakerStore::new(&runtime_config.storage_root),
            speaker_sample_names,
        ) {
            Ok(samples) => samples,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        },
        SpeakerLabel::Fixed(_) => Vec::new(),
    };

    match run_capture_with_interrupt_monitor(
        &config,
        &speaker_samples,
        &speaker_label,
        &source_logger.with_component("capture"),
        &mut recorder,
        transcriber.as_mut(),
        &mut capture_store,
        interrupt_monitor,
    ) {
        Ok(result) => {
            match complete_capture_command(runtime_config.debug_enabled, &mut stdout, &result) {
                Ok(exit_code) => exit_code,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
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
    application_only_speaker: Option<String>,
    interrupt_state: Arc<SignalInterruptState>,
    root_logger: &LineLogger,
) -> ExitCode {
    let application_speaker_label =
        speaker_label_from_only_speaker(application_only_speaker.clone());
    let session_dir = match FileSystemCaptureStore::create_session_dir(&runtime_config.storage_root)
    {
        Ok(session_dir) => session_dir,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let app_speaker_samples = match &application_only_speaker {
        Some(_) => Vec::new(),
        None => match load_known_speaker_samples(
            &FileSystemSpeakerStore::new(&runtime_config.storage_root),
            speaker_sample_names,
        ) {
            Ok(samples) => samples,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        },
    };
    let microphone_config = runtime_config.clone();
    let application_config = runtime_config.clone();
    let microphone_session_dir = session_dir.clone();
    let application_session_dir = session_dir.clone();
    let application_bundle_id = bundle_id;
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
    let metadata = build_mixed_capture_metadata(
        runtime_config,
        &application_bundle_id,
        &microphone_speaker,
        application_only_speaker.as_deref(),
    );
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
                &application_speaker_label,
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
            match complete_mixed_capture_command(runtime_config.debug_enabled, &mut stdout, &result)
            {
                Ok(exit_code) => exit_code,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
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
    application_only_speaker: Option<&str>,
) -> MixedCaptureSessionMetadata {
    let microphone_capture_config = capture_config_from_runtime_config(
        runtime_config,
        &SpeakerLabel::Fixed(microphone_speaker.to_string()),
    );
    let application_speaker_label =
        speaker_label_from_only_speaker(application_only_speaker.map(ToOwned::to_owned));
    let application_capture_config =
        capture_config_from_runtime_config(runtime_config, &application_speaker_label);

    MixedCaptureSessionMetadata {
        mode: "mixed".to_string(),
        application_bundle_id: application_bundle_id.to_string(),
        microphone_speaker: microphone_speaker.to_string(),
        source_settings: vec![
            MixedCaptureSourceSettings {
                source: TranscriptSource::Microphone,
                recording_duration_ms: duration_to_millis(
                    microphone_capture_config.capture_policy.recording_duration,
                ),
                capture_duration_ms: duration_to_millis(
                    microphone_capture_config.capture_policy.capture_duration,
                ),
                capture_overlap_ms: duration_to_millis(
                    microphone_capture_config.capture_policy.capture_overlap,
                ),
                capture_silence_threshold_dbfs: microphone_capture_config
                    .silence_request_policy
                    .silence_threshold_dbfs,
                capture_silence_min_duration_ms: duration_to_millis(
                    microphone_capture_config
                        .silence_request_policy
                        .silence_min_duration,
                ),
                capture_tail_silence_min_duration_ms: duration_to_millis(
                    microphone_capture_config
                        .silence_request_policy
                        .tail_silence_min_duration,
                ),
                transcription_model: microphone_capture_config.transcription_model.to_string(),
                transcription_language: microphone_capture_config
                    .transcription_language
                    .to_string(),
                response_format: response_format_value(microphone_capture_config.response_format)
                    .to_string(),
                chunking_strategy: chunking_strategy_value(
                    microphone_capture_config.chunking_strategy,
                )
                .to_string(),
                merge_policy: microphone_capture_config.merge_policy.clone(),
                fixed_speaker: Some(microphone_speaker.to_string()),
            },
            MixedCaptureSourceSettings {
                source: TranscriptSource::Application,
                recording_duration_ms: duration_to_millis(
                    application_capture_config.capture_policy.recording_duration,
                ),
                capture_duration_ms: duration_to_millis(
                    application_capture_config.capture_policy.capture_duration,
                ),
                capture_overlap_ms: duration_to_millis(
                    application_capture_config.capture_policy.capture_overlap,
                ),
                capture_silence_threshold_dbfs: application_capture_config
                    .silence_request_policy
                    .silence_threshold_dbfs,
                capture_silence_min_duration_ms: duration_to_millis(
                    application_capture_config
                        .silence_request_policy
                        .silence_min_duration,
                ),
                capture_tail_silence_min_duration_ms: duration_to_millis(
                    application_capture_config
                        .silence_request_policy
                        .tail_silence_min_duration,
                ),
                transcription_model: application_capture_config.transcription_model.to_string(),
                transcription_language: application_capture_config
                    .transcription_language
                    .to_string(),
                response_format: response_format_value(application_capture_config.response_format)
                    .to_string(),
                chunking_strategy: chunking_strategy_value(
                    application_capture_config.chunking_strategy,
                )
                .to_string(),
                merge_policy: application_capture_config.merge_policy,
                fixed_speaker: application_only_speaker.map(ToOwned::to_owned),
            },
        ],
        source_outcomes: Vec::new(),
    }
}

fn run_capture_pipeline<R, S>(
    runtime_config: &Config,
    speaker_samples: &[KnownSpeakerSample],
    speaker_label: &SpeakerLabel,
    source_logger: &LineLogger,
    interrupt_monitor: &dyn diarize_log::InterruptMonitor,
    recorder: &mut R,
    capture_store: &mut S,
) -> Result<CaptureRunResult, String>
where
    R: Recorder,
    S: diarize_log::CaptureStore,
{
    let config = capture_config_from_runtime_config(runtime_config, speaker_label);
    let mut transcriber = build_transcriber(
        runtime_config,
        &speaker_samples
            .iter()
            .map(|sample| sample.speaker_name.clone())
            .collect::<Vec<_>>(),
        speaker_label,
        source_logger.with_component("transcriber"),
    )
    .map_err(|error| error.to_string())?;

    run_capture_with_interrupt_monitor(
        &config,
        speaker_samples,
        speaker_label,
        &source_logger.with_component("capture"),
        recorder,
        transcriber.as_mut(),
        capture_store,
        interrupt_monitor,
    )
    .map_err(|error| error.to_string())
}

fn ensure_capture_dependencies_available(
    runtime_config: &Config,
    speaker_sample_names: &[String],
    speaker_label: &SpeakerLabel,
) -> Result<(), String> {
    if !requires_speaker_embedder_preflight(runtime_config, speaker_sample_names, speaker_label) {
        return Ok(());
    }

    PythonSpeakerEmbedder::default()
        .check_available()
        .map_err(|error| format!("speaker embedder dependency check failed: {error}"))
}

fn ensure_application_capture_available(
    bundle_id: &str,
    logger: &LineLogger,
) -> Result<(), String> {
    ScreenCaptureKitApplicationRecorder::new(
        bundle_id.to_string(),
        logger.with_component("recorder"),
    )
    .check_available()
    .map_err(|error| format!("application capture dependency check failed: {error}"))
}

fn ensure_audio_source_preflight_available(
    runtime_config: &Config,
    speaker_samples: &[String],
    audio_source: &AudioSource,
    root_logger: &LineLogger,
) -> Result<(), String> {
    match audio_source {
        AudioSource::Microphone { only_speaker } => ensure_capture_dependencies_available(
            runtime_config,
            speaker_samples,
            &speaker_label_from_only_speaker(only_speaker.clone()),
        ),
        AudioSource::Application {
            bundle_id,
            only_speaker,
        } => {
            let application_logger = root_logger.with_source(LogSource::Application);
            ensure_application_capture_available(bundle_id, &application_logger).and_then(|()| {
                ensure_capture_dependencies_available(
                    runtime_config,
                    speaker_samples,
                    &speaker_label_from_only_speaker(only_speaker.clone()),
                )
            })
        }
        AudioSource::Mixed {
            bundle_id,
            application_only_speaker,
            ..
        } => {
            let application_logger = root_logger.with_source(LogSource::Application);
            let application_speaker_label =
                speaker_label_from_only_speaker(application_only_speaker.clone());
            ensure_application_capture_available(bundle_id, &application_logger).and_then(|()| {
                ensure_capture_dependencies_available(
                    runtime_config,
                    speaker_samples,
                    &application_speaker_label,
                )
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorReport {
    checks: Vec<DoctorCheckResult>,
}

impl DoctorReport {
    fn has_failure(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == DoctorCheckStatus::Failed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorCheckResult {
    name: &'static str,
    status: DoctorCheckStatus,
    message: String,
    fix: DoctorFixAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoctorCheckStatus {
    Ok,
    Failed,
}

impl DoctorCheckStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "fail",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DoctorFixAvailability {
    Automatic { plan: &'static str },
    Manual { reason: &'static str },
    NotNeeded,
}

fn build_doctor_report(
    runtime_config: &Config,
    speaker_samples: &[String],
    audio_source: &AudioSource,
    root_logger: &LineLogger,
) -> DoctorReport {
    let mut checks = vec![
        DoctorCheckResult {
            name: "config",
            status: DoctorCheckStatus::Ok,
            message: "runtime config loaded".to_string(),
            fix: DoctorFixAvailability::NotNeeded,
        },
        DoctorCheckResult {
            name: "openai_api_key",
            status: DoctorCheckStatus::Ok,
            message: format!(
                "OPENAI_API_KEY is configured from {}",
                runtime_config.openai_api_key_source
            ),
            fix: DoctorFixAvailability::Manual {
                reason: "secret values must be provided by the user",
            },
        },
        DoctorCheckResult {
            name: "storage_root",
            status: DoctorCheckStatus::Ok,
            message: runtime_config.storage_root.display().to_string(),
            fix: DoctorFixAvailability::Automatic {
                plan: "create the storage root directory when it is missing",
            },
        },
    ];

    checks.push(match runtime_config.pyannote_api_key.as_ref() {
        Some(_) => DoctorCheckResult {
            name: "pyannote_api_key",
            status: DoctorCheckStatus::Ok,
            message: "PYANNOTE_API_KEY is configured".to_string(),
            fix: DoctorFixAvailability::Manual {
                reason: "secret values must be provided by the user",
            },
        },
        None => DoctorCheckResult {
            name: "pyannote_api_key",
            status: DoctorCheckStatus::Failed,
            message: "PYANNOTE_API_KEY is not configured".to_string(),
            fix: DoctorFixAvailability::Manual {
                reason: "secret values must be provided by the user",
            },
        },
    });

    match ensure_audio_source_preflight_available(
        runtime_config,
        speaker_samples,
        audio_source,
        root_logger,
    ) {
        Ok(()) => checks.push(DoctorCheckResult {
            name: "audio_source",
            status: DoctorCheckStatus::Ok,
            message: "preflight passed".to_string(),
            fix: DoctorFixAvailability::NotNeeded,
        }),
        Err(error) => checks.push(DoctorCheckResult {
            name: "audio_source",
            status: DoctorCheckStatus::Failed,
            message: error,
            fix: DoctorFixAvailability::Manual {
                reason: "recording permissions, running target application, and hardware access require user action",
            },
        }),
    }

    DoctorReport { checks }
}

fn requires_speaker_embedder_preflight(
    runtime_config: &Config,
    speaker_sample_names: &[String],
    speaker_label: &SpeakerLabel,
) -> bool {
    runtime_config.transcription_pipeline == TranscriptionPipeline::Separated
        && matches!(speaker_label, SpeakerLabel::KeepOriginal)
        && !speaker_sample_names.is_empty()
}

fn build_transcriber(
    runtime_config: &Config,
    speaker_sample_names: &[String],
    speaker_label: &SpeakerLabel,
    logger: LineLogger,
) -> Result<Box<dyn Transcriber>, String> {
    if matches!(speaker_label, SpeakerLabel::Fixed(_)) {
        return OpenAiSimpleTranscriber::new(runtime_config.openai_api_key.clone(), logger)
            .map(|transcriber| Box::new(transcriber) as Box<dyn Transcriber>)
            .map_err(|error| error.to_string());
    }

    match runtime_config.transcription_pipeline {
        TranscriptionPipeline::Legacy => {
            OpenAiTranscriber::new(runtime_config.openai_api_key.clone(), logger)
                .map(|transcriber| Box::new(transcriber) as Box<dyn Transcriber>)
                .map_err(|error| error.to_string())
        }
        TranscriptionPipeline::Separated => {
            let pyannote_api_key = runtime_config
                .pyannote_api_key
                .clone()
                .ok_or_else(|| "PYANNOTE_API_KEY is required for separated pipeline".to_string())?;
            let speaker_store = FileSystemSpeakerStore::new(&runtime_config.storage_root);
            let known_embeddings =
                load_known_speaker_embeddings(&speaker_store, speaker_sample_names)
                    .map_err(|error| error.to_string())?;
            if !known_embeddings.is_empty() {
                PythonSpeakerEmbedder::default()
                    .check_available()
                    .map_err(|error| {
                        format!("speaker embedder dependency check failed: {error}")
                    })?;
            }
            SeparatedTranscriber::new(
                runtime_config.openai_api_key.clone(),
                pyannote_api_key,
                runtime_config.pyannote_max_speakers,
                known_embeddings,
                logger,
            )
            .map(|transcriber| Box::new(transcriber) as Box<dyn Transcriber>)
            .map_err(|error| error.to_string())
        }
    }
}

fn load_known_speaker_embeddings<S>(
    speaker_store: &S,
    speaker_names: &[String],
) -> Result<Vec<KnownSpeakerEmbedding>, diarize_log::SpeakerStoreError>
where
    S: SpeakerStore,
{
    speaker_names
        .iter()
        .map(|speaker_name| speaker_store.read_embedding(speaker_name))
        .collect()
}

fn capture_config_from_runtime_config(
    runtime_config: &Config,
    speaker_label: &SpeakerLabel,
) -> CaptureConfig {
    let config = CaptureConfig::new(
        runtime_config.recording_duration,
        runtime_config.capture_duration,
        runtime_config.capture_overlap,
        runtime_config.transcription_language.clone(),
    );
    let (transcription_model, response_format) = match speaker_label {
        SpeakerLabel::KeepOriginal => (config.transcription_model, config.response_format),
        SpeakerLabel::Fixed(_) => (SIMPLE_TRANSCRIPTION_MODEL, ResponseFormat::Json),
    };
    CaptureConfig {
        silence_request_policy: runtime_config.capture_silence_request_policy.clone(),
        merge_policy: runtime_config.transcript_merge_policy.clone(),
        transcription_model,
        response_format,
        ..config
    }
}

fn speaker_label_from_only_speaker(only_speaker: Option<String>) -> SpeakerLabel {
    match only_speaker {
        Some(speaker_name) => SpeakerLabel::Fixed(speaker_name),
        None => SpeakerLabel::KeepOriginal,
    }
}

fn complete_capture_command<W>(
    debug_enabled: bool,
    output: &mut W,
    result: &CaptureRunResult,
) -> Result<ExitCode, DebugOutputError>
where
    W: Write,
{
    write_debug_transcript(debug_enabled, output, &result.transcripts)?;
    Ok(if result.completed_without_failures() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn complete_mixed_capture_command<W>(
    debug_enabled: bool,
    output: &mut W,
    result: &MixedCaptureRunResult,
) -> Result<ExitCode, DebugOutputError>
where
    W: Write,
{
    write_debug_transcript(debug_enabled, output, &result.debug_transcripts)?;
    Ok(if result.completed_without_failures() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn complete_speaker_command<W>(output: &mut W, result: SpeakerCommandResult) -> io::Result<ExitCode>
where
    W: Write,
{
    match result {
        SpeakerCommandResult::Updated => Ok(ExitCode::SUCCESS),
        SpeakerCommandResult::ListedSpeakers(speaker_names) => {
            for speaker_name in speaker_names {
                writeln!(output, "{speaker_name}")?;
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn complete_doctor_command<W>(
    output: &mut W,
    report: &DoctorReport,
    fix: bool,
) -> io::Result<ExitCode>
where
    W: Write,
{
    for check in &report.checks {
        writeln!(
            output,
            "[{}] {}: {}",
            check.status.label(),
            check.name,
            check.message
        )?;
    }

    if fix {
        writeln!(output, "fix plan:")?;
        for check in &report.checks {
            match &check.fix {
                DoctorFixAvailability::Automatic { plan } => {
                    writeln!(output, "- {}: {plan}", check.name)?;
                }
                DoctorFixAvailability::Manual { reason } => {
                    writeln!(output, "- {}: manual ({reason})", check.name)?;
                }
                DoctorFixAvailability::NotNeeded => {}
            }
        }
    }

    if report.has_failure() {
        Ok(ExitCode::FAILURE)
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn duration_to_millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).expect("duration in millis must fit into u64")
}

fn response_format_value(format: ResponseFormat) -> &'static str {
    match format {
        ResponseFormat::DiarizedJson => "diarized_json",
        ResponseFormat::Json => "json",
    }
}

fn chunking_strategy_value(strategy: ChunkingStrategy) -> &'static str {
    match strategy {
        ChunkingStrategy::Auto => "auto",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diarize_log::config::ConfigSource;
    use diarize_log::domain::{SilenceRequestPolicy, TranscriptMergePolicy};
    use diarize_log::{
        CaptureTranscriptionFailure, DiarizedTranscript, KnownSpeakerEmbedding,
        MergedTranscriptSegment, RecordedAudio, SourcedTranscriptSegment, TranscriptSegment,
        TranscriptionLanguage,
    };
    use diarize_log::{MixedCaptureSourceOutcome, MixedCaptureSourceStatus};
    use std::cell::RefCell;
    use std::io;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::Duration;

    #[derive(Clone, Default)]
    struct SpySpeakerStore {
        reads: Rc<RefCell<Vec<String>>>,
        samples: Vec<KnownSpeakerSample>,
        read_error: Option<diarize_log::SpeakerStoreError>,
    }

    impl SpeakerStore for SpySpeakerStore {
        fn create_sample(
            &mut self,
            _speaker_name: &str,
            _audio: &RecordedAudio,
        ) -> Result<(), diarize_log::SpeakerStoreError> {
            unimplemented!("speaker sample creation is not used in these tests")
        }

        fn create_embedding(
            &mut self,
            _speaker_name: &str,
            _embedding: &KnownSpeakerEmbedding,
        ) -> Result<(), diarize_log::SpeakerStoreError> {
            unimplemented!("speaker embedding creation is not used in these tests")
        }

        fn remove_sample(
            &mut self,
            _speaker_name: &str,
        ) -> Result<(), diarize_log::SpeakerStoreError> {
            unimplemented!("speaker sample removal is not used in these tests")
        }

        fn list_samples(&self) -> Result<Vec<String>, diarize_log::SpeakerStoreError> {
            unimplemented!("speaker sample listing is not used in these tests")
        }

        fn read_sample(
            &self,
            speaker_name: &str,
        ) -> Result<KnownSpeakerSample, diarize_log::SpeakerStoreError> {
            self.reads.borrow_mut().push(speaker_name.to_string());
            if let Some(error) = &self.read_error {
                return Err(error.clone());
            }

            self.samples
                .iter()
                .find(|sample| sample.speaker_name == speaker_name)
                .cloned()
                .ok_or_else(|| diarize_log::SpeakerStoreError::SpeakerNotFound {
                    speaker_name: speaker_name.to_string(),
                })
        }

        fn read_embedding(
            &self,
            speaker_name: &str,
        ) -> Result<KnownSpeakerEmbedding, diarize_log::SpeakerStoreError> {
            Ok(KnownSpeakerEmbedding {
                speaker_name: speaker_name.to_string(),
                model: "fake-ecapa".to_string(),
                vector: vec![0.1, 0.2],
            })
        }
    }

    struct FailingWriter;

    impl io::Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("write failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn sample_audio() -> RecordedAudio {
        RecordedAudio {
            wav_bytes: vec![0x52, 0x49, 0x46, 0x46],
            content_type: "audio/wav",
        }
    }

    fn sample_known_speaker(name: &str) -> KnownSpeakerSample {
        KnownSpeakerSample {
            speaker_name: name.to_string(),
            audio: sample_audio(),
        }
    }

    fn sample_transcript(text: &str, speaker: &str) -> DiarizedTranscript {
        DiarizedTranscript {
            text: text.to_string(),
            segments: vec![TranscriptSegment {
                speaker: speaker.to_string(),
                start_ms: 0,
                end_ms: 800,
                text: text.to_string(),
            }],
        }
    }

    fn sample_config() -> Config {
        Config {
            openai_api_key: "test-api-key".to_string(),
            openai_api_key_source: ConfigSource::Environment,
            pyannote_api_key: None,
            recording_duration: Duration::from_secs(90),
            capture_duration: Duration::from_secs(30),
            capture_overlap: Duration::from_secs(5),
            capture_silence_request_policy: SilenceRequestPolicy {
                silence_threshold_dbfs: -33.5,
                silence_min_duration: Duration::from_millis(900),
                tail_silence_min_duration: Duration::from_millis(400),
            },
            speaker_sample_duration: Duration::from_secs(7),
            transcription_language: TranscriptionLanguage::Fixed("ja".to_string()),
            transcription_pipeline: TranscriptionPipeline::Legacy,
            pyannote_max_speakers: None,
            transcript_merge_policy: TranscriptMergePolicy {
                min_overlap_chars: 12,
                min_alignment_ratio: 0.82,
                min_trigram_similarity: 0.61,
            },
            debug_enabled: true,
            storage_root: PathBuf::from("/tmp/diarize-log-tests"),
        }
    }

    fn sample_capture_run_result() -> CaptureRunResult {
        CaptureRunResult {
            started_at_unix_ms: 1_700_000_000_000,
            transcripts: vec![sample_transcript("hello", "spk_0")],
            merged_segments: vec![MergedTranscriptSegment {
                speaker: "spk_0".to_string(),
                start_ms: 0,
                end_ms: 800,
                text: "hello".to_string(),
            }],
            transcription_failures: Vec::new(),
        }
    }

    fn sample_mixed_capture_run_result() -> MixedCaptureRunResult {
        MixedCaptureRunResult {
            final_segments: vec![SourcedTranscriptSegment {
                source: TranscriptSource::Application,
                speaker: "spk_0".to_string(),
                start_ms: 0,
                end_ms: 800,
                text: "hello".to_string(),
            }],
            source_outcomes: vec![
                MixedCaptureSourceOutcome {
                    source: TranscriptSource::Microphone,
                    started_at_unix_ms: 1_700_000_000_000,
                    status: MixedCaptureSourceStatus::Succeeded,
                    transcription_failure_count: 0,
                    error_message: None,
                },
                MixedCaptureSourceOutcome {
                    source: TranscriptSource::Application,
                    started_at_unix_ms: 1_700_000_000_100,
                    status: MixedCaptureSourceStatus::Succeeded,
                    transcription_failure_count: 0,
                    error_message: None,
                },
            ],
            debug_transcripts: vec![sample_transcript("hello", "spk_0")],
        }
    }

    #[test]
    /// separated pipeline で既知話者サンプルによる話者同定を行う場合だけ embedder の起動前検査を要求する。
    fn requires_embedder_preflight_for_separated_known_speaker_identification() {
        let mut config = sample_config();
        config.transcription_pipeline = TranscriptionPipeline::Separated;

        assert!(requires_speaker_embedder_preflight(
            &config,
            &["speaker".to_string()],
            &SpeakerLabel::KeepOriginal,
        ));
    }

    #[test]
    /// 固定話者指定では separated pipeline でも speaker embedder を使わないため起動前検査を省く。
    fn skips_embedder_preflight_for_fixed_speaker_capture() {
        let mut config = sample_config();
        config.transcription_pipeline = TranscriptionPipeline::Separated;

        assert!(!requires_speaker_embedder_preflight(
            &config,
            &["speaker".to_string()],
            &SpeakerLabel::Fixed("speaker".to_string()),
        ));
    }

    #[test]
    /// 既知話者サンプルが無い separated pipeline は話者同定を行わないため embedder の起動前検査を省く。
    fn skips_embedder_preflight_without_speaker_samples() {
        let mut config = sample_config();
        config.transcription_pipeline = TranscriptionPipeline::Separated;

        assert!(!requires_speaker_embedder_preflight(
            &config,
            &[],
            &SpeakerLabel::KeepOriginal,
        ));
    }

    #[test]
    /// 既知話者サンプル読み込みは CLI 指定順を保ったまま保存先へ委譲する。
    fn loads_known_speaker_samples_in_cli_order() {
        let speaker_store = SpySpeakerStore {
            reads: Rc::new(RefCell::new(Vec::new())),
            samples: vec![sample_known_speaker("sato"), sample_known_speaker("suzuki")],
            read_error: None,
        };

        let samples =
            load_known_speaker_samples(&speaker_store, &["suzuki".to_string(), "sato".to_string()])
                .unwrap();

        assert_eq!(
            *speaker_store.reads.borrow(),
            vec!["suzuki".to_string(), "sato".to_string()]
        );
        assert_eq!(
            samples
                .iter()
                .map(|sample| sample.speaker_name.as_str())
                .collect::<Vec<_>>(),
            vec!["suzuki", "sato"]
        );
    }

    #[test]
    /// mixed capture metadata には runtime config と source ごとの差分設定を保存する。
    fn builds_mixed_capture_metadata_from_runtime_config() {
        let metadata =
            build_mixed_capture_metadata(&sample_config(), "com.apple.Safari", "me", Some("guest"));

        assert_eq!(metadata.mode, "mixed");
        assert_eq!(metadata.application_bundle_id, "com.apple.Safari");
        assert_eq!(metadata.microphone_speaker, "me");
        assert_eq!(metadata.source_outcomes, Vec::new());
        assert_eq!(metadata.source_settings.len(), 2);
        assert_eq!(
            metadata.source_settings[0].source,
            TranscriptSource::Microphone
        );
        assert_eq!(
            metadata.source_settings[0].fixed_speaker,
            Some("me".to_string())
        );
        assert_eq!(metadata.source_settings[0].recording_duration_ms, 90_000);
        assert_eq!(metadata.source_settings[0].capture_duration_ms, 30_000);
        assert_eq!(metadata.source_settings[0].capture_overlap_ms, 5_000);
        assert_eq!(
            metadata.source_settings[0].capture_silence_threshold_dbfs,
            -33.5
        );
        assert_eq!(
            metadata.source_settings[0].capture_silence_min_duration_ms,
            900
        );
        assert_eq!(
            metadata.source_settings[0].capture_tail_silence_min_duration_ms,
            400
        );
        assert_eq!(metadata.source_settings[0].transcription_language, "ja");
        assert_eq!(
            metadata.source_settings[0].transcription_model,
            "gpt-4o-transcribe"
        );
        assert_eq!(metadata.source_settings[0].response_format, "json");
        assert_eq!(metadata.source_settings[0].chunking_strategy, "auto");
        assert_eq!(
            metadata.source_settings[0].merge_policy,
            sample_config().transcript_merge_policy
        );
        assert_eq!(
            metadata.source_settings[1].source,
            TranscriptSource::Application
        );
        assert_eq!(
            metadata.source_settings[1].transcription_model,
            "gpt-4o-transcribe"
        );
        assert_eq!(metadata.source_settings[1].response_format, "json");
        assert_eq!(
            metadata.source_settings[1].fixed_speaker,
            Some("guest".to_string())
        );
    }

    #[test]
    /// capture 完了時は debug transcript を出力し、失敗が無ければ成功コードを返す。
    fn completes_capture_command_with_success_exit_code_after_debug_output() {
        let mut output = Vec::new();

        let exit_code =
            complete_capture_command(true, &mut output, &sample_capture_run_result()).unwrap();

        assert_eq!(exit_code, ExitCode::SUCCESS);
        let stdout = String::from_utf8(output).unwrap();
        assert!(stdout.contains("\"text\": \"hello\""));
    }

    #[test]
    /// partial failure を含む capture 結果は transcript を出力しても失敗コードを返す。
    fn completes_capture_command_with_failure_exit_code_for_partial_failure() {
        let mut output = Vec::new();
        let mut result = sample_capture_run_result();
        result
            .transcription_failures
            .push(CaptureTranscriptionFailure {
                capture_index: 2,
                capture_start_ms: 800,
                message: "request failed".to_string(),
            });

        let exit_code = complete_capture_command(true, &mut output, &result).unwrap();

        assert_eq!(exit_code, ExitCode::FAILURE);
        assert!(!output.is_empty());
    }

    #[test]
    /// debug transcript の出力に失敗した場合は capture 完了処理も失敗する。
    fn fails_capture_command_completion_when_debug_output_write_fails() {
        let error =
            complete_capture_command(true, &mut FailingWriter, &sample_capture_run_result())
                .unwrap_err();

        assert!(error.to_string().contains("failed to"));
        assert!(error.to_string().contains("debug stdout"));
    }

    #[test]
    /// mixed capture 完了時は debug transcript を出力し、全 source 成功なら成功コードを返す。
    fn completes_mixed_capture_command_with_success_exit_code_after_debug_output() {
        let mut output = Vec::new();

        let exit_code =
            complete_mixed_capture_command(true, &mut output, &sample_mixed_capture_run_result())
                .unwrap();

        assert_eq!(exit_code, ExitCode::SUCCESS);
        let stdout = String::from_utf8(output).unwrap();
        assert!(stdout.contains("\"text\": \"hello\""));
    }

    #[test]
    /// 話者一覧出力は 1 行 1 話者の現在の CLI 表示を維持する。
    fn completes_speaker_command_result_by_printing_each_name_on_its_own_line() {
        let mut output = Vec::new();

        let exit_code = complete_speaker_command(
            &mut output,
            SpeakerCommandResult::ListedSpeakers(vec!["sato".to_string(), "suzuki".to_string()]),
        )
        .unwrap();

        assert_eq!(exit_code, ExitCode::SUCCESS);
        assert_eq!(String::from_utf8(output).unwrap(), "sato\nsuzuki\n");
    }

    #[test]
    /// doctor 完了時は各診断項目の状態を標準出力へ書く。
    fn completes_doctor_command_with_check_results() {
        let mut output = Vec::new();
        let report = DoctorReport {
            checks: vec![
                DoctorCheckResult {
                    name: "config",
                    status: DoctorCheckStatus::Ok,
                    message: "runtime config loaded".to_string(),
                    fix: DoctorFixAvailability::NotNeeded,
                },
                DoctorCheckResult {
                    name: "pyannote_api_key",
                    status: DoctorCheckStatus::Ok,
                    message: "PYANNOTE_API_KEY is configured".to_string(),
                    fix: DoctorFixAvailability::Manual {
                        reason: "secret values must be provided by the user",
                    },
                },
            ],
        };

        let exit_code = complete_doctor_command(&mut output, &report, false).unwrap();

        assert_eq!(exit_code, ExitCode::SUCCESS);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "[ok] config: runtime config loaded\n[ok] pyannote_api_key: PYANNOTE_API_KEY is configured\n"
        );
    }

    #[test]
    /// doctor --fix は自動修正候補と手動対応が必要な項目を分けて出力する。
    fn completes_doctor_fix_command_with_repair_plan() {
        let mut output = Vec::new();
        let report = DoctorReport {
            checks: vec![
                DoctorCheckResult {
                    name: "storage_root",
                    status: DoctorCheckStatus::Ok,
                    message: "/tmp/diarize-log".to_string(),
                    fix: DoctorFixAvailability::Automatic {
                        plan: "create the storage root directory when it is missing",
                    },
                },
                DoctorCheckResult {
                    name: "openai_api_key",
                    status: DoctorCheckStatus::Ok,
                    message: "OPENAI_API_KEY is configured from .env".to_string(),
                    fix: DoctorFixAvailability::Manual {
                        reason: "secret values must be provided by the user",
                    },
                },
            ],
        };

        let exit_code = complete_doctor_command(&mut output, &report, true).unwrap();

        assert_eq!(exit_code, ExitCode::SUCCESS);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "[ok] storage_root: /tmp/diarize-log\n[ok] openai_api_key: OPENAI_API_KEY is configured from .env\nfix plan:\n- storage_root: create the storage root directory when it is missing\n- openai_api_key: manual (secret values must be provided by the user)\n"
        );
    }

    #[test]
    /// 失敗した診断項目があれば doctor の終了コードは失敗にする。
    fn completes_doctor_command_with_failure_exit_code_when_any_check_fails() {
        let mut output = Vec::new();
        let report = DoctorReport {
            checks: vec![DoctorCheckResult {
                name: "audio_source",
                status: DoctorCheckStatus::Failed,
                message: "application capture dependency check failed".to_string(),
                fix: DoctorFixAvailability::Manual {
                    reason: "recording permissions, running target application, and hardware access require user action",
                },
            }],
        };

        let exit_code = complete_doctor_command(&mut output, &report, false).unwrap();

        assert_eq!(exit_code, ExitCode::FAILURE);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "[fail] audio_source: application capture dependency check failed\n"
        );
    }
}
