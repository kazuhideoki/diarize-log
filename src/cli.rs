use crate::application::SpeakerCommand;
use clap::{Parser, Subcommand};
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

/// CLI 起動時の振る舞いです。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Run,
    Speaker(SpeakerCommand),
    PrintOutput(String),
}

/// CLI 引数の解釈失敗です。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliArgumentError {
    Parse { message: String },
    RelativePathArgument { value: PathBuf },
}

impl fmt::Display for CliArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse { message } => f.write_str(message),
            Self::RelativePathArgument { value } => {
                write!(f, "relative path is not allowed: {}", value.display())
            }
        }
    }
}

impl std::error::Error for CliArgumentError {}

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Records audio, requests diarized transcription, and stores the capture.",
    long_about = None
)]
struct CliArgs {
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
        match self.command {
            None => Ok(CliAction::Run),
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

        assert_eq!(action, CliAction::Run);
    }

    #[test]
    /// `--help` を受け取ると help 表示モードを返す。
    fn returns_show_help_action_when_help_flag_is_given() {
        let action =
            parse_cli_args([OsString::from("diarize-log"), OsString::from("--help")]).unwrap();

        match action {
            CliAction::PrintOutput(message) => {
                assert!(message.contains("Usage: diarize-log"));
                assert!(message.contains("-h, --help"));
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
