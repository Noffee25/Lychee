//! The audio device side: a cpal stream fed from a lock-free ring buffer.
//!
//! The real-time callback never allocates or blocks. It only `try_lock`s the current [`Pipe`]
//! (the control thread holds that lock just long enough to swap in a new one on seek/load) and
//! otherwise works on atomics. It runs the equalizer and crossfeed ([`Dsp`]) before the volume.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use parking_lot::Mutex;

use crate::AudioError;
use crate::dsp::{Dsp, DspParams, MAX_CHANNELS};

/// Where in the sample stream a new track (segment) starts.
#[derive(Clone, Copy, Debug)]
pub struct Marker {
    pub at_frame: u64,
    pub token: u64,
    pub base_ms: u64,
}

/// Consumer side of the sample pipe, owned by the callback while it plays.
pub struct Pipe {
    pub samples: rtrb::Consumer<f32>,
    pub markers: rtrb::Consumer<Marker>,
    consumed: u64,
    current: Option<Marker>,
    fade_in: u32,
}

impl Pipe {
    pub fn new(samples: rtrb::Consumer<f32>, markers: rtrb::Consumer<Marker>) -> Self {
        Pipe { samples, markers, consumed: 0, current: None, fade_in: 0 }
    }
}

/// State shared between the control thread and the callback.
pub struct Shared {
    pub pipe: Mutex<Option<Pipe>>,
    pub playing: AtomicBool,
    volume_bits: AtomicU32,
    pub device_lost: AtomicBool,
    /// Token of the segment currently audible (0 = none).
    pub token: AtomicU64,
    pub base_ms: AtomicU64,
    pub segment_frames: AtomicU64,
    /// Frames consumed from the current pipe.
    pub consumed: AtomicU64,
    pub sample_rate: AtomicU32,
    /// Equalizer and crossfeed settings.
    pub dsp: DspParams,
}

impl Shared {
    pub fn new() -> Self {
        Shared {
            pipe: Mutex::new(None),
            playing: AtomicBool::new(false),
            volume_bits: AtomicU32::new(1.0f32.to_bits()),
            device_lost: AtomicBool::new(false),
            token: AtomicU64::new(0),
            base_ms: AtomicU64::new(0),
            segment_frames: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            sample_rate: AtomicU32::new(48_000),
            dsp: DspParams::default(),
        }
    }

    pub fn set_volume(&self, v: f32) {
        self.volume_bits.store(v.clamp(0.0, 1.0).to_bits(), Relaxed);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Relaxed))
    }

    /// (token, position in ms) of what is audible right now.
    pub fn position(&self) -> Option<(u64, u64)> {
        let token = self.token.load(Relaxed);
        if token == 0 {
            return None;
        }
        let rate = u64::from(self.sample_rate.load(Relaxed).max(1));
        let ms = self.base_ms.load(Relaxed) + self.segment_frames.load(Relaxed) * 1000 / rate;
        Some((token, ms))
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

/// An open output stream.
pub struct Output {
    _stream: cpal::Stream,
    pub sample_rate: u32,
    pub channels: usize,
    pub device_name: String,
}

/// Names of all output devices of the default host.
pub fn device_names() -> Vec<String> {
    let host = cpal::default_host();
    let Ok(devices) = host.output_devices() else { return Vec::new() };
    let mut seen = std::collections::HashSet::new();
    devices
        .filter_map(|d| d.description().ok().map(|d| d.name().to_string()))
        .filter(|name| !name.starts_with("Discard all samples"))
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

impl Output {
    pub fn open(device_name: Option<&str>, shared: Arc<Shared>) -> Result<Output, AudioError> {
        let host = cpal::default_host();
        let named = device_name.and_then(|name| {
            host.output_devices().ok()?.find(|d| d.description().map(|x| x.name() == name).unwrap_or(false))
        });
        let device = named
            .or_else(|| host.default_output_device())
            .ok_or_else(|| AudioError::Device("no audio output device found".into()))?;
        let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "Default output".into());
        let default = device.default_output_config().map_err(|e| AudioError::Device(e.to_string()))?;
        let rate = default.sample_rate();
        let channels = default.channels();

        // Prefer f32 at the device's native rate and channel count when it is offered.
        let mut format = default.sample_format();
        if format != SampleFormat::F32 {
            if let Ok(mut configs) = device.supported_output_configs() {
                if configs.any(|c| c.sample_format() == SampleFormat::F32 && c.channels() == channels && c.contains_rate(rate)) {
                    format = SampleFormat::F32;
                }
            }
        }
        let config = StreamConfig { channels, sample_rate: rate, buffer_size: cpal::BufferSize::Default };
        shared.sample_rate.store(rate, Relaxed);

        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, config, shared),
            SampleFormat::I16 => build::<i16>(&device, config, shared),
            SampleFormat::I32 => build::<i32>(&device, config, shared),
            SampleFormat::U16 => build::<u16>(&device, config, shared),
            SampleFormat::I24 => build::<cpal::I24>(&device, config, shared),
            SampleFormat::U8 => build::<u8>(&device, config, shared),
            SampleFormat::F64 => build::<f64>(&device, config, shared),
            other => return Err(AudioError::Device(format!("unsupported sample format {other}"))),
        }?;
        stream.play().map_err(|e| AudioError::Device(e.to_string()))?;
        tracing::info!("audio output: {name} ({rate} Hz, {channels} ch, {format})");
        Ok(Output { _stream: stream, sample_rate: rate, channels: channels as usize, device_name: name })
    }
}

fn build<T>(device: &cpal::Device, config: StreamConfig, shared: Arc<Shared>) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let rate = config.sample_rate as f32;
    // ~6 ms fades on pause/resume and on a fresh pipe, so there are no clicks.
    let fade_step = 1.0 / (0.006 * rate);
    let fade_in_frames = (0.004 * rate) as u32;
    let mut fade = 0.0f32;
    let mut gain = shared.volume();
    let err_shared = shared.clone();
    let mut dsp = Dsp::new(channels, config.sample_rate);
    let mut frame_buf = [0.0f32; MAX_CHANNELS];

    let data_cb = move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
        dsp.sync(&shared.dsp);
        let playing = shared.playing.load(Relaxed);
        let target_vol = perceptual(shared.volume());
        let mut guard = shared.pipe.try_lock();
        let mut pipe = guard.as_mut().and_then(|g| g.as_mut());

        for frame in data.chunks_exact_mut(channels) {
            let fade_target = if playing { 1.0 } else { 0.0 };
            if fade < fade_target {
                fade = (fade + fade_step).min(1.0);
            } else if fade > fade_target {
                fade = (fade - fade_step).max(0.0);
            }
            gain += (target_vol - gain) * 0.002;

            let Some(p) = pipe.as_deref_mut() else {
                frame.fill(T::EQUILIBRIUM);
                continue;
            };
            if fade <= 0.0 {
                frame.fill(T::EQUILIBRIUM);
                continue;
            }
            // Switch segments when we reach the next track's first frame.
            while let Ok(m) = p.markers.peek() {
                if m.at_frame > p.consumed {
                    break;
                }
                if p.current.is_none() {
                    p.fade_in = fade_in_frames;
                }
                p.current = Some(*m);
                let _ = p.markers.pop();
            }
            if p.samples.slots() < channels {
                frame.fill(T::EQUILIBRIUM); // underrun or end of stream
                continue;
            }
            let mut g = gain * fade;
            if p.fade_in > 0 {
                g *= 1.0 - p.fade_in as f32 / fade_in_frames.max(1) as f32;
                p.fade_in -= 1;
            }
            if channels <= MAX_CHANNELS {
                let shaped = &mut frame_buf[..channels];
                for v in shaped.iter_mut() {
                    *v = p.samples.pop().unwrap_or(0.0);
                }
                dsp.process(shaped);
                for (s, v) in frame.iter_mut().zip(shaped.iter()) {
                    *s = T::from_sample(v * g);
                }
            } else {
                for s in frame.iter_mut() {
                    let v = p.samples.pop().unwrap_or(0.0);
                    *s = T::from_sample(v * g);
                }
            }
            p.consumed += 1;
        }

        if let Some(p) = pipe {
            shared.consumed.store(p.consumed, Relaxed);
            if let Some(m) = p.current {
                shared.base_ms.store(m.base_ms, Relaxed);
                shared.segment_frames.store(p.consumed - m.at_frame, Relaxed);
                shared.token.store(m.token, Relaxed);
            }
        }
    };
    let err_cb = move |err: cpal::Error| {
        use cpal::ErrorKind;
        match err.kind() {
            ErrorKind::Xrun => tracing::debug!("audio xrun"),
            ErrorKind::DeviceNotAvailable | ErrorKind::DeviceChanged => {
                tracing::warn!("audio device lost: {err}");
                err_shared.device_lost.store(true, Relaxed);
            }
            _ => tracing::warn!("audio stream error: {err}"),
        }
    };
    device.build_output_stream::<T, _, _>(config, data_cb, err_cb, None).map_err(|e| AudioError::Device(e.to_string()))
}

/// Volume slider position (0..1) to linear gain. Cubic feels even across the whole range.
pub fn perceptual(v: f32) -> f32 {
    v.clamp(0.0, 1.0).powi(3)
}
