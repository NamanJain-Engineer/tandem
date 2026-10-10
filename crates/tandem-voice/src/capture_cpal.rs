//! The OS microphone through cpal (CoreAudio, WASAPI, ALSA/PipeWire). Each
//! callback downmixes to mono and hands the block to the capture ring.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::downmix;
use crate::capture::{AudioBackend, CaptureError, ErrorSink, InputStream, SampleSink};
use crate::devices::DeviceInfo;

#[derive(Default)]
pub struct CpalBackend;

fn open_error(e: impl std::fmt::Display) -> CaptureError {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if lower.contains("permission") || lower.contains("denied") || lower.contains("not authorized")
    {
        CaptureError::PermissionDenied
    } else {
        CaptureError::Open(msg)
    }
}

impl AudioBackend for CpalBackend {
    fn devices(&self) -> Vec<DeviceInfo> {
        let host = cpal::default_host();
        let default = host.default_input_device().and_then(|d| d.name().ok());
        let Ok(devices) = host.input_devices() else {
            return Vec::new();
        };
        let mut seen = std::collections::HashSet::new();
        devices
            .filter_map(|d| d.name().ok())
            .filter(|n| seen.insert(n.clone()))
            // ALSA lists plugin aliases that aren't microphones.
            .filter(|n| {
                !matches!(
                    n.as_str(),
                    "null" | "samplerate" | "speexrate" | "jack" | "oss" | "upmix" | "vdownmix"
                )
            })
            .map(|name| {
                let is_default = default.as_deref() == Some(name.as_str());
                DeviceInfo::new(name.clone(), name, is_default)
            })
            .collect()
    }

    fn open(
        &self,
        device: &str,
        mut sink: SampleSink,
        on_error: ErrorSink,
    ) -> Result<Box<dyn InputStream>, CaptureError> {
        let host = cpal::default_host();
        let dev = host
            .input_devices()
            .map_err(open_error)?
            .find(|d| d.name().map(|n| n == device).unwrap_or(false))
            .ok_or(CaptureError::NoDevice)?;
        let config = dev.default_input_config().map_err(open_error)?;
        let channels = config.channels();
        let rate = config.sample_rate().0;
        let format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();
        let err = move |e: cpal::StreamError| on_error(CaptureError::Stream(e.to_string()));
        let mut mono = Vec::with_capacity(4096);
        let stream = match format {
            cpal::SampleFormat::F32 => dev.build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    mono.clear();
                    downmix(data, channels, &mut mono);
                    sink(&mono);
                },
                err,
                None,
            ),
            cpal::SampleFormat::I16 => {
                let mut floats = Vec::with_capacity(4096);
                dev.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        floats.clear();
                        floats.extend(data.iter().map(|s| *s as f32 / 32_768.0));
                        mono.clear();
                        downmix(&floats, channels, &mut mono);
                        sink(&mono);
                    },
                    err,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let mut floats = Vec::with_capacity(4096);
                dev.build_input_stream(
                    &stream_config,
                    move |data: &[u16], _| {
                        floats.clear();
                        floats.extend(data.iter().map(|s| (*s as f32 - 32_768.0) / 32_768.0));
                        mono.clear();
                        downmix(&floats, channels, &mut mono);
                        sink(&mono);
                    },
                    err,
                    None,
                )
            }
            other => {
                return Err(CaptureError::Open(format!(
                    "unsupported sample format {other:?}"
                )))
            }
        }
        .map_err(open_error)?;
        // Opened paused: the first take resumes it.
        let _ = stream.pause();
        Ok(Box::new(CpalStream { stream, rate }))
    }
}

struct CpalStream {
    stream: cpal::Stream,
    rate: u32,
}

impl InputStream for CpalStream {
    fn play(&self) -> Result<(), CaptureError> {
        self.stream
            .play()
            .map_err(|e| CaptureError::Stream(e.to_string()))
    }

    fn pause(&self) -> Result<(), CaptureError> {
        self.stream
            .pause()
            .map_err(|e| CaptureError::Stream(e.to_string()))
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }
}
