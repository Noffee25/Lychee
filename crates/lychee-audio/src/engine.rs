//! The control thread: decodes ahead of the audio callback, handles commands, reports events.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use crate::AudioError;
use crate::dsp::EqDesign;
use crate::output::{Marker, Output, Pipe, Shared};
use crate::remix::remix;
use crate::resample::StreamResampler;
use crate::source::{Source, StreamInfo};

/// The longest crossfade, in milliseconds.
pub const MAX_CROSSFADE_MS: u32 = 12_000;
/// Fades shorter than this aren't worth it: those transitions play gaplessly.
const SHORTEST_FADE_MS: u64 = 50;

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
    Preload { token: u64, path: PathBuf, gain: f32, may_fade: bool },
    SetGain { token: u64, gain: f32 },
    SetFade { token: u64, may_fade: bool },
    SetCrossfade(u32),
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

    /// Open the next track ahead of time so it starts without a gap. With crossfade on (see
    /// [`Engine::set_crossfade`]) and `may_fade`, the current track fades into it instead.
    pub fn preload(&self, token: u64, path: PathBuf, gain: f32, may_fade: bool) {
        let _ = self.tx.send(Cmd::Preload { token, path, gain, may_fade });
    }

    /// Change whether the pre-loaded track `token` may be faded into.
    pub fn set_fade(&self, token: u64, may_fade: bool) {
        let _ = self.tx.send(Cmd::SetFade { token, may_fade });
    }

    /// How long one track fades into the next, in milliseconds (0 = off: gapless). Only
    /// transitions pre-loaded with `may_fade` fade.
    pub fn set_crossfade(&self, ms: u32) {
        let _ = self.tx.send(Cmd::SetCrossfade(ms.min(MAX_CROSSFADE_MS)));
    }

    /// The equalizer, or `None` to turn it off. Heard within a few milliseconds.
    pub fn set_eq(&self, eq: Option<&EqDesign>) {
        self.shared.dsp.set_eq(eq);
    }

    /// Headphone crossfeed: 0 = off, 1–3 = light, medium, strong (stereo output only).
    pub fn set_crossfeed(&self, level: u32) {
        self.shared.dsp.set_crossfeed(level);
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
    /// The track before may fade into this one (kept for a seek back into the previous track).
    may_fade: bool,
    resampler: Option<StreamResampler>,
    /// Frames decoded so far, at the output rate, counted from the start of the track.
    pos: u64,
    /// Where in the track the current stream started (the load or seek position; 0 for a track
    /// that follows another).
    stream_start: u64,
    done: bool,
}

/// A crossfade in progress: the end of `outgoing` mixed into the start of the current track. Both
/// are decoded as the device needs them, so a fade never makes the decoder run seconds ahead.
struct Fade {
    outgoing: Active,
    /// Decoded but not yet mixed, from the outgoing and the incoming track.
    old: VecDeque<f32>,
    new: VecDeque<f32>,
    frames: u64,
    done: u64,
}

/// A track opened ahead of time for a gapless hand-over (or a crossfade).
struct Next {
    token: u64,
    path: PathBuf,
    source: Source,
    gain: f32,
    may_fade: bool,
}

struct Worker {
    rx: Receiver<Cmd>,
    events: Sender<EngineEvent>,
    shared: Arc<Shared>,
    device: Option<String>,
    output: Option<Output>,
    /// (sample rate, channels) of the output.
    format: Option<(u32, usize)>,
    /// Tests run without a device, at this format.
    #[cfg(test)]
    test_format: Option<(u32, usize)>,
    producer: Option<rtrb::Producer<f32>>,
    markers: Option<rtrb::Producer<Marker>>,
    pushed_samples: u64,
    current: Option<Active>,
    /// The previous track after a gapless hand-over, kept until it is no longer audible so a seek
    /// in its last second still works.
    retired: Option<Active>,
    next: Option<Next>,
    pending: Vec<f32>,
    fade: Option<Fade>,
    /// For tracks whose length is only estimated (see `Source::exact_duration`), the newest
    /// samples near the end wait here instead of going to the device, so the fade can start from
    /// the real end. Their decoders (MP3, AAC) are fast, so holding a few seconds back is never heard.
    tail: VecDeque<f32>,
    crossfade_ms: u32,
    scratch: Vec<f32>,
    remixed: Vec<f32>,
    decoded: Vec<f32>,
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
            format: None,
            #[cfg(test)]
            test_format: None,
            producer: None,
            markers: None,
            pushed_samples: 0,
            current: None,
            retired: None,
            next: None,
            pending: Vec::new(),
            fade: None,
            tail: VecDeque::new(),
            crossfade_ms: 0,
            scratch: Vec::new(),
            remixed: Vec::new(),
            decoded: Vec::new(),
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
            Cmd::Preload { token, path, gain, may_fade } => match Source::open(&path) {
                Ok(source) => self.next = Some(Next { token, path, source, gain, may_fade }),
                Err(e) => {
                    tracing::warn!("could not pre-load {}: {e}", path.display());
                    self.next = None;
                }
            },
            Cmd::CancelPreload => self.next = None,
            Cmd::SetGain { token, gain } => self.set_gain(token, gain),
            Cmd::SetFade { token, may_fade } => {
                if let Some(next) = self.next.as_mut().filter(|n| n.token == token) {
                    next.may_fade = may_fade;
                }
            }
            Cmd::SetCrossfade(ms) => self.crossfade_ms = ms,
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
        #[cfg(test)]
        if let Some(format) = self.test_format {
            self.format = Some(format);
            return true;
        }
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
                self.format = Some((out.sample_rate, out.channels));
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
        let (rate, channels) = self.format.unwrap_or((48_000, 2));
        let (samples_tx, samples_rx) = rtrb::RingBuffer::new(rate as usize * channels);
        let (markers_tx, markers_rx) = rtrb::RingBuffer::new(16);
        let old = self.shared.pipe.lock().replace(Pipe::new(samples_rx, markers_rx));
        drop(old); // freed here, never on the audio thread
        self.producer = Some(samples_tx);
        self.markers = Some(markers_tx);
        self.pushed_samples = 0;
        self.pending.clear();
        self.fade = None;
        self.tail.clear();
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

    fn activate(&self, token: u64, path: PathBuf, source: Source, gain: f32, may_fade: bool) -> Active {
        let resampler = self.format.and_then(|(rate, channels)| {
            if source.sample_rate == rate {
                return None;
            }
            match StreamResampler::new(source.sample_rate, rate, channels) {
                Ok(r) => Some(r),
                Err(e) => {
                    tracing::warn!("resampler unavailable, playing at the wrong speed: {e}");
                    None
                }
            }
        });
        Active { token, path, source, gain, may_fade, resampler, pos: 0, stream_start: 0, done: false }
    }

    fn load(&mut self, token: u64, path: PathBuf, start_ms: u64, play: bool, gain: f32) {
        self.ended_for = None;
        self.retired = None;
        self.next = None;
        self.current = None;
        self.fade = None;
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
        let mut cur = self.activate(token, path, source, gain, false);
        cur.pos = self.frames(base);
        cur.stream_start = cur.pos;
        self.current = Some(cur);
        self.begin_segment(token, base, 0);
        self.last_started = 0;
        self.shared.playing.store(play, Relaxed);
    }

    fn seek(&mut self, ms: u64) {
        let audible = self.shared.token.load(Relaxed);
        // After a hand-over (gapless, or the start of a crossfade) the decoder may already be on the
        // next track while the previous one is still audible; seeking then means going back to the
        // previous one.
        let back = match self.fade.take() {
            Some(fade) if fade.outgoing.token == audible => Some(fade.outgoing),
            _ => self.retired.take().filter(|r| r.token == audible),
        };
        if let Some(previous) = back {
            if let Some(mut moved_on) = self.current.take() {
                if moved_on.source.seek(0).is_ok() {
                    self.next = Some(Next {
                        token: moved_on.token,
                        path: moved_on.path,
                        source: moved_on.source,
                        gain: moved_on.gain,
                        may_fade: moved_on.may_fade,
                    });
                }
            }
            self.current = Some(previous);
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
        let mut cur = self.activate(token, cur.path, cur.source, cur.gain, cur.may_fade); // fresh resampler state
        cur.pos = self.frames(actual);
        cur.stream_start = cur.pos;
        self.current = Some(cur);
        self.ended_for = None;
        self.new_pipe();
        self.begin_segment(token, actual, 0);
    }

    /// Milliseconds as frames at the output rate.
    fn frames(&self, ms: u64) -> u64 {
        ms * u64::from(self.format.map_or(48_000, |f| f.0)) / 1000
    }

    fn set_gain(&mut self, token: u64, gain: f32) {
        if let Some(next) = self.next.as_mut().filter(|n| n.token == token) {
            next.gain = gain;
        }
        // Samples of the current (or just retired) track already sit in the ring buffer with the
        // old gain: decode again from what is audible now.
        let mut decoded = false;
        let fading = self.fade.as_mut().map(|f| &mut f.outgoing);
        for active in [self.current.as_mut(), self.retired.as_mut(), fading].into_iter().flatten() {
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
        self.fade = None;
        self.tail.clear();
        self.shared.token.store(0, Relaxed);
        self.last_started = 0;
    }

    /// Recreate the output stream (device change or device lost) and resume where we were.
    fn reopen_output(&mut self) {
        let position = self.shared.position();
        self.output = None;
        self.format = None;
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
        let Some((_, channels)) = self.format else { return false };
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
            if self.fade.is_some() {
                self.step_fade(channels);
                worked = true;
                continue;
            }
            let plan = self.fade_plan();
            let shortest = self.frames(SHORTEST_FADE_MS);
            // Where the next decoded sample lands in the stream (unless it's held back).
            let stream_at = (self.pushed_samples + self.pending.len() as u64) / channels as u64;
            let rate = u64::from(self.format.map_or(48_000, |f| f.0));
            let Some(cur) = self.current.as_mut() else { break };
            if !cur.done {
                let before = cur.pos;
                self.decoded.clear();
                match decode_packet(cur, channels, &mut self.scratch, &mut self.remixed, &mut self.decoded) {
                    Ok(true) => {}
                    Ok(false) => cur.done = true,
                    Err(e) => {
                        cur.done = true;
                        let _ = self.events.send(EngineEvent::Error { token: Some(cur.token), message: e.to_string() });
                    }
                }
                let (pos, done, stream_start) = (cur.pos, cur.done, cur.stream_start);
                match plan {
                    // The length is exact: the fade starts in this packet, or should have already
                    // (the next track was pre-loaded late): then it starts now and still ends with
                    // the track. Not if the stream started inside the fade, though: after a seek
                    // into the last seconds, those seconds play as they are (fading from the start
                    // of the stream would show the next song at once, even while paused).
                    Some((start, frames)) if cur.source.exact_duration => {
                        let fade = (pos > start && !done && start > stream_start)
                            .then(|| {
                                let begin = start.max(before);
                                (begin - before, (start + frames).saturating_sub(begin))
                            })
                            .filter(|(_, len)| *len >= shortest);
                        match fade {
                            Some((skip, len)) => {
                                let split = (skip as usize * channels).min(self.decoded.len());
                                let decoded = std::mem::take(&mut self.decoded);
                                self.pending.extend_from_slice(&decoded[..split]);
                                self.begin_fade(channels, &decoded[split..], len);
                                self.decoded = decoded;
                            }
                            None => self.pending.extend_from_slice(&self.decoded),
                        }
                    }
                    // The length is a guess: from a while before the guessed end, hold the newest
                    // samples back (as many as the fade is long); at the real end they're mixed in.
                    Some((start, frames)) => {
                        let guessed = cur.source.duration_ms.unwrap_or(0) * rate / 1000;
                        let hold_from = start.saturating_sub(guessed * 15 / 100);
                        if pos > hold_from {
                            let split = (hold_from.saturating_sub(before) as usize * channels).min(self.decoded.len());
                            self.pending.extend_from_slice(&self.decoded[..split]);
                            self.tail.extend(self.decoded[split..].iter().copied());
                            let keep = frames as usize * channels;
                            if self.tail.len() > keep {
                                let excess = self.tail.len() - keep;
                                self.pending.extend(self.tail.drain(..excess));
                            }
                        } else {
                            self.pending.extend(self.tail.drain(..));
                            self.pending.extend_from_slice(&self.decoded);
                        }
                    }
                    // No fade (any more): whatever was held back plays as it is.
                    None => {
                        self.pending.extend(self.tail.drain(..));
                        self.pending.extend_from_slice(&self.decoded);
                    }
                }
                worked = true;
                continue;
            }
            // The track is fully decoded. A held-back tail is its real end: fade that into the
            // next track (unless it's all that was decoded since a seek), or let it play out.
            if !self.tail.is_empty() {
                if plan.is_some() && stream_at > 0 {
                    self.fade_tail(channels, shortest);
                } else {
                    self.pending.extend(self.tail.drain(..));
                }
                worked = true;
                continue;
            }
            // Hand over to the pre-loaded track without a gap.
            if let Some(Next { token, path, source, gain, may_fade }) = self.next.take() {
                self.emit(EngineEvent::Loaded { token, duration_ms: source.duration_ms, info: source.info.clone() });
                let next = self.activate(token, path, source, gain, may_fade);
                let at_frame = self.pushed_samples / channels as u64;
                self.begin_segment(token, 0, at_frame);
                self.retired = self.current.replace(next);
                worked = true;
                continue;
            }
            break;
        }
        worked
    }

    /// Where the current track should start fading into the pre-loaded one, and for how many
    /// frames: the crossfade length, but never more than half of either track.
    fn fade_plan(&self) -> Option<(u64, u64)> {
        if self.crossfade_ms == 0 {
            return None;
        }
        let next = self.next.as_ref().filter(|n| n.may_fade)?;
        let cur = self.current.as_ref()?;
        let total = self.frames(cur.source.duration_ms?);
        let frames = self
            .frames(u64::from(self.crossfade_ms))
            .min(total / 2)
            .min(next.source.duration_ms.map_or(u64::MAX, |d| self.frames(d) / 2));
        // Too short to hear as a fade: play gaplessly.
        (frames >= self.frames(SHORTEST_FADE_MS)).then(|| (total - frames, frames))
    }

    /// Fade the held-back end of the (finished) current track into the pre-loaded one, for at most
    /// half the next track's length.
    fn fade_tail(&mut self, channels: usize, shortest: u64) {
        let tail_frames = (self.tail.len() / channels) as u64;
        let cap = self.next.as_ref().and_then(|n| n.source.duration_ms).map_or(u64::MAX, |d| self.frames(d) / 2);
        let frames = tail_frames.min(cap);
        if frames < shortest {
            self.pending.extend(self.tail.drain(..));
            return;
        }
        let unmixed = (tail_frames - frames) as usize * channels;
        self.pending.extend(self.tail.drain(..unmixed));
        let first: Vec<f32> = self.tail.drain(..).collect();
        self.begin_fade(channels, &first, frames);
    }

    /// Start fading the current track (whose decoded `first` samples belong to the fade) into the
    /// pre-loaded one. The next track starts, as far as positions and events go, where the fade begins.
    fn begin_fade(&mut self, channels: usize, first: &[f32], frames: u64) {
        let Some(Next { token, path, source, gain, may_fade }) = self.next.take() else {
            self.pending.extend_from_slice(first);
            return;
        };
        self.emit(EngineEvent::Loaded { token, duration_ms: source.duration_ms, info: source.info.clone() });
        let incoming = self.activate(token, path, source, gain, may_fade);
        let at_frame = (self.pushed_samples + self.pending.len() as u64) / channels as u64;
        self.begin_segment(token, 0, at_frame);
        tracing::debug!("crossfade into track {token}: {} ms", frames * 1000 / u64::from(self.format.map_or(48_000, |f| f.0)));
        let outgoing = self.current.replace(incoming).expect("a current track");
        self.retired = None;
        self.fade = Some(Fade { outgoing, old: first.iter().copied().collect(), new: VecDeque::new(), frames, done: 0 });
    }

    /// Mix the next stretch of a crossfade.
    fn step_fade(&mut self, channels: usize) {
        const AHEAD: usize = 4096;
        if self.current.is_none() {
            self.fade = None;
            return;
        }
        let (Some(fade), Some(cur)) = (self.fade.as_mut(), self.current.as_mut()) else { return };
        // Keep a little of each decoded.
        for (track, buf) in [(&mut fade.outgoing, &mut fade.old), (cur, &mut fade.new)] {
            while buf.len() < AHEAD * channels && !track.done {
                self.decoded.clear();
                match decode_packet(track, channels, &mut self.scratch, &mut self.remixed, &mut self.decoded) {
                    Ok(true) => buf.extend(self.decoded.iter().copied()),
                    Ok(false) => {
                        buf.extend(self.decoded.iter().copied());
                        track.done = true;
                    }
                    Err(e) => {
                        track.done = true;
                        let _ = self.events.send(EngineEvent::Error { token: Some(track.token), message: e.to_string() });
                    }
                }
            }
        }
        let cur = self.current.as_ref().expect("checked above");
        let ready = |buf: &VecDeque<f32>, done: bool| if done { usize::MAX } else { buf.len() / channels };
        let left = (fade.frames - fade.done) as usize;
        let mut n = left.min(ready(&fade.old, fade.outgoing.done)).min(ready(&fade.new, cur.done));
        if fade.outgoing.done && cur.done {
            // Nothing more is coming: mix what's left, without padding the fade with silence.
            n = n.min(fade.old.len().max(fade.new.len()) / channels);
            if n == 0 {
                fade.done = fade.frames;
            }
        }
        self.pending.reserve(n * channels);
        for f in 0..n {
            // Equal power: the loudness stays even while one song gives way to the other.
            let t = ((fade.done as usize + f) as f32 + 0.5) / fade.frames as f32 * std::f32::consts::FRAC_PI_2;
            let (fade_out, fade_in) = (t.cos(), t.sin());
            for _ in 0..channels {
                let old = fade.old.pop_front().unwrap_or(0.0);
                let new = fade.new.pop_front().unwrap_or(0.0);
                self.pending.push(old * fade_out + new * fade_in);
            }
        }
        fade.done += n as u64;
        if fade.done >= fade.frames {
            // Done: the rest of what was decoded of the new track carries on. The old one is kept
            // while it may still be audible, so a seek in its last seconds still works.
            if let Some(fade) = self.fade.take() {
                self.pending.extend(fade.new);
                self.retired = Some(fade.outgoing);
            }
        }
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
        let Some(channels) = self.format.map(|f| f.1 as u64) else { return };
        if let Some(cur) = &self.current {
            if cur.done
                && self.next.is_none()
                && self.pending.is_empty()
                && self.fade.is_none()
                && self.tail.is_empty()
                && self.ended_for != Some(cur.token)
            {
                let consumed = self.shared.consumed.load(Relaxed);
                if consumed >= self.pushed_samples / channels {
                    self.ended_for = Some(cur.token);
                    self.emit(EngineEvent::Ended { token: cur.token });
                }
            }
        }
    }
}

/// Decode one packet of `cur` into `out`, at the output's rate and channel count and with the
/// track's gain. `Ok(false)` at the end of the track (the resampler's last samples included).
fn decode_packet(
    cur: &mut Active,
    channels: usize,
    scratch: &mut Vec<f32>,
    remixed: &mut Vec<f32>,
    out: &mut Vec<f32>,
) -> Result<bool, AudioError> {
    scratch.clear();
    if !cur.source.decode_next(scratch)? {
        if let Some(r) = cur.resampler.as_mut() {
            let before = out.len();
            let _ = r.finish(out);
            cur.pos += ((out.len() - before) / channels) as u64;
        }
        return Ok(false);
    }
    remixed.clear();
    remix(scratch, cur.source.channels, channels, remixed);
    if cur.gain != 1.0 {
        let gain = cur.gain;
        remixed.iter_mut().for_each(|s| *s *= gain);
    }
    let before = out.len();
    match cur.resampler.as_mut() {
        Some(r) => {
            if let Err(e) = r.process(remixed, out) {
                tracing::warn!("{e}");
            }
        }
        None => out.extend_from_slice(remixed),
    }
    cur.pos += ((out.len() - before) / channels) as u64;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const RATE: u32 = 8000;

    /// A folder for one test's files, removed when the test ends (pass or fail).
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(test: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("lychee-engine-{}-{test}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        /// A stereo 16-bit WAV whose frame `i` holds `value(i)` on both channels.
        fn wav(&self, name: &str, frames: usize, value: impl Fn(usize) -> i16) -> PathBuf {
            wav(&self.0.join(name), frames, value)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn wav(path: &Path, frames: usize, value: impl Fn(usize) -> i16) -> PathBuf {
        let data_len = (frames * 4) as u32;
        let mut b = Vec::with_capacity(44 + data_len as usize);
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&RATE.to_le_bytes());
        b.extend_from_slice(&(RATE * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let v = value(i).to_le_bytes();
            b.extend_from_slice(&v);
            b.extend_from_slice(&v);
        }
        std::fs::write(path, b).unwrap();
        path.to_path_buf()
    }

    fn worker() -> Worker {
        let (_tx, rx) = crossbeam_channel::unbounded();
        let (etx, _erx) = crossbeam_channel::unbounded();
        let mut w = Worker::new(rx, etx, Arc::new(Shared::new()), None);
        w.test_format = Some((RATE, 2));
        w
    }

    /// Run the decoder until everything is out, as the device would take it. Returns the left
    /// channel and where each track started (frame, token).
    fn play_out(w: &mut Worker) -> (Vec<f32>, Vec<(u64, u64)>) {
        let (mut left, mut starts) = (Vec::new(), Vec::new());
        let mut frame = Vec::new();
        for _ in 0..100_000 {
            let worked = w.fill();
            let mut got = false;
            {
                let mut pipe = w.shared.pipe.lock();
                let pipe = pipe.as_mut().expect("a pipe");
                while let Ok(m) = pipe.markers.pop() {
                    starts.push((m.at_frame, m.token));
                }
                while let Ok(v) = pipe.samples.pop() {
                    got = true;
                    frame.push(v);
                    if frame.len() == 2 {
                        assert_eq!(frame[0], frame[1], "both channels carry the same signal");
                        left.push(frame[0]);
                        frame.clear();
                    }
                }
            }
            if !worked && !got {
                break;
            }
        }
        (left, starts)
    }

    fn pcm(v: i16) -> f32 {
        f32::from(v) / 32768.0
    }

    #[test]
    fn gapless_without_crossfade() {
        let dir = Scratch::new("gapless_without_crossfade");
        let a = dir.wav("gapless-a.wav", 3000, |i| (i % 1000) as i16);
        let b = dir.wav("gapless-b.wav", 2000, |i| -((i % 1000) as i16) - 1);
        let mut w = worker();
        w.load(1, a, 0, true, 1.0);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0, may_fade: true });
        let (out, starts) = play_out(&mut w);
        assert_eq!(out.len(), 5000);
        assert_eq!(starts, vec![(0, 1), (3000, 2)]);
        assert!(out[..3000].iter().enumerate().all(|(i, v)| *v == pcm((i % 1000) as i16)));
        assert!(out[3000..].iter().enumerate().all(|(i, v)| *v == pcm(-((i % 1000) as i16) - 1)));
    }

    #[test]
    fn crossfade_mixes_the_end_into_the_start() {
        let dir = Scratch::new("crossfade_mixes_the_end_into_the_start");
        let a = dir.wav("fade-a.wav", 8000, |_| 16384); // 0.5
        let b = dir.wav("fade-b.wav", 8000, |_| -8192); // -0.25
        let mut w = worker();
        w.handle(Cmd::SetCrossfade(250)); // 2000 frames at 8 kHz
        w.load(1, a, 0, true, 1.0);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0, may_fade: true });
        let (out, starts) = play_out(&mut w);
        assert_eq!(out.len(), 8000 + 8000 - 2000, "the fade overlaps the two songs");
        assert_eq!(starts, vec![(0, 1), (6000, 2)], "the next song starts where the fade begins");
        assert!(out[..6000].iter().all(|v| *v == 0.5));
        for f in [0usize, 500, 1000, 1999] {
            let t = (f as f32 + 0.5) / 2000.0 * std::f32::consts::FRAC_PI_2;
            let want = 0.5 * t.cos() - 0.25 * t.sin();
            assert!((out[6000 + f] - want).abs() < 1e-6, "frame {f}: {} vs {want}", out[6000 + f]);
        }
        assert!(out[8000..].iter().all(|v| *v == -0.25));
    }

    #[test]
    fn album_transitions_stay_gapless_with_crossfade_on() {
        let dir = Scratch::new("album_transitions_stay_gapless_with_crossfade_on");
        let a = dir.wav("album-a.wav", 4000, |_| 1000);
        let b = dir.wav("album-b.wav", 4000, |_| 2000);
        let mut w = worker();
        w.handle(Cmd::SetCrossfade(250));
        w.load(1, a, 0, true, 1.0);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0, may_fade: false });
        let (out, starts) = play_out(&mut w);
        assert_eq!(out.len(), 8000);
        assert_eq!(starts, vec![(0, 1), (4000, 2)]);
        assert!(out[..4000].iter().all(|v| *v == pcm(1000)) && out[4000..].iter().all(|v| *v == pcm(2000)));
    }

    #[test]
    fn short_songs_fade_for_at_most_half_their_length() {
        let dir = Scratch::new("short_songs_fade_for_at_most_half_their_length");
        let a = dir.wav("short-a.wav", 8000, |_| 1000);
        let b = dir.wav("short-b.wav", 1000, |_| 2000); // 125 ms
        let mut w = worker();
        w.handle(Cmd::SetCrossfade(500));
        w.load(1, a, 0, true, 1.0);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0, may_fade: true });
        let (out, starts) = play_out(&mut w);
        assert_eq!(starts, vec![(0, 1), (7500, 2)]);
        assert_eq!(out.len(), 8500);
        assert!(out[8000..].iter().all(|v| *v == pcm(2000)));
    }

    /// Take whatever the decoder has pushed, as the device would (without changing what counts as
    /// audible: in tests that stays the first track).
    fn drain(w: &mut Worker) {
        let mut pipe = w.shared.pipe.lock();
        let pipe = pipe.as_mut().expect("a pipe");
        while pipe.samples.pop().is_ok() {}
    }

    #[test]
    fn seeking_back_after_the_fade_was_decoded_plays_the_first_song_again() {
        let dir = Scratch::new("seeking_back_after_the_fade_was_decoded_plays_the_first_song_again");
        let a = dir.wav("seek-a.wav", 8000, |_| 1000);
        let b = dir.wav("seek-b.wav", 8000, |_| 2000);
        let mut w = worker();
        w.handle(Cmd::SetCrossfade(250));
        w.load(1, a, 0, true, 1.0);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0, may_fade: true });
        // The decoder reaches the fade, but song 1 is still what's audible.
        for _ in 0..50 {
            w.fill();
            drain(&mut w);
            if w.current.as_ref().is_some_and(|c| c.token == 2) {
                break;
            }
        }
        assert_eq!(w.current.as_ref().map(|c| c.token), Some(2), "the decoder moved on");
        w.seek(500);
        assert_eq!(w.current.as_ref().map(|c| c.token), Some(1));
        assert!(w.fade.is_none());
        assert!(w.next.as_ref().is_some_and(|n| n.token == 2 && n.may_fade));
        let (out, starts) = play_out(&mut w);
        // From 500 ms (frame 4000) the rest of song 1, then the fade, then song 2. (A seek can
        // land a frame early.)
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0], (0, 1));
        assert!(starts[1].1 == 2 && starts[1].0.abs_diff(2000) <= 1, "{starts:?}");
        assert!(out.len().abs_diff(4000 + 8000 - 2000) <= 1, "{}", out.len());
        assert!(out[out.len() - 6000..].iter().all(|v| *v == pcm(2000)));
    }

    #[test]
    fn a_long_crossfade_doesnt_make_the_decoder_run_ahead() {
        let dir = Scratch::new("a_long_crossfade_doesnt_make_the_decoder_run_ahead");
        // Slow decoders (Opus) would leave the device waiting if a fade needed seconds decoded first.
        let a = dir.wav("long-a.wav", 80_000, |_| 1000); // 10 s
        let mut w = worker();
        w.handle(Cmd::SetCrossfade(5000));
        w.load(1, a, 0, true, 1.0);
        w.fill();
        let decoded = w.current.as_ref().map_or(0, |c| c.pos);
        assert!(w.pushed_samples > 0, "audio is ready at once");
        assert!(decoded <= u64::from(RATE) + 4096, "decoded {decoded} frames ahead");
    }

    #[test]
    fn a_late_preload_still_fades_and_ends_with_the_track() {
        let dir = Scratch::new("a_late_preload_still_fades_and_ends_with_the_track");
        let a = dir.wav("late-a.wav", 80_000, |_| 1000); // 10 s
        let b = dir.wav("late-b.wav", 8000, |_| 2000);
        let mut w = worker();
        w.handle(Cmd::SetCrossfade(1000)); // would start at frame 72 000
        w.load(1, a, 0, true, 1.0);
        let mut taken = 0usize;
        while w.current.as_ref().is_some_and(|c| c.pos < 74_000) {
            w.fill();
            let mut pipe = w.shared.pipe.lock();
            let pipe = pipe.as_mut().expect("a pipe");
            while pipe.samples.pop().is_ok() {
                taken += 1;
            }
        }
        let decoded = w.current.as_ref().map_or(0, |c| c.pos);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0, may_fade: true });
        let (out, starts) = play_out(&mut w);
        let total = taken / 2 + out.len();
        let fade = 80_000 - decoded as usize;
        assert!(fade > 0 && fade < 6000, "{fade}");
        assert_eq!(starts, vec![(0, 1), (decoded, 2)], "the fade starts where the decoder was");
        assert_eq!(total, 88_000 - fade);
        assert!(out[out.len() - (8000 - fade)..].iter().all(|v| *v == pcm(2000)));
    }

    #[test]
    fn a_seek_into_the_last_seconds_plays_them_as_they_are() {
        let dir = Scratch::new("a_seek_into_the_last_seconds_plays_them_as_they_are");
        // Fading from the start of the new stream would show the next song at once (and as
        // playing, even when paused).
        let a = dir.wav("window-a.wav", 80_000, |_| 1000); // 10 s, decoded in several packets
        let b = dir.wav("window-b.wav", 64_000, |_| 2000); // long enough not to shorten the fade
        let mut w = worker();
        w.handle(Cmd::SetCrossfade(2000)); // the fade would start at 8 s
        w.load(1, a, 9000, false, 1.0);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0, may_fade: true });
        let (out, starts) = play_out(&mut w);
        assert_eq!(starts.len(), 2);
        assert!(starts[1].1 == 2 && starts[1].0.abs_diff(8000) <= 1, "gapless after the rest of song 1: {starts:?}");
        assert!(out.len().abs_diff(8000 + 64_000) <= 1);
        assert_eq!(w.shared.token.load(Relaxed), 1, "song 2 isn't audible before the device gets there");
        // A seek to just before the fade still fades.
        let a = dir.wav("window-c.wav", 80_000, |_| 1000);
        let b = dir.wav("window-d.wav", 64_000, |_| 2000);
        let mut w = worker();
        w.handle(Cmd::SetCrossfade(2000));
        w.load(1, a, 7000, false, 1.0);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0, may_fade: true });
        let (_, starts) = play_out(&mut w);
        assert!(starts[1].0.abs_diff(8000) <= 1, "the fade starts at 8 s, 1 s into the stream: {starts:?}");
    }

    /// Load `a` as a track whose length is only a guess (as for an MP3 without a Xing header).
    fn load_guessed(w: &mut Worker, a: PathBuf, guess_ms: u64) {
        w.load(1, a, 0, true, 1.0);
        let cur = w.current.as_mut().expect("loaded");
        cur.source.exact_duration = false;
        cur.source.duration_ms = Some(guess_ms);
    }

    #[test]
    fn a_track_longer_than_its_guessed_length_fades_from_its_real_end() {
        let dir = Scratch::new("a_track_longer_than_its_guessed_length_fades_from_its_real_end");
        let a = dir.wav("guess-short-a.wav", 8000, |_| 1000); // 1 s, guessed at 0.5 s
        let b = dir.wav("guess-short-b.wav", 8000, |_| 2000);
        let mut w = worker();
        w.handle(Cmd::SetCrossfade(250));
        load_guessed(&mut w, a, 500);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0, may_fade: true });
        let (out, starts) = play_out(&mut w);
        assert_eq!(starts, vec![(0, 1), (6000, 2)], "nothing of song 1 is lost");
        assert_eq!(out.len(), 8000 + 8000 - 2000);
        assert!(out[..6000].iter().all(|v| *v == pcm(1000)));
        assert!(out[8000..].iter().all(|v| *v == pcm(2000)));
    }

    #[test]
    fn a_track_shorter_than_its_guessed_length_plays_out_in_full() {
        let dir = Scratch::new("a_track_shorter_than_its_guessed_length_plays_out_in_full");
        let a = dir.wav("guess-long-a.wav", 8000, |_| 1000); // 1 s, guessed at 2 s
        let b = dir.wav("guess-long-b.wav", 8000, |_| 2000);
        let mut w = worker();
        w.handle(Cmd::SetCrossfade(250));
        load_guessed(&mut w, a, 2000);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0, may_fade: true });
        let (out, starts) = play_out(&mut w);
        assert_eq!(starts, vec![(0, 1), (8000, 2)]);
        assert_eq!(out.len(), 16_000);
    }
}
