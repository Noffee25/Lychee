//! Streaming sample-rate conversion with an exact output length.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};

use crate::AudioError;

const CHUNK: usize = 1024;

/// Wraps rubato's FFT resampler for a stream fed in arbitrary-sized pieces.
///
/// The resampler's start-up delay is trimmed and the tail is flushed, so a track resampled from
/// N input frames yields `round(N * to / from)` output frames. That keeps gapless transitions
/// seamless and the reported position accurate.
pub struct StreamResampler {
    inner: Fft<f32>,
    channels: usize,
    ratio: f64,
    pending: Vec<f32>,
    out_buf: Vec<f32>,
    delay_left: usize,
    frames_in: u64,
    frames_out: u64,
}

impl StreamResampler {
    pub fn new(from: u32, to: u32, channels: usize) -> Result<Self, AudioError> {
        let inner = Fft::<f32>::new(from as usize, to as usize, CHUNK, channels, FixedSync::Input)
            .map_err(|e| AudioError::Decode(format!("resampler: {e}")))?;
        let out_buf = vec![0.0; inner.output_frames_max() * channels];
        let delay_left = inner.output_delay();
        Ok(Self {
            inner,
            channels,
            ratio: f64::from(to) / f64::from(from),
            pending: Vec::with_capacity(CHUNK * channels * 2),
            out_buf,
            delay_left,
            frames_in: 0,
            frames_out: 0,
        })
    }

    /// Feed interleaved input; resampled output is appended to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), AudioError> {
        self.frames_in += (input.len() / self.channels) as u64;
        self.pending.extend_from_slice(input);
        loop {
            let need = self.inner.input_frames_next();
            if self.pending.len() < need * self.channels {
                return Ok(());
            }
            let (read, written) = self.run_chunk(need, None)?;
            self.pending.drain(..read * self.channels);
            self.emit(written, out, u64::MAX);
        }
    }

    /// Flush everything still buffered at the end of a stream.
    pub fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), AudioError> {
        let target = (self.frames_in as f64 * self.ratio).round() as u64;
        // Process what is left as a partial chunk, then push silence until the delayed tail is out.
        let mut guard = 0;
        while self.frames_out < target && guard < 64 {
            guard += 1;
            let need = self.inner.input_frames_next();
            let have = (self.pending.len() / self.channels).min(need);
            self.pending.resize(need * self.channels, 0.0);
            let indexing = Indexing { input_offset: 0, output_offset: 0, partial_len: Some(have), active_channels_mask: None };
            let (_, written) = self.run_chunk(need, Some(&indexing))?;
            self.pending.clear();
            self.emit(written, out, target);
        }
        Ok(())
    }

    fn run_chunk(&mut self, frames: usize, indexing: Option<&Indexing>) -> Result<(usize, usize), AudioError> {
        let input = InterleavedSlice::new(&self.pending[..frames * self.channels], self.channels, frames)
            .map_err(|e| AudioError::Decode(format!("resampler input: {e:?}")))?;
        let capacity = self.out_buf.len() / self.channels;
        let mut output = InterleavedSlice::new_mut(&mut self.out_buf, self.channels, capacity)
            .map_err(|e| AudioError::Decode(format!("resampler output: {e:?}")))?;
        self.inner
            .process_into_buffer(&input, &mut output, indexing)
            .map_err(|e| AudioError::Decode(format!("resampling failed: {e}")))
    }

    fn emit(&mut self, written: usize, out: &mut Vec<f32>, limit: u64) {
        let mut start = 0;
        if self.delay_left > 0 {
            start = self.delay_left.min(written);
            self.delay_left -= start;
        }
        let mut frames = written - start;
        if self.frames_out + frames as u64 > limit {
            frames = limit.saturating_sub(self.frames_out) as usize;
        }
        out.extend_from_slice(&self.out_buf[start * self.channels..(start + frames) * self.channels]);
        self.frames_out += frames as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_length_matches_ratio() {
        let mut r = StreamResampler::new(44_100, 48_000, 2).unwrap();
        let input = vec![0.25f32; 44_100 * 2]; // one second of stereo
        let mut out = Vec::new();
        for chunk in input.chunks(777 * 2) {
            r.process(chunk, &mut out).unwrap();
        }
        r.finish(&mut out).unwrap();
        assert_eq!(out.len() / 2, 48_000);
        // A DC signal stays DC (allowing for edge effects of the filter).
        let mid = &out[20_000..20_010];
        assert!(mid.iter().all(|s| (s - 0.25).abs() < 0.01), "{mid:?}");
    }
}
