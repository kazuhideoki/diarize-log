/// 録音済みの WAV 音声データです。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedAudio {
    pub wav_bytes: Vec<u8>,
    pub content_type: &'static str,
}
