//! Lyrics from LRCLIB (lrclib.net), synced (LRC) when available.

use serde_json::Value;

use crate::{Client, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lyrics {
    /// LRC text (`[mm:ss.xx]line`) when synced, plain text otherwise.
    pub text: String,
    pub synced: bool,
    pub instrumental: bool,
}

impl Client {
    /// Lyrics for a track. The duration (in seconds) makes the match exact; LRCLIB's search is
    /// used when there's no exact entry.
    pub fn lyrics(&self, artist: &str, title: &str, album: &str, duration_s: Option<u64>) -> Result<Option<Lyrics>> {
        if title.trim().is_empty() || artist.trim().is_empty() {
            return Ok(None);
        }
        if let Some(d) = duration_s {
            let d = d.to_string();
            let mut query = vec![("artist_name", artist), ("track_name", title), ("duration", d.as_str())];
            if !album.trim().is_empty() {
                query.push(("album_name", album));
            }
            if let Some(json) = self.get_json("https://lrclib.net/api/get", &query)? {
                if let Some(l) = parse(&json) {
                    return Ok(Some(l));
                }
            }
        }
        let Some(json) = self.get_json("https://lrclib.net/api/search", &[("track_name", title), ("artist_name", artist)])? else {
            return Ok(None);
        };
        let hits = json.as_array().cloned().unwrap_or_default();
        Ok(best_hit(&hits, artist, duration_s).and_then(parse))
    }
}

fn parse(v: &Value) -> Option<Lyrics> {
    let instrumental = v["instrumental"].as_bool().unwrap_or(false);
    let synced = v["syncedLyrics"].as_str().map(str::trim).filter(|s| !s.is_empty());
    let plain = v["plainLyrics"].as_str().map(str::trim).filter(|s| !s.is_empty());
    match (synced, plain) {
        (Some(s), _) => Some(Lyrics { text: s.to_string(), synced: true, instrumental }),
        (None, Some(p)) => Some(Lyrics { text: p.to_string(), synced: false, instrumental }),
        (None, None) if instrumental => Some(Lyrics { text: String::new(), synced: false, instrumental }),
        _ => None,
    }
}

/// The search hit by the same artist with the closest duration, synced lyrics first.
fn best_hit<'a>(hits: &'a [Value], artist: &str, duration_s: Option<u64>) -> Option<&'a Value> {
    let artist = artist.to_lowercase();
    hits.iter()
        .filter(|h| h["artistName"].as_str().is_some_and(|a| a.to_lowercase().contains(&artist) || artist.contains(&a.to_lowercase())))
        .filter(|h| match (duration_s, h["duration"].as_f64()) {
            (Some(want), Some(got)) => (got - want as f64).abs() <= 3.0,
            _ => true,
        })
        .max_by_key(|h| {
            let synced = h["syncedLyrics"].as_str().is_some_and(|s| !s.trim().is_empty());
            let plain = h["plainLyrics"].as_str().is_some_and(|s| !s.trim().is_empty());
            (synced as u8) * 2 + plain as u8
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_synced_and_close_duration() {
        let hits: Vec<Value> = serde_json::from_str(
            r#"[
            {"artistName": "Other", "duration": 200, "syncedLyrics": "[00:01.00]x"},
            {"artistName": "Massive Attack", "duration": 300, "plainLyrics": "far"},
            {"artistName": "Massive Attack", "duration": 379, "plainLyrics": "plain"},
            {"artistName": "Massive Attack", "duration": 380, "syncedLyrics": "[00:01.00]synced", "plainLyrics": "p"}
        ]"#,
        )
        .unwrap();
        let best = best_hit(&hits, "massive attack", Some(379)).unwrap();
        let l = parse(best).unwrap();
        assert!(l.synced);
        assert_eq!(l.text, "[00:01.00]synced");
        assert!(best_hit(&hits, "nobody", Some(379)).is_none());
    }
}
