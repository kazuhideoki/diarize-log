use crate::adapters::{LineLogger, PythonSpeakerEmbedder};
use crate::application::ports::{
    Logger, SpeakerEmbedder, Transcriber, TranscriberError, TranscriptionRequest,
};
use crate::domain::{
    DiarizationSegment, DiarizedTranscript, KnownSpeakerEmbedding, RecordedAudio,
    SpeakerIdentification, SpeechTurn, SpeechTurnPolicy, TranscriptSegment, build_speech_turns,
};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use reqwest::blocking::{Client, multipart};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const OPENAI_TRANSCRIPTIONS_ENDPOINT: &str = "https://api.openai.com/v1/audio/transcriptions";
const OPENAI_ASR_MODEL: &str = "gpt-4o-transcribe";
const PYANNOTE_MEDIA_ENDPOINT: &str = "https://api.pyannote.ai/v1/media/input";
const PYANNOTE_DIARIZE_ENDPOINT: &str = "https://api.pyannote.ai/v1/diarize";
const PYANNOTE_JOBS_ENDPOINT: &str = "https://api.pyannote.ai/v1/jobs";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const PYANNOTE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const PYANNOTE_POLL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const UNKNOWN_SPEAKER: &str = "UNKNOWN";
const IDENTIFICATION_MIN_SCORE: f32 = 0.50;
const IDENTIFICATION_MIN_MARGIN: f32 = 0.15;

/// 話者分離、話者同定、ASR を別々に実行して統合します。
pub struct SeparatedTranscriber {
    client: Client,
    openai_api_key: String,
    pyannote_api_key: String,
    pyannote_max_speakers: Option<u64>,
    known_embeddings: Vec<KnownSpeakerEmbedding>,
    speaker_embedder: PythonSpeakerEmbedder,
    logger: LineLogger,
}

impl SeparatedTranscriber {
    pub fn new(
        openai_api_key: String,
        pyannote_api_key: String,
        pyannote_max_speakers: Option<u64>,
        known_embeddings: Vec<KnownSpeakerEmbedding>,
        logger: LineLogger,
    ) -> Result<Self, TranscriberError> {
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| TranscriberError::BuildHttpClient(error.to_string()))?;
        Ok(Self {
            client,
            openai_api_key,
            pyannote_api_key,
            pyannote_max_speakers,
            known_embeddings,
            speaker_embedder: PythonSpeakerEmbedder::default(),
            logger,
        })
    }
}

impl Transcriber for SeparatedTranscriber {
    fn transcribe(
        &mut self,
        request: TranscriptionRequest<'_>,
    ) -> Result<DiarizedTranscript, TranscriberError> {
        let _ = self.logger.debug(&format!(
            "separated transcription started: pyannote_model=precision-2 asr_model={OPENAI_ASR_MODEL} max_speakers={:?} known_speakers={}",
            self.pyannote_max_speakers,
            self.known_embeddings.len()
        ));
        let capture_duration_ms = wav_duration_ms(request.audio)?;
        let diarization = self.diarize(request.audio)?;
        let turns = build_speech_turns(
            &diarization,
            capture_duration_ms,
            &SpeechTurnPolicy::recommended(),
        );
        let speaker_names = self.identify_speakers(request.audio, &turns)?;
        let texts = self.transcribe_turns(request.audio, &turns, request.language)?;
        let segments = turns
            .into_iter()
            .zip(texts)
            .filter_map(|(turn, text)| {
                let text = text.trim().to_string();
                if text.is_empty() {
                    return None;
                }
                Some(TranscriptSegment {
                    speaker: speaker_names
                        .get(&turn.anonymous_speaker)
                        .cloned()
                        .unwrap_or_else(|| UNKNOWN_SPEAKER.to_string()),
                    start_ms: turn.start_ms,
                    end_ms: turn.end_ms,
                    text,
                })
            })
            .collect::<Vec<_>>();
        let text = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        Ok(DiarizedTranscript { text, segments })
    }
}

impl SeparatedTranscriber {
    fn diarize(&self, audio: &RecordedAudio) -> Result<Vec<DiarizationSegment>, TranscriberError> {
        let media_id = format!(
            "diarize-log-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time must be after Unix epoch")
                .as_nanos()
        );
        let media_url = format!("media://{media_id}");
        let upload = self.create_pyannote_media(&media_url)?;
        self.upload_pyannote_media(&upload.url, audio)?;
        let job_id = self.create_pyannote_job(&media_url)?;
        self.poll_pyannote_job(&job_id)
    }

    fn create_pyannote_media(
        &self,
        media_url: &str,
    ) -> Result<PyannoteMediaUpload, TranscriberError> {
        let response = self
            .client
            .post(PYANNOTE_MEDIA_ENDPOINT)
            .bearer_auth(&self.pyannote_api_key)
            .json(&PyannoteMediaRequest {
                url: media_url.to_string(),
            })
            .send()
            .map_err(|error| TranscriberError::SendRequest(error.to_string()))?;
        parse_json_response(response, "pyannote media upload")
    }

    fn upload_pyannote_media(
        &self,
        upload_url: &str,
        audio: &RecordedAudio,
    ) -> Result<(), TranscriberError> {
        let response = self
            .client
            .put(upload_url)
            .header("Content-Type", "application/octet-stream")
            .body(audio.wav_bytes.clone())
            .send()
            .map_err(|error| TranscriberError::SendRequest(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            return Err(TranscriberError::ApiError {
                status_code: status,
                body,
            });
        }
        Ok(())
    }

    fn create_pyannote_job(&self, media_url: &str) -> Result<String, TranscriberError> {
        let response = self
            .client
            .post(PYANNOTE_DIARIZE_ENDPOINT)
            .bearer_auth(&self.pyannote_api_key)
            .json(&PyannoteDiarizeRequest {
                url: media_url,
                model: "precision-2",
                exclusive: true,
                max_speakers: self.pyannote_max_speakers,
            })
            .send()
            .map_err(|error| TranscriberError::SendRequest(error.to_string()))?;
        let job: PyannoteJobResponse = parse_json_response(response, "pyannote diarize")?;
        Ok(job.job_id)
    }

    fn poll_pyannote_job(&self, job_id: &str) -> Result<Vec<DiarizationSegment>, TranscriberError> {
        let started_at = Instant::now();
        loop {
            if started_at.elapsed() > PYANNOTE_POLL_TIMEOUT {
                return Err(TranscriberError::SendRequest(
                    "pyannote diarization polling timed out".to_string(),
                ));
            }
            let response = self
                .client
                .get(format!("{PYANNOTE_JOBS_ENDPOINT}/{job_id}"))
                .bearer_auth(&self.pyannote_api_key)
                .send()
                .map_err(|error| TranscriberError::SendRequest(error.to_string()))?;
            let job: PyannoteDiarizationResult =
                parse_json_response(response, "pyannote diarize poll")?;
            match job.status.as_str() {
                "succeeded" | "done" => return Ok(job.into_segments()),
                "failed" | "canceled" | "cancelled" => {
                    return Err(TranscriberError::ApiError {
                        status_code: 200,
                        body: format!("pyannote job {job_id} failed: {:?}", job.error),
                    });
                }
                _ => thread::sleep(PYANNOTE_POLL_INTERVAL),
            }
        }
    }

    fn identify_speakers(
        &self,
        audio: &RecordedAudio,
        turns: &[SpeechTurn],
    ) -> Result<BTreeMap<String, String>, TranscriberError> {
        if self.known_embeddings.is_empty() {
            return Ok(BTreeMap::new());
        }

        let mut vectors_by_speaker: BTreeMap<String, Vec<(Vec<f32>, u64)>> = BTreeMap::new();
        for turn in turns {
            let clipped = clip_audio(audio, turn.start_ms, turn.end_ms)?;
            let embedding = self
                .speaker_embedder
                .embed_speaker(&turn.anonymous_speaker, &clipped)
                .map_err(|error| TranscriberError::SendRequest(error.to_string()))?;
            vectors_by_speaker
                .entry(turn.anonymous_speaker.clone())
                .or_default()
                .push((embedding.vector, turn.end_ms.saturating_sub(turn.start_ms)));
        }

        let mut names = BTreeMap::new();
        for (anonymous_speaker, weighted_vectors) in vectors_by_speaker {
            let averaged = average_vectors(&weighted_vectors);
            let identification = identify_vector(&averaged, &self.known_embeddings);
            names.insert(
                anonymous_speaker,
                identification
                    .map(|result| result.speaker_name)
                    .unwrap_or_else(|| UNKNOWN_SPEAKER.to_string()),
            );
        }

        Ok(names)
    }

    fn transcribe_turns(
        &self,
        audio: &RecordedAudio,
        turns: &[SpeechTurn],
        language: Option<&str>,
    ) -> Result<Vec<String>, TranscriberError> {
        let mut outputs = vec![String::new(); turns.len()];
        let mut next_index = 0;
        while next_index < turns.len() {
            let batch_end = (next_index + 2).min(turns.len());
            let mut handles = Vec::new();
            for (offset, turn) in turns[next_index..batch_end].iter().cloned().enumerate() {
                let client = self.client.clone();
                let api_key = self.openai_api_key.clone();
                let source_audio = audio.clone();
                let language = language.map(ToOwned::to_owned);
                handles.push(thread::spawn(move || {
                    transcribe_one_turn(
                        &client,
                        &api_key,
                        &source_audio,
                        &turn,
                        language.as_deref(),
                    )
                    .map(|text| (next_index + offset, text))
                }));
            }
            for handle in handles {
                let (index, text) = handle.join().map_err(|_| {
                    TranscriberError::SendRequest("ASR worker thread panicked".to_string())
                })??;
                outputs[index] = text;
            }
            next_index = batch_end;
        }
        Ok(outputs)
    }
}

fn transcribe_one_turn(
    client: &Client,
    api_key: &str,
    audio: &RecordedAudio,
    turn: &SpeechTurn,
    language: Option<&str>,
) -> Result<String, TranscriberError> {
    let clipped = clip_audio(audio, turn.start_ms, turn.end_ms)?;
    let audio_part = multipart::Part::bytes(clipped.wav_bytes)
        .file_name("turn.wav")
        .mime_str(clipped.content_type)
        .map_err(|error| TranscriberError::InvalidMimeType(error.to_string()))?;
    let mut form = multipart::Form::new()
        .part("file", audio_part)
        .text("model", OPENAI_ASR_MODEL.to_string())
        .text("response_format", "json".to_string());
    if let Some(language) = language {
        form = form.text("language", language.to_string());
    }
    let response = client
        .post(OPENAI_TRANSCRIPTIONS_ENDPOINT)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .map_err(|error| TranscriberError::SendRequest(error.to_string()))?;
    let asr: OpenAiTextResponse = parse_json_response(response, "openai text transcription")?;
    Ok(asr.text)
}

fn parse_json_response<T>(
    response: reqwest::blocking::Response,
    context: &str,
) -> Result<T, TranscriberError>
where
    T: for<'de> Deserialize<'de>,
{
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| TranscriberError::ReadResponseBody(error.to_string()))?;
    if !status.is_success() {
        return Err(TranscriberError::ApiError {
            status_code: status.as_u16(),
            body,
        });
    }
    serde_json::from_str(&body).map_err(|source| TranscriberError::ParseResponseBody {
        source: format!("{context}: {source}"),
        body,
    })
}

fn identify_vector(
    vector: &[f32],
    known_embeddings: &[KnownSpeakerEmbedding],
) -> Option<SpeakerIdentification> {
    let mut scores = known_embeddings
        .iter()
        .map(|embedding| {
            (
                embedding.speaker_name.clone(),
                cosine_similarity(vector, &embedding.vector),
            )
        })
        .collect::<Vec<_>>();
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    let (speaker_name, best_score) = scores.first()?.clone();
    let second_score = scores.get(1).map(|(_, score)| *score).unwrap_or(0.0);
    let margin = best_score - second_score;
    if best_score >= IDENTIFICATION_MIN_SCORE && margin >= IDENTIFICATION_MIN_MARGIN {
        Some(SpeakerIdentification {
            speaker_name,
            score: best_score,
            margin,
        })
    } else {
        None
    }
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let dot = left.iter().zip(right).map(|(a, b)| a * b).sum::<f32>();
    let left_norm = left.iter().map(|value| value * value).sum::<f32>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f32>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    dot / (left_norm * right_norm)
}

fn average_vectors(weighted_vectors: &[(Vec<f32>, u64)]) -> Vec<f32> {
    let Some((first, _)) = weighted_vectors.first() else {
        return Vec::new();
    };
    let mut output = vec![0.0; first.len()];
    let mut total_weight = 0_f32;
    for (vector, weight) in weighted_vectors {
        if vector.len() != output.len() {
            continue;
        }
        let weight = *weight as f32;
        total_weight += weight;
        for (index, value) in vector.iter().enumerate() {
            output[index] += value * weight;
        }
    }
    if total_weight > 0.0 {
        for value in &mut output {
            *value /= total_weight;
        }
    }
    output
}

fn wav_duration_ms(audio: &RecordedAudio) -> Result<u64, TranscriberError> {
    let reader = WavReader::new(Cursor::new(audio.wav_bytes.clone()))
        .map_err(|error| TranscriberError::SendRequest(format!("failed to read wav: {error}")))?;
    let spec = reader.spec();
    Ok(u64::from(reader.duration()) * 1_000 / u64::from(spec.sample_rate))
}

fn clip_audio(
    audio: &RecordedAudio,
    start_ms: u64,
    end_ms: u64,
) -> Result<RecordedAudio, TranscriberError> {
    let reader = WavReader::new(Cursor::new(audio.wav_bytes.clone()))
        .map_err(|error| TranscriberError::SendRequest(format!("failed to read wav: {error}")))?;
    let spec = reader.spec();
    match spec.sample_format {
        SampleFormat::Float => clip_samples::<f32>(reader, spec, start_ms, end_ms),
        SampleFormat::Int => clip_samples::<i32>(reader, spec, start_ms, end_ms),
    }
}

fn clip_samples<T>(
    reader: WavReader<Cursor<Vec<u8>>>,
    spec: WavSpec,
    start_ms: u64,
    end_ms: u64,
) -> Result<RecordedAudio, TranscriberError>
where
    T: hound::Sample + Copy,
{
    let samples = reader
        .into_samples::<T>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            TranscriberError::SendRequest(format!("failed to read wav samples: {error}"))
        })?;
    let channels = usize::from(spec.channels);
    let total_frames = samples.len() / channels;
    let start_frame = millis_to_frames(start_ms, spec.sample_rate);
    let end_frame = millis_to_frames(end_ms, spec.sample_rate).min(total_frames as u64);
    if end_frame <= start_frame {
        return Err(TranscriberError::SendRequest(format!(
            "invalid turn range: start_ms={start_ms} end_ms={end_ms}"
        )));
    }
    let start_sample = usize::try_from(start_frame)
        .expect("frame count must fit usize")
        .saturating_mul(channels);
    let end_sample = usize::try_from(end_frame)
        .expect("frame count must fit usize")
        .saturating_mul(channels);
    let mut output = Cursor::new(Vec::new());
    let mut writer = WavWriter::new(&mut output, spec)
        .map_err(|error| TranscriberError::SendRequest(format!("failed to encode wav: {error}")))?;
    for sample in &samples[start_sample..end_sample] {
        writer.write_sample(*sample).map_err(|error| {
            TranscriberError::SendRequest(format!("failed to encode wav: {error}"))
        })?;
    }
    writer
        .finalize()
        .map_err(|error| TranscriberError::SendRequest(format!("failed to encode wav: {error}")))?;
    Ok(RecordedAudio {
        wav_bytes: output.into_inner(),
        content_type: "audio/wav",
    })
}

fn millis_to_frames(millis: u64, sample_rate: u32) -> u64 {
    millis.saturating_mul(u64::from(sample_rate)) / 1_000
}

#[derive(Serialize)]
struct PyannoteMediaRequest {
    url: String,
}

#[derive(Deserialize)]
struct PyannoteMediaUpload {
    #[serde(rename = "url")]
    url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PyannoteDiarizeRequest<'a> {
    url: &'a str,
    model: &'a str,
    exclusive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_speakers: Option<u64>,
}

#[derive(Deserialize)]
struct PyannoteJobResponse {
    #[serde(alias = "jobId", alias = "id")]
    job_id: String,
}

#[derive(Deserialize, Debug)]
struct PyannoteDiarizationResult {
    status: String,
    output: Option<PyannoteDiarizationOutput>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

impl PyannoteDiarizationResult {
    fn into_segments(self) -> Vec<DiarizationSegment> {
        let output = self.output.unwrap_or_default();
        let source = if output.exclusive_diarization.is_empty() {
            output.diarization
        } else {
            output.exclusive_diarization
        };
        source
            .into_iter()
            .map(|segment| DiarizationSegment {
                anonymous_speaker: segment.speaker,
                start_ms: seconds_to_millis(segment.start),
                end_ms: seconds_to_millis(segment.end),
            })
            .collect()
    }
}

#[derive(Deserialize, Debug, Default)]
struct PyannoteDiarizationOutput {
    #[serde(default, alias = "exclusiveDiarization")]
    exclusive_diarization: Vec<PyannoteSegment>,
    #[serde(default)]
    diarization: Vec<PyannoteSegment>,
}

#[derive(Deserialize, Debug)]
struct PyannoteSegment {
    speaker: String,
    start: f64,
    end: f64,
}

#[derive(Deserialize)]
struct OpenAiTextResponse {
    text: String,
}

fn seconds_to_millis(seconds: f64) -> u64 {
    (seconds * 1_000.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 類似度が score と margin の両方の閾値を満たす場合だけ既知話者名を返す。
    fn identifies_vector_only_when_score_and_margin_are_high_enough() {
        let known = vec![
            embedding("oki", vec![1.0, 0.0]),
            embedding("sato", vec![0.0, 1.0]),
        ];

        let identified = identify_vector(&[0.9, 0.1], &known).unwrap();
        let unknown = identify_vector(&[0.6, 0.5], &known);

        assert_eq!(identified.speaker_name, "oki");
        assert!(unknown.is_none());
    }

    fn embedding(name: &str, vector: Vec<f32>) -> KnownSpeakerEmbedding {
        KnownSpeakerEmbedding {
            speaker_name: name.to_string(),
            model: "fake".to_string(),
            vector,
        }
    }
}
