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
        if let Some(openai_api_key) = read_openai_api_key_from_dotenv(dotenv_path)? {
            return Ok(Self {
                openai_api_key,
                openai_api_key_source: ConfigSource::DotEnv,
                debug_enabled: read_debug_enabled(),
            });
        }

        let openai_api_key =
            std::env::var(OPENAI_API_KEY_ENV_VAR).map_err(|_| ConfigError::MissingOpenAiApiKey)?;

        Ok(Self {
            openai_api_key,
            openai_api_key_source: ConfigSource::Environment,
            debug_enabled: read_debug_enabled(),
        })
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
    MissingOpenAiApiKey,
    ReadDotEnv(DotenvError),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingOpenAiApiKey => {
                write!(f, "{OPENAI_API_KEY_ENV_VAR} is not set")
            }
            Self::ReadDotEnv(source) => write!(f, "failed to read .env: {source}"),
        }
    }
}

impl std::error::Error for ConfigError {}

fn read_debug_enabled() -> bool {
    match std::env::var(DEBUG_ENV_VAR) {
        Ok(value) => matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"),
        Err(_) => false,
    }
}

fn read_openai_api_key_from_dotenv(dotenv_path: &Path) -> Result<Option<String>, ConfigError> {
    match from_filename_iter(dotenv_path) {
        Ok(iter) => {
            for item in iter {
                let (key, value) = item.map_err(ConfigError::ReadDotEnv)?;
                if key == OPENAI_API_KEY_ENV_VAR {
                    return Ok(Some(value));
                }
            }

            Ok(None)
        }
        Err(DotenvError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConfigError::ReadDotEnv(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, ConfigError, ConfigSource};
    use std::sync::{Mutex, OnceLock};

    #[test]
    /// .env に OPENAI_API_KEY があれば環境変数より優先して使う。
    fn prefers_dotenv_api_key_over_environment_variable() {
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
        assert_eq!(config.openai_api_key, "from-dotenv");
        assert_eq!(config.openai_api_key_source, ConfigSource::DotEnv);
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
    /// .env と環境変数のどちらにもキーがなければエラーにする。
    fn returns_error_when_api_key_is_missing_everywhere() {
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
        assert!(matches!(result, Err(ConfigError::MissingOpenAiApiKey)));
    }

    #[test]
    /// 真偽値として許可した値が入っていれば debug_enabled を有効にする。
    fn resolves_debug_enabled_for_truthy_values() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let dotenv_path = temp_dir.path().join(".env");
        std::fs::write(&dotenv_path, "OPENAI_API_KEY=from-dotenv\n").unwrap();
        let original = std::env::var_os("DIARIZE_LOG_DEBUG");
        unsafe {
            std::env::set_var("DIARIZE_LOG_DEBUG", "true");
        }

        let config = Config::from_dotenv_path(&dotenv_path).unwrap();

        restore_env_var("DIARIZE_LOG_DEBUG", original);
        assert!(config.debug_enabled);
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
