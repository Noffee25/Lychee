//! The control thread: decodes ahead of the audio callback, handles commands, reports events.

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

    /// The equalizer, or `None` to turn it off. Heard within a few milliseconds.
    pub fn set_eq(&self, eq: Option<&EqDesign>) {
        self.shared.dsp.set_eq(eq);
    }

    /// Headphone crossfeed: 0 = off, 1–3 = light, medium, strong (stereo output only).
    pub fn set_crossfeed(&self, level: u32) {
        self.shared.dsp.set_crossfeed(level);
    }

    /// Mono: the left and right channels mixed together and played on both sides (the front pair
    /// on surround outputs). Crossfeed stays off meanwhile.
    pub fn set_mono(&self, on: bool) {
        self.shared.dsp.set_mono(on);
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
            let Some(cur) = self.current.as_mut() else { break };
            if !cur.done {
                match decode_packet(cur, channels, &mut self.scratch, &mut self.remixed, &mut self.pending) {
                    Ok(true) => {}
                    Ok(false) => cur.done = true,
                    Err(e) => {
                        cur.done = true;
                        let _ = self.events.send(EngineEvent::Error { token: Some(cur.token), message: e.to_string() });
                    }
                }
                worked = true;
                continue;
            }
            // Current track fully pushed: hand over to the pre-loaded one without a gap.
            if let Some(Next { token, path, source, gain }) = self.next.take() {
                self.emit(EngineEvent::Loaded { token, duration_ms: source.duration_ms, info: source.info.clone() });
                let next = self.activate(token, path, source, gain);
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
            let _ = r.finish(out);
        }
        return Ok(false);
    }
    remixed.clear();
    remix(scratch, cur.source.channels, channels, remixed);
    if cur.gain != 1.0 {
        let gain = cur.gain;
        remixed.iter_mut().for_each(|s| *s *= gain);
    }
    match cur.resampler.as_mut() {
        Some(r) => {
            if let Err(e) = r.process(remixed, out) {
                tracing::warn!("{e}");
            }
        }
        None => out.extend_from_slice(remixed),
    }
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
    fn songs_follow_each_other_without_a_gap() {
        let dir = Scratch::new("songs_follow_each_other_without_a_gap");
        let a = dir.wav("gapless-a.wav", 3000, |i| (i % 1000) as i16);
        let b = dir.wav("gapless-b.wav", 2000, |i| -((i % 1000) as i16) - 1);
        let mut w = worker();
        w.load(1, a, 0, true, 1.0);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0 });
        let (out, starts) = play_out(&mut w);
        assert_eq!(out.len(), 5000);
        assert_eq!(starts, vec![(0, 1), (3000, 2)]);
        assert!(out[..3000].iter().enumerate().all(|(i, v)| *v == pcm((i % 1000) as i16)));
        assert!(out[3000..].iter().enumerate().all(|(i, v)| *v == pcm(-((i % 1000) as i16) - 1)));
    }

    /// Take whatever the decoder has pushed, as the device would (without changing what counts as
    /// audible: in tests that stays the first track).
    fn drain(w: &mut Worker) {
        let mut pipe = w.shared.pipe.lock();
        let pipe = pipe.as_mut().expect("a pipe");
        while pipe.samples.pop().is_ok() {}
    }

    #[test]
    fn seeking_back_after_the_next_song_was_decoded_plays_the_first_song_again() {
        let dir = Scratch::new("seeking_back_after_the_next_song_was_decoded_plays_the_first_song_again");
        let a = dir.wav("seek-a.wav", 8000, |_| 1000);
        let b = dir.wav("seek-b.wav", 8000, |_| 2000);
        let mut w = worker();
        w.load(1, a, 0, true, 1.0);
        w.handle(Cmd::Preload { token: 2, path: b, gain: 1.0 });
        // The decoder moves on to song 2, but song 1 is still what's audible.
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
        assert!(w.next.as_ref().is_some_and(|n| n.token == 2));
        let (out, starts) = play_out(&mut w);
        // From 500 ms (frame 4000) the rest of song 1, then all of song 2. (A seek can land a
        // frame early.)
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0], (0, 1));
        assert!(starts[1].1 == 2 && starts[1].0.abs_diff(4000) <= 1, "{starts:?}");
        assert!(out.len().abs_diff(4000 + 8000) <= 1, "{}", out.len());
        assert!(out[out.len() - 8000..].iter().all(|v| *v == pcm(2000)));
    }
}
