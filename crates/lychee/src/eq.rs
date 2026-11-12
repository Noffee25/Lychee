//! Equalizer presets and the curve drawn in Settings → Audio.

use lychee_audio::{EQ_BANDS, EQ_FREQS, EQ_RANGE_DB, EqDesign};

/// A named curve, in dB per band (32 Hz … 16 kHz).
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub bands: [f32; EQ_BANDS],
}

/// The user's own curve: what the bands are set to after dragging one.
pub const CUSTOM: &str = "custom";

pub const PRESETS: &[Preset] = &[
    Preset { id: "flat", name: "Flat", bands: [0.0; EQ_BANDS] },
    Preset { id: "bass-boost", name: "Bass boost", bands: [6.0, 5.5, 4.5, 2.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0] },
    Preset { id: "bass-cut", name: "Less bass", bands: [-6.0, -5.0, -3.5, -2.0, -0.5, 0.0, 0.0, 0.0, 0.0, 0.0] },
    Preset { id: "treble-boost", name: "Treble boost", bands: [0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 2.0, 3.5, 5.0, 6.0] },
    Preset { id: "treble-cut", name: "Less treble", bands: [0.0, 0.0, 0.0, 0.0, 0.0, -0.5, -2.0, -3.5, -5.0, -6.0] },
    Preset { id: "loudness", name: "Loudness", bands: [5.0, 4.0, 1.5, 0.0, -1.0, 0.0, 0.0, 1.0, 3.5, 4.5] },
    Preset { id: "vocal", name: "Vocal", bands: [-2.0, -1.5, -1.0, 0.5, 2.0, 3.0, 3.0, 2.0, 0.5, -0.5] },
    Preset { id: "spoken", name: "Spoken word", bands: [-4.0, -2.5, -1.0, 0.5, 2.5, 3.5, 3.5, 2.5, 1.0, -1.0] },
    Preset { id: "acoustic", name: "Acoustic", bands: [3.5, 3.0, 2.5, 1.0, 1.5, 1.5, 2.5, 3.0, 2.5, 1.5] },
    Preset { id: "classical", name: "Classical", bands: [3.5, 3.0, 2.0, 1.0, -0.5, -0.5, 0.0, 1.5, 2.5, 3.0] },
    Preset { id: "jazz", name: "Jazz", bands: [3.0, 2.0, 1.0, 1.5, -1.0, -1.0, 0.0, 1.0, 2.0, 3.0] },
    Preset { id: "pop", name: "Pop", bands: [-1.0, 0.0, 1.5, 3.0, 3.5, 2.5, 1.0, 0.0, -0.5, -1.0] },
    Preset { id: "rock", name: "Rock", bands: [4.5, 3.5, 2.0, 0.5, -1.0, -1.0, 0.5, 2.0, 3.0, 3.5] },
    Preset { id: "electronic", name: "Electronic", bands: [4.5, 4.0, 1.5, 0.0, -1.5, 1.0, 0.5, 1.0, 3.5, 4.5] },
    Preset { id: "hip-hop", name: "Hip-hop", bands: [5.0, 4.5, 2.0, 3.0, -1.0, -1.0, 1.0, -0.5, 1.5, 2.5] },
    Preset { id: "small-speakers", name: "Small speakers", bands: [5.0, 4.0, 3.0, 1.5, 0.0, -1.0, -1.0, 0.0, 1.0, 1.5] },
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Band labels under the graph.
pub const LABELS: [&str; EQ_BANDS] = ["32", "64", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];

/// The graph spans ten octaves, half an octave beyond the outer bands on either side.
const LOW_HZ: f64 = 32.0 / std::f64::consts::SQRT_2;

/// Where `freq` sits across the graph (0 = left edge, 1 = right edge).
pub fn x_of(freq: f64) -> f64 {
    (freq / LOW_HZ).log2() / 10.0
}

/// Round a dragged band to half a dB, within range.
pub fn snap(db: f32) -> f32 {
    ((db * 2.0).round() / 2.0).clamp(-EQ_RANGE_DB, EQ_RANGE_DB)
}

/// The curve as SVG path commands in a 1000 × 1000 box (+12 dB at the top, −12 at the bottom),
/// and the same curve closed along the 0 dB line, for the tinted area under it.
pub fn paths(design: &EqDesign, rate: u32) -> (String, String) {
    use std::fmt::Write;
    let rate = f64::from(rate.max(8000));
    let range = f64::from(EQ_RANGE_DB);
    let mut line = String::with_capacity(4096);
    const STEPS: usize = 160;
    for k in 0..=STEPS {
        let x = k as f64 / STEPS as f64;
        let freq = LOW_HZ * 2f64.powf(x * 10.0);
        let db = if freq < rate * 0.49 { design.curve_db(freq, rate) } else { design.curve_db(rate * 0.49, rate) };
        let y = ((range - db) / (2.0 * range) * 1000.0).clamp(0.0, 1000.0);
        let _ = write!(line, "{}{:.1} {:.1} ", if k == 0 { "M " } else { "L " }, x * 1000.0, y);
    }
    let area = format!("{line}L 1000 500 L 0 500 Z");
    (line.trim_end().to_string(), area)
}

/// Every band at its centre frequency, for placing the handles.
pub fn band_positions() -> [f64; EQ_BANDS] {
    EQ_FREQS.map(x_of)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_stay_in_range_and_are_unique() {
        let mut ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len());
        assert!(PRESETS.iter().all(|p| p.bands.iter().all(|b| b.abs() <= EQ_RANGE_DB)));
        assert!(preset(CUSTOM).is_none());
    }

    #[test]
    fn bands_sit_an_octave_apart_across_the_graph() {
        let x = band_positions();
        // 125 Hz … 16 kHz are rounded octaves of 32 Hz (128 Hz … 16.4 kHz would be exact).
        assert!((x[0] - 0.05).abs() < 1e-9 && (x[9] - 0.95).abs() < 0.005, "{x:?}");
        assert!(x.windows(2).all(|w| (w[1] - w[0] - 0.1).abs() < 0.01));
    }

    #[test]
    fn flat_is_a_straight_line_through_the_middle() {
        let (line, area) = paths(&EqDesign::new(&[0.0; EQ_BANDS]), 48_000);
        assert!(line.starts_with("M 0.0 500.0 L "));
        assert!(line.split(' ').filter(|t| t.contains('.')).skip(1).step_by(2).all(|y| y == "500.0"));
        assert!(area.ends_with("L 1000 500 L 0 500 Z"));
        assert_eq!(snap(3.26), 3.5);
        assert_eq!(snap(-20.0), -12.0);
    }
}
