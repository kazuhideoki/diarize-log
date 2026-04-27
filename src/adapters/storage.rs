use crate::application::ports::{
    CaptureSessionMetadata, CaptureStore, CaptureStoreError, MixedCaptureSessionMetadata,
    MixedCaptureStore, SpeakerStore, SpeakerStoreError,
};
use crate::domain::{
    DiarizedTranscript, KnownSpeakerEmbedding, KnownSpeakerSample, MergeAuditEntry,
    MergedTranscriptSegment, RecordedAudio, SourcedTranscriptSegment, TranscriptSource,
};
use serde::Serialize;
use std::fs::{File, OpenOptions, create_dir_all, remove_file};
use std::io::Write;
use std::path::{Path, PathBuf};
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

const RUNS_DIR_NAME: &str = "runs";

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionPaths {
    audios_dir: PathBuf,
    captures_dir: PathBuf,
    metadata_path: PathBuf,
    merged_path: PathBuf,
    merge_audit_path: PathBuf,
}

impl SessionPaths {
    fn at_session_root(session_dir: &Path) -> Self {
        Self {
            audios_dir: session_dir.join("audios"),
            captures_dir: session_dir.join("captures"),
            metadata_path: session_dir.join("metadata.json"),
            merged_path: session_dir.join("merged.jsonl"),
            merge_audit_path: session_dir.join("merge-audit.jsonl"),
        }
    }

    fn for_source(session_dir: &Path, source: TranscriptSource) -> Self {
        let source_dir = session_dir
            .join("sources")
            .join(source.as_storage_dir_name());

        Self {
            audios_dir: source_dir.join("audios"),
            captures_dir: source_dir.join("captures"),
            metadata_path: source_dir.join("metadata.json"),
            merged_path: source_dir.join("merged.jsonl"),
            merge_audit_path: source_dir.join("merge-audit.jsonl"),
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

/// filesystem へ最終統合 transcript を保存します。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSystemMergedTranscriptStore {
    metadata_path: PathBuf,
    merged_path: PathBuf,
}

/// filesystem へ話者サンプルを保存します。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSystemSpeakerStore {
    speakers_dir: PathBuf,
}

#[derive(Serialize)]
struct StoredCapture<'a> {
    capture_start_ms: u64,
    #[serde(flatten)]
    transcript: &'a DiarizedTranscript,
}

impl FileSystemCaptureStore {
    pub fn create_session_dir(storage_root: &Path) -> Result<PathBuf, CaptureStoreError> {
        let session_dir_name = current_session_dir_name()?;
        let session_dir = storage_root.join(RUNS_DIR_NAME).join(session_dir_name);
        create_dir_all(&session_dir)
            .map_err(|error| CaptureStoreError::CreateSession(error.to_string()))?;
        Ok(session_dir)
    }

    pub fn new(storage_root: &Path) -> Result<Self, CaptureStoreError> {
        let session_dir = Self::create_session_dir(storage_root)?;
        Self::from_paths(SessionPaths::at_session_root(&session_dir))
    }

    pub fn new_for_source(
        session_dir: &Path,
        source: TranscriptSource,
    ) -> Result<Self, CaptureStoreError> {
        Self::from_paths(SessionPaths::for_source(session_dir, source))
    }

    fn from_paths(paths: SessionPaths) -> Result<Self, CaptureStoreError> {
        create_dir_all(&paths.audios_dir)
            .map_err(|error| CaptureStoreError::CreateSession(error.to_string()))?;
        create_dir_all(&paths.captures_dir)
            .map_err(|error| CaptureStoreError::CreateSession(error.to_string()))?;
        File::create(&paths.merged_path)
            .map_err(|error| CaptureStoreError::OpenMerged(error.to_string()))?;
        File::create(&paths.merge_audit_path)
            .map_err(|error| CaptureStoreError::OpenMergeAudit(error.to_string()))?;

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

impl FileSystemMergedTranscriptStore {
    pub fn new(session_dir: &Path) -> Result<Self, CaptureStoreError> {
        create_dir_all(session_dir)
            .map_err(|error| CaptureStoreError::CreateSession(error.to_string()))?;
        let metadata_path = session_dir.join("metadata.json");
        let merged_path = session_dir.join("merged.jsonl");
        File::create(&merged_path)
            .map_err(|error| CaptureStoreError::OpenMerged(error.to_string()))?;

        Ok(Self {
            metadata_path,
            merged_path,
        })
    }

    fn persist_segments(
        &mut self,
        segments: &[SourcedTranscriptSegment],
    ) -> Result<(), CaptureStoreError> {
        let mut merged_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.merged_path)
            .map_err(|error| CaptureStoreError::OpenMerged(error.to_string()))?;
        for segment in segments {
            serde_json::to_writer(&mut merged_file, segment)
                .map_err(|error| CaptureStoreError::SerializeMerged(error.to_string()))?;
            merged_file
                .write_all(b"\n")
                .map_err(|error| CaptureStoreError::WriteMerged(error.to_string()))?;
        }

        Ok(())
    }

    fn persist_metadata(
        &mut self,
        metadata: &MixedCaptureSessionMetadata,
    ) -> Result<(), CaptureStoreError> {
        let mut metadata_file = File::create(&self.metadata_path)
            .map_err(|error| CaptureStoreError::WriteMetadata(error.to_string()))?;
        serde_json::to_writer_pretty(&mut metadata_file, metadata)
            .map_err(|error| CaptureStoreError::SerializeMetadata(error.to_string()))?;
        metadata_file
            .write_all(b"\n")
            .map_err(|error| CaptureStoreError::WriteMetadata(error.to_string()))?;
        Ok(())
    }
}

impl MixedCaptureStore for FileSystemMergedTranscriptStore {
    fn persist_mixed_session_metadata(
        &mut self,
        metadata: &MixedCaptureSessionMetadata,
    ) -> Result<(), CaptureStoreError> {
        self.persist_metadata(metadata)
    }

    fn persist_final_segments(
        &mut self,
        segments: &[SourcedTranscriptSegment],
    ) -> Result<(), CaptureStoreError> {
        self.persist_segments(segments)
    }
}

impl FileSystemSpeakerStore {
    pub fn new(storage_root: &Path) -> Self {
        Self {
            speakers_dir: storage_root.join("speakers"),
        }
    }

    fn sample_path(&self, speaker_name: &str) -> Result<PathBuf, SpeakerStoreError> {
        validate_speaker_name(speaker_name)?;
        Ok(self.speakers_dir.join(format!("{speaker_name}.wav")))
    }

    fn embedding_path(&self, speaker_name: &str) -> Result<PathBuf, SpeakerStoreError> {
        validate_speaker_name(speaker_name)?;
        Ok(self
            .speakers_dir
            .join(format!("{speaker_name}.embedding.json")))
    }
}

impl CaptureStore for FileSystemCaptureStore {
    fn persist_session_metadata(
        &mut self,
        metadata: &CaptureSessionMetadata,
    ) -> Result<(), CaptureStoreError> {
        self.ensure_session_dirs()?;

        let mut metadata_file = File::create(&self.paths.metadata_path)
            .map_err(|error| CaptureStoreError::WriteMetadata(error.to_string()))?;
        serde_json::to_writer_pretty(&mut metadata_file, metadata)
            .map_err(|error| CaptureStoreError::SerializeMetadata(error.to_string()))?;
        metadata_file
            .write_all(b"\n")
            .map_err(|error| CaptureStoreError::WriteMetadata(error.to_string()))?;

        Ok(())
    }

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

    fn persist_merged_segments(
        &mut self,
        segments: &[MergedTranscriptSegment],
    ) -> Result<(), CaptureStoreError> {
        self.ensure_session_dirs()?;

        let mut merged_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.paths.merged_path)
            .map_err(|error| CaptureStoreError::OpenMerged(error.to_string()))?;
        for segment in segments {
            serde_json::to_writer(&mut merged_file, segment)
                .map_err(|error| CaptureStoreError::SerializeMerged(error.to_string()))?;
            merged_file
                .write_all(b"\n")
                .map_err(|error| CaptureStoreError::WriteMerged(error.to_string()))?;
        }

        Ok(())
    }

    fn persist_merge_audit_entries(
        &mut self,
        entries: &[MergeAuditEntry],
    ) -> Result<(), CaptureStoreError> {
        self.ensure_session_dirs()?;

        let mut merge_audit_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.paths.merge_audit_path)
            .map_err(|error| CaptureStoreError::OpenMergeAudit(error.to_string()))?;
        for entry in entries {
            serde_json::to_writer(&mut merge_audit_file, entry)
                .map_err(|error| CaptureStoreError::SerializeMergeAudit(error.to_string()))?;
            merge_audit_file
                .write_all(b"\n")
                .map_err(|error| CaptureStoreError::WriteMergeAudit(error.to_string()))?;
        }

        Ok(())
    }
}

impl SpeakerStore for FileSystemSpeakerStore {
    fn create_sample(
        &mut self,
        speaker_name: &str,
        audio: &RecordedAudio,
    ) -> Result<(), SpeakerStoreError> {
        create_dir_all(&self.speakers_dir)
            .map_err(|error| SpeakerStoreError::CreateDirectory(error.to_string()))?;

        let sample_path = self.sample_path(speaker_name)?;
        if sample_path.exists() {
            return Err(SpeakerStoreError::SpeakerAlreadyExists {
                speaker_name: speaker_name.to_string(),
            });
        }

        let mut sample_file = File::create(sample_path)
            .map_err(|error| SpeakerStoreError::WriteSample(error.to_string()))?;
        sample_file
            .write_all(&audio.wav_bytes)
            .map_err(|error| SpeakerStoreError::WriteSample(error.to_string()))?;

        Ok(())
    }

    fn create_embedding(
        &mut self,
        speaker_name: &str,
        embedding: &KnownSpeakerEmbedding,
    ) -> Result<(), SpeakerStoreError> {
        create_dir_all(&self.speakers_dir)
            .map_err(|error| SpeakerStoreError::CreateDirectory(error.to_string()))?;

        let embedding_path = self.embedding_path(speaker_name)?;
        let mut embedding_file = File::create(embedding_path)
            .map_err(|error| SpeakerStoreError::WriteEmbedding(error.to_string()))?;
        serde_json::to_writer_pretty(&mut embedding_file, embedding)
            .map_err(|error| SpeakerStoreError::WriteEmbedding(error.to_string()))?;
        embedding_file
            .write_all(b"\n")
            .map_err(|error| SpeakerStoreError::WriteEmbedding(error.to_string()))?;

        Ok(())
    }

    fn remove_sample(&mut self, speaker_name: &str) -> Result<(), SpeakerStoreError> {
        let sample_path = self.sample_path(speaker_name)?;
        if !sample_path.exists() {
            return Err(SpeakerStoreError::SpeakerNotFound {
                speaker_name: speaker_name.to_string(),
            });
        }

        remove_file(sample_path)
            .map_err(|error| SpeakerStoreError::DeleteSample(error.to_string()))?;
        let embedding_path = self.embedding_path(speaker_name)?;
        if embedding_path.exists() {
            remove_file(embedding_path)
                .map_err(|error| SpeakerStoreError::DeleteSample(error.to_string()))?;
        }

        Ok(())
    }

    fn list_samples(&self) -> Result<Vec<String>, SpeakerStoreError> {
        if !self.speakers_dir.exists() {
            return Ok(Vec::new());
        }

        let mut speaker_names = Vec::new();
        for entry in std::fs::read_dir(&self.speakers_dir)
            .map_err(|error| SpeakerStoreError::ListSamples(error.to_string()))?
        {
            let path = entry
                .map_err(|error| SpeakerStoreError::ListSamples(error.to_string()))?
                .path();
            if path.extension().is_none_or(|extension| extension != "wav") {
                continue;
            }

            let speaker_name =
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .ok_or_else(|| {
                        SpeakerStoreError::ListSamples(format!(
                            "speaker sample filename is not valid UTF-8: {}",
                            path.display()
                        ))
                    })?;
            speaker_names.push(speaker_name.to_string());
        }
        speaker_names.sort();

        Ok(speaker_names)
    }

    fn read_sample(&self, speaker_name: &str) -> Result<KnownSpeakerSample, SpeakerStoreError> {
        let sample_path = self.sample_path(speaker_name)?;
        if !sample_path.exists() {
            return Err(SpeakerStoreError::SpeakerNotFound {
                speaker_name: speaker_name.to_string(),
            });
        }

        let wav_bytes = std::fs::read(sample_path)
            .map_err(|error| SpeakerStoreError::ReadSample(error.to_string()))?;

        Ok(KnownSpeakerSample {
            speaker_name: speaker_name.to_string(),
            audio: RecordedAudio {
                wav_bytes,
                content_type: "audio/wav",
            },
        })
    }

    fn read_embedding(
        &self,
        speaker_name: &str,
    ) -> Result<KnownSpeakerEmbedding, SpeakerStoreError> {
        let embedding_path = self.embedding_path(speaker_name)?;
        if !embedding_path.exists() {
            return Err(SpeakerStoreError::SpeakerNotFound {
                speaker_name: speaker_name.to_string(),
            });
        }

        let body = std::fs::read_to_string(embedding_path)
            .map_err(|error| SpeakerStoreError::ReadEmbedding(error.to_string()))?;
        serde_json::from_str(&body)
            .map_err(|error| SpeakerStoreError::ReadEmbedding(error.to_string()))
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

fn validate_speaker_name(speaker_name: &str) -> Result<(), SpeakerStoreError> {
    if speaker_name.is_empty()
        || speaker_name == "."
        || speaker_name == ".."
        || speaker_name.contains(std::path::MAIN_SEPARATOR)
    {
        return Err(SpeakerStoreError::InvalidSpeakerName {
            speaker_name: speaker_name.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{FileSystemCaptureStore, FileSystemMergedTranscriptStore, FileSystemSpeakerStore};
    use crate::application::ports::{
        CaptureSessionMetadata, CaptureStore, MixedCaptureSessionMetadata,
        MixedCaptureSourceOutcome, MixedCaptureSourceSettings, MixedCaptureSourceStatus,
        MixedCaptureStore, SpeakerStore, SpeakerStoreError,
    };
    use crate::domain::{
        DiarizedTranscript, KnownSpeakerSample, MergeAuditEntry, MergeAuditOutcome,
        MergeOverlapRangeSnapshot, MergedTranscriptSegment, RecordedAudio,
        SourcedTranscriptSegment, TranscriptMergePolicy, TranscriptSegment, TranscriptSource,
    };

    #[test]
    /// runs 配下のセッションに audios と captures ディレクトリおよび空の merged.jsonl を作成して開始時刻付き transcript を書き出す。
    fn persists_audio_wav_and_capture_json_and_keeps_merged_jsonl_empty_before_merge() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemCaptureStore::new(temp_dir.path()).unwrap();

        store.persist_audio(1, &sample_audio()).unwrap();
        store
            .persist_transcript(1, 1_420, &sample_transcript())
            .unwrap();

        let runs_dir = temp_dir.path().join("runs");
        let mut session_dirs = std::fs::read_dir(&runs_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        session_dirs.sort();

        assert_eq!(session_dirs.len(), 1);

        let session_dir = &session_dirs[0];
        let audio_path = session_dir.join("audios").join("capture-000001.wav");
        let capture_path = session_dir.join("captures").join("capture-000001.json");
        let merged_path = session_dir.join("merged.jsonl");

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
        assert_eq!(std::fs::read_to_string(merged_path).unwrap(), "");
    }

    #[test]
    /// wav と transcript のファイル名は runs 配下のセッション内で 6 桁ゼロ埋めの連番にする。
    fn names_capture_files_with_zero_padded_sequence() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemCaptureStore::new(temp_dir.path()).unwrap();

        store.persist_audio(12, &sample_audio()).unwrap();
        store
            .persist_transcript(12, 12_000, &sample_transcript())
            .unwrap();

        let session_dir = std::fs::read_dir(temp_dir.path().join("runs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();

        assert!(session_dir.join("audios/capture-000012.wav").exists());
        assert!(session_dir.join("captures/capture-000012.json").exists());
    }

    #[test]
    /// merged segment は absolute 時刻つき JSONL として追記する。
    fn appends_merged_segments_to_jsonl() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemCaptureStore::new(temp_dir.path()).unwrap();

        store
            .persist_merged_segments(&[
                MergedTranscriptSegment {
                    speaker: "spk_0".to_string(),
                    start_ms: 1_000,
                    end_ms: 2_300,
                    text: "こんにちは".to_string(),
                },
                MergedTranscriptSegment {
                    speaker: "spk_1".to_string(),
                    start_ms: 2_500,
                    end_ms: 4_000,
                    text: "よろしくお願いします".to_string(),
                },
            ])
            .unwrap();

        let session_dir = std::fs::read_dir(temp_dir.path().join("runs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();

        assert_eq!(
            std::fs::read_to_string(session_dir.join("merged.jsonl")).unwrap(),
            concat!(
                "{\"speaker\":\"spk_0\",\"start_ms\":1000,\"end_ms\":2300,\"text\":\"こんにちは\"}\n",
                "{\"speaker\":\"spk_1\",\"start_ms\":2500,\"end_ms\":4000,\"text\":\"よろしくお願いします\"}\n"
            )
        );
    }

    #[test]
    /// mixed 実行向けの source store は source 配下に metadata / merged / audit を含む必須ファイル群を保存する。
    fn stores_source_files_under_source_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let session_dir = temp_dir.path().join("runs").join("session-001");
        let mut store = FileSystemCaptureStore::new_for_source(
            &session_dir,
            crate::domain::TranscriptSource::Microphone,
        )
        .unwrap();

        store.persist_audio(1, &sample_audio()).unwrap();
        store
            .persist_transcript(1, 100, &sample_transcript())
            .unwrap();
        store
            .persist_session_metadata(&CaptureSessionMetadata {
                recording_duration_ms: 10_000,
                capture_duration_ms: 5_000,
                capture_overlap_ms: 500,
                capture_silence_threshold_dbfs: -42.0,
                capture_silence_min_duration_ms: 700,
                capture_tail_silence_min_duration_ms: 250,
                transcription_model: "model".to_string(),
                transcription_language: "auto".to_string(),
                response_format: "diarized_json".to_string(),
                chunking_strategy: "auto".to_string(),
                merge_policy: TranscriptMergePolicy::recommended(),
                fixed_speaker: None,
            })
            .unwrap();
        store
            .persist_merged_segments(&[MergedTranscriptSegment {
                speaker: "me".to_string(),
                start_ms: 100,
                end_ms: 300,
                text: "hello".to_string(),
            }])
            .unwrap();
        store
            .persist_merge_audit_entries(&[MergeAuditEntry {
                capture_index: 1,
                previous_overlap_range: MergeOverlapRangeSnapshot {
                    start_ms: 100,
                    end_ms: 200,
                    text: "a".to_string(),
                    normalized_char_count: 1,
                },
                current_overlap_range: MergeOverlapRangeSnapshot {
                    start_ms: 200,
                    end_ms: 300,
                    text: "a".to_string(),
                    normalized_char_count: 1,
                },
                outcome: MergeAuditOutcome::Skipped {
                    reason: crate::domain::MergeSkipReason::NoOverlapRange,
                    previous_normalized_chars: 1,
                    current_normalized_chars: 1,
                    required_min_overlap_chars: 10,
                },
            }])
            .unwrap();

        assert!(
            session_dir
                .join("sources")
                .join("microphone")
                .join("audios")
                .join("capture-000001.wav")
                .exists()
        );
        assert!(
            session_dir
                .join("sources")
                .join("microphone")
                .join("captures")
                .join("capture-000001.json")
                .exists()
        );
        assert!(
            session_dir
                .join("sources")
                .join("microphone")
                .join("metadata.json")
                .exists()
        );
        assert!(
            session_dir
                .join("sources")
                .join("microphone")
                .join("merged.jsonl")
                .exists()
        );
        assert!(
            session_dir
                .join("sources")
                .join("microphone")
                .join("merge-audit.jsonl")
                .exists()
        );
    }

    #[test]
    /// mixed session root には metadata.json と最終 merged.jsonl を保存する。
    fn persists_mixed_session_metadata_and_final_merged_segments() {
        let temp_dir = tempfile::tempdir().unwrap();
        let session_dir = temp_dir.path().join("runs").join("session-001");
        let mut store = FileSystemMergedTranscriptStore::new(&session_dir).unwrap();

        store
            .persist_mixed_session_metadata(&MixedCaptureSessionMetadata {
                mode: "mixed".to_string(),
                application_bundle_id: "us.zoom.xos".to_string(),
                microphone_speaker: "me".to_string(),
                source_settings: vec![
                    MixedCaptureSourceSettings {
                        source: TranscriptSource::Microphone,
                        recording_duration_ms: 40_000,
                        capture_duration_ms: 30_000,
                        capture_overlap_ms: 18_000,
                        capture_silence_threshold_dbfs: -42.0,
                        capture_silence_min_duration_ms: 700,
                        capture_tail_silence_min_duration_ms: 250,
                        transcription_model: "gpt-4o-transcribe-diarize".to_string(),
                        transcription_language: "ja".to_string(),
                        response_format: "diarized_json".to_string(),
                        chunking_strategy: "auto".to_string(),
                        merge_policy: TranscriptMergePolicy::recommended(),
                        fixed_speaker: Some("me".to_string()),
                    },
                    MixedCaptureSourceSettings {
                        source: TranscriptSource::Application,
                        recording_duration_ms: 40_000,
                        capture_duration_ms: 30_000,
                        capture_overlap_ms: 18_000,
                        capture_silence_threshold_dbfs: -42.0,
                        capture_silence_min_duration_ms: 700,
                        capture_tail_silence_min_duration_ms: 250,
                        transcription_model: "gpt-4o-transcribe-diarize".to_string(),
                        transcription_language: "ja".to_string(),
                        response_format: "diarized_json".to_string(),
                        chunking_strategy: "auto".to_string(),
                        merge_policy: TranscriptMergePolicy::recommended(),
                        fixed_speaker: None,
                    },
                ],
                source_outcomes: vec![
                    MixedCaptureSourceOutcome {
                        source: TranscriptSource::Microphone,
                        started_at_unix_ms: 1_000,
                        status: MixedCaptureSourceStatus::Succeeded,
                        transcription_failure_count: 0,
                        error_message: None,
                    },
                    MixedCaptureSourceOutcome {
                        source: TranscriptSource::Application,
                        started_at_unix_ms: 1_050,
                        status: MixedCaptureSourceStatus::Failed,
                        transcription_failure_count: 0,
                        error_message: Some("capture failed".to_string()),
                    },
                ],
            })
            .unwrap();
        store
            .persist_final_segments(&[
                SourcedTranscriptSegment {
                    source: TranscriptSource::Microphone,
                    speaker: "me".to_string(),
                    start_ms: 1_000,
                    end_ms: 2_300,
                    text: "こんにちは".to_string(),
                },
                SourcedTranscriptSegment {
                    source: TranscriptSource::Application,
                    speaker: "spk_1".to_string(),
                    start_ms: 2_500,
                    end_ms: 4_000,
                    text: "よろしくお願いします".to_string(),
                },
            ])
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(session_dir.join("merged.jsonl")).unwrap(),
            concat!(
                "{\"source\":\"microphone\",\"speaker\":\"me\",\"start_ms\":1000,\"end_ms\":2300,\"text\":\"こんにちは\"}\n",
                "{\"source\":\"application\",\"speaker\":\"spk_1\",\"start_ms\":2500,\"end_ms\":4000,\"text\":\"よろしくお願いします\"}\n"
            )
        );
        assert!(
            std::fs::read_to_string(session_dir.join("metadata.json"))
                .unwrap()
                .contains("\"mode\": \"mixed\"")
        );
    }

    #[test]
    /// session metadata は `metadata.json` に保存し、merge の判断過程は `merge-audit.jsonl` へ追記する。
    fn persists_metadata_json_and_merge_audit_jsonl() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemCaptureStore::new(temp_dir.path()).unwrap();

        store
            .persist_session_metadata(&CaptureSessionMetadata {
                recording_duration_ms: 40_000,
                capture_duration_ms: 30_000,
                capture_overlap_ms: 18_000,
                capture_silence_threshold_dbfs: -42.0,
                capture_silence_min_duration_ms: 700,
                capture_tail_silence_min_duration_ms: 250,
                transcription_model: "gpt-4o-transcribe-diarize".to_string(),
                transcription_language: "ja".to_string(),
                response_format: "diarized_json".to_string(),
                chunking_strategy: "auto".to_string(),
                merge_policy: TranscriptMergePolicy::recommended(),
                fixed_speaker: Some("me".to_string()),
            })
            .unwrap();
        store
            .persist_merge_audit_entries(&[MergeAuditEntry {
                capture_index: 2,
                previous_overlap_range: MergeOverlapRangeSnapshot {
                    start_ms: 12_000,
                    end_ms: 18_000,
                    text: "EFGHIJKLMNOP".to_string(),
                    normalized_char_count: 12,
                },
                current_overlap_range: MergeOverlapRangeSnapshot {
                    start_ms: 12_000,
                    end_ms: 18_000,
                    text: "EFGHIJKLMNOP".to_string(),
                    normalized_char_count: 12,
                },
                outcome: MergeAuditOutcome::Accepted {
                    overlap_chars: 12,
                    alignment_ratio: 1.0,
                    trigram_similarity: 1.0,
                    current_prefix_trim_chars: 0,
                    overlap_text_source: crate::domain::MergeOverlapTextSource::CurrentOverlapRange,
                },
            }])
            .unwrap();

        let session_dir = std::fs::read_dir(temp_dir.path().join("runs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();

        assert_eq!(
            std::fs::read_to_string(session_dir.join("metadata.json")).unwrap(),
            concat!(
                "{\n",
                "  \"recording_duration_ms\": 40000,\n",
                "  \"capture_duration_ms\": 30000,\n",
                "  \"capture_overlap_ms\": 18000,\n",
                "  \"capture_silence_threshold_dbfs\": -42.0,\n",
                "  \"capture_silence_min_duration_ms\": 700,\n",
                "  \"capture_tail_silence_min_duration_ms\": 250,\n",
                "  \"transcription_model\": \"gpt-4o-transcribe-diarize\",\n",
                "  \"transcription_language\": \"ja\",\n",
                "  \"response_format\": \"diarized_json\",\n",
                "  \"chunking_strategy\": \"auto\",\n",
                "  \"merge_policy\": {\n",
                "    \"min_overlap_chars\": 10,\n",
                "    \"min_alignment_ratio\": 0.8,\n",
                "    \"min_trigram_similarity\": 0.55\n",
                "  },\n",
                "  \"fixed_speaker\": \"me\"\n",
                "}\n"
            )
        );
        assert_eq!(
            std::fs::read_to_string(session_dir.join("merge-audit.jsonl")).unwrap(),
            concat!(
                "{\"capture_index\":2,\"previous_overlap_range\":{\"start_ms\":12000,\"end_ms\":18000,\"text\":\"EFGHIJKLMNOP\",\"normalized_char_count\":12},",
                "\"current_overlap_range\":{\"start_ms\":12000,\"end_ms\":18000,\"text\":\"EFGHIJKLMNOP\",\"normalized_char_count\":12},",
                "\"outcome\":{\"result\":\"accepted\",\"overlap_chars\":12,\"alignment_ratio\":1.0,\"trigram_similarity\":1.0,\"current_prefix_trim_chars\":0,\"overlap_text_source\":\"current_overlap_range\"}}\n"
            )
        );
    }

    #[test]
    /// 話者サンプルは speakers 配下に `<speaker_name>.wav` で保存する。
    fn persists_speaker_sample_under_speakers_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemSpeakerStore::new(temp_dir.path());

        store.create_sample("suzuki", &sample_audio()).unwrap();

        assert_eq!(
            std::fs::read(temp_dir.path().join("speakers").join("suzuki.wav")).unwrap(),
            sample_audio().wav_bytes
        );
    }

    #[test]
    /// 同名の話者サンプルが既に存在する場合は上書きせずエラーにする。
    fn returns_error_when_speaker_sample_already_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemSpeakerStore::new(temp_dir.path());
        store.create_sample("suzuki", &sample_audio()).unwrap();

        let error = store.create_sample("suzuki", &sample_audio()).unwrap_err();

        assert_eq!(
            error,
            SpeakerStoreError::SpeakerAlreadyExists {
                speaker_name: "suzuki".to_string(),
            }
        );
    }

    #[test]
    /// 登録済みの話者サンプルを削除する。
    fn removes_existing_speaker_sample() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemSpeakerStore::new(temp_dir.path());
        store.create_sample("suzuki", &sample_audio()).unwrap();

        store.remove_sample("suzuki").unwrap();

        assert!(!temp_dir.path().join("speakers").join("suzuki.wav").exists());
    }

    #[test]
    /// 登録済みの話者サンプルは capture 添付用に読み出せる。
    fn reads_registered_speaker_sample_for_capture_attachment() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemSpeakerStore::new(temp_dir.path());
        store.create_sample("suzuki", &sample_audio()).unwrap();

        let sample = store.read_sample("suzuki").unwrap();

        assert_eq!(
            sample,
            KnownSpeakerSample {
                speaker_name: "suzuki".to_string(),
                audio: sample_audio(),
            }
        );
    }

    #[test]
    /// 削除対象が存在しない場合はエラーにする。
    fn returns_error_when_removing_unknown_speaker_sample() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = FileSystemSpeakerStore::new(temp_dir.path());

        let error = store.remove_sample("suzuki").unwrap_err();

        assert_eq!(
            error,
            SpeakerStoreError::SpeakerNotFound {
                speaker_name: "suzuki".to_string(),
            }
        );
    }

    #[test]
    /// 話者サンプル保存ディレクトリがまだ無い場合は空一覧を返す。
    fn returns_empty_list_when_speaker_directory_does_not_exist() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = FileSystemSpeakerStore::new(temp_dir.path());

        let speakers = store.list_samples().unwrap();

        assert!(speakers.is_empty());
    }

    #[test]
    /// 登録済みの話者サンプル名をファイル名から昇順で一覧する。
    fn lists_registered_speaker_names_in_sorted_order() {
        let temp_dir = tempfile::tempdir().unwrap();
        let speakers_dir = temp_dir.path().join("speakers");
        std::fs::create_dir_all(&speakers_dir).unwrap();
        std::fs::write(speakers_dir.join("tanaka.wav"), b"RIFF").unwrap();
        std::fs::write(speakers_dir.join("sato.wav"), b"RIFF").unwrap();
        std::fs::write(speakers_dir.join("notes.txt"), b"ignore").unwrap();

        let store = FileSystemSpeakerStore::new(temp_dir.path());

        let speakers = store.list_samples().unwrap();

        assert_eq!(speakers, vec!["sato".to_string(), "tanaka".to_string()]);
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
