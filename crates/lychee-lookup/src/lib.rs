//! Online metadata for the tag editor.
//!
//! * [`musicbrainz`]: find the release a set of files belongs to and read its track list;
//! * [`coverart`]: front covers from the Cover Art Archive;
//! * [`lyrics`]: synced or plain lyrics from LRCLIB;
//! * [`matching`]: guess what to search for from tags and folder names, and score candidates
//!   against the files (track count, durations, titles).
//!
//! All calls block; run them on a worker thread. One [`Client`] should be shared so the
//! MusicBrainz rate limit (one request per second) holds across lookups.

pub mod coverart;
pub mod lyrics;
pub mod matching;
pub mod musicbrainz;

use std::time::{Duration, Instant};

use parking_lot::Mutex;

pub use lyrics::Lyrics;
pub use matching::{Candidate, FileFacts, Guess};
pub use musicbrainz::{Medium, Release, ReleaseSummary, Track};

const USER_AGENT: &str = concat!("Lychee/", env!("CARGO_PKG_VERSION"), " ( https://github.com/Y2Kwastaken/Lychee )");

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("the service answered {0}")]
    Status(u16),
    #[error("unexpected answer: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Client {
    agent: ureq::Agent,
    /// When the last MusicBrainz request went out.
    last_musicbrainz: Mutex<Option<Instant>>,
}

impl Default for Client {
    fn default() -> Self {
        Client::new()
    }
}

impl Client {
    pub fn new() -> Client {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(USER_AGENT)
            .build();
        Client { agent: config.into(), last_musicbrainz: Mutex::new(None) }
    }

    /// GET a URL; `Ok(None)` for 404.
    fn get(&self, url: &str, query: &[(&str, &str)], accept: &str) -> Result<Option<ureq::http::Response<ureq::Body>>> {
        let mut request = self.agent.get(url).header("Accept", accept);
        for (k, v) in query {
            request = request.query(*k, *v);
        }
        let response = request.call().map_err(|e| Error::Network(e.to_string()))?;
        match response.status().as_u16() {
            200..=299 => Ok(Some(response)),
            404 => Ok(None),
            code => Err(Error::Status(code)),
        }
    }

    fn get_json(&self, url: &str, query: &[(&str, &str)]) -> Result<Option<serde_json::Value>> {
        let Some(mut response) = self.get(url, query, "application/json")? else { return Ok(None) };
        let text = response.body_mut().read_to_string().map_err(|e| Error::Network(e.to_string()))?;
        serde_json::from_str(&text).map(Some).map_err(|e| Error::Parse(e.to_string()))
    }

    /// MusicBrainz asks for at most one request per second; wait our turn, and retry once
    /// after a pause when it says we're going too fast.
    fn musicbrainz(&self, path: &str, query: &[(&str, &str)]) -> Result<Option<serde_json::Value>> {
        let url = format!("https://musicbrainz.org/ws/2/{path}");
        let mut query = query.to_vec();
        query.push(("fmt", "json"));
        for attempt in 0..3 {
            {
                let mut last = self.last_musicbrainz.lock();
                if let Some(at) = *last {
                    let wait = Duration::from_millis(1100).saturating_sub(at.elapsed());
                    std::thread::sleep(wait);
                }
                *last = Some(Instant::now());
            }
            match self.get_json(&url, &query) {
                Err(Error::Status(503)) if attempt < 2 => std::thread::sleep(Duration::from_secs(2)),
                other => return other,
            }
        }
        Err(Error::Status(503))
    }
}
