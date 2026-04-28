mod load;
mod resolve;

use crate::application::TranscriptionLanguage;
use crate::domain::{SilenceRequestPolicy, TranscriptMergePolicy};
use dotenvy::Error as DotenvError;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 既定の `.env` ファイルパスです。
pub const DEFAULT_DOTENV_PATH: &str = ".env";
const DEFAULT_TRANSCRIPTION_LANGUAGE: &str = "ja";
const OPENAI_API_KEY_ENV_VAR: &str = "OPENAI_API_KEY";
const RECORDING_DURATION_SECONDS_ENV_VAR: &str = "DIARIZE_LOG_RECORDING_DURATION_SECONDS";
const CAPTURE_DURATION_SECONDS_ENV_VAR: &str = "DIARIZE_LOG_CAPTURE_DURATION_SECONDS";
const CAPTURE_OVERLAP_SECONDS_ENV_VAR: &str = "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS";
const CAPTURE_SILENCE_THRESHOLD_DBFS_ENV_VAR: &str = "DIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS";
const CAPTURE_SILENCE_MIN_DURATION_MS_ENV_VAR: &str = "DIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS";
const CAPTURE_TAIL_SILENCE_MIN_DURATION_MS_ENV_VAR: &str =
    "DIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS";
const SPEAKER_SAMPLE_DURATION_SECONDS_ENV_VAR: &str = "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS";
const TRANSCRIPTION_LANGUAGE_ENV_VAR: &str = "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE";
const DEBUG_ENV_VAR: &str = "DIARIZE_LOG_DEBUG";
const STORAGE_ROOT_ENV_VAR: &str = "DIARIZE_LOG_STORAGE_ROOT";
const MERGE_MIN_OVERLAP_CHARS_ENV_VAR: &str = "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS";
const MERGE_ALIGNMENT_RATIO_ENV_VAR: &str = "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO";
const MERGE_TRIGRAM_SIMILARITY_ENV_VAR: &str = "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY";
const TRANSCRIPTION_PIPELINE_ENV_VAR: &str = "DIARIZE_LOG_TRANSCRIPTION_PIPELINE";
const PYANNOTE_API_KEY_ENV_VAR: &str = "PYANNOTE_API_KEY";
const DIARIZATION_MAX_SPEAKERS_ENV_VAR: &str = "DIARIZE_LOG_DIARIZATION_MAX_SPEAKERS";

/// 文字起こし pipeline の選択です。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptionPipeline {
    Legacy,
    Separated,
}

impl fmt::Display for TranscriptionPipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Legacy => f.write_str("legacy"),
            Self::Separated => f.write_str("separated"),
        }
    }
}

/// 実行時設定の読み込み結果です。
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub openai_api_key: String,
    pub openai_api_key_source: ConfigSource,
    pub pyannote_api_key: Option<String>,
    pub recording_duration: Duration,
    pub capture_duration: Duration,
    pub capture_overlap: Duration,
    pub capture_silence_request_policy: SilenceRequestPolicy,
    pub speaker_sample_duration: Duration,
    pub transcription_language: TranscriptionLanguage,
    pub transcription_pipeline: TranscriptionPipeline,
    pub diarization_max_speakers: Option<u64>,
    pub transcript_merge_policy: TranscriptMergePolicy,
    pub debug_enabled: bool,
    pub storage_root: PathBuf,
}

impl Config {
    /// `.env` と環境変数から実行時設定を解決します。
    pub fn from_dotenv_path(dotenv_path: &Path) -> Result<Self, ConfigError> {
        let env_config = RawConfig::from_env();
        let dotenv_config = RawConfig::from_dotenv_path(dotenv_path)?;
        env_config.merge_missing(dotenv_config).validate()
    }
}

/// 設定値の取得元です。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    DotEnv,
    Environment,
}

impl fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DotEnv => f.write_str(".env"),
            Self::Environment => f.write_str("environment"),
        }
    }
}

/// 設定ロード時の失敗です。
#[derive(Debug)]
pub enum ConfigError {
    ReadDotEnv(DotenvError),
    InvalidConfig(Vec<ConfigValidationError>),
}

/// 設定値の検証失敗です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigValidationError {
    MissingRequiredValue {
        name: &'static str,
    },
    EmptyValue {
        name: &'static str,
        source: ConfigSource,
    },
    InvalidBooleanValue {
        name: &'static str,
        value: String,
        source: ConfigSource,
    },
    InvalidPositiveIntegerValue {
        name: &'static str,
        value: String,
        source: ConfigSource,
    },
    InvalidUnitIntervalValue {
        name: &'static str,
        value: String,
        source: ConfigSource,
    },
    InvalidTranscriptionLanguageModeValue {
        name: &'static str,
        value: String,
        source: ConfigSource,
    },
    InvalidTranscriptionPipelineValue {
        name: &'static str,
        value: String,
        source: ConfigSource,
    },
    InvalidNegativeDecibelValue {
        name: &'static str,
        value: String,
        source: ConfigSource,
    },
    InvalidCaptureOverlap {
        overlap_name: &'static str,
        capture_duration_name: &'static str,
        overlap_seconds: u64,
        capture_duration_seconds: u64,
        overlap_source: ConfigSource,
    },
    InvalidTailSilenceDuration {
        silence_name: &'static str,
        tail_silence_name: &'static str,
        silence_duration_ms: u64,
        tail_silence_duration_ms: u64,
        tail_silence_source: ConfigSource,
    },
    RelativePathValue {
        name: &'static str,
        value: String,
        source: ConfigSource,
    },
    MissingPyannoteApiKey {
        pipeline_name: &'static str,
        api_key_name: &'static str,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadDotEnv(source) => write!(f, "failed to read .env: {source}"),
            Self::InvalidConfig(errors) => {
                let messages = errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                write!(f, "invalid config: {messages}")
            }
        }
    }
}

impl fmt::Display for ConfigValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequiredValue { name } => write!(f, "missing required value: {name}"),
            Self::EmptyValue { name, source } => {
                write!(f, "empty value for {name} from {source}")
            }
            Self::InvalidBooleanValue {
                name,
                value,
                source,
            } => write!(f, "invalid boolean value for {name} from {source}: {value}"),
            Self::InvalidPositiveIntegerValue {
                name,
                value,
                source,
            } => write!(
                f,
                "invalid positive integer value for {name} from {source}: {value}"
            ),
            Self::InvalidUnitIntervalValue {
                name,
                value,
                source,
            } => write!(
                f,
                "invalid unit interval value for {name} from {source}: {value}"
            ),
            Self::InvalidTranscriptionLanguageModeValue {
                name,
                value,
                source,
            } => write!(
                f,
                "invalid transcription language mode value for {name} from {source}: {value}"
            ),
            Self::InvalidTranscriptionPipelineValue {
                name,
                value,
                source,
            } => write!(
                f,
                "invalid transcription pipeline value for {name} from {source}: {value}"
            ),
            Self::InvalidNegativeDecibelValue {
                name,
                value,
                source,
            } => write!(
                f,
                "invalid negative dBFS value for {name} from {source}: {value}"
            ),
            Self::InvalidCaptureOverlap {
                overlap_name,
                capture_duration_name,
                overlap_seconds,
                capture_duration_seconds,
                overlap_source,
            } => write!(
                f,
                "{overlap_name} from {overlap_source} must be smaller than {capture_duration_name}: overlap_seconds={overlap_seconds} capture_duration_seconds={capture_duration_seconds}"
            ),
            Self::InvalidTailSilenceDuration {
                silence_name,
                tail_silence_name,
                silence_duration_ms,
                tail_silence_duration_ms,
                tail_silence_source,
            } => write!(
                f,
                "{tail_silence_name} from {tail_silence_source} must be smaller than or equal to {silence_name}: tail_silence_duration_ms={tail_silence_duration_ms} silence_duration_ms={silence_duration_ms}"
            ),
            Self::RelativePathValue {
                name,
                value,
                source,
            } => write!(
                f,
                "relative path is not allowed for {name} from {source}: {value}"
            ),
            Self::MissingPyannoteApiKey {
                pipeline_name,
                api_key_name,
            } => write!(
                f,
                "{api_key_name} is required when {pipeline_name}=separated"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigValue<T> {
    value: T,
    source: ConfigSource,
}

impl<T> ConfigValue<T> {
    fn new(value: T, source: ConfigSource) -> Self {
        Self { value, source }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RawConfig {
    openai_api_key: Option<ConfigValue<String>>,
    pyannote_api_key: Option<ConfigValue<String>>,
    recording_duration_seconds: Option<ConfigValue<String>>,
    capture_duration_seconds: Option<ConfigValue<String>>,
    capture_overlap_seconds: Option<ConfigValue<String>>,
    capture_silence_threshold_dbfs: Option<ConfigValue<String>>,
    capture_silence_min_duration_ms: Option<ConfigValue<String>>,
    capture_tail_silence_min_duration_ms: Option<ConfigValue<String>>,
    speaker_sample_duration_seconds: Option<ConfigValue<String>>,
    transcription_language: Option<ConfigValue<String>>,
    transcription_pipeline: Option<ConfigValue<String>>,
    diarization_max_speakers: Option<ConfigValue<String>>,
    merge_min_overlap_chars: Option<ConfigValue<String>>,
    merge_alignment_ratio: Option<ConfigValue<String>>,
    merge_trigram_similarity: Option<ConfigValue<String>>,
    debug_enabled: Option<ConfigValue<String>>,
    storage_root: Option<ConfigValue<String>>,
}

#[cfg(test)]
mod tests {
    use super::{Config, ConfigError, ConfigSource, ConfigValidationError, TranscriptionPipeline};
    use crate::application::TranscriptionLanguage;
    use crate::domain::{SilenceRequestPolicy, TranscriptMergePolicy};
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    #[test]
    /// 環境変数は .env より優先し、追加した capture 設定も解決する。
    fn prefers_environment_variables_over_dotenv_for_required_values() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=18\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=3\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS=14\nDIARIZE_LOG_MERGE_ALIGNMENT_RATIO=0.9\nDIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY=0.7\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();

        let original = std::env::var_os("OPENAI_API_KEY");
        let original_duration = std::env::var_os("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
        let original_capture_duration = std::env::var_os("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
        let original_capture_overlap = std::env::var_os("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
        let original_sample_duration =
            std::env::var_os("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
        let original_transcription_language =
            std::env::var_os("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE");
        let original_merge_min_overlap_chars =
            std::env::var_os("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
        let original_merge_alignment_ratio = std::env::var_os("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
        let original_merge_trigram_similarity =
            std::env::var_os("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
        let original_storage_root = std::env::var_os("DIARIZE_LOG_STORAGE_ROOT");
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "from-env");
            std::env::set_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS", "45");
            std::env::set_var("DIARIZE_LOG_CAPTURE_DURATION_SECONDS", "20");
            std::env::set_var("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS", "5");
            std::env::set_var("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS", "8");
            std::env::set_var("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE", "en");
            std::env::set_var("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS", "11");
            std::env::set_var("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO", "0.85");
            std::env::set_var("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY", "0.6");
            std::env::set_var("DIARIZE_LOG_STORAGE_ROOT", storage_root.as_os_str());
        }

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_var("OPENAI_API_KEY", original);
        restore_env_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS", original_duration);
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
            original_capture_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
            original_capture_overlap,
        );
        restore_env_var(
            "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS",
            original_sample_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE",
            original_transcription_language,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            original_merge_min_overlap_chars,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            original_merge_alignment_ratio,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            original_merge_trigram_similarity,
        );
        restore_env_var("DIARIZE_LOG_STORAGE_ROOT", original_storage_root);
        assert_eq!(config.openai_api_key, "from-env");
        assert_eq!(config.openai_api_key_source, ConfigSource::Environment);
        assert_eq!(config.recording_duration, Duration::from_secs(45));
        assert_eq!(config.capture_duration, Duration::from_secs(20));
        assert_eq!(config.capture_overlap, Duration::from_secs(5));
        assert_eq!(config.speaker_sample_duration, Duration::from_secs(8));
        assert_eq!(
            config.transcription_language,
            TranscriptionLanguage::Fixed("en".to_string())
        );
        assert_eq!(config.transcript_merge_policy.min_overlap_chars, 11);
        assert_eq!(config.transcript_merge_policy.min_alignment_ratio, 0.85);
        assert_eq!(config.transcript_merge_policy.min_trigram_similarity, 0.6);
        assert!(!config.debug_enabled);
        assert_eq!(config.storage_root, storage_root);
    }

    #[test]
    /// 環境変数が無ければ .env の必須設定を解決する。
    fn resolves_required_values_from_dotenv() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=12\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS=12\nDIARIZE_LOG_MERGE_ALIGNMENT_RATIO=0.88\nDIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY=0.66\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();

        let original_api_key = std::env::var_os("OPENAI_API_KEY");
        let original_duration = std::env::var_os("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
        let original_capture_duration = std::env::var_os("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
        let original_capture_overlap = std::env::var_os("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
        let original_sample_duration =
            std::env::var_os("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
        let original_transcription_language =
            std::env::var_os("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE");
        let original_merge_min_overlap_chars =
            std::env::var_os("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
        let original_merge_alignment_ratio = std::env::var_os("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
        let original_merge_trigram_similarity =
            std::env::var_os("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
        let original_storage_root = std::env::var_os("DIARIZE_LOG_STORAGE_ROOT");
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
            std::env::remove_var("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE");
            std::env::remove_var("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
            std::env::remove_var("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
            std::env::remove_var("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
            std::env::remove_var("DIARIZE_LOG_STORAGE_ROOT");
        }

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_var("OPENAI_API_KEY", original_api_key);
        restore_env_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS", original_duration);
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
            original_capture_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
            original_capture_overlap,
        );
        restore_env_var(
            "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS",
            original_sample_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE",
            original_transcription_language,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            original_merge_min_overlap_chars,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            original_merge_alignment_ratio,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            original_merge_trigram_similarity,
        );
        restore_env_var("DIARIZE_LOG_STORAGE_ROOT", original_storage_root);
        assert_eq!(config.openai_api_key, "from-dotenv");
        assert_eq!(config.openai_api_key_source, ConfigSource::DotEnv);
        assert_eq!(config.recording_duration, Duration::from_secs(30));
        assert_eq!(config.capture_duration, Duration::from_secs(12));
        assert_eq!(config.capture_overlap, Duration::from_secs(2));
        assert_eq!(config.speaker_sample_duration, Duration::from_secs(6));
        assert_eq!(
            config.transcription_language,
            TranscriptionLanguage::Fixed("ja".to_string())
        );
        assert_eq!(config.transcript_merge_policy.min_overlap_chars, 12);
        assert_eq!(config.transcript_merge_policy.min_alignment_ratio, 0.88);
        assert_eq!(config.transcript_merge_policy.min_trigram_similarity, 0.66);
        assert_eq!(config.storage_root, storage_root);
    }

    #[test]
    /// transcription language は .env から読み込み、許可値ならそのまま解決する。
    fn resolves_transcription_language_from_dotenv() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=12\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_TRANSCRIPTION_LANGUAGE=en\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();

        let original_language = std::env::var_os("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE");
        }

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_var("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE", original_language);
        assert_eq!(
            config.transcription_language,
            TranscriptionLanguage::Fixed("en".to_string())
        );
    }

    #[test]
    /// separated pipeline では pyannote API key と任意の max speakers を解決する。
    fn resolves_separated_pipeline_settings_from_dotenv() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nPYANNOTE_API_KEY=pyannote-key\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=12\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_TRANSCRIPTION_PIPELINE=separated\nDIARIZE_LOG_DIARIZATION_MAX_SPEAKERS=4\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();

        let env_names = [
            "OPENAI_API_KEY",
            "PYANNOTE_API_KEY",
            "DIARIZE_LOG_RECORDING_DURATION_SECONDS",
            "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
            "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
            "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS",
            "DIARIZE_LOG_TRANSCRIPTION_PIPELINE",
            "DIARIZE_LOG_DIARIZATION_MAX_SPEAKERS",
            "DIARIZE_LOG_STORAGE_ROOT",
        ];
        let originals = snapshot_env_vars(&env_names);
        clear_env_vars(&env_names);

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_vars(originals);
        assert_eq!(
            config.transcription_pipeline,
            TranscriptionPipeline::Separated
        );
        assert_eq!(config.pyannote_api_key, Some("pyannote-key".to_string()));
        assert_eq!(config.diarization_max_speakers, Some(4));
    }

    #[test]
    /// config 解決時点では CLI override 前なので legacy と diarization max speakers の組み合わせを保持する。
    fn resolves_legacy_pipeline_with_diarization_max_speakers_before_cli_override() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=12\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_TRANSCRIPTION_PIPELINE=legacy\nDIARIZE_LOG_DIARIZATION_MAX_SPEAKERS=4\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();

        let env_names = [
            "OPENAI_API_KEY",
            "DIARIZE_LOG_RECORDING_DURATION_SECONDS",
            "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
            "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
            "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS",
            "DIARIZE_LOG_TRANSCRIPTION_PIPELINE",
            "DIARIZE_LOG_DIARIZATION_MAX_SPEAKERS",
            "DIARIZE_LOG_STORAGE_ROOT",
        ];
        let originals = snapshot_env_vars(&env_names);
        clear_env_vars(&env_names);

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_vars(originals);
        assert_eq!(config.transcription_pipeline, TranscriptionPipeline::Legacy);
        assert_eq!(config.diarization_max_speakers, Some(4));
    }

    #[test]
    /// separated pipeline で pyannote API key が無い場合は設定エラーにする。
    fn returns_error_when_separated_pipeline_has_no_pyannote_api_key() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=12\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_TRANSCRIPTION_PIPELINE=separated\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();

        let env_names = [
            "OPENAI_API_KEY",
            "PYANNOTE_API_KEY",
            "DIARIZE_LOG_RECORDING_DURATION_SECONDS",
            "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
            "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
            "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS",
            "DIARIZE_LOG_TRANSCRIPTION_PIPELINE",
            "DIARIZE_LOG_STORAGE_ROOT",
        ];
        let originals = snapshot_env_vars(&env_names);
        clear_env_vars(&env_names);

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_vars(originals);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::MissingPyannoteApiKey {
                pipeline_name: "DIARIZE_LOG_TRANSCRIPTION_PIPELINE",
                api_key_name: "PYANNOTE_API_KEY",
            }]
        ));
    }

    #[test]
    /// 必須設定が欠けると不足しているキーをまとめて返す。
    fn returns_error_when_required_values_are_missing_everywhere() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let original_api_key = std::env::var_os("OPENAI_API_KEY");
        let original_duration = std::env::var_os("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
        let original_capture_duration = std::env::var_os("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
        let original_capture_overlap = std::env::var_os("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
        let original_sample_duration =
            std::env::var_os("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
        let original_merge_min_overlap_chars =
            std::env::var_os("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
        let original_merge_alignment_ratio = std::env::var_os("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
        let original_merge_trigram_similarity =
            std::env::var_os("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
        let original_storage_root = std::env::var_os("DIARIZE_LOG_STORAGE_ROOT");
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
            std::env::remove_var("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
            std::env::remove_var("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
            std::env::remove_var("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
            std::env::remove_var("DIARIZE_LOG_STORAGE_ROOT");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("OPENAI_API_KEY", original_api_key);
        restore_env_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS", original_duration);
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
            original_capture_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
            original_capture_overlap,
        );
        restore_env_var(
            "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS",
            original_sample_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            original_merge_min_overlap_chars,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            original_merge_alignment_ratio,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            original_merge_trigram_similarity,
        );
        restore_env_var("DIARIZE_LOG_STORAGE_ROOT", original_storage_root);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![
                ConfigValidationError::MissingRequiredValue {
                    name: "OPENAI_API_KEY"
                },
                ConfigValidationError::MissingRequiredValue {
                    name: "DIARIZE_LOG_RECORDING_DURATION_SECONDS"
                },
                ConfigValidationError::MissingRequiredValue {
                    name: "DIARIZE_LOG_CAPTURE_DURATION_SECONDS"
                },
                ConfigValidationError::MissingRequiredValue {
                    name: "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS"
                },
                ConfigValidationError::MissingRequiredValue {
                    name: "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS"
                },
                ConfigValidationError::MissingRequiredValue {
                    name: "DIARIZE_LOG_STORAGE_ROOT"
                },
            ]
        ));
    }

    #[test]
    /// overlap は capture 長より小さくなければ設定エラーにする。
    fn returns_error_when_capture_overlap_is_not_smaller_than_capture_duration() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=15\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=15\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_duration = std::env::var_os("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
        let original_capture_duration = std::env::var_os("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
        let original_capture_overlap = std::env::var_os("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
        let original_sample_duration =
            std::env::var_os("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
        let original_merge_min_overlap_chars =
            std::env::var_os("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
        let original_merge_alignment_ratio = std::env::var_os("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
        let original_merge_trigram_similarity =
            std::env::var_os("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
        let original_storage_root = std::env::var_os("DIARIZE_LOG_STORAGE_ROOT");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
            std::env::remove_var("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
            std::env::remove_var("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
            std::env::remove_var("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
            std::env::remove_var("DIARIZE_LOG_STORAGE_ROOT");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS", original_duration);
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
            original_capture_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
            original_capture_overlap,
        );
        restore_env_var(
            "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS",
            original_sample_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            original_merge_min_overlap_chars,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            original_merge_alignment_ratio,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            original_merge_trigram_similarity,
        );
        restore_env_var("DIARIZE_LOG_STORAGE_ROOT", original_storage_root);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::InvalidCaptureOverlap {
                overlap_name: "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
                capture_duration_name: "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
                overlap_seconds: 15,
                capture_duration_seconds: 15,
                overlap_source: ConfigSource::DotEnv,
            }]
        ));
    }

    #[test]
    /// tail 側の必要無音長が通常時より長いと、無音分割ルールが逆転するので設定エラーにする。
    fn returns_error_when_tail_silence_duration_exceeds_silence_duration() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=15\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=3\nDIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS=700\nDIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS=750\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_duration = std::env::var_os("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
        let original_capture_duration = std::env::var_os("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
        let original_capture_overlap = std::env::var_os("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
        let original_capture_silence_min_duration =
            std::env::var_os("DIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS");
        let original_capture_tail_silence_min_duration =
            std::env::var_os("DIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS");
        let original_sample_duration =
            std::env::var_os("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
        let original_merge_min_overlap_chars =
            std::env::var_os("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
        let original_merge_alignment_ratio = std::env::var_os("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
        let original_merge_trigram_similarity =
            std::env::var_os("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
        let original_storage_root = std::env::var_os("DIARIZE_LOG_STORAGE_ROOT");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS");
            std::env::remove_var("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
            std::env::remove_var("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
            std::env::remove_var("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
            std::env::remove_var("DIARIZE_LOG_STORAGE_ROOT");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS", original_duration);
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
            original_capture_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
            original_capture_overlap,
        );
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS",
            original_capture_silence_min_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS",
            original_capture_tail_silence_min_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS",
            original_sample_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            original_merge_min_overlap_chars,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            original_merge_alignment_ratio,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            original_merge_trigram_similarity,
        );
        restore_env_var("DIARIZE_LOG_STORAGE_ROOT", original_storage_root);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::InvalidTailSilenceDuration {
                silence_name: "DIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS",
                tail_silence_name: "DIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS",
                silence_duration_ms: 700,
                tail_silence_duration_ms: 750,
                tail_silence_source: ConfigSource::DotEnv,
            }]
        ));
    }

    #[test]
    /// 保存先に相対パスを指定すると設定エラーにする。
    fn returns_error_when_storage_root_is_relative_path() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        std::fs::write(
            &dotenv_path,
            "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=10\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_STORAGE_ROOT=./storage\n",
        )
        .unwrap();
        let original_duration = std::env::var_os("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
        let original_capture_duration = std::env::var_os("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
        let original_capture_overlap = std::env::var_os("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
        let original_sample_duration =
            std::env::var_os("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
        let original_merge_min_overlap_chars =
            std::env::var_os("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
        let original_merge_alignment_ratio = std::env::var_os("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
        let original_merge_trigram_similarity =
            std::env::var_os("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
        let original_storage_root = std::env::var_os("DIARIZE_LOG_STORAGE_ROOT");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
            std::env::remove_var("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
            std::env::remove_var("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
            std::env::remove_var("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
            std::env::remove_var("DIARIZE_LOG_STORAGE_ROOT");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS", original_duration);
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
            original_capture_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
            original_capture_overlap,
        );
        restore_env_var(
            "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS",
            original_sample_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            original_merge_min_overlap_chars,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            original_merge_alignment_ratio,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            original_merge_trigram_similarity,
        );
        restore_env_var("DIARIZE_LOG_STORAGE_ROOT", original_storage_root);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::RelativePathValue {
                name: "DIARIZE_LOG_STORAGE_ROOT",
                value: "./storage".to_string(),
                source: ConfigSource::DotEnv,
            }]
        ));
    }

    #[test]
    /// 空文字の環境変数があれば .env に妥当な値があってもフォールバックしない。
    fn does_not_fall_back_to_dotenv_when_environment_variable_is_empty() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=10\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_DEBUG=false\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_api_key = std::env::var_os("OPENAI_API_KEY");
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("OPENAI_API_KEY", original_api_key);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::EmptyValue {
                name: "OPENAI_API_KEY",
                source: ConfigSource::Environment,
            }]
        ));
    }

    #[test]
    /// transcription language が空白だけなら設定エラーにする。
    fn returns_error_when_transcription_language_is_blank() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=10\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_TRANSCRIPTION_LANGUAGE=   \nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_language = std::env::var_os("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE", original_language);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::EmptyValue {
                name: "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE",
                source: ConfigSource::DotEnv,
            }]
        ));
    }

    #[test]
    /// transcription language は日本語と英語と auto だけを許可する。
    fn returns_error_for_unsupported_transcription_language() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=10\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_TRANSCRIPTION_LANGUAGE=fr\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_language = std::env::var_os("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE", original_language);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::InvalidTranscriptionLanguageModeValue {
                name: "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE",
                value: "fr".to_string(),
                source: ConfigSource::DotEnv,
            }]
        ));
    }

    #[test]
    /// transcription language に auto を指定したら自動判定モードとして解決する。
    fn resolves_auto_transcription_language_from_dotenv() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=10\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_TRANSCRIPTION_LANGUAGE=auto\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_language = std::env::var_os("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE");
        }

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_var("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE", original_language);
        assert_eq!(config.transcription_language, TranscriptionLanguage::Auto);
    }

    #[test]
    /// .env が存在しなくても必要な設定が環境変数に揃っていれば解決する。
    fn resolves_required_values_from_environment_when_dotenv_file_is_missing() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join("missing.env");
        let storage_root = sample_storage_root(temp_dir.path());
        let original_vars = snapshot_env_vars(all_config_env_var_names());
        clear_env_vars(all_config_env_var_names());
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "from-env");
            std::env::set_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS", "30");
            std::env::set_var("DIARIZE_LOG_CAPTURE_DURATION_SECONDS", "10");
            std::env::set_var("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS", "2");
            std::env::set_var("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS", "6");
            std::env::set_var("DIARIZE_LOG_STORAGE_ROOT", storage_root.as_os_str());
        }

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_vars(original_vars);
        assert_eq!(config.openai_api_key, "from-env");
        assert_eq!(config.openai_api_key_source, ConfigSource::Environment);
        assert_eq!(config.recording_duration, Duration::from_secs(30));
        assert_eq!(config.capture_duration, Duration::from_secs(10));
        assert_eq!(config.capture_overlap, Duration::from_secs(2));
        assert_eq!(config.speaker_sample_duration, Duration::from_secs(6));
        assert_eq!(config.storage_root, storage_root);
    }

    #[test]
    /// optional 設定が未指定なら既定の推奨値で解決する。
    fn resolves_recommended_defaults_for_optional_values() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=10\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_vars = snapshot_env_vars(&[
            "DIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS",
            "DIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS",
            "DIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS",
            "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE",
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            "DIARIZE_LOG_DEBUG",
        ]);
        clear_env_vars(&[
            "DIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS",
            "DIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS",
            "DIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS",
            "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE",
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            "DIARIZE_LOG_DEBUG",
        ]);

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_vars(original_vars);
        assert_eq!(
            config.capture_silence_request_policy,
            SilenceRequestPolicy::recommended()
        );
        assert_eq!(
            config.transcription_language,
            TranscriptionLanguage::Fixed("ja".to_string())
        );
        assert_eq!(
            config.transcript_merge_policy,
            TranscriptMergePolicy::recommended()
        );
        assert!(!config.debug_enabled);
    }

    #[test]
    /// optional 設定も環境変数が .env より優先される。
    fn prefers_environment_variables_over_dotenv_for_optional_values() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=10\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS=-42.0\nDIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS=800\nDIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS=400\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_TRANSCRIPTION_LANGUAGE=ja\nDIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS=14\nDIARIZE_LOG_MERGE_ALIGNMENT_RATIO=0.9\nDIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY=0.7\nDIARIZE_LOG_DEBUG=false\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_vars = snapshot_env_vars(&[
            "DIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS",
            "DIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS",
            "DIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS",
            "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE",
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            "DIARIZE_LOG_DEBUG",
        ]);
        clear_env_vars(&[
            "DIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS",
            "DIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS",
            "DIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS",
            "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE",
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            "DIARIZE_LOG_DEBUG",
        ]);
        unsafe {
            std::env::set_var("DIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS", "-30.5");
            std::env::set_var("DIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS", "900");
            std::env::set_var("DIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS", "450");
            std::env::set_var("DIARIZE_LOG_TRANSCRIPTION_LANGUAGE", "auto");
            std::env::set_var("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS", "11");
            std::env::set_var("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO", "0.85");
            std::env::set_var("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY", "0.6");
            std::env::set_var("DIARIZE_LOG_DEBUG", "true");
        }

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_vars(original_vars);
        assert_eq!(
            config.capture_silence_request_policy.silence_threshold_dbfs,
            -30.5
        );
        assert_eq!(
            config.capture_silence_request_policy.silence_min_duration,
            Duration::from_millis(900)
        );
        assert_eq!(
            config
                .capture_silence_request_policy
                .tail_silence_min_duration,
            Duration::from_millis(450)
        );
        assert_eq!(config.transcription_language, TranscriptionLanguage::Auto);
        assert_eq!(config.transcript_merge_policy.min_overlap_chars, 11);
        assert_eq!(config.transcript_merge_policy.min_alignment_ratio, 0.85);
        assert_eq!(config.transcript_merge_policy.min_trigram_similarity, 0.6);
        assert!(config.debug_enabled);
    }

    #[test]
    /// 正の整数が必要な設定に不正値を入れると設定エラーにする。
    fn returns_error_for_invalid_positive_integer_value() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=abc\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=10\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_duration = std::env::var_os("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS", original_duration);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::InvalidPositiveIntegerValue {
                name: "DIARIZE_LOG_RECORDING_DURATION_SECONDS",
                value: "abc".to_string(),
                source: ConfigSource::DotEnv,
            }]
        ));
    }

    #[test]
    /// 0 以上 1 以下が必要な設定に範囲外の値を入れると設定エラーにする。
    fn returns_error_for_invalid_unit_interval_value() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=10\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_MERGE_ALIGNMENT_RATIO=1.2\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_ratio = std::env::var_os("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO", original_ratio);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::InvalidUnitIntervalValue {
                name: "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
                value: "1.2".to_string(),
                source: ConfigSource::DotEnv,
            }]
        ));
    }

    #[test]
    /// 負の dBFS が必要な設定に 0 以上の値を入れると設定エラーにする。
    fn returns_error_for_invalid_negative_decibel_value() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=10\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS=0\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_threshold = std::env::var_os("DIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var(
            "DIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS",
            original_threshold,
        );
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::InvalidNegativeDecibelValue {
                name: "DIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS",
                value: "0".to_string(),
                source: ConfigSource::DotEnv,
            }]
        ));
    }

    #[test]
    #[cfg(unix)]
    /// .env が未発見以外の I/O 失敗なら読み込みエラーとして返す。
    fn returns_error_for_unreadable_dotenv_path() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let protected_dir = temp_dir.path().join("protected");
        fs::create_dir(&protected_dir).unwrap();
        fs::set_permissions(&protected_dir, fs::Permissions::from_mode(0o000)).unwrap();
        let dotenv_path = protected_dir.join(".env");
        let original_vars = snapshot_env_vars(all_config_env_var_names());
        clear_env_vars(all_config_env_var_names());

        let result = Config::from_dotenv_path(&dotenv_path);

        fs::set_permissions(&protected_dir, fs::Permissions::from_mode(0o755)).unwrap();
        restore_env_vars(original_vars);
        assert!(matches!(result, Err(ConfigError::ReadDotEnv(_))));
    }

    #[test]
    /// debug 値が不正なら設定エラーにする。
    fn returns_error_for_invalid_boolean_value() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let storage_root = sample_storage_root(temp_dir.path());
        std::fs::write(
            &dotenv_path,
            format!(
                "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_RECORDING_DURATION_SECONDS=30\nDIARIZE_LOG_CAPTURE_DURATION_SECONDS=10\nDIARIZE_LOG_CAPTURE_OVERLAP_SECONDS=2\nDIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS=6\nDIARIZE_LOG_DEBUG=maybe\nDIARIZE_LOG_STORAGE_ROOT={}\n",
                storage_root.display()
            ),
        )
        .unwrap();
        let original_duration = std::env::var_os("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
        let original_capture_duration = std::env::var_os("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
        let original_capture_overlap = std::env::var_os("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
        let original_sample_duration =
            std::env::var_os("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
        let original_merge_min_overlap_chars =
            std::env::var_os("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
        let original_merge_alignment_ratio = std::env::var_os("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
        let original_merge_trigram_similarity =
            std::env::var_os("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
        let original_debug = std::env::var_os("DIARIZE_LOG_DEBUG");
        let original_storage_root = std::env::var_os("DIARIZE_LOG_STORAGE_ROOT");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS");
            std::env::remove_var("DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS");
            std::env::remove_var("DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS");
            std::env::remove_var("DIARIZE_LOG_MERGE_ALIGNMENT_RATIO");
            std::env::remove_var("DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY");
            std::env::remove_var("DIARIZE_LOG_DEBUG");
            std::env::remove_var("DIARIZE_LOG_STORAGE_ROOT");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("DIARIZE_LOG_RECORDING_DURATION_SECONDS", original_duration);
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
            original_capture_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
            original_capture_overlap,
        );
        restore_env_var(
            "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS",
            original_sample_duration,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            original_merge_min_overlap_chars,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            original_merge_alignment_ratio,
        );
        restore_env_var(
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            original_merge_trigram_similarity,
        );
        restore_env_var("DIARIZE_LOG_DEBUG", original_debug);
        restore_env_var("DIARIZE_LOG_STORAGE_ROOT", original_storage_root);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::InvalidBooleanValue {
                name: "DIARIZE_LOG_DEBUG",
                value: "maybe".to_string(),
                source: ConfigSource::DotEnv,
            }]
        ));
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

    fn restore_env_vars(originals: Vec<(&'static str, Option<std::ffi::OsString>)>) {
        for (name, original) in originals {
            restore_env_var(name, original);
        }
    }

    fn snapshot_env_vars(
        names: &[&'static str],
    ) -> Vec<(&'static str, Option<std::ffi::OsString>)> {
        names
            .iter()
            .map(|&name| (name, std::env::var_os(name)))
            .collect()
    }

    fn clear_env_vars(names: &[&str]) {
        for name in names {
            unsafe {
                std::env::remove_var(name);
            }
        }
    }

    fn all_config_env_var_names() -> &'static [&'static str] {
        &[
            "OPENAI_API_KEY",
            "DIARIZE_LOG_RECORDING_DURATION_SECONDS",
            "DIARIZE_LOG_CAPTURE_DURATION_SECONDS",
            "DIARIZE_LOG_CAPTURE_OVERLAP_SECONDS",
            "DIARIZE_LOG_CAPTURE_SILENCE_THRESHOLD_DBFS",
            "DIARIZE_LOG_CAPTURE_SILENCE_MIN_DURATION_MS",
            "DIARIZE_LOG_CAPTURE_TAIL_SILENCE_MIN_DURATION_MS",
            "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS",
            "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE",
            "DIARIZE_LOG_TRANSCRIPTION_PIPELINE",
            "PYANNOTE_API_KEY",
            "DIARIZE_LOG_DIARIZATION_MAX_SPEAKERS",
            "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS",
            "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO",
            "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY",
            "DIARIZE_LOG_DEBUG",
            "DIARIZE_LOG_STORAGE_ROOT",
        ]
    }

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn sample_storage_root(base_dir: &Path) -> PathBuf {
        base_dir.join("storage-root")
    }
}
