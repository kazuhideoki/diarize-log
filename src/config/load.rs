use super::*;
use dotenvy::{Error as DotenvError, from_filename_iter};
use std::path::Path;

impl RawConfig {
    pub(super) fn from_env() -> Self {
        Self {
            openai_api_key: read_env_var(OPENAI_API_KEY_ENV_VAR, ConfigSource::Environment),
            pyannote_api_key: read_env_var(PYANNOTE_API_KEY_ENV_VAR, ConfigSource::Environment),
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
            capture_silence_threshold_dbfs: read_env_var(
                CAPTURE_SILENCE_THRESHOLD_DBFS_ENV_VAR,
                ConfigSource::Environment,
            ),
            capture_silence_min_duration_ms: read_env_var(
                CAPTURE_SILENCE_MIN_DURATION_MS_ENV_VAR,
                ConfigSource::Environment,
            ),
            capture_tail_silence_min_duration_ms: read_env_var(
                CAPTURE_TAIL_SILENCE_MIN_DURATION_MS_ENV_VAR,
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
            transcription_pipeline: read_env_var(
                TRANSCRIPTION_PIPELINE_ENV_VAR,
                ConfigSource::Environment,
            ),
            pyannote_max_speakers: read_env_var(
                PYANNOTE_MAX_SPEAKERS_ENV_VAR,
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

    pub(super) fn from_dotenv_path(dotenv_path: &Path) -> Result<Self, ConfigError> {
        let mut raw = Self::default();

        match from_filename_iter(dotenv_path) {
            Ok(iter) => {
                for item in iter {
                    let (key, value) = item.map_err(ConfigError::ReadDotEnv)?;
                    match key.as_str() {
                        OPENAI_API_KEY_ENV_VAR => {
                            raw.openai_api_key = Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        PYANNOTE_API_KEY_ENV_VAR => {
                            raw.pyannote_api_key =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
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
                        CAPTURE_SILENCE_THRESHOLD_DBFS_ENV_VAR => {
                            raw.capture_silence_threshold_dbfs =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        CAPTURE_SILENCE_MIN_DURATION_MS_ENV_VAR => {
                            raw.capture_silence_min_duration_ms =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        CAPTURE_TAIL_SILENCE_MIN_DURATION_MS_ENV_VAR => {
                            raw.capture_tail_silence_min_duration_ms =
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
                        TRANSCRIPTION_PIPELINE_ENV_VAR => {
                            raw.transcription_pipeline =
                                Some(ConfigValue::new(value, ConfigSource::DotEnv))
                        }
                        PYANNOTE_MAX_SPEAKERS_ENV_VAR => {
                            raw.pyannote_max_speakers =
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

    pub(super) fn merge_missing(self, fallback: Self) -> Self {
        Self {
            openai_api_key: self.openai_api_key.or(fallback.openai_api_key),
            pyannote_api_key: self.pyannote_api_key.or(fallback.pyannote_api_key),
            recording_duration_seconds: self
                .recording_duration_seconds
                .or(fallback.recording_duration_seconds),
            capture_duration_seconds: self
                .capture_duration_seconds
                .or(fallback.capture_duration_seconds),
            capture_overlap_seconds: self
                .capture_overlap_seconds
                .or(fallback.capture_overlap_seconds),
            capture_silence_threshold_dbfs: self
                .capture_silence_threshold_dbfs
                .or(fallback.capture_silence_threshold_dbfs),
            capture_silence_min_duration_ms: self
                .capture_silence_min_duration_ms
                .or(fallback.capture_silence_min_duration_ms),
            capture_tail_silence_min_duration_ms: self
                .capture_tail_silence_min_duration_ms
                .or(fallback.capture_tail_silence_min_duration_ms),
            speaker_sample_duration_seconds: self
                .speaker_sample_duration_seconds
                .or(fallback.speaker_sample_duration_seconds),
            transcription_language: self
                .transcription_language
                .or(fallback.transcription_language),
            transcription_pipeline: self
                .transcription_pipeline
                .or(fallback.transcription_pipeline),
            pyannote_max_speakers: self
                .pyannote_max_speakers
                .or(fallback.pyannote_max_speakers),
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
}

fn read_env_var(name: &'static str, source: ConfigSource) -> Option<ConfigValue<String>> {
    std::env::var(name)
        .ok()
        .map(|value| ConfigValue::new(value, source))
}
