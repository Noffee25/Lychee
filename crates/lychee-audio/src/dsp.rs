//! Sound shaping in the audio callback: a ten-band graphic equalizer, headphone crossfeed and mono.
//!
//! The callback owns a [`Dsp`]. Settings reach it through [`DspParams`] in the shared state: atomics
//! behind a version counter, so the real-time thread never locks or allocates and never uses a
//! half-written update. Changes glide over a few tens of milliseconds at most, so moving a band or
//! switching crossfeed or mono on doesn't click. With everything off the samples pass through
//! untouched.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence};

use parking_lot::Mutex;

/// Bands of the graphic equalizer, an octave apart.
pub const EQ_BANDS: usize = 10;
/// Centre frequencies of the bands, in Hz.
pub const EQ_FREQS: [f64; EQ_BANDS] = [32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
/// How far a band goes up or down, in dB.
pub const EQ_RANGE_DB: f32 = 12.0;

/// The band filters are about 1.5 octaves wide, so neighbours overlap and settings blend into a
/// smooth curve; [`EqDesign::new`] corrects for the overlap.
const EQ_Q: f64 = 0.9;
/// Equalizers are designed for this rate; the callback builds its filters for the device's own.
const DESIGN_RATE: f64 = 48_000.0;
/// Filter gains the overlap correction may use.
const MAX_FILTER_DB: f64 = 24.0;

/// Second-order section (normalised so a0 = 1).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    const IDENTITY: Biquad = Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    /// A peaking filter (the "Audio EQ Cookbook" design): `db` at `freq`, fading out on either side.
    fn peaking(freq: f64, q: f64, db: f64, rate: f64) -> Biquad {
        // Bands at or above Nyquist can't be built at this rate: they're left out.
        if db == 0.0 || freq >= rate * 0.49 {
            return Biquad::IDENTITY;
        }
        let a = 10f64.powf(db / 40.0);
        let w = std::f64::consts::TAU * freq / rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        Biquad {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * cos / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }

    /// Gain at `freq`, in dB.
    fn gain_db(&self, freq: f64, rate: f64) -> f64 {
        let w = std::f64::consts::TAU * freq / rate;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let (nr, ni) = (self.b0 + self.b1 * c1 + self.b2 * c2, -(self.b1 * s1 + self.b2 * s2));
        let (dr, di) = (1.0 + self.a1 * c1 + self.a2 * c2, -(self.a1 * s1 + self.a2 * s2));
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).max(1e-30).log10()
    }
}

/// What the equalizer does with a set of band settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqDesign {
    /// Gain of each band's filter, in dB. Neighbouring filters overlap, so these differ from the band
    /// settings: they're chosen so the combined curve passes through every setting.
    pub filters: [f64; EQ_BANDS],
    /// Level change in front of the filters, in dB (zero or less): room for the biggest boost, so
    /// boosted songs never distort.
    pub preamp: f64,
}

impl EqDesign {
    /// Band settings (dB, clamped to ±[`EQ_RANGE_DB`]) to filters.
    pub fn new(bands: &[f32; EQ_BANDS]) -> EqDesign {
        let target: [f64; EQ_BANDS] = std::array::from_fn(|i| f64::from(bands[i].clamp(-EQ_RANGE_DB, EQ_RANGE_DB)));
        // How much each filter, at a test gain, moves the curve at every band's centre. Solving with
        // that matrix gets close; a few corrections against the real curve finish the job.
        const TEST_DB: f64 = 12.0;
        let mut effect = [[0.0; EQ_BANDS]; EQ_BANDS];
        for (i, f) in EQ_FREQS.iter().enumerate() {
            let filter = Biquad::peaking(*f, EQ_Q, TEST_DB, DESIGN_RATE);
            for (j, centre) in EQ_FREQS.iter().enumerate() {
                effect[j][i] = filter.gain_db(*centre, DESIGN_RATE) / TEST_DB;
            }
        }
        let clamp = |g: [f64; EQ_BANDS]| g.map(|x| x.clamp(-MAX_FILTER_DB, MAX_FILTER_DB));
        let mut filters = clamp(solve(effect, target));
        for _ in 0..3 {
            let design = EqDesign { filters, preamp: 0.0 };
            let error: [f64; EQ_BANDS] = std::array::from_fn(|j| target[j] - design.curve_db(EQ_FREQS[j], DESIGN_RATE));
            let step = solve(effect, error);
            filters = clamp(std::array::from_fn(|i| filters[i] + step[i]));
        }
        if target.iter().all(|t| *t == 0.0) {
            filters = [0.0; EQ_BANDS];
        }
        let design = EqDesign { filters, preamp: 0.0 };
        // The highest point of the curve across the audible range.
        let peak = (0..=240)
            .map(|k| 20.0 * 1000f64.powf(f64::from(k) / 240.0))
            .map(|f| design.curve_db(f, DESIGN_RATE))
            .fold(0.0f64, f64::max);
        EqDesign { filters, preamp: -peak }
    }

    /// The equalizer's curve at `freq` (without the pre-amp), in dB.
    pub fn curve_db(&self, freq: f64, rate: f64) -> f64 {
        EQ_FREQS.iter().zip(self.filters).map(|(f, db)| Biquad::peaking(*f, EQ_Q, db, rate).gain_db(freq, rate)).sum()
    }
}

/// Solve `m · x = b` (Gaussian elimination with partial pivoting).
fn solve(mut m: [[f64; EQ_BANDS]; EQ_BANDS], mut b: [f64; EQ_BANDS]) -> [f64; EQ_BANDS] {
    for col in 0..EQ_BANDS {
        let pivot = (col..EQ_BANDS).max_by(|x, y| m[*x][col].abs().total_cmp(&m[*y][col].abs())).unwrap_or(col);
        m.swap(col, pivot);
        b.swap(col, pivot);
        if m[col][col].abs() < 1e-12 {
            continue;
        }
        for row in 0..EQ_BANDS {
            if row != col {
                let f = m[row][col] / m[col][col];
                for k in col..EQ_BANDS {
                    m[row][k] -= f * m[col][k];
                }
                b[row] -= f * b[col];
            }
        }
    }
    std::array::from_fn(|i| if m[i][i].abs() < 1e-12 { 0.0 } else { b[i] / m[i][i] })
}

/// Headphone crossfeed strengths: (cut-off in Hz, how much of the other side is fed in, in dB).
/// These are the classic Bauer stereophonic-to-binaural settings from bs2b.
const CROSSFEED: [(f64, f64); 3] = [(700.0, 4.5), (700.0, 6.0), (650.0, 9.5)];

/// Settings shared with the callback.
pub struct DspParams {
    /// Even while stable, odd while a writer is changing the values below.
    version: AtomicU64,
    filters: [AtomicU32; EQ_BANDS],
    preamp: AtomicU32,
    crossfeed: AtomicU32,
    mono: AtomicBool,
    writer: Mutex<()>,
}

/// One consistent reading of [`DspParams`].
#[derive(Clone, Copy, Debug, PartialEq)]
struct Snapshot {
    filters: [f64; EQ_BANDS],
    preamp: f64,
    crossfeed: u32,
    mono: bool,
}

impl Default for DspParams {
    fn default() -> Self {
        DspParams {
            version: AtomicU64::new(0),
            filters: std::array::from_fn(|_| AtomicU32::new(0f32.to_bits())),
            preamp: AtomicU32::new(0f32.to_bits()),
            crossfeed: AtomicU32::new(0),
            mono: AtomicBool::new(false),
            writer: Mutex::new(()),
        }
    }
}

impl DspParams {
    /// The equalizer to use, or `None` to switch it off.
    pub fn set_eq(&self, eq: Option<&EqDesign>) {
        let off = EqDesign { filters: [0.0; EQ_BANDS], preamp: 0.0 };
        let eq = eq.unwrap_or(&off);
        self.write(|p| {
            for (slot, db) in p.filters.iter().zip(eq.filters) {
                slot.store((db as f32).to_bits(), Ordering::Relaxed);
            }
            p.preamp.store((eq.preamp as f32).to_bits(), Ordering::Relaxed);
        });
    }

    /// 0 = off, 1–3 = light, medium, strong.
    pub fn set_crossfeed(&self, level: u32) {
        self.write(|p| p.crossfeed.store(level.min(CROSSFEED.len() as u32), Ordering::Relaxed));
    }

    /// Mono: left and right mixed together and played on both sides (crossfeed then has nothing
    /// to do, so it stays off).
    pub fn set_mono(&self, on: bool) {
        self.write(|p| p.mono.store(on, Ordering::Relaxed));
    }

    fn write(&self, change: impl FnOnce(&Self)) {
        let _one_writer = self.writer.lock();
        let v = self.version.load(Ordering::Relaxed);
        self.version.store(v + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        change(self);
        self.version.store(v + 2, Ordering::Release);
    }

    /// The values, if they changed since version `seen` and no writer is busy (else try later).
    fn read(&self, seen: u64) -> Option<(u64, Snapshot)> {
        let v1 = self.version.load(Ordering::Acquire);
        if v1 == seen || v1 % 2 == 1 {
            return None;
        }
        let snapshot = Snapshot {
            filters: std::array::from_fn(|i| f64::from(f32::from_bits(self.filters[i].load(Ordering::Relaxed)))),
            preamp: f64::from(f32::from_bits(self.preamp.load(Ordering::Relaxed))),
            crossfeed: self.crossfeed.load(Ordering::Relaxed),
            mono: self.mono.load(Ordering::Relaxed),
        };
        fence(Ordering::Acquire);
        (self.version.load(Ordering::Relaxed) == v1).then_some((v1, snapshot))
    }
}

/// Frames between steps of the glide towards new settings.
const STEP_FRAMES: u32 = 32;
/// Most channels the equalizer handles (more and it stays off).
pub const MAX_CHANNELS: usize = 16;

/// Bauer crossfeed for one strength: each side gets a low-passed copy of the other, and its own
/// highs are lifted to keep the overall balance. A design by Boris Mikhaylov (bs2b).
#[derive(Clone, Copy, Debug, Default)]
struct Crossfeed {
    lo_a0: f64,
    lo_b1: f64,
    hi_a0: f64,
    hi_a1: f64,
    hi_b1: f64,
    gain: f64,
}

impl Crossfeed {
    fn new(level: u32, rate: f64) -> Crossfeed {
        let (cut, feed) = CROSSFEED[(level.max(1) as usize - 1).min(CROSSFEED.len() - 1)];
        let lo_db = feed * -5.0 / 6.0 - 3.0;
        let hi_db = feed / 6.0 - 3.0;
        let lo_gain = 10f64.powf(lo_db / 20.0);
        let hi_gain = 1.0 - 10f64.powf(hi_db / 20.0);
        let hi_cut = cut * 2f64.powf((lo_db - 20.0 * hi_gain.log10()) / 12.0);
        let x_lo = (-std::f64::consts::TAU * cut / rate).exp();
        let x_hi = (-std::f64::consts::TAU * hi_cut / rate).exp();
        Crossfeed {
            lo_a0: lo_gain * (1.0 - x_lo),
            lo_b1: x_lo,
            hi_a0: 1.0 - hi_gain * (1.0 - x_hi),
            hi_a1: -x_hi,
            hi_b1: x_hi,
            gain: 1.0 / (1.0 - hi_gain + lo_gain),
        }
    }
}

/// The callback's sound shaping. Built with the stream (allocates there), then only processes.
pub struct Dsp {
    rate: f64,
    channels: usize,
    seen: u64,
    // Equalizer: filter gains now and where they're heading, in dB.
    eq_now: [f64; EQ_BANDS],
    eq_target: [f64; EQ_BANDS],
    pre_now: f64,
    pre_target: f64,
    pre_gain: f64,
    filters: [Biquad; EQ_BANDS],
    /// Bands whose filter does something (a band at 0 dB is skipped).
    live: [bool; EQ_BANDS],
    /// Transposed direct form II state, per channel and band.
    state: Vec<[[f64; 2]; EQ_BANDS]>,
    eq_on: bool,
    // Crossfeed: the strength wanted (0 = off) and the one the filters are built for; how much of
    // the processed signal is heard (0–1) glides, and a new strength fades out the old one first.
    feed: Crossfeed,
    feed_want: u32,
    feed_level: u32,
    feed_mix: f64,
    lo: [f64; 2],
    hi: [f64; 2],
    last_in: [f64; 2],
    // Mono: wanted, and how far the front left and right are mixed together (0–1, glides).
    mono_want: bool,
    mono_mix: f64,
    countdown: u32,
}

impl Dsp {
    pub fn new(channels: usize, rate: u32) -> Dsp {
        Dsp {
            rate: f64::from(rate.max(8000)),
            channels,
            seen: u64::MAX,
            eq_now: [0.0; EQ_BANDS],
            eq_target: [0.0; EQ_BANDS],
            pre_now: 0.0,
            pre_target: 0.0,
            pre_gain: 1.0,
            filters: [Biquad::IDENTITY; EQ_BANDS],
            live: [false; EQ_BANDS],
            state: vec![[[0.0; 2]; EQ_BANDS]; channels.min(MAX_CHANNELS)],
            eq_on: false,
            feed: Crossfeed::default(),
            feed_want: 0,
            feed_level: 0,
            feed_mix: 0.0,
            lo: [0.0; 2],
            hi: [0.0; 2],
            last_in: [0.0; 2],
            mono_want: false,
            mono_mix: 0.0,
            countdown: 0,
        }
    }

    /// Pick up new settings. Call once per callback.
    pub fn sync(&mut self, params: &DspParams) {
        let Some((version, s)) = params.read(self.seen) else { return };
        self.seen = version;
        if self.channels <= MAX_CHANNELS {
            self.eq_target = s.filters;
            self.pre_target = s.preamp;
        }
        if self.channels == 2 {
            self.feed_want = if s.mono { 0 } else { s.crossfeed };
        }
        if self.channels >= 2 {
            self.mono_want = s.mono;
        }
    }

    /// Nothing to do: the samples pass through as they are.
    pub fn idle(&self) -> bool {
        !self.eq_on && self.eq_target.iter().all(|g| *g == 0.0) && self.pre_target == 0.0
            && self.feed_mix == 0.0 && self.feed_want == 0
            && self.mono_mix == 0.0 && !self.mono_want
    }

    /// Move the settings a step towards their targets.
    fn step(&mut self) {
        const DB_STEP: f64 = 0.75; // per 32 frames: 12 dB in about 11 ms at 48 kHz
        let mut moved = false;
        for i in 0..EQ_BANDS {
            let (now, target) = (self.eq_now[i], self.eq_target[i]);
            if now != target {
                self.eq_now[i] = if (target - now).abs() <= DB_STEP { target } else { now + DB_STEP.copysign(target - now) };
                self.filters[i] = Biquad::peaking(EQ_FREQS[i], EQ_Q, self.eq_now[i], self.rate);
                self.live[i] = self.filters[i] != Biquad::IDENTITY;
                if !self.live[i] {
                    for ch in &mut self.state {
                        ch[i] = [0.0; 2];
                    }
                }
                moved = true;
            }
        }
        if self.pre_now != self.pre_target {
            let d = self.pre_target - self.pre_now;
            self.pre_now = if d.abs() <= DB_STEP { self.pre_target } else { self.pre_now + DB_STEP.copysign(d) };
            self.pre_gain = 10f64.powf(self.pre_now / 20.0);
            moved = true;
        }
        let active = self.eq_now.iter().any(|g| *g != 0.0) || self.pre_now != 0.0;
        if active && !self.eq_on {
            // Coming out of bypass: start from silence, as a filter that was never fed.
            for ch in &mut self.state {
                *ch = [[0.0; 2]; EQ_BANDS];
            }
        }
        self.eq_on = active;
        if moved || self.eq_on {
            // Flush values too small to matter, so silence never turns into slow denormals.
            for ch in &mut self.state {
                for s in ch.iter_mut().flatten() {
                    if s.abs() < 1e-25 {
                        *s = 0.0;
                    }
                }
            }
        }
        // Crossfeed: glide in, out, or (for a new strength) out and back in with the new filters.
        if self.feed_mix == 0.0 && self.feed_want != self.feed_level {
            self.feed_level = self.feed_want;
            if self.feed_level > 0 {
                self.feed = Crossfeed::new(self.feed_level, self.rate);
                self.lo = [0.0; 2];
                self.hi = [0.0; 2];
                self.last_in = [0.0; 2];
            }
        }
        let target = if self.feed_level > 0 && self.feed_want == self.feed_level { 1.0 } else { 0.0 };
        self.feed_mix = glide(self.feed_mix, target, self.rate);
        self.mono_mix = glide(self.mono_mix, if self.mono_want { 1.0 } else { 0.0 }, self.rate);
        if self.feed_mix > 0.0 {
            for s in self.lo.iter_mut().chain(self.hi.iter_mut()).chain(self.last_in.iter_mut()) {
                if s.abs() < 1e-25 {
                    *s = 0.0;
                }
            }
        }
    }

    /// Process one interleaved frame in place.
    #[inline]
    pub fn process(&mut self, frame: &mut [f32]) {
        if self.countdown == 0 {
            self.countdown = STEP_FRAMES;
            self.step();
        }
        self.countdown -= 1;
        if self.eq_on {
            for (c, sample) in frame.iter_mut().enumerate().take(self.state.len()) {
                let mut x = f64::from(*sample) * self.pre_gain;
                for ((f, s), live) in self.filters.iter().zip(self.state[c].iter_mut()).zip(self.live) {
                    if !live {
                        continue;
                    }
                    let y = f.b0 * x + s[0];
                    s[0] = f.b1 * x - f.a1 * y + s[1];
                    s[1] = f.b2 * x - f.a2 * y;
                    x = y;
                }
                *sample = x as f32;
            }
        }
        if self.feed_mix > 0.0 && self.feed_level > 0 && frame.len() == 2 {
            let (l, r) = (f64::from(frame[0]), f64::from(frame[1]));
            let x = &self.feed;
            self.lo[0] = x.lo_a0 * l + x.lo_b1 * self.lo[0];
            self.lo[1] = x.lo_a0 * r + x.lo_b1 * self.lo[1];
            self.hi[0] = x.hi_a0 * l + x.hi_a1 * self.last_in[0] + x.hi_b1 * self.hi[0];
            self.hi[1] = x.hi_a0 * r + x.hi_a1 * self.last_in[1] + x.hi_b1 * self.hi[1];
            self.last_in = [l, r];
            let wet_l = (self.hi[0] + self.lo[1]) * x.gain;
            let wet_r = (self.hi[1] + self.lo[0]) * x.gain;
            let m = self.feed_mix;
            frame[0] = (l + (wet_l - l) * m) as f32;
            frame[1] = (r + (wet_r - r) * m) as f32;
        }
        // Mono goes last, so both sides end up exactly the same.
        if self.mono_mix > 0.0 && frame.len() >= 2 {
            let (l, r) = (f64::from(frame[0]), f64::from(frame[1]));
            let mid = (l + r) * 0.5;
            if self.mono_mix >= 1.0 {
                frame[0] = mid as f32;
                frame[1] = mid as f32;
            } else {
                let m = self.mono_mix;
                frame[0] = (l + (mid - l) * m) as f32;
                frame[1] = (r + (mid - r) * m) as f32;
            }
        }
    }
}

/// One step of a 20 ms glide from `now` towards `target` (both 0–1).
fn glide(now: f64, target: f64, rate: f64) -> f64 {
    let d = f64::from(STEP_FRAMES) / (0.02 * rate);
    if target > now { (now + d).min(target) } else { (now - d).max(target) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn design(bands: [f32; EQ_BANDS]) -> EqDesign {
        EqDesign::new(&bands)
    }

    #[test]
    fn curve_passes_through_the_band_settings() {
        let cases: [[f32; EQ_BANDS]; 5] = [
            [0.0, 0.0, 0.0, 0.0, 12.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [5.0, 4.0, 2.0, 0.0, -2.0, -2.0, 0.0, 2.0, 4.0, 5.0],
            [6.0; EQ_BANDS],
            [0.0, 0.0, 0.0, 0.0, 0.0, 12.0, -12.0, 0.0, 0.0, 0.0],
        ];
        for bands in cases {
            let d = design(bands);
            for (i, f) in EQ_FREQS.iter().enumerate() {
                let got = d.curve_db(*f, DESIGN_RATE);
                assert!((got - f64::from(bands[i])).abs() < 0.1, "{bands:?}: band {i} is {got:.2} dB");
            }
        }
    }

    #[test]
    fn preamp_makes_room_for_the_biggest_boost() {
        assert_eq!(design([0.0; EQ_BANDS]), EqDesign { filters: [0.0; EQ_BANDS], preamp: 0.0 });
        let cut = design([-6.0; EQ_BANDS]);
        assert!(cut.preamp.abs() < 1e-9, "cuts need no room: {}", cut.preamp);
        let boost = design([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 6.0, 0.0, 0.0]);
        assert!((boost.preamp + 6.0).abs() < 0.3, "{}", boost.preamp);
    }

    /// Run a sine through the callback-side equalizer and measure its level (RMS, after settling).
    fn level_db(dsp: &mut Dsp, freq: f64, rate: f64) -> f64 {
        let n = rate as usize;
        let (mut sum, mut count) = (0.0f64, 0usize);
        for i in 0..n {
            let v = (std::f64::consts::TAU * freq * i as f64 / rate).sin() as f32 * 0.25;
            let mut frame = [v, v];
            dsp.process(&mut frame);
            if i >= n / 4 {
                sum += f64::from(frame[0]).powi(2);
                count += 1;
            }
        }
        let amplitude = (2.0 * sum / count as f64).sqrt();
        20.0 * (amplitude / 0.25).log10()
    }

    #[test]
    fn callback_filters_match_the_curve() {
        let params = DspParams::default();
        let d = design([0.0, 6.0, 0.0, 0.0, -4.0, 0.0, 0.0, 0.0, 3.0, 0.0]);
        params.set_eq(Some(&d));
        for rate in [44_100.0, 48_000.0, 96_000.0] {
            for f in [64.0, 500.0, 1000.0, 8000.0] {
                let mut dsp = Dsp::new(2, rate as u32);
                dsp.sync(&params);
                let got = level_db(&mut dsp, f, rate);
                let want = d.curve_db(f, rate) + d.preamp;
                assert!((got - want).abs() < 0.2, "{f} Hz at {rate}: {got:.2} dB, curve {want:.2}");
            }
        }
    }

    #[test]
    fn off_means_untouched() {
        let params = DspParams::default();
        let mut dsp = Dsp::new(2, 48_000);
        params.set_eq(Some(&design([3.0; EQ_BANDS])));
        params.set_crossfeed(2);
        params.set_mono(true);
        dsp.sync(&params);
        for _ in 0..4800 {
            dsp.process(&mut [0.3, -0.2]);
        }
        params.set_eq(None);
        params.set_crossfeed(0);
        params.set_mono(false);
        dsp.sync(&params);
        for _ in 0..4800 {
            dsp.process(&mut [0.3, -0.2]);
        }
        assert!(dsp.idle());
        let mut frame = [0.123_456_7f32, -0.765_432_1];
        dsp.process(&mut frame);
        assert_eq!(frame, [0.123_456_7, -0.765_432_1]);
    }

    #[test]
    fn crossfeed_mixes_the_sides_and_keeps_mono_level() {
        let params = DspParams::default();
        params.set_crossfeed(3);
        let mut dsp = Dsp::new(2, 48_000);
        dsp.sync(&params);
        // A low tone on the left only: some of it reaches the right.
        let mut right = 0.0f32;
        for i in 0..48_000 {
            let v = (std::f64::consts::TAU * 200.0 * f64::from(i) / 48_000.0).sin() as f32 * 0.5;
            let mut frame = [v, 0.0];
            dsp.process(&mut frame);
            if i > 24_000 {
                right = right.max(frame[1].abs());
            }
        }
        assert!(right > 0.08, "{right}");
        // The same tone on both sides stays at about the same level.
        let mut dsp = Dsp::new(2, 48_000);
        dsp.sync(&params);
        let mut peak = 0.0f32;
        for i in 0..48_000 {
            let v = (std::f64::consts::TAU * 200.0 * f64::from(i) / 48_000.0).sin() as f32 * 0.5;
            let mut frame = [v, v];
            dsp.process(&mut frame);
            if i > 24_000 {
                peak = peak.max(frame[0].abs());
            }
        }
        assert!((0.4..0.6).contains(&peak), "{peak}");
    }

    #[test]
    fn mono_plays_the_same_on_both_sides_and_glides_there() {
        let params = DspParams::default();
        params.set_crossfeed(2);
        params.set_mono(true);
        let mut dsp = Dsp::new(2, 48_000);
        dsp.sync(&params);
        let tone = |i: usize| (std::f64::consts::TAU * 300.0 * i as f64 / 48_000.0).sin() as f32 * 0.5;
        let (mut last, mut biggest_step) = ([0.0f32; 2], 0.0f32);
        for i in 0..48_000 {
            if i == 24_000 {
                params.set_mono(false);
                dsp.sync(&params);
            }
            // A tone on the left only, with a constant offset on the right.
            let mut frame = [tone(i), 0.25];
            dsp.process(&mut frame);
            if i > 960 && i < 24_000 {
                assert_eq!(frame[0], frame[1], "frame {i}: both sides the same");
                assert!((frame[0] - (tone(i) + 0.25) / 2.0).abs() < 1e-6, "frame {i}: half of each side");
            }
            if i > 0 {
                biggest_step = biggest_step.max((frame[0] - last[0]).abs()).max((frame[1] - last[1]).abs());
            }
            last = frame;
        }
        // Turning mono off glides back to stereo (crossfeed included) without a jump.
        assert!(biggest_step < 0.03, "{biggest_step}");
        assert_eq!(dsp.mono_mix, 0.0);
        assert_eq!(dsp.feed_mix, 1.0, "crossfeed is back once mono is off");
        // A mono stream (one channel) has nothing to mix.
        let mut dsp = Dsp::new(1, 48_000);
        dsp.sync(&params);
        params.set_crossfeed(0);
        params.set_mono(true);
        dsp.sync(&params);
        let mut frame = [0.3f32];
        dsp.process(&mut frame);
        assert_eq!(frame, [0.3]);
        assert!(dsp.idle());
    }

    #[test]
    fn torn_updates_are_never_used() {
        let params = DspParams::default();
        let (v, s) = params.read(u64::MAX).expect("initial values");
        assert_eq!(s.crossfeed, 0);
        assert!(params.read(v).is_none(), "nothing new");
        params.set_crossfeed(1);
        let (v2, s) = params.read(v).expect("changed");
        assert!(v2 > v);
        assert_eq!(s.crossfeed, 1);
        // A writer in the middle of an update: readers wait.
        params.version.fetch_add(1, Ordering::Relaxed);
        assert!(params.read(v2).is_none());
    }

    #[test]
    fn crossfeed_strength_changes_glide_and_silence_stays_cheap() {
        let params = DspParams::default();
        params.set_crossfeed(1);
        let mut dsp = Dsp::new(2, 48_000);
        dsp.sync(&params);
        let tone = |i: usize| (std::f64::consts::TAU * 300.0 * i as f64 / 48_000.0).sin() as f32 * 0.5;
        let mut last = 0.0f32;
        let mut biggest_step = 0.0f32;
        for i in 0..48_000 {
            if i == 24_000 {
                params.set_crossfeed(3);
                dsp.sync(&params);
            }
            let mut frame = [tone(i), 0.0];
            dsp.process(&mut frame);
            if i > 4800 {
                biggest_step = biggest_step.max((frame[1] - last).abs());
            }
            last = frame[1];
        }
        // A 300 Hz tone at this level moves at most ~0.02 between samples: no jump when the
        // strength changed.
        assert!(biggest_step < 0.03, "{biggest_step}");
        assert_eq!(dsp.feed_level, 3);
        // Digital silence: the filters settle at zero rather than in slow subnormal numbers.
        for _ in 0..96_000 {
            dsp.process(&mut [0.0, 0.0]);
        }
        assert!(dsp.lo.iter().chain(&dsp.hi).chain(&dsp.last_in).all(|s| *s == 0.0 || s.is_normal()));
    }
}
