use super::*;
use std::path::PathBuf;
use std::time::Duration;

impl RawConfig {
    pub(super) fn validate(self) -> Result<Config, ConfigError> {
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
        let pyannote_api_key = match self.pyannote_api_key {
            Some(value) => {
                if value.value.trim().is_empty() {
                    errors.push(ConfigValidationError::EmptyValue {
                        name: PYANNOTE_API_KEY_ENV_VAR,
                        source: value.source,
                    });
                    None
                } else {
                    Some(value.value)
                }
            }
            None => None,
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
        let default_silence_request_policy = SilenceRequestPolicy::recommended();
        let capture_silence_threshold_dbfs = match self.capture_silence_threshold_dbfs {
            Some(value) => {
                match parse_negative_decibel(value, CAPTURE_SILENCE_THRESHOLD_DBFS_ENV_VAR) {
                    Ok(dbfs) => Some(dbfs),
                    Err(error) => {
                        errors.push(error);
                        None
                    }
                }
            }
            None => Some(default_silence_request_policy.silence_threshold_dbfs),
        };
        let capture_silence_min_duration = match self.capture_silence_min_duration_ms {
            Some(value) => {
                match parse_positive_integer(value, CAPTURE_SILENCE_MIN_DURATION_MS_ENV_VAR) {
                    Ok(duration_ms) => Some(Duration::from_millis(duration_ms)),
                    Err(error) => {
                        errors.push(error);
                        None
                    }
                }
            }
            None => Some(default_silence_request_policy.silence_min_duration),
        };
        let capture_tail_silence_min_duration = match self.capture_tail_silence_min_duration_ms {
            Some(value) => {
                let source = value.source;
                match parse_positive_integer(value, CAPTURE_TAIL_SILENCE_MIN_DURATION_MS_ENV_VAR) {
                    Ok(duration_ms) => Some((duration_ms, source)),
                    Err(error) => {
                        errors.push(error);
                        None
                    }
                }
            }
            None => Some((
                u64::try_from(
                    default_silence_request_policy
                        .tail_silence_min_duration
                        .as_millis(),
                )
                .expect("tail silence duration must fit into u64"),
                ConfigSource::Environment,
            )),
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
            None => Some(TranscriptionLanguage::Fixed(
                DEFAULT_TRANSCRIPTION_LANGUAGE.to_string(),
            )),
        };
        let transcription_pipeline = match self.transcription_pipeline {
            Some(value) => {
                match parse_transcription_pipeline(value, TRANSCRIPTION_PIPELINE_ENV_VAR) {
                    Ok(pipeline) => Some(pipeline),
                    Err(error) => {
                        errors.push(error);
                        None
                    }
                }
            }
            None => Some(TranscriptionPipeline::Legacy),
        };
        let pyannote_max_speakers = match self.pyannote_max_speakers {
            Some(value) => match parse_positive_integer(value, PYANNOTE_MAX_SPEAKERS_ENV_VAR) {
                Ok(max_speakers) => Some(Some(max_speakers)),
                Err(error) => {
                    errors.push(error);
                    None
                }
            },
            None => Some(None),
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
        let capture_tail_silence_min_duration =
            capture_tail_silence_min_duration.and_then(|(tail_duration_ms, tail_source)| {
                if let Some(silence_duration) = capture_silence_min_duration
                    && tail_duration_ms
                        > u64::try_from(silence_duration.as_millis())
                            .expect("silence duration must fit into u64")
                {
                    errors.push(ConfigValidationError::InvalidTailSilenceDuration {
                        silence_name: CAPTURE_SILENCE_MIN_DURATION_MS_ENV_VAR,
                        tail_silence_name: CAPTURE_TAIL_SILENCE_MIN_DURATION_MS_ENV_VAR,
                        silence_duration_ms: u64::try_from(silence_duration.as_millis())
                            .expect("silence duration must fit into u64"),
                        tail_silence_duration_ms: tail_duration_ms,
                        tail_silence_source: tail_source,
                    });
                    return None;
                }

                Some(Duration::from_millis(tail_duration_ms))
            });

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
        let capture_silence_threshold_dbfs = match capture_silence_threshold_dbfs {
            Some(value) => value,
            None => unreachable!("validated missing capture silence threshold"),
        };
        let capture_silence_min_duration = match capture_silence_min_duration {
            Some(value) => value,
            None => unreachable!("validated missing capture silence duration"),
        };
        let capture_tail_silence_min_duration = match capture_tail_silence_min_duration {
            Some(value) => value,
            None => unreachable!("validated missing capture tail silence duration"),
        };
        let speaker_sample_duration = match speaker_sample_duration {
            Some(value) => value,
            None => unreachable!("validated missing speaker sample duration"),
        };
        let transcription_language = match transcription_language {
            Some(value) => value,
            None => unreachable!("validated missing transcription language"),
        };
        let transcription_pipeline = match transcription_pipeline {
            Some(value) => value,
            None => unreachable!("validated missing transcription pipeline"),
        };
        if transcription_pipeline == TranscriptionPipeline::Separated && pyannote_api_key.is_none()
        {
            return Err(ConfigError::InvalidConfig(vec![
                ConfigValidationError::MissingPyannoteApiKey {
                    pipeline_name: TRANSCRIPTION_PIPELINE_ENV_VAR,
                    api_key_name: PYANNOTE_API_KEY_ENV_VAR,
                },
            ]));
        }
        let pyannote_max_speakers = match pyannote_max_speakers {
            Some(value) => value,
            None => unreachable!("validated missing pyannote max speakers"),
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
            pyannote_api_key,
            recording_duration,
            capture_duration,
            capture_overlap,
            capture_silence_request_policy: SilenceRequestPolicy {
                silence_threshold_dbfs: capture_silence_threshold_dbfs,
                silence_min_duration: capture_silence_min_duration,
                tail_silence_min_duration: capture_tail_silence_min_duration,
            },
            speaker_sample_duration,
            transcription_language,
            transcription_pipeline,
            pyannote_max_speakers,
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

fn parse_negative_decibel(
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
        Ok(parsed) if parsed.is_finite() && parsed < 0.0 => Ok(parsed),
        _ => Err(ConfigValidationError::InvalidNegativeDecibelValue {
            name,
            value: value.value,
            source: value.source,
        }),
    }
}

fn parse_transcription_language(
    value: ConfigValue<String>,
    name: &'static str,
) -> Result<TranscriptionLanguage, ConfigValidationError> {
    let trimmed = value.value.trim();
    if trimmed.is_empty() {
        return Err(ConfigValidationError::EmptyValue {
            name,
            source: value.source,
        });
    }

    match trimmed {
        "auto" => Ok(TranscriptionLanguage::Auto),
        "ja" | "en" => Ok(TranscriptionLanguage::Fixed(trimmed.to_string())),
        _ => Err(
            ConfigValidationError::InvalidTranscriptionLanguageModeValue {
                name,
                value: value.value,
                source: value.source,
            },
        ),
    }
}

fn parse_transcription_pipeline(
    value: ConfigValue<String>,
    name: &'static str,
) -> Result<TranscriptionPipeline, ConfigValidationError> {
    let trimmed = value.value.trim();
    if trimmed.is_empty() {
        return Err(ConfigValidationError::EmptyValue {
            name,
            source: value.source,
        });
    }

    match trimmed {
        "legacy" => Ok(TranscriptionPipeline::Legacy),
        "separated" => Ok(TranscriptionPipeline::Separated),
        _ => Err(ConfigValidationError::InvalidTranscriptionPipelineValue {
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
