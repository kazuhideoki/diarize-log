use serde::Serialize;

/// 文字起こし結果の由来となる音源です。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Hash, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSource {
    Microphone,
    Application,
}

impl TranscriptSource {
    /// storage 上のディレクトリ名として使う文字列を返します。
    pub fn as_storage_dir_name(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::Application => "application",
        }
    }

    /// source 間統合時の安定ソート順を返します。
    pub fn sort_order(self) -> u8 {
        match self {
            Self::Microphone => 0,
            Self::Application => 1,
        }
    }
}
