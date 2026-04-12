use crate::debug_log;
use crate::ports::{
    DiarizedTranscript, Transcriber, TranscriberError, TranscriptSegment, TranscriptionRequest,
};
use reqwest::blocking::{Client, multipart};
use serde::Deserialize;

const TRANSCRIPTIONS_ENDPOINT: &str = "https://api.openai.com/v1/audio/transcriptions";

/// OpenAI の話者分離文字起こし API を呼び出します。
#[derive(Debug)]
pub struct OpenAiTranscriber {
    client: Client,
    api_key: String,
    debug_enabled: bool,
}

impl OpenAiTranscriber {
    pub fn new(api_key: String, debug_enabled: bool) -> Result<Self, TranscriberError> {
        let client = Client::builder()
            .build()
            .map_err(|error| TranscriberError::BuildHttpClient(error.to_string()))?;

        Ok(Self {
            client,
            api_key,
            debug_enabled,
        })
    }
}

impl Transcriber for OpenAiTranscriber {
    fn transcribe(
        &mut self,
        request: TranscriptionRequest<'_>,
    ) -> Result<DiarizedTranscript, TranscriberError> {
        debug_log(
            self.debug_enabled,
            &format!(
                "sending transcription request: endpoint={TRANSCRIPTIONS_ENDPOINT} model={} response_format={} chunking_strategy={} audio_bytes={}",
                request.model,
                request.response_format.as_api_value(),
                request.chunking_strategy.as_api_value(),
                request.audio.wav_bytes.len()
            ),
        );
        let audio_part = multipart::Part::bytes(request.audio.wav_bytes.clone())
            .file_name("recording.wav")
            .mime_str(request.audio.content_type)
            .map_err(|error| TranscriberError::InvalidMimeType(error.to_string()))?;
        let form = multipart::Form::new()
            .part("file", audio_part)
            .text("model", request.model.to_owned())
            .text(
                "response_format",
                request.response_format.as_api_value().to_owned(),
            )
            .text(
                "chunking_strategy",
                request.chunking_strategy.as_api_value().to_owned(),
            );
        let response = self
            .client
            .post(TRANSCRIPTIONS_ENDPOINT)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .map_err(|error| TranscriberError::SendRequest(error.to_string()))?;
        let status = response.status();
        debug_log(
            self.debug_enabled,
            &format!("transcription response status: {status}"),
        );
        let body = response
            .text()
            .map_err(|error| TranscriberError::ReadResponseBody(error.to_string()))?;
        debug_log(
            self.debug_enabled,
            &format!("transcription response body bytes={}", body.len()),
        );

        if !status.is_success() {
            return Err(TranscriberError::ApiError {
                status_code: status.as_u16(),
                body,
            });
        }

        let api_response: ApiDiarizedTranscript =
            serde_json::from_str(&body).map_err(|source| TranscriberError::ParseResponseBody {
                source: source.to_string(),
                body,
            })?;
        debug_log(
            self.debug_enabled,
            &format!(
                "transcription parsed: text_chars={} segments={}",
                api_response.text.chars().count(),
                api_response.segments.len()
            ),
        );

        Ok(api_response.into_domain())
    }
}

#[derive(Debug, Deserialize)]
struct ApiDiarizedTranscript {
    text: String,
    segments: Vec<ApiTranscriptSegment>,
}

impl ApiDiarizedTranscript {
    fn into_domain(self) -> DiarizedTranscript {
        DiarizedTranscript {
            text: self.text,
            segments: self
                .segments
                .into_iter()
                .map(ApiTranscriptSegment::into_domain)
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiTranscriptSegment {
    speaker: String,
    start: f64,
    end: f64,
    text: String,
}

impl ApiTranscriptSegment {
    fn into_domain(self) -> TranscriptSegment {
        TranscriptSegment {
            speaker: self.speaker,
            start_ms: seconds_to_millis(self.start),
            end_ms: seconds_to_millis(self.end),
            text: self.text,
        }
    }
}

fn seconds_to_millis(seconds: f64) -> u64 {
    (seconds * 1_000.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::{ApiDiarizedTranscript, ApiTranscriptSegment};
    use crate::ports::{DiarizedTranscript, TranscriptSegment};

    #[test]
    /// API の秒単位セグメントをミリ秒単位の出力モデルへ変換する。
    fn converts_api_segments_from_seconds_to_milliseconds() {
        let api_transcript = ApiDiarizedTranscript {
            text: "hello".to_string(),
            segments: vec![ApiTranscriptSegment {
                speaker: "spk_0".to_string(),
                start: 0.125,
                end: 1.875,
                text: "hello".to_string(),
            }],
        };

        let transcript = api_transcript.into_domain();

        assert_eq!(
            transcript,
            DiarizedTranscript {
                text: "hello".to_string(),
                segments: vec![TranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 125,
                    end_ms: 1_875,
                    text: "hello".to_string(),
                }],
            }
        );
    }
}
