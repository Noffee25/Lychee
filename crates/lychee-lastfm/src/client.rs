//! A small, blocking Last.fm API client.

use std::collections::BTreeMap;
use std::time::Duration;

use lychee_core::md5::Md5;
use serde_json::Value;

use crate::{Credentials, Scrobble, Session, TrackInfo};

const API_URL: &str = "https://ws.audioscrobbler.com/2.0/";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Last.fm error {code}: {message}")]
    Api { code: u32, message: String },
    #[error("could not reach Last.fm: {0}")]
    Network(String),
    #[error("unexpected response from Last.fm: {0}")]
    Parse(String),
}

impl Error {
    /// The session or API key is invalid; the user has to connect again.
    pub fn is_auth(&self) -> bool {
        matches!(self, Error::Api { code: 4 | 9 | 10 | 14 | 15 | 26, .. })
    }

    /// Worth retrying later (offline, service hiccup, rate limit).
    pub fn is_temporary(&self) -> bool {
        matches!(self, Error::Network(_) | Error::Parse(_) | Error::Api { code: 8 | 11 | 16 | 29, .. })
    }

    /// The user has not approved the token in the browser (yet).
    pub fn is_unauthorized_token(&self) -> bool {
        matches!(self, Error::Api { code: 14, .. })
    }
}

/// Browser URL where the user approves a token.
pub fn auth_url(api_key: &str, token: &str) -> String {
    format!("https://www.last.fm/api/auth/?api_key={api_key}&token={token}")
}

pub struct Client {
    credentials: Credentials,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(credentials: Credentials) -> Client {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent(concat!("Lychee/", env!("CARGO_PKG_VERSION"), " (+https://github.com/Noffee25/Lychee)"))
            .build();
        Client { credentials, agent: config.into() }
    }

    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    /// Step 1 of desktop auth: get a token to approve in the browser.
    pub fn get_token(&self) -> Result<String, Error> {
        let v = self.call("auth.getToken", BTreeMap::new())?;
        v["token"].as_str().map(str::to_string).ok_or_else(|| Error::Parse("missing token".into()))
    }

    /// Step 2: exchange the approved token for a session.
    pub fn get_session(&self, token: &str) -> Result<Session, Error> {
        let mut p = BTreeMap::new();
        p.insert("token", token.to_string());
        let v = self.call("auth.getSession", p)?;
        let s = &v["session"];
        match (s["name"].as_str(), s["key"].as_str()) {
            (Some(name), Some(key)) => Ok(Session { username: name.into(), key: key.into() }),
            _ => Err(Error::Parse("missing session".into())),
        }
    }

    pub fn update_now_playing(&self, session: &Session, track: &TrackInfo) -> Result<(), Error> {
        let mut p = BTreeMap::new();
        p.insert("sk", session.key.clone());
        track_params(track, None, &mut p);
        self.call("track.updateNowPlaying", p).map(|_| ())
    }

    /// Submit up to 50 scrobbles. Returns how many Last.fm accepted.
    pub fn scrobble(&self, session: &Session, batch: &[Scrobble]) -> Result<usize, Error> {
        let mut p = BTreeMap::new();
        p.insert("sk", session.key.clone());
        let mut owned: Vec<(String, String)> = Vec::new();
        for (i, s) in batch.iter().take(50).enumerate() {
            let mut one = BTreeMap::new();
            track_params(&s.track, Some(s.timestamp), &mut one);
            for (k, v) in one {
                owned.push((format!("{k}[{i}]"), v));
            }
        }
        let v = self.call_owned("track.scrobble", p, owned)?;
        let accepted = v["scrobbles"]["@attr"]["accepted"]
            .as_u64()
            .or_else(|| v["scrobbles"]["@attr"]["accepted"].as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(batch.len() as u64);
        Ok(accepted as usize)
    }

    pub fn set_loved(&self, session: &Session, artist: &str, title: &str, loved: bool) -> Result<(), Error> {
        let mut p = BTreeMap::new();
        p.insert("sk", session.key.clone());
        p.insert("artist", artist.to_string());
        p.insert("track", title.to_string());
        self.call(if loved { "track.love" } else { "track.unlove" }, p).map(|_| ())
    }

    fn call(&self, method: &str, params: BTreeMap<&'static str, String>) -> Result<Value, Error> {
        self.call_owned(method, params, Vec::new())
    }

    fn call_owned(
        &self,
        method: &str,
        params: BTreeMap<&'static str, String>,
        extra: Vec<(String, String)>,
    ) -> Result<Value, Error> {
        let mut all: BTreeMap<String, String> = params.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        all.extend(extra);
        all.insert("method".into(), method.into());
        all.insert("api_key".into(), self.credentials.api_key.clone());
        let sig = signature(&all, &self.credentials.api_secret);
        all.insert("api_sig".into(), sig);
        all.insert("format".into(), "json".into());

        let mut response = self
            .agent
            .post(API_URL)
            .send_form(all.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .map_err(|e| Error::Network(e.to_string()))?;
        let body = response.body_mut().read_to_string().map_err(|e| Error::Network(e.to_string()))?;
        let value: Value = serde_json::from_str(&body).map_err(|e| Error::Parse(format!("{e} ({} bytes)", body.len())))?;
        if let Some(code) = value["error"].as_u64() {
            let message = value["message"].as_str().unwrap_or("unknown error").to_string();
            return Err(Error::Api { code: code as u32, message });
        }
        Ok(value)
    }
}

fn track_params(track: &TrackInfo, timestamp: Option<i64>, p: &mut BTreeMap<&'static str, String>) {
    p.insert("artist", track.artist.clone());
    p.insert("track", track.title.clone());
    if let Some(album) = track.album.as_ref().filter(|a| !a.is_empty()) {
        p.insert("album", album.clone());
    }
    if let Some(aa) = track.album_artist.as_ref().filter(|a| !a.is_empty() && **a != track.artist) {
        p.insert("albumArtist", aa.clone());
    }
    if let Some(d) = track.duration_secs {
        p.insert("duration", d.to_string());
    }
    if let Some(n) = track.track_number {
        p.insert("trackNumber", n.to_string());
    }
    if let Some(ts) = timestamp {
        p.insert("timestamp", ts.to_string());
    }
}

/// `api_sig`: MD5 of every parameter (sorted by name, `format` excluded) followed by the secret.
fn signature(params: &BTreeMap<String, String>, secret: &str) -> String {
    let mut hasher = Md5::new();
    for (k, v) in params {
        if k == "format" || k == "callback" {
            continue;
        }
        hasher.update(k.as_bytes());
        hasher.update(v.as_bytes());
    }
    hasher.update(secret.as_bytes());
    hasher.finish_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_matches_the_documented_algorithm() {
        // md5("api_keyxxxxmethodauth.getSessiontokenyyyyilovecher")
        let mut p = BTreeMap::new();
        p.insert("token".to_string(), "yyyy".to_string());
        p.insert("api_key".to_string(), "xxxx".to_string());
        p.insert("method".to_string(), "auth.getSession".to_string());
        p.insert("format".to_string(), "json".to_string());
        // `echo -n api_keyxxxxmethodauth.getSessiontokenyyyyilovecher | md5sum`
        assert_eq!(signature(&p, "ilovecher"), "823df9939bce8abaf766d357c0eb08e1");
    }

    #[test]
    fn error_classification() {
        assert!(Error::Api { code: 9, message: String::new() }.is_auth());
        assert!(Error::Api { code: 29, message: String::new() }.is_temporary());
        assert!(Error::Network("offline".into()).is_temporary());
        assert!(!Error::Api { code: 6, message: String::new() }.is_temporary());
    }
}
