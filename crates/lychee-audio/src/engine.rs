//! The control thread: decodes ahead of the audio callback, handles commands, reports events.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use crate::output::{Marker, Output, Pipe, Shared};
use crate::remix::remix;
use crate::resample::StreamResampler;
use crate::source::{Source, StreamInfo};

/// Something the engine wants the rest of the app to know.
#[derive(Clone, Debug)]
pub enum EngineEvent {
    /// A track (loaded or pre-loaded) was opened successfully.
    Loaded { token: u64, duration_ms: Option<u64>, info: StreamInfo },
    /// Audio for `token` became audible. For gapless transitions this is the moment the next
    /// track actually starts playing.
    Started { token: u64 },
    /// `token` played to the end and nothing was queued after it.
    Ended { token: u64 },
    Error { token: Option<u64>, message: String },
    OutputChanged { device: String, sample_rate: u32, channels: u16 },
}

enum Cmd {
    Load { token: u64, path: PathBuf, start_ms: u64, play: bool, gain: f32 },
    Preload { token: u64, path: PathBuf, gain: f32 },
    SetGain { token: u64, gain: f32 },
    CancelPreload,
    SetPlaying(bool),
    Seek(u64),
    Stop,
    SetDevice(Option<String>),
    Quit,
}

/// Handle to the playback engine. All methods are non-blocking.
pub struct Engine {
    tx: Sender<Cmd>,
    events: Receiver<EngineEvent>,
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn new(device: Option<String>, volume: f32) -> Engine {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (etx, erx) = crossbeam_channel::unbounded();
        let shared = Arc::new(Shared::new());
        shared.set_volume(volume);
        let worker_shared = shared.clone();
        let handle = thread::Builder::new()
            .name("lychee-audio".into())
            .spawn(move || Worker::new(rx, etx, worker_shared, device).run())
            .expect("failed to spawn the audio thread");
        Engine { tx, events: erx, shared, handle: Some(handle) }
    }

    pub fn events(&self) -> Receiver<EngineEvent> {
        self.events.clone()
    }

    /// Open `path` and start at `start_ms`. `token` identifies this track in later events.
    /// `gain` is a linear factor for this track alone (volume levelling); 1.0 leaves it as is.
    pub fn load(&self, token: u64, path: PathBuf, start_ms: u64, play: bool, gain: f32) {
        let _ = self.tx.send(Cmd::Load { token, path, start_ms, play, gain });
    }

    /// Open the next track ahead of time so it starts without a gap.
    pub fn preload(&self, token: u64, path: PathBuf, gain: f32) {
        let _ = self.tx.send(Cmd::Preload { token, path, gain });
    }

    /// Change a loaded or pre-loaded track's gain. Audio already decoded with the old gain is
    /// thrown away, so the change is heard at once.
    pub fn set_gain(&self, token: u64, gain: f32) {
        let _ = self.tx.send(Cmd::SetGain { token, gain });
    }

    pub fn cancel_preload(&self) {
        let _ = self.tx.send(Cmd::CancelPreload);
    }

    pub fn play(&self) {
        self.shared.playing.store(true, Relaxed);
        let _ = self.tx.send(Cmd::SetPlaying(true));
    }

    pub fn pause(&self) {
        self.shared.playing.store(false, Relaxed);
        let _ = self.tx.send(Cmd::SetPlaying(false));
    }

    pub fn seek(&self, ms: u64) {
        let _ = self.tx.send(Cmd::Seek(ms));
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared.set_volume(volume);
    }

    pub fn set_device(&self, device: Option<String>) {
        let _ = self.tx.send(Cmd::SetDevice(device));
    }

    /// (token, ms) of what is audible right now.
    pub fn position(&self) -> Option<(u64, u64)> {
        self.shared.position()
    }

    pub fn device_names() -> Vec<String> {
        crate::output::device_names()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct Active {
    token: u64,
    path: PathBuf,
    source: Source,
    gain: f32,
    resampler: Option<StreamResampler>,
    done: bool,
}

/// A track opened ahead of time for a gapless hand-over.
struct Next {
    token: u64,
    path: PathBuf,
    source: Source,
    gain: f32,
}

struct Worker {
    rx: Receiver<Cmd>,
    events: Sender<EngineEvent>,
    shared: Arc<Shared>,
    device: Option<String>,
    output: Option<Output>,
    producer: Option<rtrb::Producer<f32>>,
    markers: Option<rtrb::Producer<Marker>>,
    pushed_samples: u64,
    current: Option<Active>,
    /// The previous track after a gapless hand-over, kept until it is no longer audible so a seek
    /// in its last second still works.
    retired: Option<Active>,
    next: Option<Next>,
    pending: Vec<f32>,
    scratch: Vec<f32>,
    remixed: Vec<f32>,
    last_started: u64,
    ended_for: Option<u64>,
}

impl Worker {
    fn new(rx: Receiver<Cmd>, events: Sender<EngineEvent>, shared: Arc<Shared>, device: Option<String>) -> Self {
        Worker {
            rx,
            events,
            shared,
            device,
            output: None,
            producer: None,
            markers: None,
            pushed_samples: 0,
            current: None,
            retired: None,
            next: None,
            pending: Vec::new(),
            scratch: Vec::new(),
            remixed: Vec::new(),
            last_started: 0,
            ended_for: None,
        }
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.events.send(event);
    }

    fn run(mut self) {
        let mut worked = false;
        loop {
            let next_cmd = if self.current.is_none() && self.output.is_none() {
                self.rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
            } else {
                let wait = if worked { Duration::ZERO } else { Duration::from_millis(10) };
                self.rx.recv_timeout(wait)
            };
            match next_cmd {
                Ok(cmd) => {
                    if !self.handle(cmd) {
                        break;
                    }
                    while let Ok(cmd) = self.rx.try_recv() {
                        if !self.handle(cmd) {
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if self.shared.device_lost.swap(false, Relaxed) {
                self.reopen_output();
            }
            worked = self.fill();
            self.check_progress();
        }
        *self.shared.pipe.lock() = None;
    }

    fn handle(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Load { token, path, start_ms, play, gain } => self.load(token, path, start_ms, play, gain),
            Cmd::Preload { token, path, gain } => match Source::open(&path) {
                Ok(source) => self.next = Some(Next { token, path, source, gain }),
                Err(e) => {
                    tracing::warn!("could not pre-load {}: {e}", path.display());
                    self.next = None;
                }
            },
            Cmd::CancelPreload => self.next = None,
            Cmd::SetGain { token, gain } => self.set_gain(token, gain),
            Cmd::SetPlaying(playing) => self.shared.playing.store(playing, Relaxed),
            Cmd::Seek(ms) => self.seek(ms),
            Cmd::Stop => self.stop(),
            Cmd::SetDevice(device) => {
                self.device = device;
                self.reopen_output();
            }
            Cmd::Quit => return false,
        }
        true
    }

    fn ensure_output(&mut self) -> bool {
        if self.output.is_some() {
            return true;
        }
        match Output::open(self.device.as_deref(), self.shared.clone()) {
            Ok(out) => {
                self.emit(EngineEvent::OutputChanged {
                    device: out.device_name.clone(),
                    sample_rate: out.sample_rate,
                    channels: out.channels as u16,
                });
                self.output = Some(out);
                true
            }
            Err(e) => {
                self.emit(EngineEvent::Error { token: None, message: e.to_string() });
                false
            }
        }
    }

    fn new_pipe(&mut self) {
        let (rate, channels) = self.output.as_ref().map(|o| (o.sample_rate, o.channels)).unwrap_or((48_000, 2));
        let (samples_tx, samples_rx) = rtrb::RingBuffer::new(rate as usize * channels);
        let (markers_tx, markers_rx) = rtrb::RingBuffer::new(16);
        let old = self.shared.pipe.lock().replace(Pipe::new(samples_rx, markers_rx));
        drop(old); // freed here, never on the audio thread
        self.producer = Some(samples_tx);
        self.markers = Some(markers_tx);
        self.pushed_samples = 0;
        self.pending.clear();
        self.shared.consumed.store(0, Relaxed);
    }

    /// Mark the start of a segment in the pipe and show its position right away.
    fn begin_segment(&mut self, token: u64, base_ms: u64, at_frame: u64) {
        if let Some(markers) = self.markers.as_mut() {
            let _ = markers.push(Marker { at_frame, token, base_ms });
        }
        if at_frame == 0 {
            self.shared.base_ms.store(base_ms, Relaxed);
            self.shared.segment_frames.store(0, Relaxed);
            self.shared.token.store(token, Relaxed);
        }
    }

    fn activate(&self, token: u64, path: PathBuf, source: Source, gain: f32) -> Active {
        let resampler = self.output.as_ref().and_then(|out| {
            if source.sample_rate == out.sample_rate {
                return None;
            }
            match StreamResampler::new(source.sample_rate, out.sample_rate, out.channels) {
                Ok(r) => Some(r),
                Err(e) => {
                    tracing::warn!("resampler unavailable, playing at the wrong speed: {e}");
                    None
                }
            }
        });
        Active { token, path, source, gain, resampler, done: false }
    }

    fn load(&mut self, token: u64, path: PathBuf, start_ms: u64, play: bool, gain: f32) {
        self.ended_for = None;
        self.retired = None;
        self.next = None;
        self.current = None;
        if !self.ensure_output() {
            self.shared.playing.store(false, Relaxed);
            return;
        }
        self.new_pipe();
        let mut source = match Source::open(&path) {
            Ok(s) => s,
            Err(e) => {
                self.shared.playing.store(false, Relaxed);
                self.shared.token.store(0, Relaxed);
                self.emit(EngineEvent::Error { token: Some(token), message: e.to_string() });
                return;
            }
        };
        let base = if start_ms > 0 { source.seek(start_ms).unwrap_or(0) } else { 0 };
        self.emit(EngineEvent::Loaded { token, duration_ms: source.duration_ms, info: source.info.clone() });
        self.current = Some(self.activate(token, path, source, gain));
        self.begin_segment(token, base, 0);
        self.last_started = 0;
        self.shared.playing.store(play, Relaxed);
    }

    fn seek(&mut self, ms: u64) {
        let audible = self.shared.token.load(Relaxed);
        // After a gapless hand-over the decoder may already be on the next track while the
        // previous one is still audible; seeking then means going back to the previous one.
        if self.retired.as_ref().is_some_and(|r| r.token == audible) {
            if let Some(mut moved_on) = self.current.take() {
                if moved_on.source.seek(0).is_ok() {
                    self.next =
                        Some(Next { token: moved_on.token, path: moved_on.path, source: moved_on.source, gain: moved_on.gain });
                }
            }
            self.current = self.retired.take();
        }
        let Some(mut cur) = self.current.take() else { return };
        let actual = match cur.source.seek(ms) {
            Ok(actual) => actual,
            Err(e) => {
                tracing::warn!("seek failed: {e}");
                self.current = Some(cur);
                return;
            }
        };
        let token = cur.token;
        cur.done = false;
        let cur = self.activate(token, cur.path, cur.source, cur.gain); // fresh resampler state
        self.current = Some(cur);
        self.ended_for = None;
        self.new_pipe();
        self.begin_segment(token, actual, 0);
    }

    fn set_gain(&mut self, token: u64, gain: f32) {
        if let Some(next) = self.next.as_mut().filter(|n| n.token == token) {
            next.gain = gain;
        }
        // Samples of the current (or just retired) track already sit in the ring buffer with the
        // old gain: decode again from what is audible now.
        let mut decoded = false;
        for active in [self.current.as_mut(), self.retired.as_mut()].into_iter().flatten() {
            if active.token == token && active.gain != gain {
                active.gain = gain;
                decoded = true;
            }
        }
        tracing::debug!("gain {gain:.3} for track {token}{}", if decoded { ", decoding again" } else { "" });
        if decoded {
            if let Some((_, ms)) = self.shared.position() {
                self.seek(ms);
            }
        }
    }

    fn stop(&mut self) {
        self.shared.playing.store(false, Relaxed);
        *self.shared.pipe.lock() = None;
        self.producer = None;
        self.markers = None;
        self.current = None;
        self.retired = None;
        self.next = None;
        self.pending.clear();
        self.shared.token.store(0, Relaxed);
        self.last_started = 0;
    }

    /// Recreate the output stream (device change or device lost) and resume where we were.
    fn reopen_output(&mut self) {
        let position = self.shared.position();
        self.output = None;
        if !self.ensure_output() {
            return;
        }
        if self.current.is_some() || self.retired.is_some() {
            let ms = position.map(|(_, ms)| ms).unwrap_or(0);
            self.seek(ms);
        } else {
            self.new_pipe();
        }
    }

    /// Decode and push as much as the ring buffer accepts. Returns true if anything happened.
    fn fill(&mut self) -> bool {
        let Some(channels) = self.output.as_ref().map(|o| o.channels) else { return false };
        if self.producer.is_none() {
            return false;
        }
        let mut worked = false;
        loop {
            let producer = self.producer.as_mut().expect("checked above");
            if !self.pending.is_empty() {
                let (pushed, _) = producer.push_partial_slice(&self.pending);
                let n = pushed.len();
                self.pushed_samples += n as u64;
                self.pending.drain(..n);
                if !self.pending.is_empty() {
                    break; // ring full
                }
                worked = true;
            }
            if producer.slots() < 8192 {
                break;
            }
            let Some(cur) = self.current.as_mut() else { break };
            if !cur.done {
                self.scratch.clear();
                match cur.source.decode_next(&mut self.scratch) {
                    Ok(true) => {
                        self.remixed.clear();
                        remix(&self.scratch, cur.source.channels, channels, &mut self.remixed);
                        if cur.gain != 1.0 {
                            let gain = cur.gain;
                            self.remixed.iter_mut().for_each(|s| *s *= gain);
                        }
                        match cur.resampler.as_mut() {
                            Some(r) => {
                                if let Err(e) = r.process(&self.remixed, &mut self.pending) {
                                    tracing::warn!("{e}");
                                }
                            }
                            None => self.pending.extend_from_slice(&self.remixed),
                        }
                    }
                    Ok(false) => {
                        if let Some(r) = cur.resampler.as_mut() {
                            let _ = r.finish(&mut self.pending);
                        }
                        cur.done = true;
                    }
                    Err(e) => {
                        let token = cur.token;
                        cur.done = true;
                        self.emit(EngineEvent::Error { token: Some(token), message: e.to_string() });
                    }
                }
                worked = true;
                continue;
            }
            // Current track fully pushed: hand over to the pre-loaded one without a gap.
            if self.pending.is_empty() {
                if let Some(Next { token, path, source, gain }) = self.next.take() {
                    self.emit(EngineEvent::Loaded { token, duration_ms: source.duration_ms, info: source.info.clone() });
                    let next = self.activate(token, path, source, gain);
                    let at_frame = self.pushed_samples / channels as u64;
                    self.begin_segment(token, 0, at_frame);
                    self.retired = self.current.replace(next);
                    worked = true;
                    continue;
                }
            }
            break;
        }
        worked
    }

    fn check_progress(&mut self) {
        let audible = self.shared.token.load(Relaxed);
        if audible != 0 && audible != self.last_started {
            self.last_started = audible;
            if self.retired.as_ref().is_some_and(|r| r.token != audible) {
                self.retired = None;
            }
            self.emit(EngineEvent::Started { token: audible });
        }
        let Some(channels) = self.output.as_ref().map(|o| o.channels as u64) else { return };
        if let Some(cur) = &self.current {
            if cur.done && self.next.is_none() && self.pending.is_empty() && self.ended_for != Some(cur.token) {
                let consumed = self.shared.consumed.load(Relaxed);
                if consumed >= self.pushed_samples / channels {
                    self.ended_for = Some(cur.token);
                    self.emit(EngineEvent::Ended { token: cur.token });
                }
            }
        }
    }
}
