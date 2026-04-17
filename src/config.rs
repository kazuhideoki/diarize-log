use crate::domain::TranscriptMergePolicy;
use dotenvy::{Error as DotenvError, from_filename_iter};
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
const SPEAKER_SAMPLE_DURATION_SECONDS_ENV_VAR: &str = "DIARIZE_LOG_SPEAKER_SAMPLE_DURATION_SECONDS";
const TRANSCRIPTION_LANGUAGE_ENV_VAR: &str = "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE";
const DEBUG_ENV_VAR: &str = "DIARIZE_LOG_DEBUG";
const STORAGE_ROOT_ENV_VAR: &str = "DIARIZE_LOG_STORAGE_ROOT";
const MERGE_MIN_OVERLAP_CHARS_ENV_VAR: &str = "DIARIZE_LOG_MERGE_MIN_OVERLAP_CHARS";
const MERGE_ALIGNMENT_RATIO_ENV_VAR: &str = "DIARIZE_LOG_MERGE_ALIGNMENT_RATIO";
const MERGE_TRIGRAM_SIMILARITY_ENV_VAR: &str = "DIARIZE_LOG_MERGE_TRIGRAM_SIMILARITY";

/// 実行時設定の読み込み結果です。
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub openai_api_key: String,
    pub openai_api_key_source: ConfigSource,
    pub recording_duration: Duration,
    pub capture_duration: Duration,
    pub capture_overlap: Duration,
    pub speaker_sample_duration: Duration,
    pub transcription_language: String,
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
    InvalidTranscriptionLanguageValue {
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
    RelativePathValue {
        name: &'static str,
        value: String,
        source: ConfigSource,
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
            Self::InvalidTranscriptionLanguageValue {
                name,
                value,
                source,
            } => write!(
                f,
                "invalid transcription language value for {name} from {source}: {value}"
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
            Self::RelativePathValue {
                name,
                value,
                source,
            } => write!(
                f,
                "relative path is not allowed for {name} from {source}: {value}"
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
    recording_duration_seconds: Option<ConfigValue<String>>,
    capture_duration_seconds: Option<ConfigValue<String>>,
    capture_overlap_seconds: Option<ConfigValue<String>>,
    speaker_sample_duration_seconds: Option<ConfigValue<String>>,
    transcription_language: Option<ConfigValue<String>>,
    merge_min_overlap_chars: Option<ConfigValue<String>>,
    merge_alignment_ratio: Option<ConfigValue<String>>,
    merge_trigram_similarity: Option<ConfigValue<String>>,
    debug_enabled: Option<ConfigValue<String>>,
    storage_root: Option<ConfigValue<String>>,
}

impl RawConfig {
    fn from_env() -> Self {
        Self {
            openai_api_key: read_env_var(OPENAI_API_KEY_ENV_VAR, ConfigSource::Environment),
            recording_duration_seconds: read_env_var(
                RECORDING_DURATION_SECONDS_ENV_VAR,
                ConfigSource::Environment,
            ),
            capture_duration_seconds: read_env_var(
                CAPTURE_DURATION_SECONDS_ENV_VAR,
                ConfigSource::Environment,
            ),
            capture_overlap_seconds: read_env_var(
                CAPTURE_OVERLAP_SECONDS_ENV_VAR,
                ConfigSource::Environment,
            ),
            speaker_sample_duration_seconds: read_env_var(
                SPEAKER_SAMPLE_DURATION_SECONDS_ENV_VAR,
                ConfigSource::Environment,
            ),
            transcription_language: read_env_var(
                TRANSCRIPTION_LANGUAGE_ENV_VAR,
                ConfigSource::Environment,
            ),
            merge_min_overlap_chars: read_env_var(
                MERGE_MIN_OVERLAP_CHARS_ENV_VAR,
                ConfigSource::Environment,
            ),
            merge_alignment_ratio: read_env_var(
                MERGE_ALIGNMENT_RATIO_ENV_VAR,
                ConfigSource::Environment,
            ),
            merge_trigram_similarity: read_env_var(
                MERGE_TRIGRAM_SIMILARITY_ENV_VAR,
                ConfigSource::Environment,
            ),
            debug_enabled: read_env_var(DEBUG_ENV_VAR, ConfigSource::Environment),
            storage_root: read_env_var(STORAGE_ROOT_ENV_VAR, ConfigSource::Environment),
        }
    }

    fn from_dotenv_path(dotenv_path: &Path) -> Result<Self, ConfigError> {
        let mut raw = Self::default();

        match from_filename_iter(dotenv_path) {
            Ok(iter) => {
                for item in iter {
                    let (key, value) = item.map_err(ConfigError::ReadDotEnv)?;
                    match key.as_str() {
                        OPENAI_API_KEY_ENV_VAR => {
                            raw.openai_api_key = Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        RECORDING_DURATION_SECONDS_ENV_VAR => {
                            raw.recording_duration_seconds =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        CAPTURE_DURATION_SECONDS_ENV_VAR => {
                            raw.capture_duration_seconds =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        CAPTURE_OVERLAP_SECONDS_ENV_VAR => {
                            raw.capture_overlap_seconds =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        SPEAKER_SAMPLE_DURATION_SECONDS_ENV_VAR => {
                            raw.speaker_sample_duration_seconds =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        TRANSCRIPTION_LANGUAGE_ENV_VAR => {
                            raw.transcription_language =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        MERGE_MIN_OVERLAP_CHARS_ENV_VAR => {
                            raw.merge_min_overlap_chars =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        MERGE_ALIGNMENT_RATIO_ENV_VAR => {
                            raw.merge_alignment_ratio =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        MERGE_TRIGRAM_SIMILARITY_ENV_VAR => {
                            raw.merge_trigram_similarity =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        DEBUG_ENV_VAR => {
                            raw.debug_enabled = Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        STORAGE_ROOT_ENV_VAR => {
                            raw.storage_root = Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        _ => {}
                    }
                }

                Ok(raw)
            }
            Err(DotenvError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(raw),
            Err(error) => Err(ConfigError::ReadDotEnv(error)),
        }
    }

    fn merge_missing(self, fallback: Self) -> Self {
        Self {
            openai_api_key: self.openai_api_key.or(fallback.openai_api_key),
            recording_duration_seconds: self
                .recording_duration_seconds
                .or(fallback.recording_duration_seconds),
            capture_duration_seconds: self
                .capture_duration_seconds
                .or(fallback.capture_duration_seconds),
            capture_overlap_seconds: self
                .capture_overlap_seconds
                .or(fallback.capture_overlap_seconds),
            speaker_sample_duration_seconds: self
                .speaker_sample_duration_seconds
                .or(fallback.speaker_sample_duration_seconds),
            transcription_language: self
                .transcription_language
                .or(fallback.transcription_language),
            merge_min_overlap_chars: self
                .merge_min_overlap_chars
                .or(fallback.merge_min_overlap_chars),
            merge_alignment_ratio: self
                .merge_alignment_ratio
                .or(fallback.merge_alignment_ratio),
            merge_trigram_similarity: self
                .merge_trigram_similarity
                .or(fallback.merge_trigram_similarity),
            debug_enabled: self.debug_enabled.or(fallback.debug_enabled),
            storage_root: self.storage_root.or(fallback.storage_root),
        }
    }

    fn validate(self) -> Result<Config, ConfigError> {
        let mut errors = Vec::new();

        let openai_api_key = match self.openai_api_key {
            Some(value) => {
                if value.value.trim().is_empty() {
                    errors.push(ConfigValidationError::EmptyValue {
                        name: OPENAI_API_KEY_ENV_VAR,
                        source: value.source,
                    });
                }
                Some(value)
            }
            None => {
                errors.push(ConfigValidationError::MissingRequiredValue {
                    name: OPENAI_API_KEY_ENV_VAR,
                });
                None
            }
        };

        let recording_duration = match self.recording_duration_seconds {
            Some(value) => {
                match parse_positive_integer(value, RECORDING_DURATION_SECONDS_ENV_VAR) {
                    Ok(seconds) => Some(Duration::from_secs(seconds)),
                    Err(error) => {
                        errors.push(error);
                        None
                    }
                }
            }
            None => {
                errors.push(ConfigValidationError::MissingRequiredValue {
                    name: RECORDING_DURATION_SECONDS_ENV_VAR,
                });
                None
            }
        };

        let capture_duration = match self.capture_duration_seconds {
            Some(value) => {
                let source = value.source;
                match parse_positive_integer(value, CAPTURE_DURATION_SECONDS_ENV_VAR) {
                    Ok(seconds) => Some((seconds, source)),
                    Err(error) => {
                        errors.push(error);
                        None
                    }
                }
            }
            None => {
                errors.push(ConfigValidationError::MissingRequiredValue {
                    name: CAPTURE_DURATION_SECONDS_ENV_VAR,
                });
                None
            }
        };

        let capture_overlap = match self.capture_overlap_seconds {
            Some(value) => {
                let source = value.source;
                match parse_positive_integer(value, CAPTURE_OVERLAP_SECONDS_ENV_VAR) {
                    Ok(seconds) => Some((seconds, source)),
                    Err(error) => {
                        errors.push(error);
                        None
                    }
                }
            }
            None => {
                errors.push(ConfigValidationError::MissingRequiredValue {
                    name: CAPTURE_OVERLAP_SECONDS_ENV_VAR,
                });
                None
            }
        };

        let debug_enabled = match self.debug_enabled {
            Some(value) => match parse_bool(value, DEBUG_ENV_VAR) {
                Ok(parsed) => parsed,
                Err(error) => {
                    errors.push(error);
                    false
                }
            },
            None => false,
        };

        let speaker_sample_duration = match self.speaker_sample_duration_seconds {
            Some(value) => {
                match parse_positive_integer(value, SPEAKER_SAMPLE_DURATION_SECONDS_ENV_VAR) {
                    Ok(seconds) => Some(Duration::from_secs(seconds)),
                    Err(error) => {
                        errors.push(error);
                        None
                    }
                }
            }
            None => {
                errors.push(ConfigValidationError::MissingRequiredValue {
                    name: SPEAKER_SAMPLE_DURATION_SECONDS_ENV_VAR,
                });
                None
            }
        };

        let transcription_language = match self.transcription_language {
            Some(value) => {
                match parse_transcription_language(value, TRANSCRIPTION_LANGUAGE_ENV_VAR) {
                    Ok(language) => Some(language),
                    Err(error) => {
                        errors.push(error);
                        None
                    }
                }
            }
            None => Some(DEFAULT_TRANSCRIPTION_LANGUAGE.to_string()),
        };

        let storage_root = match self.storage_root {
            Some(value) => match parse_absolute_path(value, STORAGE_ROOT_ENV_VAR) {
                Ok(path) => Some(path),
                Err(error) => {
                    errors.push(error);
                    None
                }
            },
            None => {
                errors.push(ConfigValidationError::MissingRequiredValue {
                    name: STORAGE_ROOT_ENV_VAR,
                });
                None
            }
        };

        let default_merge_policy = TranscriptMergePolicy::recommended();
        let merge_min_overlap_chars = match self.merge_min_overlap_chars {
            Some(value) => match parse_positive_integer(value, MERGE_MIN_OVERLAP_CHARS_ENV_VAR) {
                Ok(chars) => Some(
                    usize::try_from(chars).expect("merge min overlap chars must fit into usize"),
                ),
                Err(error) => {
                    errors.push(error);
                    None
                }
            },
            None => Some(default_merge_policy.min_overlap_chars),
        };
        let merge_alignment_ratio = match self.merge_alignment_ratio {
            Some(value) => match parse_unit_interval(value, MERGE_ALIGNMENT_RATIO_ENV_VAR) {
                Ok(ratio) => Some(ratio),
                Err(error) => {
                    errors.push(error);
                    None
                }
            },
            None => Some(default_merge_policy.min_alignment_ratio),
        };
        let merge_trigram_similarity = match self.merge_trigram_similarity {
            Some(value) => match parse_unit_interval(value, MERGE_TRIGRAM_SIMILARITY_ENV_VAR) {
                Ok(ratio) => Some(ratio),
                Err(error) => {
                    errors.push(error);
                    None
                }
            },
            None => Some(default_merge_policy.min_trigram_similarity),
        };

        let capture_duration = capture_duration.and_then(|(seconds, _source)| {
            if let Some((overlap_seconds, overlap_source)) = capture_overlap
                && overlap_seconds >= seconds
            {
                errors.push(ConfigValidationError::InvalidCaptureOverlap {
                    overlap_name: CAPTURE_OVERLAP_SECONDS_ENV_VAR,
                    capture_duration_name: CAPTURE_DURATION_SECONDS_ENV_VAR,
                    overlap_seconds,
                    capture_duration_seconds: seconds,
                    overlap_source,
                });
                return None;
            }

            Some(Duration::from_secs(seconds))
        });

        let capture_overlap =
            capture_overlap.map(|(seconds, _source)| Duration::from_secs(seconds));

        if !errors.is_empty() {
            return Err(ConfigError::InvalidConfig(errors));
        }

        let openai_api_key = match openai_api_key {
            Some(value) => value,
            None => unreachable!("validated missing OPENAI_API_KEY"),
        };
        let recording_duration = match recording_duration {
            Some(value) => value,
            None => unreachable!("validated missing recording duration"),
        };
        let capture_duration = match capture_duration {
            Some(value) => value,
            None => unreachable!("validated missing capture duration"),
        };
        let capture_overlap = match capture_overlap {
            Some(value) => value,
            None => unreachable!("validated missing capture overlap"),
        };
        let speaker_sample_duration = match speaker_sample_duration {
            Some(value) => value,
            None => unreachable!("validated missing speaker sample duration"),
        };
        let transcription_language = match transcription_language {
            Some(value) => value,
            None => unreachable!("validated missing transcription language"),
        };
        let merge_min_overlap_chars = match merge_min_overlap_chars {
            Some(value) => value,
            None => unreachable!("validated missing merge min overlap chars"),
        };
        let merge_alignment_ratio = match merge_alignment_ratio {
            Some(value) => value,
            None => unreachable!("validated missing merge alignment ratio"),
        };
        let merge_trigram_similarity = match merge_trigram_similarity {
            Some(value) => value,
            None => unreachable!("validated missing merge trigram similarity"),
        };
        let storage_root = match storage_root {
            Some(value) => value,
            None => unreachable!("validated missing storage root"),
        };

        Ok(Config {
            openai_api_key: openai_api_key.value,
            openai_api_key_source: openai_api_key.source,
            recording_duration,
            capture_duration,
            capture_overlap,
            speaker_sample_duration,
            transcription_language,
            transcript_merge_policy: TranscriptMergePolicy {
                min_overlap_chars: merge_min_overlap_chars,
                min_alignment_ratio: merge_alignment_ratio,
                min_trigram_similarity: merge_trigram_similarity,
            },
            debug_enabled,
            storage_root,
        })
    }
}

fn read_env_var(name: &'static str, source: ConfigSource) -> Option<ConfigValue<String>> {
    std::env::var(name)
        .ok()
        .map(|value| ConfigValue::new(value, source))
}

fn parse_bool(
    value: ConfigValue<String>,
    name: &'static str,
) -> Result<bool, ConfigValidationError> {
    if value.value.trim().is_empty() {
        return Err(ConfigValidationError::EmptyValue {
            name,
            source: value.source,
        });
    }

    match value.value.as_str() {
        "1" | "true" | "TRUE" | "yes" | "YES" => Ok(true),
        "0" | "false" | "FALSE" | "no" | "NO" => Ok(false),
        _ => Err(ConfigValidationError::InvalidBooleanValue {
            name,
            value: value.value,
            source: value.source,
        }),
    }
}

fn parse_positive_integer(
    value: ConfigValue<String>,
    name: &'static str,
) -> Result<u64, ConfigValidationError> {
    if value.value.trim().is_empty() {
        return Err(ConfigValidationError::EmptyValue {
            name,
            source: value.source,
        });
    }

    match value.value.parse::<u64>() {
        Ok(parsed) if parsed > 0 => Ok(parsed),
        _ => Err(ConfigValidationError::InvalidPositiveIntegerValue {
            name,
            value: value.value,
            source: value.source,
        }),
    }
}

fn parse_unit_interval(
    value: ConfigValue<String>,
    name: &'static str,
) -> Result<f64, ConfigValidationError> {
    if value.value.trim().is_empty() {
        return Err(ConfigValidationError::EmptyValue {
            name,
            source: value.source,
        });
    }

    match value.value.parse::<f64>() {
        Ok(parsed) if (0.0..=1.0).contains(&parsed) => Ok(parsed),
        _ => Err(ConfigValidationError::InvalidUnitIntervalValue {
            name,
            value: value.value,
            source: value.source,
        }),
    }
}

fn parse_transcription_language(
    value: ConfigValue<String>,
    name: &'static str,
) -> Result<String, ConfigValidationError> {
    let trimmed = value.value.trim();
    if trimmed.is_empty() {
        return Err(ConfigValidationError::EmptyValue {
            name,
            source: value.source,
        });
    }

    match trimmed {
        "ja" | "en" => Ok(trimmed.to_string()),
        _ => Err(ConfigValidationError::InvalidTranscriptionLanguageValue {
            name,
            value: value.value,
            source: value.source,
        }),
    }
}

fn parse_absolute_path(
    value: ConfigValue<String>,
    name: &'static str,
) -> Result<PathBuf, ConfigValidationError> {
    if value.value.trim().is_empty() {
        return Err(ConfigValidationError::EmptyValue {
            name,
            source: value.source,
        });
    }

    let path = PathBuf::from(&value.value);
    if !path.is_absolute() {
        return Err(ConfigValidationError::RelativePathValue {
            name,
            value: value.value,
            source: value.source,
        });
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{Config, ConfigError, ConfigSource, ConfigValidationError};
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
        assert_eq!(config.transcription_language, "en");
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
        assert_eq!(config.transcription_language, "ja");
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
        assert_eq!(config.transcription_language, "en");
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
    /// transcription language は日本語と英語だけを許可する。
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
            if errors == vec![ConfigValidationError::InvalidTranscriptionLanguageValue {
                name: "DIARIZE_LOG_TRANSCRIPTION_LANGUAGE",
                value: "fr".to_string(),
                source: ConfigSource::DotEnv,
            }]
        ));
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

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn sample_storage_root(base_dir: &Path) -> PathBuf {
        base_dir.join("storage-root")
    }
}
