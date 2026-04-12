use crate::debug_log;
use crate::ports::{RecordedAudio, Recorder, RecorderError, RecordingSession};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample};
use std::io::Cursor;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

const TARGET_CHANNELS: u16 = 1;
const TARGET_SAMPLE_RATE: u32 = 24_000;
const WAIT_INTERVAL_MILLIS: u64 = 10;

/// `cpal` を使ってデフォルトマイクから継続録音します。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpalRecorder {
    debug_enabled: bool,
}

impl CpalRecorder {
    pub fn new(debug_enabled: bool) -> Self {
        Self { debug_enabled }
    }
}

impl Recorder for CpalRecorder {
    type Session = CpalRecordingSession;

    fn start_recording(&mut self) -> Result<Self::Session, RecorderError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(RecorderError::NoInputDevice)?;
        debug_log(
            self.debug_enabled,
            &format!(
                "input device selected: {}",
                device.name().unwrap_or_else(|_| "<unknown>".to_string())
            ),
        );
        let supported_config = device
            .default_input_config()
            .map_err(|error| RecorderError::ReadInputConfig(error.to_string()))?;
        let sample_format = supported_config.sample_format();
        let stream_config: cpal::StreamConfig = supported_config.into();
        let channels = stream_config.channels;
        let sample_rate = stream_config.sample_rate.0;
        debug_log(
            self.debug_enabled,
            &format!(
                "recording starts: sample_format={sample_format:?} channels={channels} sample_rate={sample_rate}"
            ),
        );

        let sample_buffer = Arc::new(Mutex::new(Vec::new()));
        let (error_sender, error_receiver) = mpsc::channel();

        let stream = match sample_format {
            cpal::SampleFormat::F32 => build_input_stream::<f32>(
                &device,
                &stream_config,
                Arc::clone(&sample_buffer),
                error_sender.clone(),
            )?,
            cpal::SampleFormat::I16 => build_input_stream::<i16>(
                &device,
                &stream_config,
                Arc::clone(&sample_buffer),
                error_sender.clone(),
            )?,
            cpal::SampleFormat::U16 => build_input_stream::<u16>(
                &device,
                &stream_config,
                Arc::clone(&sample_buffer),
                error_sender.clone(),
            )?,
            other => return Err(RecorderError::UnsupportedSampleFormat(format!("{other:?}"))),
        };

        stream
            .play()
            .map_err(|error| RecorderError::PlayStream(error.to_string()))?;

        Ok(CpalRecordingSession {
            _stream: stream,
            sample_buffer,
            error_receiver,
            channels,
            sample_rate,
            debug_enabled: self.debug_enabled,
        })
    }
}

/// 継続録音中のバッファから capture 単位で音声を切り出します。
pub struct CpalRecordingSession {
    _stream: cpal::Stream,
    sample_buffer: Arc<Mutex<Vec<i16>>>,
    error_receiver: mpsc::Receiver<RecorderError>,
    channels: u16,
    sample_rate: u32,
    debug_enabled: bool,
}

impl RecordingSession for CpalRecordingSession {
    fn wait_until(&mut self, duration: Duration) -> Result<(), RecorderError> {
        let required_frames = duration_to_frame_count(duration, self.sample_rate);

        loop {
            self.poll_callback_error()?;
            let available_frames = self.available_frame_count()?;

            if available_frames >= required_frames {
                return Ok(());
            }

            thread::sleep(Duration::from_millis(WAIT_INTERVAL_MILLIS));
        }
    }

    fn capture_wav(
        &mut self,
        start_offset: Duration,
        duration: Duration,
    ) -> Result<RecordedAudio, RecorderError> {
        self.poll_callback_error()?;

        let start_frame = duration_to_frame_count(start_offset, self.sample_rate);
        let end_frame = duration_to_frame_count(start_offset + duration, self.sample_rate);
        let available_frames = self.available_frame_count()?;

        if end_frame > available_frames {
            return Err(RecorderError::CaptureOutOfRange {
                requested_end_ms: duration_to_millis(start_offset + duration),
                available_end_ms: frames_to_millis(available_frames, self.sample_rate),
            });
        }

        let channels = usize::from(self.channels);
        let start_sample = usize::try_from(start_frame)
            .expect("frame count must fit usize")
            .saturating_mul(channels);
        let end_sample = usize::try_from(end_frame)
            .expect("frame count must fit usize")
            .saturating_mul(channels);
        let captured_samples = {
            let guard = self
                .sample_buffer
                .lock()
                .map_err(|_| RecorderError::SampleBufferPoisoned)?;
            guard[start_sample..end_sample].to_vec()
        };
        debug_log(
            self.debug_enabled,
            &format!(
                "captured pcm samples: capture_start_ms={} capture_duration_ms={} count={}",
                duration_to_millis(start_offset),
                duration_to_millis(duration),
                captured_samples.len()
            ),
        );

        let normalized_samples =
            normalize_pcm_format(captured_samples, self.channels, self.sample_rate);
        debug_log(
            self.debug_enabled,
            &format!(
                "normalized pcm samples: count={} channels={} sample_rate={}",
                normalized_samples.len(),
                TARGET_CHANNELS,
                TARGET_SAMPLE_RATE
            ),
        );

        encode_wav(normalized_samples, TARGET_CHANNELS, TARGET_SAMPLE_RATE)
    }
}

impl CpalRecordingSession {
    fn poll_callback_error(&mut self) -> Result<(), RecorderError> {
        match self.error_receiver.try_recv() {
            Ok(error) => Err(error),
            Err(mpsc::TryRecvError::Empty) => Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => Ok(()),
        }
    }

    fn available_frame_count(&self) -> Result<u64, RecorderError> {
        let sample_count = self
            .sample_buffer
            .lock()
            .map_err(|_| RecorderError::SampleBufferPoisoned)?
            .len();
        Ok((sample_count / usize::from(self.channels)) as u64)
    }
}

fn normalize_pcm_format(pcm_samples: Vec<i16>, channels: u16, sample_rate: u32) -> Vec<i16> {
    let mono_samples = downmix_to_mono(&pcm_samples, channels);
    resample_mono_pcm(&mono_samples, sample_rate, TARGET_SAMPLE_RATE)
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_buffer: Arc<Mutex<Vec<i16>>>,
    error_sender: mpsc::Sender<RecorderError>,
) -> Result<cpal::Stream, RecorderError>
where
    T: cpal::SizedSample,
    i16: FromSample<T>,
{
    let callback_error_sender = error_sender.clone();

    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut converted = Vec::with_capacity(data.len());
                for sample in data {
                    converted.push(sample_to_i16(*sample));
                }

                match sample_buffer.lock() {
                    Ok(mut guard) => guard.extend_from_slice(&converted),
                    Err(_) => {
                        let _ = error_sender.send(RecorderError::SampleBufferPoisoned);
                    }
                }
            },
            move |error| {
                let _ =
                    callback_error_sender.send(RecorderError::CallbackStream(error.to_string()));
            },
            None,
        )
        .map_err(|error| RecorderError::BuildStream(error.to_string()))
}

fn sample_to_i16<T>(sample: T) -> i16
where
    T: cpal::SizedSample,
    i16: FromSample<T>,
{
    i16::from_sample(sample)
}

fn downmix_to_mono(pcm_samples: &[i16], channels: u16) -> Vec<i16> {
    if channels == TARGET_CHANNELS {
        return pcm_samples.to_vec();
    }

    let channels = usize::from(channels);
    pcm_samples
        .chunks_exact(channels)
        .map(|frame| {
            let sum = frame.iter().map(|sample| i32::from(*sample)).sum::<i32>();
            (sum / i32::try_from(channels).expect("channels must fit into i32")) as i16
        })
        .collect()
}

fn resample_mono_pcm(
    pcm_samples: &[i16],
    input_sample_rate: u32,
    output_sample_rate: u32,
) -> Vec<i16> {
    if input_sample_rate == output_sample_rate || pcm_samples.is_empty() {
        return pcm_samples.to_vec();
    }

    let output_len = ((pcm_samples.len() as u64 * output_sample_rate as u64)
        / input_sample_rate as u64) as usize;

    (0..output_len)
        .map(|index| {
            let position = index as f64 * input_sample_rate as f64 / output_sample_rate as f64;
            let lower_index = position.floor() as usize;
            let upper_index = (lower_index + 1).min(pcm_samples.len() - 1);
            let ratio = position - lower_index as f64;
            let lower = pcm_samples[lower_index] as f64;
            let upper = pcm_samples[upper_index] as f64;
            ((lower * (1.0 - ratio)) + (upper * ratio)).round() as i16
        })
        .collect()
}

fn encode_wav(
    pcm_samples: Vec<i16>,
    channels: u16,
    sample_rate: u32,
) -> Result<RecordedAudio, RecorderError> {
    let mut wav_bytes = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(&mut wav_bytes, spec)
        .map_err(|error| RecorderError::EncodeWav(error.to_string()))?;

    for sample in pcm_samples {
        writer
            .write_sample(sample)
            .map_err(|error| RecorderError::EncodeWav(error.to_string()))?;
    }

    writer
        .finalize()
        .map_err(|error| RecorderError::EncodeWav(error.to_string()))?;

    Ok(RecordedAudio {
        wav_bytes: wav_bytes.into_inner(),
        content_type: "audio/wav",
    })
}

fn duration_to_frame_count(duration: Duration, sample_rate: u32) -> u64 {
    duration.as_secs() * u64::from(sample_rate)
        + (u64::from(duration.subsec_nanos()) * u64::from(sample_rate)) / 1_000_000_000
}

fn duration_to_millis(duration: Duration) -> u64 {
    duration.as_secs() * 1_000 + u64::from(duration.subsec_millis())
}

fn frames_to_millis(frame_count: u64, sample_rate: u32) -> u64 {
    frame_count.saturating_mul(1_000) / u64::from(sample_rate)
}

#[cfg(test)]
mod tests {
    use super::{encode_wav, normalize_pcm_format};
    use crate::ports::RecordedAudio;
    use std::io::Cursor;

    #[test]
    /// PCM サンプル列を 16bit PCM の WAV バイト列へ変換する。
    fn encodes_pcm_samples_into_wav_bytes() {
        let audio = encode_wav(vec![100, -100, 50, -50], 2, 16_000).unwrap();

        assert_eq!(
            audio,
            RecordedAudio {
                wav_bytes: audio.wav_bytes.clone(),
                content_type: "audio/wav",
            }
        );

        let reader = hound::WavReader::new(Cursor::new(audio.wav_bytes)).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, 16_000);
    }

    #[test]
    /// 48kHz ステレオの PCM サンプル列を 24kHz モノラルへ正規化して WAV に変換する。
    fn normalizes_stereo_pcm_into_24khz_mono_wav_bytes() {
        let normalized = normalize_pcm_format(
            vec![1000, 3000, 5000, 7000, 9000, 11000, 13000, 15000],
            2,
            48_000,
        );
        let audio = encode_wav(normalized, 1, 24_000).unwrap();

        let reader = hound::WavReader::new(Cursor::new(audio.wav_bytes)).unwrap();
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 24_000);

        let samples = reader
            .into_samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(samples, vec![2000, 10000]);
    }
}
