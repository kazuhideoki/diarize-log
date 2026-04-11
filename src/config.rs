use dotenvy::{Error as DotenvError, from_filename_iter};
use std::fmt;
use std::path::Path;

/// 既定の `.env` ファイルパスです。
pub const DEFAULT_DOTENV_PATH: &str = ".env";
const OPENAI_API_KEY_ENV_VAR: &str = "OPENAI_API_KEY";
const DEBUG_ENV_VAR: &str = "DIARIZE_LOG_DEBUG";

/// 実行時設定の読み込み結果です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub openai_api_key: String,
    pub openai_api_key_source: ConfigSource,
    pub debug_enabled: bool,
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
            Self::MissingRequiredValue { name } => {
                write!(f, "missing required value: {name}")
            }
            Self::EmptyValue { name, source } => {
                write!(f, "empty value for {name} from {source}")
            }
            Self::InvalidBooleanValue {
                name,
                value,
                source,
            } => write!(f, "invalid boolean value for {name} from {source}: {value}"),
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
    debug_enabled: Option<ConfigValue<String>>,
}

impl RawConfig {
    fn from_env() -> Self {
        Self {
            openai_api_key: read_env_var(OPENAI_API_KEY_ENV_VAR, ConfigSource::Environment),
            debug_enabled: read_env_var(DEBUG_ENV_VAR, ConfigSource::Environment),
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
                        DEBUG_ENV_VAR => {
                            raw.debug_enabled = Some(ConfigValue::new(value, ConfigSource::DotEnv))
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
            debug_enabled: self.debug_enabled.or(fallback.debug_enabled),
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
                value
            }
            None => {
                errors.push(ConfigValidationError::MissingRequiredValue {
                    name: OPENAI_API_KEY_ENV_VAR,
                });
                ConfigValue::new(String::new(), ConfigSource::Environment)
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

        if !errors.is_empty() {
            return Err(ConfigError::InvalidConfig(errors));
        }

        Ok(Config {
            openai_api_key: openai_api_key.value,
            openai_api_key_source: openai_api_key.source,
            debug_enabled,
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

#[cfg(test)]
mod tests {
    use super::{Config, ConfigError, ConfigSource, ConfigValidationError};
    use std::sync::{Mutex, OnceLock};

    #[test]
    /// OPENAI_API_KEY は .env より環境変数の値を優先する。
    fn prefers_environment_variable_for_api_key_over_dotenv() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        std::fs::write(&dotenv_path, "OPENAI_API_KEY=from-dotenv\n").unwrap();

        let original = std::env::var_os("OPENAI_API_KEY");
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "from-env");
        }

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_var("OPENAI_API_KEY", original);
        assert_eq!(config.openai_api_key, "from-env");
        assert_eq!(config.openai_api_key_source, ConfigSource::Environment);
        assert!(!config.debug_enabled);
    }

    #[test]
    /// .env にキーがなければ環境変数の OPENAI_API_KEY を使う。
    fn falls_back_to_environment_variable_when_dotenv_has_no_key() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        std::fs::write(&dotenv_path, "OTHER_KEY=value\n").unwrap();

        let original = std::env::var_os("OPENAI_API_KEY");
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "from-env");
        }

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_var("OPENAI_API_KEY", original);
        assert_eq!(config.openai_api_key, "from-env");
        assert_eq!(config.openai_api_key_source, ConfigSource::Environment);
    }

    #[test]
    /// .env と環境変数のどちらにも必須値がなければ一括検証でエラーにする。
    fn returns_error_when_required_values_are_missing_everywhere() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        let original = std::env::var_os("OPENAI_API_KEY");
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("OPENAI_API_KEY", original);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![ConfigValidationError::MissingRequiredValue {
                name: "OPENAI_API_KEY"
            }]
        ));
    }

    #[test]
    /// DIARIZE_LOG_DEBUG が環境変数になければ .env の値で補完する。
    fn resolves_debug_enabled_from_dotenv_when_environment_variable_is_missing() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        std::fs::write(
            &dotenv_path,
            "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_DEBUG=true\n",
        )
        .unwrap();
        let original_debug = std::env::var_os("DIARIZE_LOG_DEBUG");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_DEBUG");
        }

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_var("DIARIZE_LOG_DEBUG", original_debug);
        assert!(config.debug_enabled);
    }

    #[test]
    /// DIARIZE_LOG_DEBUG は .env より環境変数の値を優先する。
    fn prefers_environment_variable_for_debug_enabled_over_dotenv() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        std::fs::write(
            &dotenv_path,
            "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_DEBUG=false\n",
        )
        .unwrap();
        let original_debug = std::env::var_os("DIARIZE_LOG_DEBUG");
        unsafe {
            std::env::set_var("DIARIZE_LOG_DEBUG", "true");
        }

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_var("DIARIZE_LOG_DEBUG", original_debug);
        assert!(config.debug_enabled);
    }

    #[test]
    /// 真偽値として解釈できない値は設定エラーにする。
    fn returns_error_for_invalid_boolean_value() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        std::fs::write(
            &dotenv_path,
            "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_DEBUG=maybe\n",
        )
        .unwrap();
        let original_debug = std::env::var_os("DIARIZE_LOG_DEBUG");
        unsafe {
            std::env::remove_var("DIARIZE_LOG_DEBUG");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("DIARIZE_LOG_DEBUG", original_debug);
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

    #[test]
    /// 空文字の設定値は一括検証で不正値としてまとめて扱う。
    fn returns_aggregated_errors_for_empty_values() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        std::fs::write(&dotenv_path, "OPENAI_API_KEY=\nDIARIZE_LOG_DEBUG=\n").unwrap();
        let original_api_key = std::env::var_os("OPENAI_API_KEY");
        let original_debug = std::env::var_os("DIARIZE_LOG_DEBUG");
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("DIARIZE_LOG_DEBUG");
        }

        let result = Config::from_dotenv_path(&dotenv_path);

        restore_env_var("OPENAI_API_KEY", original_api_key);
        restore_env_var("DIARIZE_LOG_DEBUG", original_debug);
        assert!(matches!(
            result,
            Err(ConfigError::InvalidConfig(errors))
            if errors == vec![
                ConfigValidationError::EmptyValue {
                    name: "OPENAI_API_KEY",
                    source: ConfigSource::DotEnv,
                },
                ConfigValidationError::EmptyValue {
                    name: "DIARIZE_LOG_DEBUG",
                    source: ConfigSource::DotEnv,
                },
            ]
        ));
    }

    #[test]
    /// 環境変数に空文字があれば .env に妥当な値があってもフォールバックしない。
    fn does_not_fall_back_to_dotenv_when_environment_variable_is_empty() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        std::fs::write(
            &dotenv_path,
            "OPENAI_API_KEY=from-dotenv\nDIARIZE_LOG_DEBUG=false\n",
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
}
