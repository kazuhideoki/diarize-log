use crate::application::ports::{Transcriber, TranscriberError, TranscriptionRequest};
use crate::debug_log;
use crate::domain::{DiarizedTranscript, RecordedAudio, TranscriptSegment};
use base64::Engine;
use reqwest::blocking::{Client, multipart};
use serde::Deserialize;
use std::error::Error;

const TRANSCRIPTIONS_ENDPOINT: &str = "https://api.openai.com/v1/audio/transcriptions";

/// OpenAI の話者分離文字起こし API を呼び出します。
#[derive(Debug)]
pub struct OpenAiTranscriber {
    api_key: String,
    debug_enabled: bool,
}

impl OpenAiTranscriber {
    pub fn new(api_key: String, debug_enabled: bool) -> Result<Self, TranscriberError> {
        Ok(Self {
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
                "sending transcription request: endpoint={TRANSCRIPTIONS_ENDPOINT} model={} response_format={} chunking_strategy={} audio_bytes={} client_reuse=disabled",
                request.model,
                request.response_format.as_api_value(),
                request.chunking_strategy.as_api_value(),
                request.audio.wav_bytes.len()
            ),
        );
        let audio_part = multipart::Part::bytes(request.audio.wav_bytes.clone())
            .file_name("capture.wav")
            .mime_str(request.audio.content_type)
            .map_err(|error| TranscriberError::InvalidMimeType(error.to_string()))?;
        let mut form = multipart::Form::new()
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
        for speaker_sample in request.speaker_samples {
            form = form
                .text("known_speaker_names[]", speaker_sample.speaker_name.clone())
                .text(
                    "known_speaker_references[]",
                    audio_data_url(&speaker_sample.audio),
                );
        }
        let client = build_http_client(self.debug_enabled)?;
        let response = self
            .api_request(&client)
            .multipart(form)
            .send()
            .map_err(|error| {
                let details = RequestErrorDetails::from_reqwest_error(&error);
                debug_log(
                    self.debug_enabled,
                    &format!(
                        "transcription request transport error: {}",
                        details.summary()
                    ),
                );
                TranscriberError::SendRequest(details.summary())
            })?;
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

impl OpenAiTranscriber {
    fn api_request<'a>(&'a self, client: &'a Client) -> reqwest::blocking::RequestBuilder {
        client
            .post(TRANSCRIPTIONS_ENDPOINT)
            .bearer_auth(&self.api_key)
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

fn audio_data_url(audio: &RecordedAudio) -> String {
    format!(
        "data:{};base64,{}",
        audio.content_type,
        base64::engine::general_purpose::STANDARD.encode(&audio.wav_bytes)
    )
}

fn build_http_client(debug_enabled: bool) -> Result<Client, TranscriberError> {
    Client::builder().build().map_err(|error| {
        let source_chain = format_error_chain(&error);
        debug_log(
            debug_enabled,
            &format!("failed to build transcription http client: source_chain={source_chain}"),
        );
        TranscriberError::BuildHttpClient(source_chain)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RequestErrorDetails {
    is_builder: bool,
    is_connect: bool,
    is_request: bool,
    is_body: bool,
    is_decode: bool,
    is_redirect: bool,
    is_status: bool,
    is_timeout: bool,
    url: Option<String>,
    source_chain: String,
}

impl RequestErrorDetails {
    fn from_reqwest_error(error: &reqwest::Error) -> Self {
        Self {
            is_builder: error.is_builder(),
            is_connect: error.is_connect(),
            is_request: error.is_request(),
            is_body: error.is_body(),
            is_decode: error.is_decode(),
            is_redirect: error.is_redirect(),
            is_status: error.is_status(),
            is_timeout: error.is_timeout(),
            url: error.url().map(ToString::to_string),
            source_chain: format_error_chain(error),
        }
    }

    fn summary(&self) -> String {
        let url = self.url.as_deref().unwrap_or("<unknown>");
        format!(
            "kind={{builder:{} connect:{} request:{} body:{} decode:{} redirect:{} status:{} timeout:{}}} url={} source_chain={}",
            self.is_builder,
            self.is_connect,
            self.is_request,
            self.is_body,
            self.is_decode,
            self.is_redirect,
            self.is_status,
            self.is_timeout,
            url,
            self.source_chain
        )
    }
}

fn format_error_chain(error: &(dyn Error + 'static)) -> String {
    let mut source = Some(error);
    let mut messages = Vec::new();

    while let Some(current) = source {
        messages.push(current.to_string());
        source = current.source();
    }

    messages.join(" -> ")
}

#[cfg(test)]
mod tests {
    use super::{ApiDiarizedTranscript, ApiTranscriptSegment};
    use crate::domain::{DiarizedTranscript, RecordedAudio, TranscriptSegment};
    use std::error::Error;
    use std::fmt;

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

    #[test]
    /// 既知話者サンプルは multipart 送信用に data URL へ変換する。
    fn encodes_known_speaker_reference_as_data_url() {
        let audio = RecordedAudio {
            wav_bytes: vec![0x52, 0x49, 0x46, 0x46],
            content_type: "audio/wav",
        };

        assert_eq!(
            super::audio_data_url(&audio),
            "data:audio/wav;base64,UklGRg=="
        );
    }

    #[test]
    /// transport エラー詳細は source chain と reqwest の判定結果を連結して診断しやすくする。
    fn summarizes_request_error_details_for_debug_logging() {
        let details = super::RequestErrorDetails {
            is_builder: false,
            is_connect: true,
            is_request: true,
            is_body: false,
            is_decode: false,
            is_redirect: false,
            is_status: false,
            is_timeout: true,
            url: Some(super::TRANSCRIPTIONS_ENDPOINT.to_string()),
            source_chain: "operation timed out -> dns failed".to_string(),
        };

        assert_eq!(
            details.summary(),
            "kind={builder:false connect:true request:true body:false decode:false redirect:false status:false timeout:true} url=https://api.openai.com/v1/audio/transcriptions source_chain=operation timed out -> dns failed"
        );
    }

    #[test]
    /// source chain は最下層の原因まで順に連結する。
    fn formats_error_source_chain_in_order() {
        let error = TestError::with_source(
            "outer failure",
            TestError::with_source("middle failure", TestError::new("inner failure")),
        );

        assert_eq!(
            super::format_error_chain(&error),
            "outer failure -> middle failure -> inner failure"
        );
    }

    #[derive(Debug)]
    struct TestError {
        message: &'static str,
        source: Option<Box<TestError>>,
    }

    impl TestError {
        fn new(message: &'static str) -> Self {
            Self {
                message,
                source: None,
            }
        }

        fn with_source(message: &'static str, source: TestError) -> Self {
            Self {
                message,
                source: Some(Box::new(source)),
            }
        }
    }

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl Error for TestError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.source
                .as_deref()
                .map(|source| source as &(dyn Error + 'static))
        }
    }
}
