use crate::application::ports::{AudioClipper, AudioClipperError};
use crate::domain::RecordedAudio;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

/// `hound` を使って WAV 音声から任意区間を切り出します。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HoundAudioClipper;

impl AudioClipper for HoundAudioClipper {
    fn clip_wav_segment(
        &self,
        wav_path: &Path,
        start_offset: Duration,
        duration: Duration,
    ) -> Result<RecordedAudio, AudioClipperError> {
        let reader = WavReader::open(wav_path)
            .map_err(|error| AudioClipperError::ReadSource(error.to_string()))?;
        let spec = reader.spec();

        match spec.sample_format {
            SampleFormat::Float => clip_float_samples(reader, spec, start_offset, duration),
            SampleFormat::Int => clip_int_samples(reader, spec, start_offset, duration),
        }
    }
}

fn clip_int_samples(
    reader: WavReader<std::io::BufReader<std::fs::File>>,
    spec: WavSpec,
    start_offset: Duration,
    duration: Duration,
) -> Result<RecordedAudio, AudioClipperError> {
    let samples = reader
        .into_samples::<i32>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AudioClipperError::ReadSource(error.to_string()))?;
    let clipped_range = resolve_sample_range(&samples, spec, start_offset, duration)?;
    let clipped_samples = &samples[clipped_range];

    let mut output = Cursor::new(Vec::new());
    let mut writer = WavWriter::new(&mut output, spec)
        .map_err(|error| AudioClipperError::EncodeClip(error.to_string()))?;

    for sample in clipped_samples {
        writer
            .write_sample(*sample)
            .map_err(|error| AudioClipperError::EncodeClip(error.to_string()))?;
    }

    writer
        .finalize()
        .map_err(|error| AudioClipperError::EncodeClip(error.to_string()))?;

    Ok(RecordedAudio {
        wav_bytes: output.into_inner(),
        content_type: "audio/wav",
    })
}

fn clip_float_samples(
    reader: WavReader<std::io::BufReader<std::fs::File>>,
    spec: WavSpec,
    start_offset: Duration,
    duration: Duration,
) -> Result<RecordedAudio, AudioClipperError> {
    let samples = reader
        .into_samples::<f32>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AudioClipperError::ReadSource(error.to_string()))?;
    let clipped_range = resolve_sample_range(&samples, spec, start_offset, duration)?;
    let clipped_samples = &samples[clipped_range];

    let mut output = Cursor::new(Vec::new());
    let mut writer = WavWriter::new(&mut output, spec)
        .map_err(|error| AudioClipperError::EncodeClip(error.to_string()))?;

    for sample in clipped_samples {
        writer
            .write_sample(*sample)
            .map_err(|error| AudioClipperError::EncodeClip(error.to_string()))?;
    }

    writer
        .finalize()
        .map_err(|error| AudioClipperError::EncodeClip(error.to_string()))?;

    Ok(RecordedAudio {
        wav_bytes: output.into_inner(),
        content_type: "audio/wav",
    })
}

fn resolve_sample_range<T>(
    samples: &[T],
    spec: WavSpec,
    start_offset: Duration,
    duration: Duration,
) -> Result<std::ops::Range<usize>, AudioClipperError> {
    let channels = usize::from(spec.channels);
    let total_frames = samples.len() / channels;
    let start_frame = duration_to_frame_count(start_offset, spec.sample_rate);
    let duration_frames = duration_to_frame_count(duration, spec.sample_rate);
    let end_frame = start_frame
        .checked_add(duration_frames)
        .expect("frame count must fit into u64");

    if end_frame > total_frames as u64 {
        return Err(AudioClipperError::InvalidRange {
            requested_start_ms: duration_to_millis(start_offset),
            requested_duration_ms: duration_to_millis(duration),
            available_duration_ms: frames_to_millis(total_frames as u64, spec.sample_rate),
        });
    }

    let start_sample = usize::try_from(start_frame)
        .expect("frame count must fit usize")
        .saturating_mul(channels);
    let end_sample = usize::try_from(end_frame)
        .expect("frame count must fit usize")
        .saturating_mul(channels);

    Ok(start_sample..end_sample)
}

fn duration_to_frame_count(duration: Duration, sample_rate: u32) -> u64 {
    u64::try_from(duration.as_nanos().saturating_mul(u128::from(sample_rate)) / 1_000_000_000)
        .expect("frame count must fit into u64")
}

fn duration_to_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).expect("duration millis must fit into u64")
}

fn frames_to_millis(frames: u64, sample_rate: u32) -> u64 {
    frames
        .saturating_mul(1_000)
        .checked_div(u64::from(sample_rate))
        .expect("sample rate must not be zero")
}

#[cfg(test)]
mod tests {
    use super::HoundAudioClipper;
    use crate::application::ports::{AudioClipper, AudioClipperError};
    use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
    use std::io::Cursor;
    use std::time::Duration;

    #[test]
    /// 指定秒数の範囲だけを元 WAV と同じ spec で切り出す。
    fn clips_requested_wav_segment() {
        let temp_dir = tempfile::tempdir().unwrap();
        let wav_path = temp_dir.path().join("source.wav");
        write_test_wav(
            &wav_path,
            WavSpec {
                channels: 1,
                sample_rate: 4,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
            &[1_i16, 2, 3, 4, 5, 6, 7, 8],
        );

        let clipped = HoundAudioClipper
            .clip_wav_segment(&wav_path, Duration::from_secs(1), Duration::from_secs(1))
            .unwrap();

        let mut reader = WavReader::new(Cursor::new(clipped.wav_bytes)).unwrap();
        assert_eq!(reader.spec().sample_rate, 4);
        assert_eq!(
            reader
                .samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            vec![5, 6, 7, 8]
        );
    }

    #[test]
    /// 切り出し終端が元 WAV を超える場合はエラーにする。
    fn returns_error_when_requested_segment_exceeds_source_length() {
        let temp_dir = tempfile::tempdir().unwrap();
        let wav_path = temp_dir.path().join("source.wav");
        write_test_wav(
            &wav_path,
            WavSpec {
                channels: 1,
                sample_rate: 4,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
            &[1_i16, 2, 3, 4],
        );

        let error = HoundAudioClipper
            .clip_wav_segment(&wav_path, Duration::from_secs(1), Duration::from_secs(1))
            .unwrap_err();

        assert_eq!(
            error,
            AudioClipperError::InvalidRange {
                requested_start_ms: 1_000,
                requested_duration_ms: 1_000,
                available_duration_ms: 1_000,
            }
        );
    }

    fn write_test_wav(path: &std::path::Path, spec: WavSpec, samples: &[i16]) {
        let mut writer = WavWriter::create(path, spec).unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
    }
}
