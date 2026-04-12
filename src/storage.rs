use crate::DiarizedTranscript;
use std::fmt;
use std::fs::{File, OpenOptions, create_dir_all};
use std::io::Write;
use std::path::{Path, PathBuf};
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPaths {
    pub session_dir: PathBuf,
    pub captures_dir: PathBuf,
    pub final_path: PathBuf,
}

impl SessionPaths {
    pub fn new(storage_root: &Path, session_dir_name: &str) -> Self {
        let session_dir = storage_root.join(session_dir_name);
        let captures_dir = session_dir.join("captures");
        let final_path = session_dir.join("final.jsonl");

        Self {
            session_dir,
            captures_dir,
            final_path,
        }
    }

    pub fn capture_path(&self, capture_index: u64) -> PathBuf {
        self.captures_dir
            .join(format!("capture-{capture_index:06}.json"))
    }
}

#[derive(Debug)]
pub enum StorageError {
    CreateSession(std::io::Error),
    ResolveLocalOffset(time::error::IndeterminateOffset),
    FormatSessionName(time::error::Format),
    WriteCapture(std::io::Error),
    SerializeCapture(serde_json::Error),
    OpenFinal(std::io::Error),
    WriteFinal(std::io::Error),
    SerializeFinal(serde_json::Error),
    SerializeDebug(serde_json::Error),
    WriteDebug(std::io::Error),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateSession(source) => {
                write!(f, "failed to create storage directories: {source}")
            }
            Self::ResolveLocalOffset(source) => {
                write!(f, "failed to resolve local timezone offset: {source}")
            }
            Self::FormatSessionName(source) => {
                write!(f, "failed to format session directory name: {source}")
            }
            Self::WriteCapture(source) => write!(f, "failed to write capture file: {source}"),
            Self::SerializeCapture(source) => {
                write!(f, "failed to serialize capture file: {source}")
            }
            Self::OpenFinal(source) => write!(f, "failed to open final log file: {source}"),
            Self::WriteFinal(source) => write!(f, "failed to append final log file: {source}"),
            Self::SerializeFinal(source) => {
                write!(f, "failed to serialize final log entry: {source}")
            }
            Self::SerializeDebug(source) => {
                write!(f, "failed to serialize debug stdout: {source}")
            }
            Self::WriteDebug(source) => write!(f, "failed to write debug stdout: {source}"),
        }
    }
}

impl std::error::Error for StorageError {}

pub fn create_timestamped_session_paths(storage_root: &Path) -> Result<SessionPaths, StorageError> {
    let session_dir_name = current_session_dir_name()?;
    let paths = SessionPaths::new(storage_root, &session_dir_name);
    create_dir_all(&paths.captures_dir).map_err(StorageError::CreateSession)?;
    Ok(paths)
}

pub fn persist_capture(
    paths: &SessionPaths,
    capture_index: u64,
    transcript: &DiarizedTranscript,
) -> Result<(), StorageError> {
    create_dir_all(&paths.captures_dir).map_err(StorageError::CreateSession)?;

    let mut capture_file =
        File::create(paths.capture_path(capture_index)).map_err(StorageError::WriteCapture)?;
    serde_json::to_writer_pretty(&mut capture_file, transcript)
        .map_err(StorageError::SerializeCapture)?;
    capture_file
        .write_all(b"\n")
        .map_err(StorageError::WriteCapture)?;

    let mut final_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.final_path)
        .map_err(StorageError::OpenFinal)?;
    serde_json::to_writer(&mut final_file, transcript).map_err(StorageError::SerializeFinal)?;
    final_file
        .write_all(b"\n")
        .map_err(StorageError::WriteFinal)?;

    Ok(())
}

pub fn write_debug_transcript<W>(
    debug_enabled: bool,
    output: &mut W,
    transcript: &DiarizedTranscript,
) -> Result<(), StorageError>
where
    W: Write,
{
    if !debug_enabled {
        return Ok(());
    }

    serde_json::to_writer_pretty(&mut *output, transcript).map_err(StorageError::SerializeDebug)?;
    output.write_all(b"\n").map_err(StorageError::WriteDebug)?;

    Ok(())
}

fn current_session_dir_name() -> Result<String, StorageError> {
    let local_offset =
        UtcOffset::current_local_offset().map_err(StorageError::ResolveLocalOffset)?;
    OffsetDateTime::now_utc()
        .to_offset(local_offset)
        .format(&format_description!(
            "[year][month][day]T[hour][minute][second]_[subsecond digits:3][offset_hour sign:mandatory][offset_minute]"
        ))
        .map_err(StorageError::FormatSessionName)
}

#[cfg(test)]
mod tests {
    use super::{SessionPaths, persist_capture, write_debug_transcript};
    use crate::{DiarizedTranscript, TranscriptSegment};

    #[test]
    /// セッション配下に captures ディレクトリと final.jsonl のパスを組み立てる。
    fn builds_session_paths_under_storage_root() {
        let paths = SessionPaths::new(
            std::path::Path::new("/tmp/diarize-log/storage"),
            "20260412T153012_345+0900",
        );

        assert_eq!(
            paths.captures_dir,
            std::path::Path::new("/tmp/diarize-log/storage/20260412T153012_345+0900/captures")
        );
        assert_eq!(
            paths.final_path,
            std::path::Path::new("/tmp/diarize-log/storage/20260412T153012_345+0900/final.jsonl")
        );
        assert_eq!(
            paths.capture_path(1),
            std::path::Path::new(
                "/tmp/diarize-log/storage/20260412T153012_345+0900/captures/capture-000001.json"
            )
        );
    }

    #[test]
    /// capture 保存時に capture JSON と final.jsonl の両方へ同じ結果を書き出す。
    fn persists_capture_json_and_appends_final_jsonl() {
        let temp_dir = tempfile::tempdir().unwrap();
        let paths = SessionPaths::new(temp_dir.path(), "20260412T153012_345+0900");

        persist_capture(&paths, 1, &sample_transcript()).unwrap();

        let capture = std::fs::read_to_string(paths.capture_path(1)).unwrap();
        let final_log = std::fs::read_to_string(&paths.final_path).unwrap();

        assert_eq!(
            capture,
            serde_json::to_string_pretty(&sample_transcript()).unwrap() + "\n"
        );
        assert_eq!(
            final_log,
            serde_json::to_string(&sample_transcript()).unwrap() + "\n"
        );
    }

    #[test]
    /// debug 無効時は標準出力へ何も書かず、有効時だけ pretty JSON を出力する。
    fn writes_debug_transcript_only_when_debug_enabled() {
        let transcript = sample_transcript();
        let mut disabled_output = Vec::new();
        let mut enabled_output = Vec::new();

        write_debug_transcript(false, &mut disabled_output, &transcript).unwrap();
        write_debug_transcript(true, &mut enabled_output, &transcript).unwrap();

        assert!(disabled_output.is_empty());
        assert_eq!(
            String::from_utf8(enabled_output).unwrap(),
            serde_json::to_string_pretty(&transcript).unwrap() + "\n"
        );
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
