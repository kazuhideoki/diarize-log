use crate::ports::{AudioClipper, AudioClipperError, SpeakerStore, SpeakerStoreError};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// 話者サンプル管理ユースケースの入力です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakerCommand {
    Add {
        speaker_name: String,
        wav_path: PathBuf,
        start_second: u64,
    },
    List,
    Remove {
        speaker_name: String,
    },
}

#[derive(Debug)]
pub enum SpeakerUseCaseError {
    Clip(AudioClipperError),
    Store(SpeakerStoreError),
}

impl fmt::Display for SpeakerUseCaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Clip(error) => write!(f, "speaker sample clipping failed: {error}"),
            Self::Store(error) => write!(f, "speaker sample persistence failed: {error}"),
        }
    }
}

impl std::error::Error for SpeakerUseCaseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakerCommandResult {
    Updated,
    ListedSpeakers(Vec<String>),
}

/// 話者サンプル管理ユースケースを実行します。
pub fn run_speaker_command<C, S>(
    command: &SpeakerCommand,
    sample_duration: Duration,
    clipper: &C,
    speaker_store: &mut S,
) -> Result<SpeakerCommandResult, SpeakerUseCaseError>
where
    C: AudioClipper,
    S: SpeakerStore,
{
    match command {
        SpeakerCommand::Add {
            speaker_name,
            wav_path,
            start_second,
        } => {
            let clipped_audio = clipper
                .clip_wav_segment(
                    wav_path,
                    Duration::from_secs(*start_second),
                    sample_duration,
                )
                .map_err(SpeakerUseCaseError::Clip)?;
            speaker_store
                .create_sample(speaker_name, &clipped_audio)
                .map_err(SpeakerUseCaseError::Store)?;
            Ok(SpeakerCommandResult::Updated)
        }
        SpeakerCommand::List => speaker_store
            .list_samples()
            .map(SpeakerCommandResult::ListedSpeakers)
            .map_err(SpeakerUseCaseError::Store),
        SpeakerCommand::Remove { speaker_name } => speaker_store
            .remove_sample(speaker_name)
            .map_err(SpeakerUseCaseError::Store)
            .map(|()| SpeakerCommandResult::Updated),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{KnownSpeakerSample, RecordedAudio};
    use std::cell::RefCell;

    struct FakeAudioClipper {
        observed_requests: RefCell<Vec<(PathBuf, Duration, Duration)>>,
        clipped_audio: RecordedAudio,
    }

    impl AudioClipper for FakeAudioClipper {
        fn clip_wav_segment(
            &self,
            wav_path: &std::path::Path,
            start_offset: Duration,
            duration: Duration,
        ) -> Result<RecordedAudio, AudioClipperError> {
            self.observed_requests.borrow_mut().push((
                wav_path.to_path_buf(),
                start_offset,
                duration,
            ));
            Ok(self.clipped_audio.clone())
        }
    }

    #[derive(Default)]
    struct FakeSpeakerStore {
        created_samples: RefCell<Vec<(String, RecordedAudio)>>,
        listed_speakers: RefCell<Vec<String>>,
        removed_speakers: RefCell<Vec<String>>,
    }

    impl SpeakerStore for FakeSpeakerStore {
        fn create_sample(
            &mut self,
            speaker_name: &str,
            audio: &RecordedAudio,
        ) -> Result<(), SpeakerStoreError> {
            self.created_samples
                .borrow_mut()
                .push((speaker_name.to_string(), audio.clone()));
            Ok(())
        }

        fn remove_sample(&mut self, speaker_name: &str) -> Result<(), SpeakerStoreError> {
            self.removed_speakers
                .borrow_mut()
                .push(speaker_name.to_string());
            Ok(())
        }

        fn list_samples(&self) -> Result<Vec<String>, SpeakerStoreError> {
            Ok(self.listed_speakers.borrow().clone())
        }

        fn read_sample(&self, speaker_name: &str) -> Result<KnownSpeakerSample, SpeakerStoreError> {
            Ok(KnownSpeakerSample {
                speaker_name: speaker_name.to_string(),
                audio: sample_audio(),
            })
        }
    }

    fn sample_audio() -> RecordedAudio {
        RecordedAudio {
            wav_bytes: vec![0x52, 0x49, 0x46, 0x46],
            content_type: "audio/wav",
        }
    }

    #[test]
    /// `speaker add` は指定秒数から設定秒数ぶん WAV を切り出して保存する。
    fn clips_and_persists_speaker_sample_for_add_command() {
        let clipper = FakeAudioClipper {
            observed_requests: RefCell::new(Vec::new()),
            clipped_audio: sample_audio(),
        };
        let mut speaker_store = FakeSpeakerStore::default();
        let command = SpeakerCommand::Add {
            speaker_name: "suzuki".to_string(),
            wav_path: PathBuf::from("/tmp/source.wav"),
            start_second: 4,
        };

        run_speaker_command(
            &command,
            Duration::from_secs(6),
            &clipper,
            &mut speaker_store,
        )
        .unwrap();

        assert_eq!(
            *clipper.observed_requests.borrow(),
            vec![(
                PathBuf::from("/tmp/source.wav"),
                Duration::from_secs(4),
                Duration::from_secs(6),
            )]
        );
        assert_eq!(
            *speaker_store.created_samples.borrow(),
            vec![("suzuki".to_string(), sample_audio())]
        );
    }

    #[test]
    /// `speaker remove` は指定名の登録済みサンプル削除を保存先へ委譲する。
    fn removes_speaker_sample_for_remove_command() {
        let clipper = FakeAudioClipper {
            observed_requests: RefCell::new(Vec::new()),
            clipped_audio: sample_audio(),
        };
        let mut speaker_store = FakeSpeakerStore::default();
        let command = SpeakerCommand::Remove {
            speaker_name: "suzuki".to_string(),
        };

        run_speaker_command(
            &command,
            Duration::from_secs(6),
            &clipper,
            &mut speaker_store,
        )
        .unwrap();

        assert!(clipper.observed_requests.borrow().is_empty());
        assert_eq!(
            *speaker_store.removed_speakers.borrow(),
            vec!["suzuki".to_string()]
        );
    }

    #[test]
    /// `speaker list` は登録済みの話者名一覧取得を保存先へ委譲する。
    fn lists_registered_speakers_for_list_command() {
        let clipper = FakeAudioClipper {
            observed_requests: RefCell::new(Vec::new()),
            clipped_audio: sample_audio(),
        };
        let mut speaker_store = FakeSpeakerStore {
            created_samples: RefCell::new(Vec::new()),
            listed_speakers: RefCell::new(vec!["sato".to_string(), "suzuki".to_string()]),
            removed_speakers: RefCell::new(Vec::new()),
        };

        let result = run_speaker_command(
            &SpeakerCommand::List,
            Duration::from_secs(6),
            &clipper,
            &mut speaker_store,
        )
        .unwrap();

        assert!(clipper.observed_requests.borrow().is_empty());
        assert!(speaker_store.created_samples.borrow().is_empty());
        assert!(speaker_store.removed_speakers.borrow().is_empty());
        assert_eq!(
            result,
            SpeakerCommandResult::ListedSpeakers(vec!["sato".to_string(), "suzuki".to_string()])
        );
    }
}
