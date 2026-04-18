use super::{
    CaptureConfig, CaptureError, CaptureTranscriptionFailure, SpeakerLabel, duration_to_millis,
};
use crate::application::ports::{
    CaptureStore, Logger, Transcriber, TranscriberError, TranscriptionRequest,
};
use crate::domain::{
    CaptureMerger, CaptureRange, CapturedTranscript, DiarizedTranscript, KnownSpeakerSample,
    MergedTranscriptSegment, RecordedAudio, TranscriptSegment,
};
use std::fmt;
use std::io::Write;

#[derive(Debug)]
pub enum DebugOutputError {
    Serialize(serde_json::Error),
    Write(std::io::Error),
}

impl fmt::Display for DebugOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialize(source) => {
                write!(f, "failed to serialize debug stdout: {source}")
            }
            Self::Write(source) => write!(f, "failed to write debug stdout: {source}"),
        }
    }
}

impl std::error::Error for DebugOutputError {}

#[allow(clippy::too_many_arguments)]
pub(super) fn process_capture_audio<T, S>(
    capture_range: CaptureRange,
    audio: RecordedAudio,
    config: &CaptureConfig,
    speaker_samples: &[KnownSpeakerSample],
    speaker_label: &SpeakerLabel,
    logger: &dyn Logger,
    transcriber: &mut T,
    capture_store: &mut S,
    capture_merger: &mut CaptureMerger,
    transcripts: &mut Vec<DiarizedTranscript>,
    merged_segments: &mut Vec<MergedTranscriptSegment>,
    transcription_failures: &mut Vec<CaptureTranscriptionFailure>,
) -> Result<(), CaptureError>
where
    T: Transcriber,
    S: CaptureStore,
{
    logger
        .info(&format!(
            "transcription request sent for capture {}",
            capture_range.capture_index
        ))
        .map_err(CaptureError::Write)?;
    let capture_start_ms = duration_to_millis(capture_range.start_offset);
    let capture_end_ms = duration_to_millis(capture_range.end_offset());
    let transcript = match transcriber.transcribe(TranscriptionRequest {
        audio: &audio,
        speaker_samples,
        model: config.transcription_model,
        language: config.transcription_language.as_api_value(),
        response_format: config.response_format,
        chunking_strategy: config.chunking_strategy,
    }) {
        Ok(transcript) => transcript,
        Err(error) => {
            if !is_recoverable_transcription_error(&error) {
                return Err(CaptureError::Transcribe(error));
            }
            logger
                .info(&format!(
                    "transcription failed for capture {}, continuing: {error}",
                    capture_range.capture_index
                ))
                .map_err(CaptureError::Write)?;
            transcription_failures.push(CaptureTranscriptionFailure {
                capture_index: capture_range.capture_index,
                capture_start_ms,
                message: error.to_string(),
            });
            return Ok(());
        }
    };
    let transcript = apply_speaker_label(transcript, speaker_label);
    logger
        .info(&format!(
            "transcription response received for capture {}",
            capture_range.capture_index
        ))
        .map_err(CaptureError::Write)?;
    capture_store
        .persist_transcript(capture_range.capture_index, capture_start_ms, &transcript)
        .map_err(CaptureError::Store)?;
    let merge_batch = capture_merger.push_capture(CapturedTranscript::from_relative(
        capture_range.capture_index,
        capture_start_ms,
        capture_end_ms,
        &transcript,
    ));
    capture_store
        .persist_merge_audit_entries(&merge_batch.audit_entries)
        .map_err(CaptureError::Store)?;
    capture_store
        .persist_merged_segments(&merge_batch.finalized_segments)
        .map_err(CaptureError::Store)?;
    merged_segments.extend(merge_batch.finalized_segments);
    transcripts.push(transcript);
    Ok(())
}

fn is_recoverable_transcription_error(error: &TranscriberError) -> bool {
    matches!(error, TranscriberError::SendRequest(_))
}

fn apply_speaker_label(
    transcript: DiarizedTranscript,
    speaker_label: &SpeakerLabel,
) -> DiarizedTranscript {
    match speaker_label {
        SpeakerLabel::KeepOriginal => transcript,
        SpeakerLabel::Fixed(speaker_name) => DiarizedTranscript {
            text: transcript.text,
            segments: transcript
                .segments
                .into_iter()
                .map(|segment| TranscriptSegment {
                    speaker: speaker_name.clone(),
                    start_ms: segment.start_ms,
                    end_ms: segment.end_ms,
                    text: segment.text,
                })
                .collect(),
        },
    }
}

pub fn write_debug_transcript<W>(
    debug_enabled: bool,
    output: &mut W,
    transcripts: &[DiarizedTranscript],
) -> Result<(), DebugOutputError>
where
    W: Write,
{
    if !debug_enabled {
        return Ok(());
    }

    for transcript in transcripts {
        serde_json::to_writer_pretty(&mut *output, transcript)
            .map_err(DebugOutputError::Serialize)?;
        output.write_all(b"\n").map_err(DebugOutputError::Write)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::{ChunkingStrategy, ResponseFormat, TranscriptionLanguage};
    use std::time::Duration;

    fn sample_transcript() -> DiarizedTranscript {
        DiarizedTranscript {
            text: "こんにちは 今日はよろしくお願いします".to_string(),
            segments: vec![
                TranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 0,
                    end_ms: 900,
                    text: "こんにちは".to_string(),
                },
                TranscriptSegment {
                    speaker: "spk_1".to_string(),
                    start_ms: 950,
                    end_ms: 2_300,
                    text: "今日はよろしくお願いします".to_string(),
                },
            ],
        }
    }

    fn capture_config() -> CaptureConfig {
        CaptureConfig::new(
            Duration::from_secs(10),
            Duration::from_secs(10),
            Duration::ZERO,
            TranscriptionLanguage::Fixed("ja".to_string()),
        )
    }

    #[test]
    /// debug 無効時は標準出力へ何も書かず、有効時だけ transcript JSON を capture ごとに出力する。
    fn writes_debug_transcript_only_when_debug_enabled() {
        let transcripts = vec![sample_transcript(), sample_transcript()];
        let mut disabled_output = Vec::new();
        let mut enabled_output = Vec::new();

        write_debug_transcript(false, &mut disabled_output, &transcripts).unwrap();
        write_debug_transcript(true, &mut enabled_output, &transcripts).unwrap();

        assert!(disabled_output.is_empty());
        let parsed = serde_json::Deserializer::from_slice(&enabled_output)
            .into_iter::<serde_json::Value>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(parsed.len(), transcripts.len());
        assert_eq!(
            parsed[0]["text"],
            serde_json::Value::String(transcripts[0].text.clone())
        );
        assert_eq!(
            parsed[1]["text"],
            serde_json::Value::String(transcripts[1].text.clone())
        );
    }

    #[test]
    /// 固定話者名を指定した場合は transcript 内の全 segment の speaker を上書きする。
    fn applies_fixed_speaker_label_to_all_segments() {
        let labeled =
            apply_speaker_label(sample_transcript(), &SpeakerLabel::Fixed("me".to_string()));

        assert_eq!(
            labeled
                .segments
                .iter()
                .map(|segment| &segment.speaker)
                .collect::<Vec<_>>(),
            vec![&"me".to_string(), &"me".to_string()]
        );
    }

    #[test]
    /// send request 失敗だけを capture 単位で継続可能な transcription 失敗として扱う。
    fn treats_only_send_request_error_as_recoverable() {
        assert!(is_recoverable_transcription_error(
            &TranscriberError::SendRequest("timeout".to_string(),)
        ));
        assert!(!is_recoverable_transcription_error(
            &TranscriberError::ParseResponseBody {
                source: "invalid".to_string(),
                body: "{".to_string(),
            }
        ));
        assert_eq!(
            capture_config().response_format,
            ResponseFormat::DiarizedJson
        );
        assert_eq!(capture_config().chunking_strategy, ChunkingStrategy::Auto);
    }
}
