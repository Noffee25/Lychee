//! Lychee's playback engine.
//!
//! ```text
//! file ─► Symphonia demuxer ─► decoder (Symphonia / pure-Rust Opus) ─► channel remix ─► track gain
//!      ─► rubato resampler (only if the rates differ) ─► lock-free ring buffer
//!      ─► cpal callback (equalizer, crossfeed, mono, volume)
//! ```
//!
//! A control thread decodes about a second ahead of the device. The next track can be
//! pre-loaded so it starts on the exact sample the previous one ends (gapless). The real-time
//! callback never allocates or blocks; it runs the equalizer, headphone crossfeed and mono downmix
//! ([`dsp`]) and applies volume and short fades to avoid clicks on pause, resume and seek. Each
//! track can carry its own gain (volume levelling), applied while decoding so it changes on the
//! exact sample where the next track starts. [`loudness`] measures files that have no ReplayGain
//! tags.

pub mod dsp;
mod engine;
pub mod loudness;
mod output;
mod remix;
mod resample;
mod source;

pub use dsp::{EQ_BANDS, EQ_FREQS, EQ_RANGE_DB, EqDesign};
pub use engine::{Engine, EngineEvent};
pub use output::perceptual;
pub use source::{Source, StreamInfo};

/// File extensions Lychee can decode.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "mp3", "mp2", "mp1", "flac", "wav", "wave", "aif", "aiff", "aifc", "ogg", "oga", "opus", "m4a", "m4b", "mp4", "aac",
    "caf", "mka", "webm",
];

pub fn is_supported_extension(ext: &str) -> bool {
    SUPPORTED_EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("could not read the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Unsupported(String),
    #[error("decoding error: {0}")]
    Decode(String),
    #[error("audio device error: {0}")]
    Device(String),
}
