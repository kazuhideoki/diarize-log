use crate::application::ports::{SpeakerEmbedder, SpeakerEmbedderError};
use crate::domain::{KnownSpeakerEmbedding, RecordedAudio};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::{Command, Stdio};

const DEFAULT_EMBEDDER_COMMAND: &str = "python3";
const DEFAULT_EMBEDDER_SCRIPT: &str = "scripts/speaker_embedder.py";

/// Python の SpeechBrain worker を呼び出して speaker embedding を抽出します。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonSpeakerEmbedder {
    command: String,
    script_path: String,
}

impl Default for PythonSpeakerEmbedder {
    fn default() -> Self {
        Self {
            command: DEFAULT_EMBEDDER_COMMAND.to_string(),
            script_path: DEFAULT_EMBEDDER_SCRIPT.to_string(),
        }
    }
}

impl PythonSpeakerEmbedder {
    pub fn new(command: String, script_path: String) -> Self {
        Self {
            command,
            script_path,
        }
    }

    /// speaker embedder の Python 依存が現在の CLI 実行環境で読み込めるか検査します。
    pub fn check_available(&self) -> Result<(), SpeakerEmbedderError> {
        let output = Command::new(&self.command)
            .arg(&self.script_path)
            .arg("--check")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| SpeakerEmbedderError::SpawnProcess(error.to_string()))?;

        if !output.status.success() {
            return Err(SpeakerEmbedderError::ProcessFailed {
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(())
    }
}

impl SpeakerEmbedder for PythonSpeakerEmbedder {
    fn embed_speaker(
        &self,
        speaker_name: &str,
        audio: &RecordedAudio,
    ) -> Result<KnownSpeakerEmbedding, SpeakerEmbedderError> {
        let input = EmbedderInput {
            speaker_name,
            content_type: audio.content_type,
            wav_base64: &base64::engine::general_purpose::STANDARD.encode(&audio.wav_bytes),
        };
        let input_json = serde_json::to_vec(&input)
            .map_err(|error| SpeakerEmbedderError::WriteInput(error.to_string()))?;
        let mut child = Command::new(&self.command)
            .arg(&self.script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| SpeakerEmbedderError::SpawnProcess(error.to_string()))?;
        child
            .stdin
            .as_mut()
            .ok_or_else(|| SpeakerEmbedderError::WriteInput("stdin was not piped".to_string()))?
            .write_all(&input_json)
            .map_err(|error| SpeakerEmbedderError::WriteInput(error.to_string()))?;
        let output = child
            .wait_with_output()
            .map_err(|error| SpeakerEmbedderError::ReadOutput(error.to_string()))?;

        if !output.status.success() {
            return Err(SpeakerEmbedderError::ProcessFailed {
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        let response: EmbedderOutput = serde_json::from_slice(&output.stdout)
            .map_err(|error| SpeakerEmbedderError::ParseOutput(error.to_string()))?;
        if response.vector.is_empty() {
            return Err(SpeakerEmbedderError::InvalidOutput(
                "embedding vector is empty".to_string(),
            ));
        }

        Ok(KnownSpeakerEmbedding {
            speaker_name: response.speaker_name,
            model: response.model,
            vector: response.vector,
        })
    }
}

#[derive(Serialize)]
struct EmbedderInput<'a> {
    speaker_name: &'a str,
    content_type: &'a str,
    wav_base64: &'a str,
}

#[derive(Deserialize)]
struct EmbedderOutput {
    speaker_name: String,
    model: String,
    vector: Vec<f32>,
}
