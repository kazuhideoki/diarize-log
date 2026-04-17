use crate::application::SpeakerCommand;
use clap::{Parser, Subcommand, ValueEnum};
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

/// CLI 起動時の振る舞いです。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Run {
        speaker_samples: Vec<String>,
        audio_source: AudioSource,
    },
    Speaker(SpeakerCommand),
    PrintOutput(String),
}

/// 実行時に選ぶ音源です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioSource {
    Microphone,
    Application {
        bundle_id: String,
    },
    Mixed {
        bundle_id: String,
        microphone_speaker: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum AudioSourceKind {
    Microphone,
    Application,
    Mixed,
}

/// CLI 引数の解釈失敗です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliArgumentError {
    Parse { message: String },
    RelativePathArgument { value: PathBuf },
    TooManySpeakerSamples { count: usize, max: usize },
    MissingApplicationBundleId,
    UnexpectedApplicationBundleId,
    MissingMicrophoneSpeaker,
    UnexpectedMicrophoneSpeaker,
}

impl fmt::Display for CliArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse { message } => f.write_str(message),
            Self::RelativePathArgument { value } => {
                write!(f, "relative path is not allowed: {}", value.display())
            }
            Self::TooManySpeakerSamples { count, max } => {
                write!(
                    f,
                    "too many speaker samples: {count} provided, maximum is {max}"
                )
            }
            Self::MissingApplicationBundleId => f.write_str(
                "--application-bundle-id is required for --audio-source application or mixed",
            ),
            Self::UnexpectedApplicationBundleId => f.write_str(
                "--application-bundle-id can only be used with --audio-source application or mixed",
            ),
            Self::MissingMicrophoneSpeaker => {
                f.write_str("--microphone-speaker is required for --audio-source mixed")
            }
            Self::UnexpectedMicrophoneSpeaker => {
                f.write_str("--microphone-speaker can only be used with --audio-source mixed")
            }
        }
    }
}

impl std::error::Error for CliArgumentError {}

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Records audio, requests diarized transcription for each capture, and stores the results.",
    long_about = None
)]
struct CliArgs {
    /// Attach a registered speaker sample to the diarization request. Can be passed up to 4 times.
    #[arg(short = 's', long = "speaker-sample")]
    speaker_samples: Vec<String>,

    /// Select which audio source to capture.
    #[arg(short = 'i', long = "audio-source", value_enum, default_value_t = AudioSourceKind::Microphone)]
    audio_source: AudioSourceKind,

    /// Specify the target application's bundle ID when capturing application audio.
    #[arg(long = "application-bundle-id")]
    application_bundle_id: Option<String>,

    /// Pin the microphone source to a fixed speaker name in mixed mode.
    #[arg(long = "microphone-speaker")]
    microphone_speaker: Option<String>,

    #[command(subcommand)]
    command: Option<CliSubcommandArgs>,
}

#[derive(Debug, Subcommand)]
enum CliSubcommandArgs {
    /// 話者サンプルを管理します。
    Speaker(SpeakerSubcommandArgs),
}

#[derive(Debug, clap::Args)]
struct SpeakerSubcommandArgs {
    #[command(subcommand)]
    command: SpeakerCommandArgs,
}

#[derive(Debug, Subcommand)]
enum SpeakerCommandArgs {
    /// Cut a sample wav from the source file and register it.
    Add {
        speaker_name: String,
        wav_path: PathBuf,
        start_second: u64,
    },
    /// List registered speaker samples.
    List,
    /// Remove a registered speaker sample.
    Remove { speaker_name: String },
}

/// CLI 引数を解釈します。
pub fn parse_cli_args<I>(args: I) -> Result<CliAction, CliArgumentError>
where
    I: IntoIterator<Item = OsString>,
{
    match CliArgs::try_parse_from(args) {
        Ok(cli_args) => cli_args.into_action(),
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            Ok(CliAction::PrintOutput(error.to_string()))
        }
        Err(error) => Err(CliArgumentError::Parse {
            message: error.to_string(),
        }),
    }
}

impl CliArgs {
    fn into_action(self) -> Result<CliAction, CliArgumentError> {
        const MAX_SPEAKER_SAMPLES: usize = 4;

        if self.speaker_samples.len() > MAX_SPEAKER_SAMPLES {
            return Err(CliArgumentError::TooManySpeakerSamples {
                count: self.speaker_samples.len(),
                max: MAX_SPEAKER_SAMPLES,
            });
        }

        let audio_source = match (
            self.audio_source,
            self.application_bundle_id,
            self.microphone_speaker,
        ) {
            (AudioSourceKind::Microphone, None, None) => AudioSource::Microphone,
            (AudioSourceKind::Microphone, Some(_), _) => {
                return Err(CliArgumentError::UnexpectedApplicationBundleId);
            }
            (AudioSourceKind::Microphone, None, Some(_)) => {
                return Err(CliArgumentError::UnexpectedMicrophoneSpeaker);
            }
            (AudioSourceKind::Application, Some(bundle_id), None) => {
                AudioSource::Application { bundle_id }
            }
            (AudioSourceKind::Application, None, None) => {
                return Err(CliArgumentError::MissingApplicationBundleId);
            }
            (AudioSourceKind::Application, Some(_), Some(_))
            | (AudioSourceKind::Application, None, Some(_)) => {
                return Err(CliArgumentError::UnexpectedMicrophoneSpeaker);
            }
            (AudioSourceKind::Mixed, Some(bundle_id), Some(microphone_speaker)) => {
                AudioSource::Mixed {
                    bundle_id,
                    microphone_speaker,
                }
            }
            (AudioSourceKind::Mixed, None, Some(_)) => {
                return Err(CliArgumentError::MissingApplicationBundleId);
            }
            (AudioSourceKind::Mixed, Some(_), None) | (AudioSourceKind::Mixed, None, None) => {
                return Err(CliArgumentError::MissingMicrophoneSpeaker);
            }
        };

        match self.command {
            None => Ok(CliAction::Run {
                speaker_samples: self.speaker_samples,
                audio_source,
            }),
            Some(CliSubcommandArgs::Speaker(speaker_args)) => speaker_args.into_action(),
        }
    }
}

impl SpeakerSubcommandArgs {
    fn into_action(self) -> Result<CliAction, CliArgumentError> {
        let command = match self.command {
            SpeakerCommandArgs::Add {
                speaker_name,
                wav_path,
                start_second,
            } => {
                if !wav_path.is_absolute() {
                    return Err(CliArgumentError::RelativePathArgument { value: wav_path });
                }
                SpeakerCommand::Add {
                    speaker_name,
                    wav_path,
                    start_second,
                }
            }
            SpeakerCommandArgs::List => SpeakerCommand::List,
            SpeakerCommandArgs::Remove { speaker_name } => SpeakerCommand::Remove { speaker_name },
        };

        Ok(CliAction::Speaker(command))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    /// 引数が無いときは通常実行モードを返す。
    fn returns_run_action_when_no_flags_are_given() {
        let action = parse_cli_args([OsString::from("diarize-log")]).unwrap();

        assert_eq!(
            action,
            CliAction::Run {
                speaker_samples: Vec::new(),
                audio_source: AudioSource::Microphone,
            }
        );
    }

    #[test]
    /// `-s` を繰り返すと capture 時に添付する話者サンプル名として解釈する。
    fn parses_short_speaker_sample_flags_for_run_action() {
        let action = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("-s"),
            OsString::from("suzuki"),
            OsString::from("-s"),
            OsString::from("sato"),
        ])
        .unwrap();

        assert_eq!(
            action,
            CliAction::Run {
                speaker_samples: vec!["suzuki".to_string(), "sato".to_string()],
                audio_source: AudioSource::Microphone,
            }
        );
    }

    #[test]
    /// `-i application` と bundle ID を指定すると対象アプリ音声を選べる。
    fn parses_application_audio_source_with_short_flag() {
        let action = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("-i"),
            OsString::from("application"),
            OsString::from("--application-bundle-id"),
            OsString::from("com.apple.Safari"),
        ])
        .unwrap();

        assert_eq!(
            action,
            CliAction::Run {
                speaker_samples: Vec::new(),
                audio_source: AudioSource::Application {
                    bundle_id: "com.apple.Safari".to_string(),
                },
            }
        );
    }

    #[test]
    /// `-i mixed` ではアプリ bundle ID とマイク話者名をまとめて解釈する。
    fn parses_mixed_audio_source_with_microphone_speaker() {
        let action = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("-i"),
            OsString::from("mixed"),
            OsString::from("--application-bundle-id"),
            OsString::from("us.zoom.xos"),
            OsString::from("--microphone-speaker"),
            OsString::from("me"),
        ])
        .unwrap();

        assert_eq!(
            action,
            CliAction::Run {
                speaker_samples: Vec::new(),
                audio_source: AudioSource::Mixed {
                    bundle_id: "us.zoom.xos".to_string(),
                    microphone_speaker: "me".to_string(),
                },
            }
        );
    }

    #[test]
    /// アプリ音声指定では bundle ID が必須。
    fn rejects_application_audio_source_without_bundle_id() {
        let error = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("--audio-source"),
            OsString::from("application"),
        ])
        .unwrap_err();

        assert_eq!(error, CliArgumentError::MissingApplicationBundleId);
    }

    #[test]
    /// mixed 指定ではマイク話者名が必須。
    fn rejects_mixed_audio_source_without_microphone_speaker() {
        let error = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("--audio-source"),
            OsString::from("mixed"),
            OsString::from("--application-bundle-id"),
            OsString::from("us.zoom.xos"),
        ])
        .unwrap_err();

        assert_eq!(error, CliArgumentError::MissingMicrophoneSpeaker);
    }

    #[test]
    /// マイク指定で bundle ID を渡すと失敗する。
    fn rejects_application_bundle_id_for_microphone_audio_source() {
        let error = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("--application-bundle-id"),
            OsString::from("com.apple.Safari"),
        ])
        .unwrap_err();

        assert_eq!(error, CliArgumentError::UnexpectedApplicationBundleId);
    }

    #[test]
    /// mixed 以外でマイク話者名を渡すと失敗する。
    fn rejects_microphone_speaker_for_non_mixed_audio_source() {
        let error = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("--microphone-speaker"),
            OsString::from("me"),
        ])
        .unwrap_err();

        assert_eq!(error, CliArgumentError::UnexpectedMicrophoneSpeaker);
    }

    #[test]
    /// 話者サンプル指定は 4 件を超えると失敗する。
    fn rejects_more_than_four_speaker_samples() {
        let error = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("-s"),
            OsString::from("a"),
            OsString::from("-s"),
            OsString::from("b"),
            OsString::from("-s"),
            OsString::from("c"),
            OsString::from("-s"),
            OsString::from("d"),
            OsString::from("-s"),
            OsString::from("e"),
        ])
        .unwrap_err();

        assert_eq!(
            error,
            CliArgumentError::TooManySpeakerSamples { count: 5, max: 4 }
        );
    }

    #[test]
    /// `--help` を受け取ると help 表示モードを返す。
    fn returns_show_help_action_when_help_flag_is_given() {
        let action =
            parse_cli_args([OsString::from("diarize-log"), OsString::from("--help")]).unwrap();

        match action {
            CliAction::PrintOutput(message) => {
                assert!(message.contains(
                    "Records audio, requests diarized transcription for each capture, and stores the results."
                ));
                assert!(message.contains("Usage: diarize-log"));
                assert!(message.contains("-h, --help"));
                assert!(message.contains("-i, --audio-source <AUDIO_SOURCE>"));
                assert!(message.contains("-s, --speaker-sample <SPEAKER_SAMPLES>"));
                assert!(message.contains("--application-bundle-id <APPLICATION_BUNDLE_ID>"));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    /// `-h` を受け取ると help 表示モードを返す。
    fn returns_show_help_action_when_short_help_flag_is_given() {
        let action = parse_cli_args([OsString::from("diarize-log"), OsString::from("-h")]).unwrap();

        match action {
            CliAction::PrintOutput(message) => {
                assert!(message.contains(
                    "Records audio, requests diarized transcription for each capture, and stores the results."
                ));
                assert!(message.contains("Usage: diarize-log"));
                assert!(message.contains("-h, --help"));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    /// `speaker add` を受け取ると話者サンプル追加コマンドとして解釈する。
    fn parses_speaker_add_command() {
        let action = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("speaker"),
            OsString::from("add"),
            OsString::from("suzuki"),
            OsString::from("/tmp/source.wav"),
            OsString::from("4"),
        ])
        .unwrap();

        assert_eq!(
            action,
            CliAction::Speaker(SpeakerCommand::Add {
                speaker_name: "suzuki".to_string(),
                wav_path: PathBuf::from("/tmp/source.wav"),
                start_second: 4,
            })
        );
    }

    #[test]
    /// `speaker remove` を受け取ると話者サンプル削除コマンドとして解釈する。
    fn parses_speaker_remove_command() {
        let action = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("speaker"),
            OsString::from("remove"),
            OsString::from("suzuki"),
        ])
        .unwrap();

        assert_eq!(
            action,
            CliAction::Speaker(SpeakerCommand::Remove {
                speaker_name: "suzuki".to_string(),
            })
        );
    }

    #[test]
    /// `speaker list` を受け取ると話者サンプル一覧コマンドとして解釈する。
    fn parses_speaker_list_command() {
        let action = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("speaker"),
            OsString::from("list"),
        ])
        .unwrap();

        assert_eq!(action, CliAction::Speaker(SpeakerCommand::List));
    }

    #[test]
    /// `speaker add` の WAV パスは絶対パスでなければ失敗する。
    fn rejects_relative_wav_path_for_speaker_add() {
        let error = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("speaker"),
            OsString::from("add"),
            OsString::from("suzuki"),
            OsString::from("storage/source.wav"),
            OsString::from("4"),
        ])
        .unwrap_err();

        assert_eq!(
            error,
            CliArgumentError::RelativePathArgument {
                value: PathBuf::from("storage/source.wav"),
            }
        );
    }

    #[test]
    /// 未知のコマンドを受け取ると失敗する。
    fn rejects_unknown_argument() {
        let error = parse_cli_args([OsString::from("diarize-log"), OsString::from("--verbose")])
            .unwrap_err();

        match error {
            CliArgumentError::Parse { message } => {
                assert!(message.contains("--verbose"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    /// `speaker --help` はサブコマンド固有の usage を表示する。
    fn returns_speaker_subcommand_help_output() {
        let action = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("speaker"),
            OsString::from("--help"),
        ])
        .unwrap();

        match action {
            CliAction::PrintOutput(message) => {
                assert!(message.contains("Usage: diarize-log speaker <COMMAND>"));
                assert!(message.contains("add"));
                assert!(message.contains("remove"));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    /// `speaker add --help` は引数を含む usage を表示する。
    fn returns_speaker_add_subcommand_help_output() {
        let action = parse_cli_args([
            OsString::from("diarize-log"),
            OsString::from("speaker"),
            OsString::from("add"),
            OsString::from("--help"),
        ])
        .unwrap();

        match action {
            CliAction::PrintOutput(message) => {
                assert!(message.contains(
                    "Usage: diarize-log speaker add <SPEAKER_NAME> <WAV_PATH> <START_SECOND>"
                ));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    /// `--version` は `clap` 標準の version 表示を返す。
    fn returns_version_output_when_version_flag_is_given() {
        let action =
            parse_cli_args([OsString::from("diarize-log"), OsString::from("--version")]).unwrap();

        match action {
            CliAction::PrintOutput(message) => {
                assert!(message.contains(env!("CARGO_PKG_VERSION")));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    /// `clap` で組み立てた CLI 定義は内部整合性を満たす。
    fn clap_command_definition_is_valid() {
        CliArgs::command().debug_assert();
    }
}
