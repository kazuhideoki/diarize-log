use crate::{CaptureStore, CaptureStoreError, DiarizedTranscript, RecordedAudio};
use std::fs::{File, OpenOptions, create_dir_all};
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

impl FileSystemCaptureStore {
    pub fn new(storage_root: &Path) -> Result<Self, CaptureStoreError> {
        let session_dir_name = current_session_dir_name()?;
        let paths = SessionPaths::new(storage_root, &session_dir_name);
        create_dir_all(&paths.audios_dir).map_err(CaptureStoreError::CreateSession)?;
        create_dir_all(&paths.captures_dir).map_err(CaptureStoreError::CreateSession)?;

        Ok(Self { paths })
    }
}

impl CaptureStore for FileSystemCaptureStore {
    fn persist_capture(
        &mut self,
        capture_index: u64,
        audio: &RecordedAudio,
        transcript: &DiarizedTranscript,
    ) -> Result<(), CaptureStoreError> {
        create_dir_all(&self.paths.audios_dir).map_err(CaptureStoreError::CreateSession)?;
        create_dir_all(&self.paths.captures_dir).map_err(CaptureStoreError::CreateSession)?;

        let mut audio_file = File::create(self.paths.audio_path(capture_index))
            .map_err(CaptureStoreError::WriteAudio)?;
        audio_file
            .write_all(&audio.wav_bytes)
            .map_err(CaptureStoreError::WriteAudio)?;

        let mut capture_file = File::create(self.paths.capture_path(capture_index))
            .map_err(CaptureStoreError::WriteCapture)?;
        serde_json::to_writer_pretty(&mut capture_file, transcript)
            .map_err(CaptureStoreError::SerializeCapture)?;
        capture_file
            .write_all(b"\n")
            .map_err(CaptureStoreError::WriteCapture)?;

        let mut final_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.paths.final_path)
            .map_err(CaptureStoreError::OpenFinal)?;
        serde_json::to_writer(&mut final_file, transcript)
            .map_err(CaptureStoreError::SerializeFinal)?;
        final_file
            .write_all(b"\n")
            .map_err(CaptureStoreError::WriteFinal)?;

        Ok(())
    }
}

fn current_session_dir_name() -> Result<String, CaptureStoreError> {
    let local_offset =
        UtcOffset::current_local_offset().map_err(CaptureStoreError::ResolveLocalOffset)?;

    OffsetDateTime::now_utc()
        .to_offset(local_offset)
        .format(&format_description!(
            "[year][month][day]T[hour][minute][second]_[subsecond digits:3][offset_hour sign:mandatory][offset_minute]"
        ))
        .map_err(CaptureStoreError::FormatSessionName)
}

#[cfg(test)]
mod tests {
    use super::FileSystemCaptureStore;
    use crate::{CaptureStore, DiarizedTranscript, RecordedAudio, TranscriptSegment};

    #[test]
    /// セッション配下に audios と captures ディレクトリおよび final.jsonl を作成して録音音声と文字起こし結果を書き出す。
    fn persists_audio_wav_and_capture_json_and_appends_final_jsonl_under_session_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemCaptureStore::new(temp_dir.path()).unwrap();

        store
            .persist_capture(1, &sample_audio(), &sample_transcript())
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
            serde_json::to_string_pretty(&sample_transcript()).unwrap() + "\n"
        );
        assert_eq!(
            std::fs::read_to_string(final_path).unwrap(),
            serde_json::to_string(&sample_transcript()).unwrap() + "\n"
        );
    }

    #[test]
    /// capture ファイル名は 6 桁ゼロ埋めの連番にする。
    fn names_capture_files_with_zero_padded_sequence() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemCaptureStore::new(temp_dir.path()).unwrap();

        store
            .persist_capture(12, &sample_audio(), &sample_transcript())
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
