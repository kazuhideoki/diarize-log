use crate::adapters::LineLogger;
use crate::application::ports::{Logger, Transcriber, TranscriberError, TranscriptionRequest};
use crate::domain::{DiarizedTranscript, RecordedAudio, TranscriptSegment};
use hound::WavReader;
use reqwest::blocking::{Client, multipart};
use serde::Deserialize;
use std::io::Cursor;
use std::time::{Duration, Instant};

const TRANSCRIPTIONS_ENDPOINT: &str = "https://api.openai.com/v1/audio/transcriptions";
const SIMPLE_TRANSCRIPTION_MODEL: &str = "gpt-4o-transcribe";
const TRANSCRIPTION_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const UNLABELED_SPEAKER: &str = "UNLABELED";

/// OpenAI の通常文字起こし API を呼び出し、単一話者として扱える transcript に変換します。
pub struct OpenAiSimpleTranscriber {
    client: Client,
    api_key: String,
    logger: LineLogger,
}

impl OpenAiSimpleTranscriber {
    pub fn new(api_key: String, logger: LineLogger) -> Result<Self, TranscriberError> {
        let client = Client::builder()
            .timeout(TRANSCRIPTION_REQUEST_TIMEOUT)
            .build()
            .map_err(|error| TranscriberError::BuildHttpClient(error.to_string()))?;

        Ok(Self {
            client,
            api_key,
            logger,
        })
    }
}

impl Transcriber for OpenAiSimpleTranscriber {
    fn transcribe(
        &mut self,
        request: TranscriptionRequest<'_>,
    ) -> Result<DiarizedTranscript, TranscriberError> {
        let _ = self.logger.debug(&format!(
            "sending simple transcription request: endpoint={TRANSCRIPTIONS_ENDPOINT} model={SIMPLE_TRANSCRIPTION_MODEL} language={} audio_bytes={} timeout_ms={}",
            language_debug_label(request.language),
            request.audio.wav_bytes.len(),
            TRANSCRIPTION_REQUEST_TIMEOUT.as_millis()
        ));
        let audio_part = multipart::Part::bytes(request.audio.wav_bytes.clone())
            .file_name("capture.wav")
            .mime_str(request.audio.content_type)
            .map_err(|error| TranscriberError::InvalidMimeType(error.to_string()))?;
        let mut form = multipart::Form::new()
            .part("file", audio_part)
            .text("model", SIMPLE_TRANSCRIPTION_MODEL.to_string())
            .text("response_format", "json".to_string());
        if let Some(language) = request.language {
            form = form.text("language", language.to_owned());
        }

        let request_started_at = Instant::now();
        let response = self
            .client
            .post(TRANSCRIPTIONS_ENDPOINT)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .map_err(|error| TranscriberError::SendRequest(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|error| TranscriberError::ReadResponseBody(error.to_string()))?;
        let _ = self.logger.debug(&format!(
            "simple transcription response: status={status} elapsed_ms={} body_bytes={}",
            request_started_at.elapsed().as_millis(),
            body.len()
        ));

        if !status.is_success() {
            return Err(TranscriberError::ApiError {
                status_code: status.as_u16(),
                body,
            });
        }

        let response: OpenAiTextResponse =
            serde_json::from_str(&body).map_err(|source| TranscriberError::ParseResponseBody {
                source: source.to_string(),
                body,
            })?;
        let text = response.text.trim().to_string();
        let segments = if text.is_empty() {
            Vec::new()
        } else {
            vec![TranscriptSegment {
                speaker: UNLABELED_SPEAKER.to_string(),
                start_ms: 0,
                end_ms: wav_duration_ms(request.audio)?,
                text: text.clone(),
            }]
        };

        Ok(DiarizedTranscript { text, segments })
    }
}

#[derive(Deserialize)]
struct OpenAiTextResponse {
    text: String,
}

fn wav_duration_ms(audio: &RecordedAudio) -> Result<u64, TranscriberError> {
    let reader = WavReader::new(Cursor::new(audio.wav_bytes.clone()))
        .map_err(|error| TranscriberError::SendRequest(format!("failed to read wav: {error}")))?;
    let spec = reader.spec();
    Ok(u64::from(reader.duration()) * 1_000 / u64::from(spec.sample_rate))
}

fn language_debug_label(language: Option<&str>) -> &str {
    language.unwrap_or("<auto>")
}
