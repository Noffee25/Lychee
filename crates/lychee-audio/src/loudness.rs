//! Loudness measurement as in ITU-R BS.1770-4 / EBU R128: integrated loudness (LUFS) and the
//! sample peak. Used for volume levelling of files that have no ReplayGain tags.
//!
//! The signal is K-weighted (a high shelf and a high-pass, the curve BS.1770 defines), cut into
//! 400 ms blocks overlapping by 75 %, and blocks below −70 LUFS or 10 LU under the average are
//! gated out, so silence and quiet passages don't drag the result down.

use std::path::Path;

use crate::{AudioError, Source};

/// Result of measuring one file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loudness {
    /// Integrated loudness in LUFS; `None` when the file is (nearly) silent.
    pub lufs: Option<f64>,
    /// Highest absolute sample value (1.0 = full scale).
    pub peak: f32,
}

/// Decode `path` completely and measure it.
pub fn measure(path: &Path) -> Result<Loudness, AudioError> {
    let mut source = Source::open(path)?;
    let mut meter = Meter::new(source.channels, source.sample_rate);
    let mut buf = Vec::with_capacity(16 * 1024);
    loop {
        buf.clear();
        if !source.decode_next(&mut buf)? {
            break;
        }
        meter.push(&buf);
    }
    Ok(meter.finish())
}

/// Integrated-loudness meter fed with interleaved `f32` samples.
pub struct Meter {
    channels: usize,
    weights: Vec<f64>,
    b: [f64; 5],
    a: [f64; 5],
    /// Filter state per channel (transposed direct form II).
    state: Vec<[f64; 4]>,
    /// Frames per 100 ms step.
    step_frames: usize,
    frames_in_step: usize,
    /// Weighted sum of squares of the current step.
    step_energy: f64,
    /// Mean-square energy of the last four steps (one 400 ms block).
    recent: [f64; 4],
    steps_seen: usize,
    /// Energy of every complete 400 ms block.
    blocks: Vec<f64>,
    peak: f32,
}

impl Meter {
    pub fn new(channels: usize, sample_rate: u32) -> Meter {
        let channels = channels.max(1);
        let (b, a) = k_weighting(f64::from(sample_rate.max(8000)));
        Meter {
            channels,
            weights: channel_weights(channels),
            b,
            a,
            state: vec![[0.0; 4]; channels],
            step_frames: (sample_rate as usize / 10).max(1),
            frames_in_step: 0,
            step_energy: 0.0,
            recent: [0.0; 4],
            steps_seen: 0,
            blocks: Vec::new(),
            peak: 0.0,
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        let (b, a) = (self.b, self.a);
        for frame in samples.chunks_exact(self.channels) {
            let mut energy = 0.0;
            for (c, &x) in frame.iter().enumerate() {
                self.peak = self.peak.max(x.abs());
                let x = f64::from(x);
                let z = &mut self.state[c];
                let y = b[0] * x + z[0];
                z[0] = b[1] * x - a[1] * y + z[1];
                z[1] = b[2] * x - a[2] * y + z[2];
                z[2] = b[3] * x - a[3] * y + z[3];
                z[3] = b[4] * x - a[4] * y;
                energy += self.weights[c] * y * y;
            }
            self.step_energy += energy;
            self.frames_in_step += 1;
            if self.frames_in_step == self.step_frames {
                self.end_step();
            }
        }
    }

    fn end_step(&mut self) {
        self.recent.rotate_left(1);
        self.recent[3] = self.step_energy / self.step_frames as f64;
        self.step_energy = 0.0;
        self.frames_in_step = 0;
        self.steps_seen += 1;
        if self.steps_seen >= 4 {
            self.blocks.push(self.recent.iter().sum::<f64>() / 4.0);
        }
        // Long silences would otherwise decay the filter state into slow denormal numbers.
        for z in self.state.iter_mut().flatten() {
            if z.abs() < 1e-30 {
                *z = 0.0;
            }
        }
    }

    pub fn finish(self) -> Loudness {
        Loudness { lufs: gated_loudness(&self.blocks), peak: self.peak }
    }
}

fn loudness_of(energy: f64) -> f64 {
    -0.691 + 10.0 * energy.log10()
}

/// BS.1770's two-stage gate over the block energies.
fn gated_loudness(blocks: &[f64]) -> Option<f64> {
    let absolute = 10f64.powf((-70.0 + 0.691) / 10.0);
    let above: Vec<f64> = blocks.iter().copied().filter(|&e| e > absolute).collect();
    if above.is_empty() {
        return None;
    }
    let relative = above.iter().sum::<f64>() / above.len() as f64 * 0.1; // −10 LU
    let (sum, n) = above.iter().filter(|&&e| e > relative).fold((0.0, 0usize), |(s, n), &e| (s + e, n + 1));
    (n > 0).then(|| loudness_of(sum / n as f64))
}

/// Channel weights for the usual layouts (FLAC/WAV order: L R C LFE Ls Rs …). Mono counts twice,
/// because it plays from both speakers (as rsgain and foobar2000 do); the LFE doesn't count.
fn channel_weights(channels: usize) -> Vec<f64> {
    match channels {
        1 => vec![2.0],
        4 => vec![1.0, 1.0, 1.41, 1.41],
        5 => vec![1.0, 1.0, 1.0, 1.41, 1.41],
        n if n >= 6 => (0..n).map(|c| match c { 3 => 0.0, 0..=2 => 1.0, _ => 1.41 }).collect(),
        n => vec![1.0; n],
    }
}

/// The K-weighting filter (high shelf, then high-pass) for any sample rate, as one 4th-order
/// section. The analog prototypes are those libebur128 derives from BS.1770's 48 kHz coefficients.
fn k_weighting(rate: f64) -> ([f64; 5], [f64; 5]) {
    use std::f64::consts::PI;
    // High shelf: +4 dB above ~1.7 kHz (the effect of the head).
    let f0 = 1681.974450955533;
    let gain_db = 3.999843853973347;
    let q = 0.7071752369554196;
    let k = (PI * f0 / rate).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let pb = [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0];
    let pa = [1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0];
    // High-pass at ~38 Hz (RLB weighting).
    let f0 = 38.13547087602444;
    let q = 0.5003270373238773;
    let k = (PI * f0 / rate).tan();
    let rb = [1.0, -2.0, 1.0];
    let d = 1.0 + k / q + k * k;
    let ra = [1.0, 2.0 * (k * k - 1.0) / d, (1.0 - k / q + k * k) / d];
    (convolve(pb, rb), convolve(pa, ra))
}

fn convolve(x: [f64; 3], y: [f64; 3]) -> [f64; 5] {
    let mut out = [0.0; 5];
    for (i, xi) in x.iter().enumerate() {
        for (j, yj) in y.iter().enumerate() {
            out[i + j] += xi * yj;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, channels: usize, freq: f64, amplitude: f64, seconds: f64) -> Vec<f32> {
        let frames = (f64::from(rate) * seconds) as usize;
        let mut out = Vec::with_capacity(frames * channels);
        for n in 0..frames {
            let v = (amplitude * (2.0 * std::f64::consts::PI * freq * n as f64 / f64::from(rate)).sin()) as f32;
            out.extend(std::iter::repeat_n(v, channels));
        }
        out
    }

    fn lufs(rate: u32, channels: usize, samples: &[f32]) -> Option<f64> {
        let mut m = Meter::new(channels, rate);
        m.push(samples);
        m.finish().lufs
    }

    #[test]
    fn ebu_tech_3341_reference_levels() {
        // EBU Tech 3341 cases 1 and 2: a stereo 1 kHz sine at −23 / −33 dBFS reads −23 / −33 LUFS.
        for rate in [44_100, 48_000, 96_000] {
            for dbfs in [-23.0, -33.0] {
                let s = sine(rate, 2, 1000.0, 10f64.powf(dbfs / 20.0), 20.0);
                let l = lufs(rate, 2, &s).unwrap();
                assert!((l - dbfs).abs() < 0.1, "{rate} Hz, {dbfs} dBFS: {l}");
            }
        }
    }

    #[test]
    fn gates_silence_and_quiet_passages() {
        let rate = 48_000;
        let loud = sine(rate, 2, 1000.0, 10f64.powf(-20.0 / 20.0), 10.0);
        let mut with_silence = loud.clone();
        with_silence.extend(std::iter::repeat_n(0.0, rate as usize * 2 * 20));
        let a = lufs(rate, 2, &loud).unwrap();
        let b = lufs(rate, 2, &with_silence).unwrap();
        assert!((a - b).abs() < 0.1, "{a} vs {b}");
        assert_eq!(lufs(rate, 2, &vec![0.0; 48_000 * 2 * 3]), None);
    }

    #[test]
    fn mono_counts_as_both_speakers() {
        let rate = 48_000;
        let a = 10f64.powf(-23.0 / 20.0);
        let mono = lufs(rate, 1, &sine(rate, 1, 1000.0, a, 10.0)).unwrap();
        let stereo = lufs(rate, 2, &sine(rate, 2, 1000.0, a, 10.0)).unwrap();
        assert!((mono - stereo).abs() < 0.01, "{mono} vs {stereo}");
    }

    #[test]
    fn tracks_the_sample_peak() {
        let mut m = Meter::new(2, 48_000);
        m.push(&[0.25, -0.75, 0.5, 0.1]);
        assert_eq!(m.finish().peak, 0.75);
    }
}
