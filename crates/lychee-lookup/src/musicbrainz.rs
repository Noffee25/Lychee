//! MusicBrainz web service (ws/2): release search and lookups.

use serde_json::Value;

use crate::{Client, Error, Result};

/// A search hit: enough to show in a list and to rank before fetching the track list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReleaseSummary {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub date: String,
    pub country: String,
    pub track_count: usize,
    /// "CD", "2×Vinyl", "Digital Media"...
    pub format: String,
    pub label: String,
    pub status: String,
    pub disambiguation: String,
    pub release_group_id: String,
    /// MusicBrainz's own search score, 0–100.
    pub score: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub id: String,
    pub recording_id: String,
    pub position: u32,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    pub length_ms: Option<u64>,
    pub isrc: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Medium {
    pub position: u32,
    pub format: String,
    pub tracks: Vec<Track>,
}

/// A release with its track list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Release {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    pub date: String,
    /// First release date of the release group: the album's original year.
    pub original_date: String,
    pub country: String,
    pub label: String,
    pub catalog_number: String,
    pub barcode: String,
    pub release_group_id: String,
    pub release_type: String,
    pub genre: String,
    pub media: Vec<Medium>,
}

impl Release {
    pub fn track_count(&self) -> usize {
        self.media.iter().map(|m| m.tracks.len()).sum()
    }

    pub fn tracks(&self) -> impl Iterator<Item = (&Medium, &Track)> {
        self.media.iter().flat_map(|m| m.tracks.iter().map(move |t| (m, t)))
    }

    /// The year to tag: the original release, else this one.
    pub fn year(&self) -> &str {
        let date = if self.original_date.len() >= 4 { &self.original_date } else { &self.date };
        date.get(..4).unwrap_or("")
    }

    pub fn summary(&self) -> ReleaseSummary {
        ReleaseSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            date: self.date.clone(),
            country: self.country.clone(),
            track_count: self.track_count(),
            format: format_media(self.media.iter().map(|m| m.format.as_str())),
            label: self.label.clone(),
            status: String::new(),
            disambiguation: String::new(),
            release_group_id: self.release_group_id.clone(),
            score: 100,
        }
    }
}

/// Quote a value for a Lucene query.
fn phrase(value: &str) -> String {
    let escaped: String = value.chars().flat_map(|c| if c == '"' || c == '\\' { vec!['\\', c] } else { vec![c] }).collect();
    format!("\"{escaped}\"")
}

impl Client {
    /// Search releases by album title and artist.
    pub fn search_releases(&self, album: &str, artist: &str, limit: usize) -> Result<Vec<ReleaseSummary>> {
        let mut terms = Vec::new();
        if !album.trim().is_empty() {
            terms.push(format!("release:{}", phrase(album.trim())));
        }
        if !artist.trim().is_empty() && !artist.eq_ignore_ascii_case("various artists") {
            terms.push(format!("artist:{}", phrase(artist.trim())));
        }
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let query = terms.join(" AND ");
        let limit = limit.clamp(1, 50).to_string();
        let Some(json) = self.musicbrainz("release", &[("query", &query), ("limit", &limit)])? else { return Ok(Vec::new()) };
        Ok(json["releases"].as_array().map(|a| a.iter().map(parse_summary).collect()).unwrap_or_default())
    }

    /// Search recordings (single tracks); each hit lists releases it appears on.
    pub fn search_recordings(&self, title: &str, artist: &str, limit: usize) -> Result<Vec<ReleaseSummary>> {
        let mut terms = vec![format!("recording:{}", phrase(title.trim()))];
        if !artist.trim().is_empty() {
            terms.push(format!("artist:{}", phrase(artist.trim())));
        }
        let query = terms.join(" AND ");
        let limit = limit.clamp(1, 25).to_string();
        let Some(json) = self.musicbrainz("recording", &[("query", &query), ("limit", &limit)])? else { return Ok(Vec::new()) };
        let mut out: Vec<ReleaseSummary> = Vec::new();
        for recording in json["recordings"].as_array().into_iter().flatten() {
            let score = recording["score"].as_u64().unwrap_or(0) as u32;
            for release in recording["releases"].as_array().into_iter().flatten() {
                let mut summary = parse_summary(release);
                summary.score = score;
                if summary.artist.is_empty() {
                    summary.artist = credit(&recording["artist-credit"]).0;
                }
                if !out.iter().any(|s| s.id == summary.id) {
                    out.push(summary);
                }
            }
        }
        Ok(out)
    }

    /// The full release: track list, labels, genres, identifiers.
    pub fn release(&self, id: &str) -> Result<Option<Release>> {
        let inc = "recordings+artist-credits+labels+release-groups+genres+isrcs";
        let Some(json) = self.musicbrainz(&format!("release/{id}"), &[("inc", inc)])? else { return Ok(None) };
        parse_release(&json).map(Some).ok_or_else(|| Error::Parse("release without an id".into()))
    }

    /// Genres are usually voted on the release group rather than on one release: fill the
    /// genre from there when the release has none (one more request).
    pub fn complete_genre(&self, release: &mut Release) {
        if !release.genre.is_empty() || release.release_group_id.is_empty() {
            return;
        }
        let path = format!("release-group/{}", release.release_group_id);
        if let Ok(Some(group)) = self.musicbrainz(&path, &[("inc", "genres")]) {
            release.genre = top_genre(&group["genres"]);
        }
    }
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// An artist credit as one string ("A feat. B") and the first artist's id.
fn credit(v: &Value) -> (String, String) {
    let mut name = String::new();
    let mut first_id = String::new();
    for part in v.as_array().into_iter().flatten() {
        name.push_str(part["name"].as_str().or(part["artist"]["name"].as_str()).unwrap_or_default());
        name.push_str(part["joinphrase"].as_str().unwrap_or_default());
        if first_id.is_empty() {
            first_id = s(&part["artist"]["id"]);
        }
    }
    (name, first_id)
}

fn format_media<'a>(formats: impl Iterator<Item = &'a str>) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for f in formats {
        let f = if f.is_empty() { "Unknown" } else { f };
        match counts.iter_mut().find(|(name, _)| name == f) {
            Some((_, n)) => *n += 1,
            None => counts.push((f.to_string(), 1)),
        }
    }
    counts.into_iter().map(|(f, n)| if n > 1 { format!("{n}×{f}") } else { f }).collect::<Vec<_>>().join(" + ")
}

fn top_genre(genres: &Value) -> String {
    let best = genres.as_array().into_iter().flatten().max_by_key(|g| g["count"].as_u64().unwrap_or(0));
    best.and_then(|g| g["name"].as_str()).map(title_words).unwrap_or_default()
}

/// "trip hop" -> "Trip Hop", "r&b" -> "R&B"
fn title_words(text: &str) -> String {
    text.split(' ')
        .map(|w| {
            if w.len() <= 3 && w.contains('&') {
                return w.to_uppercase();
            }
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        })
        .collect::<Vec<String>>()
        .join(" ")
}

fn parse_summary(r: &Value) -> ReleaseSummary {
    let media: Vec<&Value> = r["media"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let track_count = r["track-count"]
        .as_u64()
        .map(|n| n as usize)
        .unwrap_or_else(|| media.iter().map(|m| m["track-count"].as_u64().unwrap_or(0) as usize).sum());
    let label = r["label-info"].as_array().and_then(|l| l.first()).map(|l| s(&l["label"]["name"])).unwrap_or_default();
    ReleaseSummary {
        id: s(&r["id"]),
        title: s(&r["title"]),
        artist: credit(&r["artist-credit"]).0,
        date: s(&r["date"]),
        country: s(&r["country"]),
        track_count,
        format: format_media(media.iter().map(|m| m["format"].as_str().unwrap_or_default())),
        label,
        status: s(&r["status"]),
        disambiguation: s(&r["disambiguation"]),
        release_group_id: s(&r["release-group"]["id"]),
        score: r["score"].as_u64().unwrap_or(0) as u32,
    }
}

fn parse_release(r: &Value) -> Option<Release> {
    let id = s(&r["id"]);
    if id.is_empty() {
        return None;
    }
    let (artist, artist_id) = credit(&r["artist-credit"]);
    let label_info = r["label-info"].as_array().and_then(|l| l.first());
    let mut media = Vec::new();
    for (i, m) in r["media"].as_array().into_iter().flatten().enumerate() {
        let mut tracks = Vec::new();
        for (j, t) in m["tracks"].as_array().into_iter().flatten().enumerate() {
            let recording = &t["recording"];
            let (track_artist, track_artist_id) = match credit(&t["artist-credit"]) {
                (name, id) if !name.is_empty() => (name, id),
                _ => credit(&recording["artist-credit"]),
            };
            tracks.push(Track {
                id: s(&t["id"]),
                recording_id: s(&recording["id"]),
                position: t["position"].as_u64().map_or(j as u32 + 1, |p| p as u32),
                title: t["title"].as_str().or(recording["title"].as_str()).unwrap_or_default().to_string(),
                artist: if track_artist.is_empty() { artist.clone() } else { track_artist },
                artist_id: if track_artist_id.is_empty() { artist_id.clone() } else { track_artist_id },
                length_ms: t["length"].as_u64().or(recording["length"].as_u64()),
                isrc: recording["isrcs"].as_array().and_then(|i| i.first()).map(s).unwrap_or_default(),
            });
        }
        media.push(Medium {
            position: m["position"].as_u64().map_or(i as u32 + 1, |p| p as u32),
            format: s(&m["format"]),
            tracks,
        });
    }
    Some(Release {
        id,
        title: s(&r["title"]),
        artist,
        artist_id,
        date: s(&r["date"]),
        original_date: s(&r["release-group"]["first-release-date"]),
        country: s(&r["country"]),
        label: label_info.map(|l| s(&l["label"]["name"])).unwrap_or_default(),
        catalog_number: label_info.map(|l| s(&l["catalog-number"])).unwrap_or_default(),
        barcode: s(&r["barcode"]),
        release_group_id: s(&r["release-group"]["id"]),
        release_type: s(&r["release-group"]["primary-type"]),
        genre: top_genre(&r["genres"]),
        media,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = r#"{
        "id": "rel-1", "title": "Mezzanine", "date": "1998-04-20", "country": "GB", "barcode": "724384559924",
        "artist-credit": [{"name": "Massive Attack", "joinphrase": "", "artist": {"id": "art-1", "name": "Massive Attack"}}],
        "label-info": [{"catalog-number": "WBRCD4", "label": {"name": "Circa"}}],
        "release-group": {"id": "rg-1", "primary-type": "Album", "first-release-date": "1998-04-17"},
        "genres": [{"name": "trip hop", "count": 9}, {"name": "electronic", "count": 3}],
        "media": [{"position": 1, "format": "CD", "tracks": [
            {"id": "t-1", "position": 1, "number": "1", "title": "Angel", "length": 379000,
             "recording": {"id": "rec-1", "title": "Angel", "length": 379000, "isrcs": ["GBAAA9800001"]}},
            {"id": "t-2", "position": 2, "number": "2", "title": "Risingson", "length": 298000,
             "artist-credit": [{"name": "Massive Attack", "joinphrase": " feat. ", "artist": {"id": "art-1"}},
                               {"name": "Horace Andy", "joinphrase": "", "artist": {"id": "art-2"}}],
             "recording": {"id": "rec-2", "title": "Risingson"}}
        ]}]
    }"#;

    #[test]
    fn parses_a_release() {
        let r = parse_release(&serde_json::from_str(RELEASE).unwrap()).unwrap();
        assert_eq!(r.title, "Mezzanine");
        assert_eq!(r.artist, "Massive Attack");
        assert_eq!((r.label.as_str(), r.catalog_number.as_str()), ("Circa", "WBRCD4"));
        assert_eq!(r.year(), "1998");
        assert_eq!(r.genre, "Trip Hop");
        assert_eq!(r.track_count(), 2);
        let t = &r.media[0].tracks[1];
        assert_eq!(t.artist, "Massive Attack feat. Horace Andy");
        assert_eq!(t.length_ms, Some(298000));
        assert_eq!(r.media[0].tracks[0].isrc, "GBAAA9800001");
        assert_eq!(r.media[0].tracks[0].artist, "Massive Attack");
        assert_eq!(r.summary().format, "CD");
    }

    #[test]
    fn formats_and_phrases() {
        assert_eq!(format_media(["CD", "CD", "DVD"].into_iter()), "2×CD + DVD");
        assert_eq!(phrase(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(title_words("r&b"), "R&B");
    }
}
