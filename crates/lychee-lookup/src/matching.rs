//! Working out which release a set of files is, and which file is which track.
//!
//! The guess comes from the tags first, then from folder names ("Artist - Album (1998)").
//! Candidates are ranked by how well their track lists fit the files: durations are the
//! strongest signal (they come from the audio, not from tags), then titles, track count and
//! the album and artist names.

use crate::musicbrainz::{Release, ReleaseSummary};
use crate::{Client, Result};

/// What we know about one file.
#[derive(Clone, Debug, Default)]
pub struct FileFacts {
    /// File name without extension.
    pub name: String,
    /// The folder the file is in, and the one above it.
    pub folder: String,
    pub parent_folder: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub duration_ms: u64,
    pub release_id: String,
    pub recording_id: String,
}

/// What to search for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Guess {
    pub album: String,
    pub artist: String,
    /// A MusicBrainz release id already in the tags.
    pub release_id: String,
    pub file_count: usize,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub summary: ReleaseSummary,
    /// The full release, once fetched.
    pub release: Option<Release>,
    /// 0–100: how well the release fits the files.
    pub score: u32,
    /// For each file, the (medium, track) indices into `release.media`.
    pub mapping: Vec<Option<(usize, usize)>>,
}

/// Values for one file from a release, ready to tag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrackTags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub year: String,
    pub genre: String,
    pub track: u32,
    pub track_total: u32,
    /// Zero for single-disc releases (nothing to say).
    pub disc: u32,
    pub disc_total: u32,
    pub label: String,
    pub catalog_number: String,
    pub isrc: String,
    pub release_id: String,
    pub release_group_id: String,
    pub recording_id: String,
    pub track_id: String,
    pub artist_id: String,
    pub album_artist_id: String,
}

pub fn track_tags(release: &Release, medium: usize, track: usize) -> Option<TrackTags> {
    let m = release.media.get(medium)?;
    let t = m.tracks.get(track)?;
    let multi_disc = release.media.len() > 1;
    Some(TrackTags {
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: release.title.clone(),
        album_artist: release.artist.clone(),
        year: release.year().to_string(),
        genre: release.genre.clone(),
        track: t.position,
        track_total: m.tracks.len() as u32,
        disc: if multi_disc { m.position } else { 0 },
        disc_total: if multi_disc { release.media.len() as u32 } else { 0 },
        label: release.label.clone(),
        catalog_number: release.catalog_number.clone(),
        isrc: t.isrc.clone(),
        release_id: release.id.clone(),
        release_group_id: release.release_group_id.clone(),
        recording_id: t.recording_id.clone(),
        track_id: t.id.clone(),
        artist_id: t.artist_id.clone(),
        album_artist_id: release.artist_id.clone(),
    })
}

/// A title from a file name without tags: "01 - Angel" -> "Angel", "3. Teardrop" -> "Teardrop".
pub fn title_from_file_name(name: &str) -> String {
    let rest = name.trim_start_matches(|c: char| c.is_ascii_digit());
    if rest.len() == name.len() {
        return name.trim().to_string();
    }
    let rest = rest.trim_start_matches([' ', '.', '-', '_', ')', '–']);
    if rest.is_empty() { name.trim().to_string() } else { rest.trim().to_string() }
}

/// Folder names that say nothing about the music in them.
const GENERIC_FOLDERS: [&str; 16] = [
    "music", "musik", "musique", "downloads", "download", "desktop", "documents", "home", "media", "audio", "mp3", "flac",
    "albums", "unsorted", "new folder", "various",
];

pub fn guess(files: &[FileFacts]) -> Guess {
    let release_id = most_common(files.iter().map(|f| f.release_id.as_str()), 0.5);
    let album = most_common(files.iter().map(|f| f.album.as_str()), 0.0);
    let album_artist = most_common(files.iter().map(|f| f.album_artist.as_str()), 0.0);
    let artist = most_common(files.iter().map(|f| f.artist.as_str()), 0.5);
    let folder = most_common(files.iter().map(|f| f.folder.as_str()), 0.0);
    let parent = most_common(files.iter().map(|f| f.parent_folder.as_str()), 0.0);
    let (folder_artist, folder_album) = split_folder(&folder);
    let album = if album.is_empty() && !is_generic(&folder_album) { folder_album } else { album };
    let parent_artist = if is_generic(&parent) { String::new() } else { split_folder(&parent).1 };
    let artist = [album_artist, artist, folder_artist, parent_artist].into_iter().find(|a| !a.is_empty()).unwrap_or_default();
    Guess { album, artist, release_id, file_count: files.len() }
}

fn is_generic(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    lower.is_empty() || GENERIC_FOLDERS.contains(&lower.as_str()) || lower.starts_with("cd") && lower[2..].trim().parse::<u32>().is_ok()
}

/// The most common non-empty value, if it covers more than `share` of the items.
fn most_common<'a>(values: impl Iterator<Item = &'a str>, share: f32) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    let mut total = 0;
    for v in values {
        total += 1;
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        match counts.iter_mut().find(|(x, _)| *x == v) {
            Some((_, n)) => *n += 1,
            None => counts.push((v, 1)),
        }
    }
    match counts.into_iter().max_by_key(|(_, n)| *n) {
        Some((v, n)) if n as f32 > share * total as f32 => v.to_string(),
        _ => String::new(),
    }
}

fn is_year(s: &str) -> bool {
    let s = s.trim().trim_matches(['(', ')', '[', ']']);
    s.len() == 4 && s.chars().all(|c| c.is_ascii_digit()) && (s.starts_with("19") || s.starts_with("20"))
}

/// Drop "[FLAC]", "(1998)", "[24-96]" and similar noise from a folder name.
fn strip_noise(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    let mut group = String::new();
    for c in name.chars() {
        match c {
            '[' | '(' | '{' => {
                if depth == 0 {
                    group.clear();
                }
                depth += 1;
                group.push(c);
            }
            ']' | ')' | '}' if depth > 0 => {
                depth -= 1;
                group.push(c);
                if depth == 0 {
                    let inner = group[1..group.len() - 1].to_lowercase();
                    let noise = is_year(&inner)
                        || inner.chars().any(|c| c.is_ascii_digit()) && !inner.chars().any(char::is_alphabetic)
                        || ["flac", "mp3", "320", "v0", "web", "cd", "vinyl", "lossless", "24bit", "16bit", "khz", "aac", "ogg", "opus", "alac"]
                            .iter()
                            .any(|w| inner.split(|c: char| !c.is_alphanumeric()).any(|t| t == *w));
                    if !noise {
                        out.push_str(&group);
                    }
                }
            }
            c if depth > 0 => group.push(c),
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "Artist - Album (1998) [FLAC]" -> ("Artist", "Album"); "1998 - Album" -> ("", "Album").
fn split_folder(name: &str) -> (String, String) {
    let cleaned = strip_noise(name);
    let parts: Vec<&str> = cleaned.split(" - ").map(str::trim).filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [] => (String::new(), String::new()),
        [one] => (String::new(), one.to_string()),
        [a, b] if is_year(a) => (String::new(), b.to_string()),
        [a, rest @ ..] => {
            let rest: Vec<&str> = rest.iter().copied().filter(|p| !is_year(p)).collect();
            (a.to_string(), rest.join(" - "))
        }
    }
}

fn normalize(s: &str) -> String {
    let lower = s.to_lowercase().replace('&', " and ");
    let cleaned: String = lower.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.strip_prefix("the ").map(str::to_string).unwrap_or(joined)
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// 0–1 likeness of two names, forgiving case, punctuation and "(Remastered)"-style suffixes.
pub fn similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (normalize(a), normalize(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let (ca, cb): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let ratio = 1.0 - levenshtein(&ca, &cb) as f32 / ca.len().max(cb.len()) as f32;
    let contained = if a.starts_with(&b) || b.starts_with(&a) {
        0.85
    } else if a.contains(&b) || b.contains(&a) {
        0.7
    } else {
        0.0
    };
    ratio.max(contained)
}

/// How well a release's track count fits the files: more tracks than files is fine (a partial
/// album), fewer is not.
fn count_fit(tracks: usize, files: usize) -> f32 {
    if tracks == 0 || files == 0 {
        return 0.3;
    }
    if tracks >= files {
        0.55 + 0.45 * files as f32 / tracks as f32
    } else {
        (tracks as f32 / files as f32).powi(2) * 0.5
    }
}

/// Rank before fetching track lists.
pub fn prescore(s: &ReleaseSummary, guess: &Guess) -> f32 {
    let mb = s.score as f32 / 100.0;
    let count = count_fit(s.track_count, guess.file_count);
    let title = if guess.album.is_empty() { 0.5 } else { similarity(&s.title, &guess.album) };
    let artist = if guess.artist.is_empty() { 0.5 } else { similarity(&s.artist, &guess.artist) };
    let official = if s.status.is_empty() || s.status == "Official" { 1.0 } else { 0.7 };
    (0.25 * mb + 0.35 * count + 0.25 * title + 0.15 * artist) * official
}

fn duration_fit(track_ms: Option<u64>, file_ms: u64) -> Option<f32> {
    let track_ms = track_ms?;
    if file_ms == 0 {
        return None;
    }
    let diff = (track_ms as f64 - file_ms as f64).abs() / 1000.0;
    Some(if diff <= 2.0 {
        1.0
    } else if diff >= 20.0 {
        0.0
    } else {
        (1.0 - (diff - 2.0) / 18.0) as f32
    })
}

/// Which track each file is: recording ids, then disc and track numbers, then titles, then
/// durations, then list order.
pub fn map_tracks(release: &Release, files: &[FileFacts]) -> Vec<Option<(usize, usize)>> {
    let all: Vec<(usize, usize)> =
        release.media.iter().enumerate().flat_map(|(mi, m)| (0..m.tracks.len()).map(move |ti| (mi, ti))).collect();
    let track = |k: usize| &release.media[all[k].0].tracks[all[k].1];
    let mut used = vec![false; all.len()];
    let mut out: Vec<Option<usize>> = vec![None; files.len()];

    for (i, f) in files.iter().enumerate() {
        if f.recording_id.is_empty() {
            continue;
        }
        if let Some(k) = (0..all.len()).find(|&k| !used[k] && track(k).recording_id == f.recording_id) {
            used[k] = true;
            out[i] = Some(k);
        }
    }
    for (i, f) in files.iter().enumerate() {
        let (None, Some(n)) = (out[i], f.track) else { continue };
        let disc = f.disc.unwrap_or(1).max(1);
        let medium = release.media.iter().position(|m| m.position == disc).or((release.media.len() == 1).then_some(0));
        let Some(mi) = medium else { continue };
        let Some(k) = (0..all.len()).find(|&k| all[k].0 == mi && track(k).position == n) else { continue };
        let t = track(k);
        let plausible = f.title.is_empty()
            || similarity(&t.title, &f.title) >= 0.35
            || duration_fit(t.length_ms, f.duration_ms).is_some_and(|d| d >= 0.9);
        if !used[k] && plausible {
            used[k] = true;
            out[i] = Some(k);
        }
    }
    // Best pairs first, for titles and then durations.
    let assign = |out: &mut Vec<Option<usize>>, used: &mut Vec<bool>, score: &dyn Fn(usize, usize) -> Option<f32>, min: f32| {
        let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
        for i in (0..files.len()).filter(|&i| out[i].is_none()) {
            for k in (0..all.len()).filter(|&k| !used[k]) {
                if let Some(s) = score(i, k).filter(|s| *s >= min) {
                    pairs.push((s, i, k));
                }
            }
        }
        pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, i, k) in pairs {
            if out[i].is_none() && !used[k] {
                out[i] = Some(k);
                used[k] = true;
            }
        }
    };
    assign(&mut out, &mut used, &|i, k| (!files[i].title.is_empty()).then(|| similarity(&track(k).title, &files[i].title)), 0.6);
    assign(&mut out, &mut used, &|i, k| duration_fit(track(k).length_ms, files[i].duration_ms), 0.9);
    let free_files: Vec<usize> = (0..files.len()).filter(|&i| out[i].is_none()).collect();
    let free_tracks: Vec<usize> = (0..all.len()).filter(|&k| !used[k]).collect();
    if !free_files.is_empty() && free_files.len() == free_tracks.len() {
        for (i, k) in free_files.into_iter().zip(free_tracks) {
            out[i] = Some(k);
        }
    }
    out.into_iter().map(|k| k.map(|k| all[k])).collect()
}

/// Score a fetched release against the files.
pub fn evaluate(release: &Release, files: &[FileFacts], guess: &Guess) -> (u32, Vec<Option<(usize, usize)>>) {
    let mapping = map_tracks(release, files);
    let (mut durations, mut duration_n, mut titles, mut title_n) = (0.0f32, 0, 0.0f32, 0);
    for (f, m) in files.iter().zip(&mapping) {
        let Some((mi, ti)) = m else { continue };
        let t = &release.media[*mi].tracks[*ti];
        if let Some(d) = duration_fit(t.length_ms, f.duration_ms) {
            durations += d;
            duration_n += 1;
        }
        if !f.title.is_empty() {
            titles += similarity(&t.title, &f.title);
            title_n += 1;
        }
    }
    let mapped = mapping.iter().filter(|m| m.is_some()).count() as f32 / files.len().max(1) as f32;
    let durations = if duration_n > 0 { durations / duration_n as f32 } else { 0.5 };
    let titles = if title_n > 0 { titles / title_n as f32 } else { 0.5 };
    let count = count_fit(release.track_count(), files.len());
    let album = if guess.album.is_empty() { 0.5 } else { similarity(&release.title, &guess.album) };
    let artist = if guess.artist.is_empty() { 0.5 } else { similarity(&release.artist, &guess.artist) };
    let mut score = 0.32 * durations + 0.2 * titles + 0.18 * count + 0.15 * album + 0.1 * artist + 0.05 * mapped;
    if !guess.release_id.is_empty() && guess.release_id == release.id {
        score = score.max(0.99);
    }
    ((score * 100.0).round().clamp(0.0, 100.0) as u32, mapping)
}

impl Candidate {
    pub fn fetched(release: Release, files: &[FileFacts], guess: &Guess) -> Candidate {
        let (score, mapping) = evaluate(&release, files, guess);
        Candidate { summary: release.summary(), release: Some(release), score, mapping }
    }
}

/// Search MusicBrainz for the files and rank what comes back. Track lists are fetched for the
/// `detailed` best hits (one second each, MusicBrainz's rate limit).
pub fn find_candidates(client: &Client, files: &[FileFacts], guess: &Guess, detailed: usize) -> Result<Vec<Candidate>> {
    let mut out: Vec<Candidate> = Vec::new();
    if !guess.release_id.is_empty() {
        if let Some(release) = client.release(&guess.release_id)? {
            out.push(Candidate::fetched(release, files, guess));
        }
    }
    let mut summaries = Vec::new();
    if !guess.album.is_empty() {
        summaries = client.search_releases(&guess.album, &guess.artist, 15)?;
        if summaries.is_empty() && !guess.artist.is_empty() {
            summaries = client.search_releases(&guess.album, "", 15)?;
        }
    }
    if summaries.is_empty() && files.len() <= 3 {
        if let Some(f) = files.iter().find(|f| !f.title.is_empty() || !f.name.is_empty()) {
            let title = if f.title.is_empty() { &f.name } else { &f.title };
            let artist = if f.artist.is_empty() { &guess.artist } else { &f.artist };
            summaries = client.search_recordings(title, artist, 10)?;
        }
    }
    summaries.retain(|s| !out.iter().any(|c| c.summary.id == s.id));
    let mut ranked: Vec<(f32, ReleaseSummary)> = summaries.into_iter().map(|s| (prescore(&s, guess), s)).collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    let budget = detailed.saturating_sub(out.len());
    for (i, (pre, summary)) in ranked.into_iter().enumerate() {
        if i < budget {
            if let Some(release) = client.release(&summary.id)? {
                let mut c = Candidate::fetched(release, files, guess);
                c.summary.status = summary.status;
                c.summary.disambiguation = summary.disambiguation;
                out.push(c);
                continue;
            }
        }
        out.push(Candidate { score: (pre * 100.0).round() as u32, summary, release: None, mapping: Vec::new() });
    }
    // Fetched candidates are scored on the real track list and come first.
    out.sort_by(|a, b| b.release.is_some().cmp(&a.release.is_some()).then(b.score.cmp(&a.score)));
    if let Some(release) = out.first_mut().and_then(|c| c.release.as_mut()) {
        client.complete_genre(release);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::musicbrainz::{Medium, Track};

    fn file(name: &str, title: &str, track: Option<u32>, ms: u64) -> FileFacts {
        FileFacts {
            name: name.into(),
            folder: "Massive Attack - Mezzanine (1998) [FLAC]".into(),
            parent_folder: "Music".into(),
            title: title.into(),
            track,
            duration_ms: ms,
            ..Default::default()
        }
    }

    fn release() -> Release {
        let t = |p: u32, title: &str, ms: u64| Track {
            id: format!("t{p}"),
            recording_id: format!("r{p}"),
            position: p,
            title: title.into(),
            length_ms: Some(ms),
            ..Default::default()
        };
        Release {
            id: "rel".into(),
            title: "Mezzanine".into(),
            artist: "Massive Attack".into(),
            media: vec![Medium {
                position: 1,
                format: "CD".into(),
                tracks: vec![t(1, "Angel", 379_000), t(2, "Risingson", 298_000), t(3, "Teardrop", 330_000)],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn guesses_from_folders() {
        let g = guess(&[file("01", "", None, 0), file("02", "", None, 0)]);
        assert_eq!((g.artist.as_str(), g.album.as_str()), ("Massive Attack", "Mezzanine"));
        assert_eq!(split_folder("1998 - Mezzanine"), (String::new(), "Mezzanine".into()));
        assert_eq!(split_folder("Artist - 2001 - Album [WEB 320]"), ("Artist".into(), "Album".into()));
        assert_eq!(split_folder("Album (Deluxe Edition)"), (String::new(), "Album (Deluxe Edition)".into()));
    }

    #[test]
    fn tags_beat_folders() {
        let mut f = file("01", "", None, 0);
        f.album = "Real Album".into();
        f.album_artist = "Real Artist".into();
        let g = guess(&[f]);
        assert_eq!((g.artist.as_str(), g.album.as_str()), ("Real Artist", "Real Album"));
    }

    #[test]
    fn maps_by_duration_when_tags_are_missing() {
        let files = [file("a", "", None, 330_500), file("b", "", None, 378_000), file("c", "", None, 297_000)];
        let mapping = map_tracks(&release(), &files);
        assert_eq!(mapping, vec![Some((0, 2)), Some((0, 0)), Some((0, 1))]);
        let (score, _) = evaluate(&release(), &files, &guess(&files));
        assert!(score >= 80, "score {score}");
    }

    #[test]
    fn maps_by_number_and_title() {
        let files = [file("x", "Teardrop (Remastered)", None, 0), file("y", "whatever", Some(1), 0)];
        let mapping = map_tracks(&release(), &files);
        assert_eq!(mapping[0], Some((0, 2)));
        // Track 1 is "Angel": the title contradicts the number and there is no duration to
        // settle it, and one file for two free tracks is no basis for guessing by order.
        assert_eq!(mapping[1], None);
    }

    #[test]
    fn titles_from_names() {
        assert_eq!(title_from_file_name("01 - Angel"), "Angel");
        assert_eq!(title_from_file_name("3. Teardrop"), "Teardrop");
        assert_eq!(title_from_file_name("Intro"), "Intro");
        assert_eq!(title_from_file_name("1999"), "1999");
    }

    #[test]
    fn similarity_is_forgiving() {
        assert_eq!(similarity("The Beatles", "beatles"), 1.0);
        assert!(similarity("Angel (2019 Remaster)", "Angel") >= 0.85);
        assert!(similarity("Angel", "Teardrop") < 0.3);
        assert_eq!(similarity("Simon & Garfunkel", "Simon and Garfunkel"), 1.0);
    }

    #[test]
    fn wrong_track_count_scores_lower() {
        let files = [file("a", "Angel", Some(1), 379_000)];
        let mut big = release();
        big.media[0].tracks.extend((4..30).map(|p| Track { position: p, title: format!("x{p}"), ..Default::default() }));
        let g = guess(&files);
        assert!(evaluate(&release(), &files, &g).0 > evaluate(&big, &files, &g).0);
    }
}
