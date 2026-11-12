//! Volume levelling (ReplayGain 2.0): every song is played at about the same loudness.
//!
//! Loudness is measured as in ITU-R BS.1770 / EBU R128 and levelled to −18 LUFS, the ReplayGain
//! 2.0 reference, so gains written by other tools (foobar2000, rsgain, beets, Picard) and gains
//! Lychee measures itself agree. Album gain keeps the loudness differences *within* an album.

use crate::{Album, Track};

/// The loudness every song is levelled to, in LUFS (ReplayGain 2.0's reference).
pub const REFERENCE_LUFS: f32 = -18.0;

/// Opus `R128_*_GAIN` tags are relative to −23 LUFS (EBU R128's broadcast level), 5 dB below
/// the ReplayGain reference.
pub const R128_TO_REPLAYGAIN_DB: f32 = 5.0;

/// ReplayGain values read from a file's tags. Gains are in dB relative to [`REFERENCE_LUFS`];
/// peaks are linear sample peaks (1.0 = full scale).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReplayGain {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

/// Loudness Lychee measured itself, for files without ReplayGain tags.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measured {
    /// Integrated loudness in LUFS; `None` for silent files or files that couldn't be decoded.
    pub lufs: Option<f32>,
    /// Sample peak, linear.
    pub peak: Option<f32>,
}

/// Where a gain came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GainSource {
    Tags,
    Measured,
}

/// The gain chosen for one song.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gain {
    /// The gain actually applied, in dB, after clipping prevention.
    pub db: f32,
    /// True when the album's gain was used (the song is played as part of its album).
    pub album: bool,
    pub source: GainSource,
}

impl Gain {
    /// Linear factor to multiply samples by.
    pub fn factor(&self) -> f32 {
        db_to_factor(self.db)
    }
}

pub fn db_to_factor(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl Track {
    /// Track gain in dB relative to [`REFERENCE_LUFS`]: from tags, else from Lychee's measurement.
    pub fn track_gain_db(&self) -> Option<(f32, GainSource)> {
        if let Some(g) = self.replay_gain.track_gain {
            return Some((g, GainSource::Tags));
        }
        self.loudness.and_then(|m| m.lufs).map(|lufs| (REFERENCE_LUFS - lufs, GainSource::Measured))
    }

    pub fn track_peak(&self) -> Option<f32> {
        self.replay_gain.track_peak.or_else(|| self.loudness.and_then(|m| m.peak))
    }
}

/// The gain for `track`. `as_album` asks for album gain (the song is played among its album's
/// songs); songs without album data fall back to track gain. `preamp_db` raises every levelled
/// song (a louder target than −18 LUFS), still limited by the song's peak. `None` when nothing
/// is known yet.
pub fn choose(track: &Track, album: Option<&Album>, as_album: bool, preamp_db: f32) -> Option<Gain> {
    let album_gain = if as_album {
        match track.replay_gain.album_gain {
            Some(g) => Some((g, track.replay_gain.album_peak.or(track.track_peak()), GainSource::Tags)),
            None => album.and_then(|a| a.gain).map(|g| {
                let source = track.track_gain_db().map_or(GainSource::Measured, |(_, s)| s);
                (g, album.and_then(|a| a.peak), source)
            }),
        }
    } else {
        None
    };
    let (db, peak, source) = match album_gain {
        Some(found) => found,
        None => {
            let (g, source) = track.track_gain_db()?;
            (g, track.track_peak(), source)
        }
    };
    Some(Gain { db: limit_to_peak(db + preamp_db, peak), album: album_gain.is_some(), source })
}

/// Lower `db` if it would push `peak` above full scale (ReplayGain's clipping prevention).
pub fn limit_to_peak(db: f32, peak: Option<f32>) -> f32 {
    match peak {
        Some(p) if p > 0.0 && db_to_factor(db) * p > 1.0 => -20.0 * p.log10(),
        _ => db,
    }
}

/// An album's gain from its songs' track gains, weighted by duration: the loudness of the whole
/// album is the power mean of the songs' loudness. `None` unless every song's gain is known.
/// (BS.1770 gates the whole album at once; this differs from that by a fraction of a dB.)
pub fn album_gain_db(songs: impl IntoIterator<Item = (Option<f32>, u64)>) -> Option<f32> {
    let mut energy = 0.0f64;
    let mut total = 0.0f64;
    for (gain, duration_ms) in songs {
        let gain = f64::from(gain?);
        let weight = duration_ms.max(1) as f64;
        // Song loudness relative to the reference is −gain; its energy is 10^(−gain/10).
        energy += weight * 10f64.powf(-gain / 10.0);
        total += weight;
    }
    (total > 0.0).then(|| (-10.0 * (energy / total).log10()) as f32)
}

/// Parse a ReplayGain gain tag: "-6.54 dB", "+2.1 dB", "-6,54", "−3 dB".
pub fn parse_gain(text: &str) -> Option<f32> {
    let t = text.trim().trim_matches('\0').trim();
    let t = t.strip_suffix("dB").or_else(|| t.strip_suffix("db")).or_else(|| t.strip_suffix("DB")).unwrap_or(t);
    let t = t.trim().replace(',', ".").replace('\u{2212}', "-");
    let t = t.strip_prefix('+').unwrap_or(&t);
    t.parse::<f32>().ok().filter(|g| g.is_finite() && g.abs() <= 64.0)
}

/// Parse a ReplayGain peak tag: "0.988525".
pub fn parse_peak(text: &str) -> Option<f32> {
    let t = text.trim().trim_matches('\0').trim().replace(',', ".");
    t.parse::<f32>().ok().filter(|p| p.is_finite() && *p > 0.0 && *p < 100.0)
}

/// Parse an Opus `R128_*_GAIN` tag (a Q7.8 integer relative to −23 LUFS) into a ReplayGain gain.
pub fn parse_r128(text: &str) -> Option<f32> {
    let q: i16 = text.trim().trim_matches('\0').parse().ok()?;
    Some(f32::from(q) / 256.0 + R128_TO_REPLAYGAIN_DB)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(tags: ReplayGain, lufs: Option<f32>, peak: Option<f32>) -> Track {
        Track { replay_gain: tags, loudness: lufs.map(|l| Measured { lufs: Some(l), peak }), ..Default::default() }
    }

    #[test]
    fn parses_tags() {
        assert_eq!(parse_gain("-6.54 dB"), Some(-6.54));
        assert_eq!(parse_gain("+2.10 dB"), Some(2.1));
        assert_eq!(parse_gain(" -3,5 db "), Some(-3.5));
        assert_eq!(parse_gain("\u{2212}1 dB"), Some(-1.0));
        assert_eq!(parse_gain("4"), Some(4.0));
        assert_eq!(parse_gain("loud"), None);
        assert_eq!(parse_gain("-900 dB"), None);
        assert_eq!(parse_peak("0.988525"), Some(0.988525));
        assert_eq!(parse_peak("0"), None);
        // −2048/256 = −8 dB relative to −23 LUFS = −3 dB relative to −18 LUFS.
        assert_eq!(parse_r128("-2048"), Some(-3.0));
        assert_eq!(parse_r128("0"), Some(5.0));
    }

    #[test]
    fn tags_win_over_measurements() {
        let tagged = track(ReplayGain { track_gain: Some(-7.0), ..Default::default() }, Some(-5.0), None);
        assert_eq!(tagged.track_gain_db(), Some((-7.0, GainSource::Tags)));
        let measured = track(ReplayGain::default(), Some(-9.5), Some(1.0));
        assert_eq!(measured.track_gain_db(), Some((-8.5, GainSource::Measured)));
        assert_eq!(track(ReplayGain::default(), None, None).track_gain_db(), None);
    }

    #[test]
    fn album_gain_is_a_power_mean() {
        // Two equally long songs, 10 dB apart: the album sits ~2.6 dB below the louder one.
        let g = album_gain_db([(Some(-10.0), 1000), (Some(0.0), 1000)]).unwrap();
        assert!((g - (-7.4)).abs() < 0.01, "{g}");
        // Equal songs give their own gain; any unknown song gives none.
        assert!((album_gain_db([(Some(-6.0), 10), (Some(-6.0), 99)]).unwrap() + 6.0).abs() < 1e-4);
        assert_eq!(album_gain_db([(Some(-6.0), 10), (None, 10)]), None);
        assert_eq!(album_gain_db([]), None);
    }

    #[test]
    fn chooses_album_or_track_gain_and_prevents_clipping() {
        let t = track(
            ReplayGain { track_gain: Some(-8.0), track_peak: Some(1.0), album_gain: Some(-6.0), album_peak: Some(1.0) },
            None,
            None,
        );
        assert_eq!(choose(&t, None, false, 0.0), Some(Gain { db: -8.0, album: false, source: GainSource::Tags }));
        assert_eq!(choose(&t, None, true, 0.0), Some(Gain { db: -6.0, album: true, source: GainSource::Tags }));
        // The pre-amp raises the gain, but never past the peak (1.0 here: at most 0 dB).
        assert_eq!(choose(&t, None, false, 4.0).unwrap().db, -4.0);
        assert_eq!(choose(&t, None, true, 8.0).unwrap().db, 0.0);

        // A quiet song may be raised only until its peak reaches full scale (0.5 → +6.02 dB).
        let quiet = track(ReplayGain { track_gain: Some(10.0), track_peak: Some(0.5), ..Default::default() }, None, None);
        let g = choose(&quiet, None, false, 0.0).unwrap();
        assert!((g.db - 6.0206).abs() < 1e-3, "{}", g.db);

        // Measured songs use the album's computed gain when played as an album.
        let m = track(ReplayGain::default(), Some(-8.0), Some(0.9));
        let album = Album { gain: Some(-12.0), peak: Some(0.99), ..Default::default() };
        let g = choose(&m, Some(&album), true, 0.0).unwrap();
        assert_eq!((g.db, g.album, g.source), (-12.0, true, GainSource::Measured));
        assert_eq!(choose(&m, Some(&album), false, 0.0).unwrap().db, -10.0);
    }
}
