use crate::application::ports::{
    InterruptMonitor, Recorder, RecorderError, RecordingSession, RecordingWaitOutcome,
};
use crate::domain::RecordedAudio;
use crate::logger::Logger;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample};
use screencapturekit::cm::CMSampleBuffer;
use screencapturekit::shareable_content::{SCDisplay, SCRunningApplication, SCShareableContent};
use screencapturekit::stream::configuration::SCStreamConfiguration;
use screencapturekit::stream::content_filter::SCContentFilter;
use screencapturekit::stream::delegate_trait::StreamCallbacks;
use screencapturekit::stream::output_type::SCStreamOutputType;
use screencapturekit::stream::sc_stream::SCStream;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

const AUDIO_DETECTION_SAMPLE_THRESHOLD: u16 = 512;
const TARGET_CHANNELS: u16 = 1;
const TARGET_SAMPLE_RATE: u32 = 24_000;
const WAIT_INTERVAL_MILLIS: u64 = 10;

/// `cpal` を使ってデフォルトマイクから継続録音します。
#[derive(Clone)]
pub struct CpalRecorder {
    logger: Logger,
}

impl CpalRecorder {
    pub fn new(logger: Logger) -> Self {
        Self { logger }
    }
}

/// `ScreenCaptureKit` を使って特定アプリケーション由来の音声を継続録音します。
#[derive(Clone)]
pub struct ScreenCaptureKitApplicationRecorder {
    bundle_id: String,
    logger: Logger,
}

impl ScreenCaptureKitApplicationRecorder {
    pub fn new(bundle_id: String, logger: Logger) -> Self {
        Self { bundle_id, logger }
    }
}

impl Recorder for CpalRecorder {
    type Session = CpalRecordingSession;

    fn start_recording(&mut self) -> Result<Self::Session, RecorderError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(RecorderError::NoInputDevice)?;
        let _ = self.logger.debug(&format!(
            "input device selected: {}",
            device.name().unwrap_or_else(|_| "<unknown>".to_string())
        ));
        let supported_config = device
            .default_input_config()
            .map_err(|error| RecorderError::ReadInputConfig(error.to_string()))?;
        let sample_format = supported_config.sample_format();
        let stream_config: cpal::StreamConfig = supported_config.into();
        let channels = stream_config.channels;
        let sample_rate = stream_config.sample_rate.0;
        let _ = self.logger.debug(&format!(
            "recording starts: sample_format={sample_format:?} channels={channels} sample_rate={sample_rate}"
        ));

        let sample_buffer = Arc::new(Mutex::new(Vec::new()));
        let audio_detected = Arc::new(AtomicBool::new(false));
        let (error_sender, error_receiver) = mpsc::channel();

        let stream = match sample_format {
            cpal::SampleFormat::F32 => build_input_stream::<f32>(
                &device,
                &stream_config,
                Arc::clone(&sample_buffer),
                Arc::clone(&audio_detected),
                error_sender.clone(),
            )?,
            cpal::SampleFormat::I16 => build_input_stream::<i16>(
                &device,
                &stream_config,
                Arc::clone(&sample_buffer),
                Arc::clone(&audio_detected),
                error_sender.clone(),
            )?,
            cpal::SampleFormat::U16 => build_input_stream::<u16>(
                &device,
                &stream_config,
                Arc::clone(&sample_buffer),
                Arc::clone(&audio_detected),
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
            audio_detected,
            audio_detection_logged: false,
            error_receiver,
            channels,
            sample_rate,
            logger: self.logger.clone(),
        })
    }
}

impl Recorder for ScreenCaptureKitApplicationRecorder {
    type Session = ScreenCaptureKitRecordingSession;

    fn start_recording(&mut self) -> Result<Self::Session, RecorderError> {
        let content = SCShareableContent::get()
            .map_err(|error| RecorderError::ReadShareableContent(error.to_string()))?;
        let application = find_application(&content, &self.bundle_id)?;
        let display = find_application_display(&content, &self.bundle_id)?;
        let _ = self.logger.debug(&format!(
            "screen capture target selected: bundle_id={} display_id={}",
            application.bundle_identifier(),
            display.display_id()
        ));

        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_including_applications(&[&application], &[])
            .build();
        let configuration = SCStreamConfiguration::new()
            .with_captures_audio(true)
            .with_channel_count(i32::from(TARGET_CHANNELS))
            .with_sample_rate(TARGET_SAMPLE_RATE as i32);
        let sample_buffer = Arc::new(Mutex::new(Vec::new()));
        let audio_detected = Arc::new(AtomicBool::new(false));
        let (error_sender, error_receiver) = mpsc::channel();
        let callback_error_sender = error_sender.clone();
        let delegate = StreamCallbacks::new().on_error(move |error| {
            let _ = callback_error_sender.send(RecorderError::CallbackStream(error.to_string()));
        });
        let mut stream = SCStream::new_with_delegate(&filter, &configuration, delegate);
        let audio_sample_buffer = Arc::clone(&sample_buffer);
        let audio_detected_flag = Arc::clone(&audio_detected);
        let audio_error_sender = error_sender.clone();
        let logger = self.logger.clone();

        let handler_registered = stream.add_output_handler(
            move |sample: CMSampleBuffer, _of_type: SCStreamOutputType| {
                match decode_screen_capture_audio(sample) {
                    Ok(decoded) => match audio_sample_buffer.lock() {
                        Ok(mut guard) => {
                            if contains_detectable_audio(&decoded) {
                                audio_detected_flag.store(true, Ordering::SeqCst);
                            }
                            guard.extend_from_slice(&decoded)
                        }
                        Err(_) => {
                            let _ = audio_error_sender.send(RecorderError::SampleBufferPoisoned);
                        }
                    },
                    Err(error) => {
                        let _ = audio_error_sender.send(error);
                    }
                }
            },
            SCStreamOutputType::Audio,
        );

        if handler_registered.is_none() {
            return Err(RecorderError::AddStreamOutput(
                "audio output handler registration returned false".to_string(),
            ));
        }

        stream
            .start_capture()
            .map_err(|error| RecorderError::StartCapture(error.to_string()))?;
        let _ = logger.debug(&format!(
            "screen capture recording starts: bundle_id={} target_sample_rate={} target_channels={}",
            self.bundle_id, TARGET_SAMPLE_RATE, TARGET_CHANNELS
        ));

        Ok(ScreenCaptureKitRecordingSession {
            stream,
            sample_buffer,
            audio_detected,
            audio_detection_logged: false,
            error_receiver,
            logger,
        })
    }
}

/// 継続録音中のバッファから capture 単位で音声を切り出します。
pub struct CpalRecordingSession {
    _stream: cpal::Stream,
    sample_buffer: Arc<Mutex<Vec<i16>>>,
    audio_detected: Arc<AtomicBool>,
    audio_detection_logged: bool,
    error_receiver: mpsc::Receiver<RecorderError>,
    channels: u16,
    sample_rate: u32,
    logger: Logger,
}

/// `ScreenCaptureKit` の継続録音中セッションです。
pub struct ScreenCaptureKitRecordingSession {
    stream: SCStream,
    sample_buffer: Arc<Mutex<Vec<i16>>>,
    audio_detected: Arc<AtomicBool>,
    audio_detection_logged: bool,
    error_receiver: mpsc::Receiver<RecorderError>,
    logger: Logger,
}

impl RecordingSession for CpalRecordingSession {
    fn wait_until(
        &mut self,
        duration: Duration,
        interrupt_monitor: &dyn InterruptMonitor,
    ) -> Result<RecordingWaitOutcome, RecorderError> {
        let required_frames = duration_to_frame_count(duration, self.sample_rate);

        loop {
            self.poll_callback_error()?;
            maybe_log_audio_detected(
                self.audio_detected.as_ref(),
                &mut self.audio_detection_logged,
                &self.logger,
            );
            let available_frames = self.available_frame_count()?;

            if available_frames >= required_frames {
                return Ok(RecordingWaitOutcome::ReachedTarget);
            }
            if interrupt_monitor.is_interrupt_requested() {
                return Ok(RecordingWaitOutcome::Interrupted);
            }

            thread::sleep(Duration::from_millis(WAIT_INTERVAL_MILLIS));
        }
    }

    fn recorded_duration(&mut self) -> Result<Duration, RecorderError> {
        self.poll_callback_error()?;
        Ok(frame_count_to_duration(
            self.available_frame_count()?,
            self.sample_rate,
        ))
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
        let _ = self.logger.debug(&format!(
            "captured pcm samples: capture_start_ms={} capture_duration_ms={} count={}",
            duration_to_millis(start_offset),
            duration_to_millis(duration),
            captured_samples.len()
        ));

        let normalized_samples =
            normalize_pcm_format(captured_samples, self.channels, self.sample_rate);
        let _ = self.logger.debug(&format!(
            "normalized pcm samples: count={} channels={} sample_rate={}",
            normalized_samples.len(),
            TARGET_CHANNELS,
            TARGET_SAMPLE_RATE
        ));

        encode_wav(normalized_samples, TARGET_CHANNELS, TARGET_SAMPLE_RATE)
    }
}

impl RecordingSession for ScreenCaptureKitRecordingSession {
    fn wait_until(
        &mut self,
        duration: Duration,
        interrupt_monitor: &dyn InterruptMonitor,
    ) -> Result<RecordingWaitOutcome, RecorderError> {
        let required_frames = duration_to_frame_count(duration, TARGET_SAMPLE_RATE);

        loop {
            self.poll_callback_error()?;
            maybe_log_audio_detected(
                self.audio_detected.as_ref(),
                &mut self.audio_detection_logged,
                &self.logger,
            );
            let available_frames = self.available_frame_count()?;

            if available_frames >= required_frames {
                return Ok(RecordingWaitOutcome::ReachedTarget);
            }
            if interrupt_monitor.is_interrupt_requested() {
                return Ok(RecordingWaitOutcome::Interrupted);
            }

            thread::sleep(Duration::from_millis(WAIT_INTERVAL_MILLIS));
        }
    }

    fn recorded_duration(&mut self) -> Result<Duration, RecorderError> {
        self.poll_callback_error()?;
        Ok(frame_count_to_duration(
            self.available_frame_count()?,
            TARGET_SAMPLE_RATE,
        ))
    }

    fn capture_wav(
        &mut self,
        start_offset: Duration,
        duration: Duration,
    ) -> Result<RecordedAudio, RecorderError> {
        self.poll_callback_error()?;

        let start_frame = duration_to_frame_count(start_offset, TARGET_SAMPLE_RATE);
        let end_frame = duration_to_frame_count(start_offset + duration, TARGET_SAMPLE_RATE);
        let available_frames = self.available_frame_count()?;

        if end_frame > available_frames {
            return Err(RecorderError::CaptureOutOfRange {
                requested_end_ms: duration_to_millis(start_offset + duration),
                available_end_ms: frames_to_millis(available_frames, TARGET_SAMPLE_RATE),
            });
        }

        let start_sample = usize::try_from(start_frame).expect("frame count must fit usize");
        let end_sample = usize::try_from(end_frame).expect("frame count must fit usize");
        let captured_samples = {
            let guard = self
                .sample_buffer
                .lock()
                .map_err(|_| RecorderError::SampleBufferPoisoned)?;
            guard[start_sample..end_sample].to_vec()
        };
        let _ = self.logger.debug(&format!(
            "captured application audio samples: capture_start_ms={} capture_duration_ms={} count={}",
            duration_to_millis(start_offset),
            duration_to_millis(duration),
            captured_samples.len()
        ));

        encode_wav(captured_samples, TARGET_CHANNELS, TARGET_SAMPLE_RATE)
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

impl ScreenCaptureKitRecordingSession {
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
        Ok(sample_count as u64)
    }
}

impl Drop for ScreenCaptureKitRecordingSession {
    fn drop(&mut self) {
        if let Err(error) = self.stream.stop_capture() {
            let _ = self
                .logger
                .debug(&format!("screen capture stop failed during drop: {error}"));
        }
    }
}

fn find_application(
    content: &SCShareableContent,
    bundle_id: &str,
) -> Result<SCRunningApplication, RecorderError> {
    content
        .applications()
        .into_iter()
        .find(|application| application.bundle_identifier() == bundle_id)
        .ok_or_else(|| RecorderError::ApplicationNotFound {
            bundle_id: bundle_id.to_string(),
        })
}

fn find_application_display(
    content: &SCShareableContent,
    bundle_id: &str,
) -> Result<SCDisplay, RecorderError> {
    let window_frames = content
        .windows()
        .into_iter()
        .filter(|window| {
            window.is_on_screen()
                && window
                    .owning_application()
                    .is_some_and(|application| application.bundle_identifier() == bundle_id)
        })
        .map(|window| window.frame())
        .collect::<Vec<_>>();
    let displays = content.displays();
    let display_frames = displays.iter().map(SCDisplay::frame).collect::<Vec<_>>();

    select_application_display_index(&window_frames, &display_frames)
        .and_then(|index| displays.get(index).cloned())
        .ok_or_else(|| RecorderError::ApplicationDisplayNotFound {
            bundle_id: bundle_id.to_string(),
        })
}

fn select_application_display_index(
    window_frames: &[screencapturekit::cg::CGRect],
    display_frames: &[screencapturekit::cg::CGRect],
) -> Option<usize> {
    if display_frames.is_empty() {
        return None;
    }

    if window_frames.is_empty() {
        return Some(0);
    }

    window_frames
        .iter()
        .flat_map(|window_frame| {
            display_frames
                .iter()
                .enumerate()
                .filter_map(move |(index, display_frame)| {
                    let overlap_area = overlapping_area(*window_frame, *display_frame);
                    if overlap_area > 0.0 {
                        Some((overlap_area, index))
                    } else {
                        None
                    }
                })
        })
        .max_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, index)| index)
        .or(Some(0))
}

fn overlapping_area(
    window_frame: screencapturekit::cg::CGRect,
    display_frame: screencapturekit::cg::CGRect,
) -> f64 {
    let overlap_width = (window_frame.max_x().min(display_frame.max_x())
        - window_frame.min_x().max(display_frame.min_x()))
    .max(0.0);
    let overlap_height = (window_frame.max_y().min(display_frame.max_y())
        - window_frame.min_y().max(display_frame.min_y()))
    .max(0.0);
    overlap_width * overlap_height
}

fn decode_screen_capture_audio(sample: CMSampleBuffer) -> Result<Vec<i16>, RecorderError> {
    let format = sample.format_description().ok_or_else(|| {
        RecorderError::DecodeCapturedAudio("missing format description".to_string())
    })?;
    let sample_rate = format.audio_sample_rate().ok_or_else(|| {
        RecorderError::DecodeCapturedAudio("missing audio sample rate".to_string())
    })?;
    let channels = format.audio_channel_count().ok_or_else(|| {
        RecorderError::DecodeCapturedAudio("missing audio channel count".to_string())
    })?;
    let bits_per_channel = format
        .audio_bits_per_channel()
        .ok_or_else(|| RecorderError::DecodeCapturedAudio("missing audio bit depth".to_string()))?;

    if format.audio_is_big_endian() {
        return Err(RecorderError::UnsupportedSampleFormat(
            "big-endian application audio is not supported".to_string(),
        ));
    }

    let audio_buffers = sample.audio_buffer_list().ok_or_else(|| {
        RecorderError::DecodeCapturedAudio("missing audio buffer list".to_string())
    })?;
    let decoded = if format.audio_is_float() && bits_per_channel == 32 {
        decode_audio_buffers_as_f32(&audio_buffers)?
    } else if !format.audio_is_float() && bits_per_channel == 16 {
        decode_audio_buffers_as_i16(&audio_buffers)?
    } else {
        return Err(RecorderError::UnsupportedSampleFormat(format!(
            "unsupported application audio format: bits_per_channel={bits_per_channel} is_float={} channels={channels} sample_rate={sample_rate}",
            format.audio_is_float()
        )));
    };

    let channel_count = u16::try_from(channels).map_err(|_| {
        RecorderError::DecodeCapturedAudio(format!("channel count does not fit u16: {channels}"))
    })?;
    let input_sample_rate = sample_rate.round() as u32;
    let _ = channel_count;
    Ok(resample_mono_pcm(
        &decoded,
        input_sample_rate,
        TARGET_SAMPLE_RATE,
    ))
}

fn decode_audio_buffers_as_f32(
    audio_buffers: &screencapturekit::cm::AudioBufferList,
) -> Result<Vec<i16>, RecorderError> {
    if audio_buffers.num_buffers() == 0 {
        return Ok(Vec::new());
    }

    if audio_buffers.num_buffers() == 1 {
        let buffer = audio_buffers
            .get(0)
            .expect("buffer index 0 must exist when num_buffers > 0");
        return decode_interleaved_f32_buffer(buffer.data(), buffer.number_channels);
    }

    decode_planar_f32_buffers(audio_buffers)
}

fn decode_audio_buffers_as_i16(
    audio_buffers: &screencapturekit::cm::AudioBufferList,
) -> Result<Vec<i16>, RecorderError> {
    if audio_buffers.num_buffers() == 0 {
        return Ok(Vec::new());
    }

    if audio_buffers.num_buffers() == 1 {
        let buffer = audio_buffers
            .get(0)
            .expect("buffer index 0 must exist when num_buffers > 0");
        return decode_interleaved_i16_buffer(buffer.data(), buffer.number_channels);
    }

    decode_planar_i16_buffers(audio_buffers)
}

fn decode_interleaved_f32_buffer(
    bytes: &[u8],
    channel_count: u32,
) -> Result<Vec<i16>, RecorderError> {
    let channels = usize::try_from(channel_count).map_err(|_| {
        RecorderError::DecodeCapturedAudio(format!(
            "channel count does not fit usize: {channel_count}"
        ))
    })?;
    if channels == 0 {
        return Ok(Vec::new());
    }
    if !bytes.len().is_multiple_of(std::mem::size_of::<f32>()) {
        return Err(RecorderError::DecodeCapturedAudio(format!(
            "application audio byte count is not aligned to f32 samples: {}",
            bytes.len()
        )));
    }

    let samples = bytes
        .chunks_exact(std::mem::size_of::<f32>())
        .map(|chunk| {
            sample_to_i16(f32::from_le_bytes(
                chunk.try_into().expect("f32 chunk size must match"),
            ))
        })
        .collect::<Vec<_>>();

    Ok(if channels == 1 {
        samples
    } else {
        downmix_to_mono(
            &samples,
            u16::try_from(channels).expect("channels must fit u16"),
        )
    })
}

fn decode_interleaved_i16_buffer(
    bytes: &[u8],
    channel_count: u32,
) -> Result<Vec<i16>, RecorderError> {
    let channels = usize::try_from(channel_count).map_err(|_| {
        RecorderError::DecodeCapturedAudio(format!(
            "channel count does not fit usize: {channel_count}"
        ))
    })?;
    if channels == 0 {
        return Ok(Vec::new());
    }
    if !bytes.len().is_multiple_of(std::mem::size_of::<i16>()) {
        return Err(RecorderError::DecodeCapturedAudio(format!(
            "application audio byte count is not aligned to i16 samples: {}",
            bytes.len()
        )));
    }

    let samples = bytes
        .chunks_exact(std::mem::size_of::<i16>())
        .map(|chunk| i16::from_le_bytes(chunk.try_into().expect("i16 chunk size must match")))
        .collect::<Vec<_>>();

    Ok(if channels == 1 {
        samples
    } else {
        downmix_to_mono(
            &samples,
            u16::try_from(channels).expect("channels must fit u16"),
        )
    })
}

fn decode_planar_f32_buffers(
    audio_buffers: &screencapturekit::cm::AudioBufferList,
) -> Result<Vec<i16>, RecorderError> {
    let planar_samples = audio_buffers
        .iter()
        .map(|buffer| {
            if buffer.data().len() % std::mem::size_of::<f32>() != 0 {
                return Err(RecorderError::DecodeCapturedAudio(format!(
                    "planar application audio byte count is not aligned to f32 samples: {}",
                    buffer.data().len()
                )));
            }

            Ok(buffer
                .data()
                .chunks_exact(std::mem::size_of::<f32>())
                .map(|chunk| {
                    sample_to_i16(f32::from_le_bytes(
                        chunk.try_into().expect("f32 chunk size must match"),
                    ))
                })
                .collect::<Vec<_>>())
        })
        .collect::<Result<Vec<_>, _>>()?;

    average_planar_channels(planar_samples)
}

fn decode_planar_i16_buffers(
    audio_buffers: &screencapturekit::cm::AudioBufferList,
) -> Result<Vec<i16>, RecorderError> {
    let planar_samples = audio_buffers
        .iter()
        .map(|buffer| {
            if buffer.data().len() % std::mem::size_of::<i16>() != 0 {
                return Err(RecorderError::DecodeCapturedAudio(format!(
                    "planar application audio byte count is not aligned to i16 samples: {}",
                    buffer.data().len()
                )));
            }

            Ok(buffer
                .data()
                .chunks_exact(std::mem::size_of::<i16>())
                .map(|chunk| {
                    i16::from_le_bytes(chunk.try_into().expect("i16 chunk size must match"))
                })
                .collect::<Vec<_>>())
        })
        .collect::<Result<Vec<_>, _>>()?;

    average_planar_channels(planar_samples)
}

fn average_planar_channels(planar_samples: Vec<Vec<i16>>) -> Result<Vec<i16>, RecorderError> {
    if planar_samples.is_empty() {
        return Ok(Vec::new());
    }

    let expected_len = planar_samples[0].len();
    if planar_samples
        .iter()
        .any(|channel_samples| channel_samples.len() != expected_len)
    {
        return Err(RecorderError::DecodeCapturedAudio(
            "planar application audio buffers have mismatched lengths".to_string(),
        ));
    }

    Ok((0..expected_len)
        .map(|index| {
            let sum = planar_samples
                .iter()
                .map(|channel_samples| i32::from(channel_samples[index]))
                .sum::<i32>();
            (sum / i32::try_from(planar_samples.len()).expect("channel count must fit i32")) as i16
        })
        .collect())
}

fn normalize_pcm_format(pcm_samples: Vec<i16>, channels: u16, sample_rate: u32) -> Vec<i16> {
    let mono_samples = downmix_to_mono(&pcm_samples, channels);
    resample_mono_pcm(&mono_samples, sample_rate, TARGET_SAMPLE_RATE)
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_buffer: Arc<Mutex<Vec<i16>>>,
    audio_detected: Arc<AtomicBool>,
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
                if contains_detectable_audio(&converted) {
                    audio_detected.store(true, Ordering::SeqCst);
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

fn contains_detectable_audio(samples: &[i16]) -> bool {
    samples
        .iter()
        .any(|sample| sample.unsigned_abs() >= AUDIO_DETECTION_SAMPLE_THRESHOLD)
}

fn maybe_log_audio_detected(detected: &AtomicBool, logged: &mut bool, logger: &Logger) {
    if *logged || !detected.load(Ordering::SeqCst) {
        return;
    }

    let _ = logger.info("audio input detected");
    *logged = true;
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

fn frame_count_to_duration(frame_count: u64, sample_rate: u32) -> Duration {
    let nanos = (u128::from(frame_count) * 1_000_000_000_u128) / u128::from(sample_rate);
    Duration::from_nanos(u64::try_from(nanos).expect("duration in nanos must fit u64"))
}

#[cfg(test)]
mod tests {
    use super::{
        AUDIO_DETECTION_SAMPLE_THRESHOLD, encode_wav, maybe_log_audio_detected,
        normalize_pcm_format, select_application_display_index,
    };
    use crate::domain::RecordedAudio;
    use crate::{LogSource, Logger};
    use screencapturekit::cg::CGRect;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct SharedBuffer {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl SharedBuffer {
        fn contents(&self) -> String {
            String::from_utf8(self.bytes.lock().unwrap().clone()).unwrap()
        }
    }

    impl std::io::Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

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

    #[test]
    /// 対象アプリの表示中ウィンドウが無いときは先頭 display をフォールバックに選ぶ。
    fn falls_back_to_first_display_when_application_has_no_on_screen_windows() {
        let display_index = select_application_display_index(
            &[],
            &[
                CGRect::new(0.0, 0.0, 1920.0, 1080.0),
                CGRect::new(1920.0, 0.0, 1920.0, 1080.0),
            ],
        );

        assert_eq!(display_index, Some(0));
    }

    #[test]
    /// 対象アプリの表示中ウィンドウがあるときは重なり面積が最大の display を選ぶ。
    fn selects_display_with_largest_overlapping_window_area() {
        let display_index = select_application_display_index(
            &[CGRect::new(2100.0, 100.0, 500.0, 400.0)],
            &[
                CGRect::new(0.0, 0.0, 1920.0, 1080.0),
                CGRect::new(1920.0, 0.0, 1920.0, 1080.0),
            ],
        );

        assert_eq!(display_index, Some(1));
    }

    #[test]
    /// しきい値未満の微小ノイズだけでは音声検知しないが、しきい値到達で検知する。
    fn detects_audio_only_after_samples_reach_threshold() {
        assert!(!super::contains_detectable_audio(&[
            0,
            12,
            -(AUDIO_DETECTION_SAMPLE_THRESHOLD as i16 - 1),
        ]));
        assert!(super::contains_detectable_audio(&[
            AUDIO_DETECTION_SAMPLE_THRESHOLD as i16
        ]));
    }

    #[test]
    /// 音声検知ログは同じ source で一度だけ出力する。
    fn writes_audio_detection_log_only_once_per_source() {
        let sink = SharedBuffer::default();
        let logger = Logger::new(sink.clone(), false)
            .with_source(LogSource::Microphone)
            .with_component("recorder");
        let detected = AtomicBool::new(false);
        let mut logged = false;

        maybe_log_audio_detected(&detected, &mut logged, &logger);
        detected.store(true, Ordering::SeqCst);
        maybe_log_audio_detected(&detected, &mut logged, &logger);
        maybe_log_audio_detected(&detected, &mut logged, &logger);

        assert_eq!(
            sink.contents(),
            "[info] [microphone] [recorder] audio input detected\n"
        );
    }
}
