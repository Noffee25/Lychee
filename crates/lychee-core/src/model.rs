use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub type TrackId = i64;
pub type AlbumId = i64;
pub type PlaylistId = i64;

/// Repeat behaviour of the play queue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    /// Off → All → One → Off, the order a repeat button cycles through.
    pub fn cycle(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaybackStatus {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// Technical description of an audio stream, as read from the file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioFormat {
    /// Short codec name shown to users, e.g. "FLAC", "MP3", "AAC".
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    /// Average audio bitrate in kbit/s.
    pub bitrate_kbps: Option<u32>,
}

impl AudioFormat {
    pub fn is_lossless(&self) -> bool {
        matches!(self.codec.as_str(), "FLAC" | "ALAC" | "WAV" | "AIFF" | "APE" | "WavPack" | "CAF")
    }

    /// "Hi-Res" in the usual industry sense: lossless and better than CD quality.
    pub fn is_hires(&self) -> bool {
        self.is_lossless()
            && (self.bit_depth.is_some_and(|b| b > 16) || self.sample_rate.is_some_and(|r| r > 48_000))
    }

    /// "24/96" for lossless files with known depth and rate, "320k" for lossy files.
    pub fn short_quality(&self) -> String {
        if self.is_lossless() {
            if let (Some(bits), Some(rate)) = (self.bit_depth, self.sample_rate) {
                return format!("{}/{}", bits, format_khz_short(rate));
            }
        }
        match self.bitrate_kbps {
            Some(kbps) if kbps > 0 => format!("{kbps}k"),
            _ => String::new(),
        }
    }

    /// "24-bit / 96 kHz" or "320 kbps".
    pub fn long_quality(&self) -> String {
        let mut parts = Vec::new();
        if let Some(bits) = self.bit_depth.filter(|_| self.is_lossless()) {
            parts.push(format!("{bits}-bit"));
        }
        if let Some(rate) = self.sample_rate {
            parts.push(format!("{} kHz", format_khz(rate)));
        }
        if !self.is_lossless() {
            if let Some(kbps) = self.bitrate_kbps.filter(|k| *k > 0) {
                parts.push(format!("{kbps} kbps"));
            }
        }
        parts.join(" / ")
    }
}

fn format_khz(rate: u32) -> String {
    if rate % 1000 == 0 { format!("{}", rate / 1000) } else { format!("{:.1}", rate as f64 / 1000.0) }
}

fn format_khz_short(rate: u32) -> String {
    match rate {
        44_100 => "44".into(),
        88_200 => "88".into(),
        176_400 => "176".into(),
        352_800 => "352".into(),
        r => format_khz(r),
    }
}

/// One audio file and everything Lychee knows about it.
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub id: TrackId,
    pub path: PathBuf,
    pub title: String,
    /// The artist tag as written; several values are joined with ", ".
    pub artist: String,
    /// The main artist, without featured ones: the first of several artist values, or the tag cut
    /// before "feat.". Use [`Track::display_main_artist`], which also covers rows saved before
    /// this was stored.
    pub main_artist: String,
    pub album: String,
    pub album_artist: String,
    pub album_id: AlbumId,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    /// One or more genres, joined with "; " (see [`Track::genres`]).
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub duration_ms: u64,
    pub format: AudioFormat,
    pub file_size: u64,
    /// Key into the artwork cache, if the track has (or shares) cover art.
    pub artwork: Option<String>,
    /// Unix seconds when the file was first added to the library.
    pub added_at: i64,
    pub play_count: u32,
    pub last_played: Option<i64>,
    pub liked: bool,
    pub liked_at: Option<i64>,
    /// False for files that were opened directly ("Open with…") but not imported.
    pub in_library: bool,
    /// ReplayGain values from the file's tags.
    pub replay_gain: crate::ReplayGain,
    /// Loudness Lychee measured, for files without ReplayGain tags (`None` until measured).
    pub loudness: Option<crate::Measured>,
}

impl Track {
    pub fn display_artist(&self) -> &str {
        if self.artist.is_empty() { "Unknown Artist" } else { &self.artist }
    }

    /// The track's genres, in tag order.
    pub fn genres(&self) -> impl Iterator<Item = &str> {
        self.genre.as_deref().into_iter().flat_map(crate::split_genres)
    }

    /// The main artist (see [`Track::main_artist`]), or "Unknown Artist".
    pub fn display_main_artist(&self) -> &str {
        let main = if self.main_artist.is_empty() { crate::main_artist(&self.artist) } else { &self.main_artist };
        if main.is_empty() { "Unknown Artist" } else { main }
    }
}

pub type TrackRef = Arc<Track>;

/// Albums are derived from tracks (grouped by album artist + album title).
#[derive(Clone, Debug, Default)]
pub struct Album {
    pub id: AlbumId,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    /// The album's genres, most common among its songs first, spelled the way most of the library
    /// spells them.
    pub genres: Vec<String>,
    /// Track ids in disc/track order.
    pub tracks: Vec<TrackId>,
    pub artwork: Option<String>,
    /// Most vivid colour of the artwork, 0xRRGGBB (buttons, highlights).
    pub accent: Option<u32>,
    /// Dominant colour of the artwork, 0xRRGGBB (header backgrounds).
    pub tint: Option<u32>,
    pub added_at: i64,
    pub duration_ms: u64,
    /// The "best" format among the album's tracks, for badges.
    pub format: AudioFormat,
    pub folder: Option<PathBuf>,
    /// Album gain in dB (see [`crate::gain::album_gain_db`]), when every song's loudness is known.
    pub gain: Option<f32>,
    /// The highest peak among the album's songs, when known.
    pub peak: Option<f32>,
}

/// Stable album id from album artist and title (case-insensitive).
pub fn album_id_for(album_artist: &str, album: &str) -> AlbumId {
    // FNV-1a 64; stable across runs and Rust versions, unlike DefaultHasher.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in album_artist.to_lowercase().bytes().chain([0u8]).chain(album.to_lowercase().bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash >> 1) as i64
}

#[derive(Clone, Debug, Default)]
pub struct Artist {
    pub name: String,
    pub albums: Vec<AlbumId>,
    pub track_count: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Playlist {
    pub id: PlaylistId,
    pub name: String,
    pub tracks: Vec<TrackId>,
    pub updated_at: i64,
}

/// A line of lyrics, optionally time-synced.
#[derive(Clone, Debug, PartialEq)]
pub struct LyricLine {
    pub time_ms: Option<u64>,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    pub synced: bool,
    /// Where the lyrics came from, e.g. "Embedded" or "song.lrc".
    pub source: String,
}

impl Lyrics {
    /// Index of the line that should be highlighted at `position_ms`.
    pub fn line_at(&self, position_ms: u64) -> Option<usize> {
        if !self.synced {
            return None;
        }
        let mut current = None;
        for (i, line) in self.lines.iter().enumerate() {
            match line.time_ms {
                Some(t) if t <= position_ms => current = Some(i),
                Some(_) => break,
                None => {}
            }
        }
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_labels() {
        let flac = AudioFormat { codec: "FLAC".into(), sample_rate: Some(96_000), bit_depth: Some(24), channels: Some(2), bitrate_kbps: Some(2304) };
        assert!(flac.is_hires());
        assert_eq!(flac.short_quality(), "24/96");
        assert_eq!(flac.long_quality(), "24-bit / 96 kHz");

        let cd = AudioFormat { codec: "FLAC".into(), sample_rate: Some(44_100), bit_depth: Some(16), ..Default::default() };
        assert!(!cd.is_hires());
        assert_eq!(cd.short_quality(), "16/44");
        assert_eq!(cd.long_quality(), "16-bit / 44.1 kHz");

        let mp3 = AudioFormat { codec: "MP3".into(), sample_rate: Some(44_100), bitrate_kbps: Some(320), ..Default::default() };
        assert_eq!(mp3.short_quality(), "320k");
        assert_eq!(mp3.long_quality(), "44.1 kHz / 320 kbps");
    }

    #[test]
    fn album_ids_are_stable_and_case_insensitive() {
        assert_eq!(album_id_for("Hollow Coast", "Tidal Memory"), album_id_for("hollow coast", "TIDAL MEMORY"));
        assert_ne!(album_id_for("Hollow Coast", "Tidal Memory"), album_id_for("Hollow Coast", "Tidal Memories"));
        assert!(album_id_for("a", "b") >= 0);
    }

    #[test]
    fn lyric_line_lookup() {
        let lyrics = Lyrics {
            lines: vec![
                LyricLine { time_ms: Some(1000), text: "one".into() },
                LyricLine { time_ms: Some(5000), text: "two".into() },
                LyricLine { time_ms: Some(9000), text: "three".into() },
            ],
            synced: true,
            source: String::new(),
        };
        assert_eq!(lyrics.line_at(0), None);
        assert_eq!(lyrics.line_at(1000), Some(0));
        assert_eq!(lyrics.line_at(6000), Some(1));
        assert_eq!(lyrics.line_at(60_000), Some(2));
    }
}
