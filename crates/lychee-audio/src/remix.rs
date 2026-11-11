//! Channel layout conversion.

/// Convert interleaved `input` with `in_ch` channels to `out_ch` channels, appending to `out`.
///
/// Mono is copied to every output channel. Surround is folded down to stereo with the usual
/// -3 dB coefficients for centre and surrounds (LFE dropped), normalised so it can't clip.
/// Assumes Symphonia's canonical channel order (FL, FR, FC, LFE, BL, BR, …).
pub fn remix(input: &[f32], in_ch: usize, out_ch: usize, out: &mut Vec<f32>) {
    if in_ch == out_ch {
        out.extend_from_slice(input);
        return;
    }
    let frames = input.len() / in_ch.max(1);
    out.reserve(frames * out_ch);
    for frame in input.chunks_exact(in_ch) {
        match (in_ch, out_ch) {
            (1, _) => out.extend(std::iter::repeat_n(frame[0], out_ch)),
            (_, 1) => out.push(frame.iter().sum::<f32>() / in_ch as f32),
            (_, 2) => {
                let (l, r) = fold_to_stereo(frame);
                out.push(l);
                out.push(r);
            }
            _ => out.extend((0..out_ch).map(|c| frame.get(c).copied().unwrap_or(0.0))),
        }
    }
}

fn fold_to_stereo(f: &[f32]) -> (f32, f32) {
    const C: f32 = std::f32::consts::FRAC_1_SQRT_2;
    match f.len() {
        2 => (f[0], f[1]),
        // FL FR FC
        3 => ((f[0] + C * f[2]) / (1.0 + C), (f[1] + C * f[2]) / (1.0 + C)),
        // FL FR BL BR
        4 => ((f[0] + C * f[2]) / (1.0 + C), (f[1] + C * f[3]) / (1.0 + C)),
        // FL FR FC BL BR
        5 => {
            let n = 1.0 + 2.0 * C;
            ((f[0] + C * f[2] + C * f[3]) / n, (f[1] + C * f[2] + C * f[4]) / n)
        }
        // FL FR FC LFE BL BR (5.1) and wider layouts: use the first six.
        _ => {
            let n = 1.0 + 2.0 * C;
            ((f[0] + C * f[2] + C * f[4]) / n, (f[1] + C * f[2] + C * f[5]) / n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_to_stereo_duplicates() {
        let mut out = Vec::new();
        remix(&[0.5, -0.25], 1, 2, &mut out);
        assert_eq!(out, vec![0.5, 0.5, -0.25, -0.25]);
    }

    #[test]
    fn surround_fold_down_never_clips() {
        let mut out = Vec::new();
        remix(&[1.0; 6], 6, 2, &mut out);
        assert!(out.iter().all(|s| s.abs() <= 1.0 + 1e-6), "{out:?}");
    }

    #[test]
    fn stereo_passthrough() {
        let mut out = Vec::new();
        remix(&[0.1, 0.2], 2, 2, &mut out);
        assert_eq!(out, vec![0.1, 0.2]);
    }
}
