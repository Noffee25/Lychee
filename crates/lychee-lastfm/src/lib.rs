//! Last.fm integration: desktop authentication, "now playing" updates, scrobbling with an
//! offline queue, and loving tracks.
//!
//! Lychee needs a Last.fm API account (key + shared secret). Official builds can bake one in
//! at compile time with the `LYCHEE_LASTFM_API_KEY` / `LYCHEE_LASTFM_API_SECRET` environment
//! variables; users can also enter their own in Settings.

mod client;
mod scrobbler;

pub use client::{Client, Error, auth_url};
pub use scrobbler::{Scrobbler, State, Status};

use serde::{Deserialize, Serialize};

/// API account credentials (from <https://www.last.fm/api/account/create>).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub api_key: String,
    pub api_secret: String,
}

impl Credentials {
    /// Credentials compiled into this build, if any.
    pub fn built_in() -> Option<Credentials> {
        let key = option_env!("LYCHEE_LASTFM_API_KEY")?;
        let secret = option_env!("LYCHEE_LASTFM_API_SECRET")?;
        (!key.is_empty() && !secret.is_empty()).then(|| Credentials { api_key: key.into(), api_secret: secret.into() })
    }

    pub fn is_complete(&self) -> bool {
        !self.api_key.trim().is_empty() && !self.api_secret.trim().is_empty()
    }
}

/// An authorised user session. The key never expires unless the user revokes access.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub username: String,
    pub key: String,
}

/// The track facts Last.fm wants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackInfo {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub duration_secs: Option<u32>,
    pub track_number: Option<u32>,
}

/// One listen. `timestamp` is when playback *started* (Unix seconds).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scrobble {
    pub track: TrackInfo,
    pub timestamp: i64,
}

/// Last.fm's rule: tracks over 30 s count once played for half their length or four minutes.
pub fn scrobble_threshold_ms(duration_ms: u64) -> Option<u64> {
    (duration_ms > 30_000).then(|| (duration_ms / 2).min(240_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_follows_lastfm_rules() {
        assert_eq!(scrobble_threshold_ms(20_000), None);
        assert_eq!(scrobble_threshold_ms(200_000), Some(100_000));
        assert_eq!(scrobble_threshold_ms(900_000), Some(240_000));
    }
}
