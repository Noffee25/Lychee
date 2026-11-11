//! App-side Last.fm glue: the browser sign-in flow and deciding when a listen counts.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use lychee_core::Track;
use lychee_lastfm::{Client, Credentials, Session, TrackInfo, auth_url, scrobble_threshold_ms};

use crate::settings::ScrobbleArtist;

pub enum LastfmMsg {
    Status(lychee_lastfm::Status),
    Connected(Session),
    ConnectFailed(String),
    Cancelled,
}

/// Desktop auth: get a token, open the approval page, then poll until the user approves.
/// Returns a flag that cancels the attempt when set.
pub fn start_auth(credentials: Credentials, tx: Sender<LastfmMsg>) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    let cancelled = cancel.clone();
    let _ = thread::Builder::new().name("lychee-lastfm-auth".into()).spawn(move || {
        let client = Client::new(credentials.clone());
        let token = match client.get_token() {
            Ok(t) => t,
            Err(e) => {
                let _ = tx.send(LastfmMsg::ConnectFailed(e.to_string()));
                return;
            }
        };
        if let Err(e) = lychee_platform::open(auth_url(&credentials.api_key, &token)) {
            tracing::warn!("could not open the browser: {e}");
        }
        let deadline = Instant::now() + Duration::from_secs(300);
        while Instant::now() < deadline {
            for _ in 0..30 {
                if cancelled.load(Ordering::Relaxed) {
                    let _ = tx.send(LastfmMsg::Cancelled);
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
            match client.get_session(&token) {
                Ok(session) => {
                    let _ = tx.send(LastfmMsg::Connected(session));
                    return;
                }
                Err(e) if e.is_unauthorized_token() => continue,
                Err(e) if e.is_temporary() => continue,
                Err(e) => {
                    let _ = tx.send(LastfmMsg::ConnectFailed(e.to_string()));
                    return;
                }
            }
        }
        let _ = tx.send(LastfmMsg::ConnectFailed("Timed out waiting for approval on last.fm.".into()));
    });
    cancel
}

/// The artist to send for `t`. Titles are always sent as tagged: Last.fm keeps "(feat. B)" there.
pub fn scrobble_artist(t: &Track, mode: ScrobbleArtist) -> &str {
    match mode {
        ScrobbleArtist::Main => t.display_main_artist(),
        ScrobbleArtist::Exact => t.display_artist(),
    }
}

pub fn track_info(t: &Track, mode: ScrobbleArtist) -> TrackInfo {
    let artist = scrobble_artist(t, mode);
    let album_artist = match mode {
        ScrobbleArtist::Main => lychee_core::main_artist(&t.album_artist),
        ScrobbleArtist::Exact => t.album_artist.as_str(),
    };
    TrackInfo {
        artist: artist.to_string(),
        title: t.title.clone(),
        album: (!t.album.is_empty()).then(|| t.album.clone()),
        album_artist: (!album_artist.is_empty() && album_artist != artist).then(|| album_artist.to_string()),
        duration_secs: (t.duration_ms > 0).then(|| (t.duration_ms / 1000) as u32),
        track_number: t.track_number,
    }
}

/// Tracks actual listening time for one play of one track (seeks don't count).
pub struct Listen {
    pub track: Arc<Track>,
    pub started_at: i64,
    listened_ms: u64,
    last_pos: u64,
    counted: bool,
}

pub enum ListenEvent {
    /// Half the track (or four minutes) was heard: count a play, and scrobble if eligible.
    Counted { scrobble: bool },
}

impl Listen {
    pub fn new(track: Arc<Track>, start_ms: u64) -> Self {
        Listen { track, started_at: lychee_core::now_unix(), listened_ms: 0, last_pos: start_ms, counted: false }
    }

    pub fn update(&mut self, pos_ms: u64, playing: bool) -> Option<ListenEvent> {
        let delta = pos_ms.saturating_sub(self.last_pos);
        if playing && delta > 0 && delta < 3_000 {
            self.listened_ms += delta;
        }
        self.last_pos = pos_ms;
        let duration = self.track.duration_ms;
        let needed = scrobble_threshold_ms(duration).unwrap_or((duration / 2).max(1));
        if !self.counted && duration > 0 && self.listened_ms >= needed {
            self.counted = true;
            return Some(ListenEvent::Counted { scrobble: scrobble_threshold_ms(duration).is_some() });
        }
        None
    }

    pub fn seeked(&mut self, pos_ms: u64) {
        self.last_pos = pos_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_artist_mode_leaves_out_featured_artists() {
        let t = Track {
            title: "Get Lucky (feat. Pharrell Williams)".into(),
            artist: "Daft Punk feat. Pharrell Williams".into(),
            album_artist: "Daft Punk".into(),
            ..Default::default()
        };
        let main = track_info(&t, ScrobbleArtist::Main);
        assert_eq!(main.artist, "Daft Punk");
        assert_eq!(main.title, "Get Lucky (feat. Pharrell Williams)", "titles are sent as tagged");
        assert_eq!(main.album_artist, None, "same as the artist");

        let exact = track_info(&t, ScrobbleArtist::Exact);
        assert_eq!(exact.artist, "Daft Punk feat. Pharrell Williams");
        assert_eq!(exact.album_artist.as_deref(), Some("Daft Punk"));

        let multi = Track { artist: "Daft Punk, Pharrell Williams".into(), main_artist: "Daft Punk".into(), ..t };
        assert_eq!(track_info(&multi, ScrobbleArtist::Main).artist, "Daft Punk");
        assert_eq!(track_info(&multi, ScrobbleArtist::Exact).artist, "Daft Punk, Pharrell Williams");
    }
}
