use crate::ports::{CaptureStore, CaptureStoreError, DiarizedTranscript, RecordedAudio};
use serde::Serialize;
use std::fs::{File, create_dir_all};
use std::io::Write;
use std::path::{Path, PathBuf};
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionPaths {
    audios_dir: PathBuf,
    captures_dir: PathBuf,
    final_path: PathBuf,
}

impl SessionPaths {
    fn new(storage_root: &Path, session_dir_name: &str) -> Self {
        let session_dir = storage_root.join(session_dir_name);

        Self {
            audios_dir: session_dir.join("audios"),
            captures_dir: session_dir.join("captures"),
            final_path: session_dir.join("final.jsonl"),
        }
    }

    fn audio_path(&self, capture_index: u64) -> PathBuf {
        self.audios_dir
            .join(format!("capture-{capture_index:06}.wav"))
    }

    fn capture_path(&self, capture_index: u64) -> PathBuf {
        self.captures_dir
            .join(format!("capture-{capture_index:06}.json"))
    }
}

/// filesystem へ capture を保存します。
#[derive(Debug)]
pub struct FileSystemCaptureStore {
    paths: SessionPaths,
}

#[derive(Serialize)]
struct StoredCapture<'a> {
    capture_start_ms: u64,
    #[serde(flatten)]
    transcript: &'a DiarizedTranscript,
}

impl FileSystemCaptureStore {
    pub fn new(storage_root: &Path) -> Result<Self, CaptureStoreError> {
        let session_dir_name = current_session_dir_name()?;
        let paths = SessionPaths::new(storage_root, &session_dir_name);
        create_dir_all(&paths.audios_dir)
            .map_err(|error| CaptureStoreError::CreateSession(error.to_string()))?;
        create_dir_all(&paths.captures_dir)
            .map_err(|error| CaptureStoreError::CreateSession(error.to_string()))?;
        File::create(&paths.final_path)
            .map_err(|error| CaptureStoreError::OpenFinal(error.to_string()))?;

        Ok(Self { paths })
    }

    fn ensure_session_dirs(&self) -> Result<(), CaptureStoreError> {
        create_dir_all(&self.paths.audios_dir)
            .map_err(|error| CaptureStoreError::CreateSession(error.to_string()))?;
        create_dir_all(&self.paths.captures_dir)
            .map_err(|error| CaptureStoreError::CreateSession(error.to_string()))?;

        Ok(())
    }
}

impl CaptureStore for FileSystemCaptureStore {
    fn persist_audio(
        &mut self,
        capture_index: u64,
        audio: &RecordedAudio,
    ) -> Result<(), CaptureStoreError> {
        self.ensure_session_dirs()?;

        let mut audio_file = File::create(self.paths.audio_path(capture_index))
            .map_err(|error| CaptureStoreError::WriteAudio(error.to_string()))?;
        audio_file
            .write_all(&audio.wav_bytes)
            .map_err(|error| CaptureStoreError::WriteAudio(error.to_string()))?;

        Ok(())
    }

    fn persist_transcript(
        &mut self,
        capture_index: u64,
        capture_start_ms: u64,
        transcript: &DiarizedTranscript,
    ) -> Result<(), CaptureStoreError> {
        self.ensure_session_dirs()?;

        let mut capture_file = File::create(self.paths.capture_path(capture_index))
            .map_err(|error| CaptureStoreError::WriteCapture(error.to_string()))?;
        serde_json::to_writer_pretty(
            &mut capture_file,
            &StoredCapture {
                capture_start_ms,
                transcript,
            },
        )
        .map_err(|error| CaptureStoreError::SerializeCapture(error.to_string()))?;
        capture_file
            .write_all(b"\n")
            .map_err(|error| CaptureStoreError::WriteCapture(error.to_string()))?;

        Ok(())
    }
}

fn current_session_dir_name() -> Result<String, CaptureStoreError> {
    let local_offset = UtcOffset::current_local_offset()
        .map_err(|error| CaptureStoreError::ResolveLocalOffset(error.to_string()))?;

    OffsetDateTime::now_utc()
        .to_offset(local_offset)
        .format(&format_description!(
            "[year][month][day]T[hour][minute][second]_[subsecond digits:3][offset_hour sign:mandatory][offset_minute]"
        ))
        .map_err(|error| CaptureStoreError::FormatSessionName(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::FileSystemCaptureStore;
    use crate::ports::{CaptureStore, DiarizedTranscript, RecordedAudio, TranscriptSegment};

    #[test]
    /// セッション配下に audios と captures ディレクトリおよび空の final.jsonl を作成して開始時刻付き transcript を書き出す。
    fn persists_audio_wav_and_capture_json_and_keeps_final_jsonl_empty_before_merge() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemCaptureStore::new(temp_dir.path()).unwrap();

        store.persist_audio(1, &sample_audio()).unwrap();
        store
            .persist_transcript(1, 1_420, &sample_transcript())
            .unwrap();

        let mut session_dirs = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        session_dirs.sort();

        assert_eq!(session_dirs.len(), 1);

        let session_dir = &session_dirs[0];
        let audio_path = session_dir.join("audios").join("capture-000001.wav");
        let capture_path = session_dir.join("captures").join("capture-000001.json");
        let final_path = session_dir.join("final.jsonl");

        assert_eq!(std::fs::read(audio_path).unwrap(), sample_audio().wav_bytes);
        assert_eq!(
            std::fs::read_to_string(capture_path).unwrap(),
            concat!(
                "{\n",
                "  \"capture_start_ms\": 1420,\n",
                "  \"text\": \"こんにちは 今日はよろしくお願いします\",\n",
                "  \"segments\": [\n",
                "    {\n",
                "      \"speaker\": \"spk_0\",\n",
                "      \"start_ms\": 0,\n",
                "      \"end_ms\": 900,\n",
                "      \"text\": \"こんにちは\"\n",
                "    },\n",
                "    {\n",
                "      \"speaker\": \"spk_1\",\n",
                "      \"start_ms\": 950,\n",
                "      \"end_ms\": 2300,\n",
                "      \"text\": \"今日はよろしくお願いします\"\n",
                "    }\n",
                "  ]\n",
                "}\n"
            )
        );
        assert_eq!(std::fs::read_to_string(final_path).unwrap(), "");
    }

    #[test]
    /// wav と transcript のファイル名は 6 桁ゼロ埋めの連番にする。
    fn names_capture_files_with_zero_padded_sequence() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemCaptureStore::new(temp_dir.path()).unwrap();

        store.persist_audio(12, &sample_audio()).unwrap();
        store
            .persist_transcript(12, 12_000, &sample_transcript())
            .unwrap();

        let session_dir = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();

        assert!(session_dir.join("audios/capture-000012.wav").exists());
        assert!(session_dir.join("captures/capture-000012.json").exists());
    }

    fn sample_audio() -> RecordedAudio {
        RecordedAudio {
            wav_bytes: vec![0x52, 0x49, 0x46, 0x46],
            content_type: "audio/wav",
        }
    }

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
}
