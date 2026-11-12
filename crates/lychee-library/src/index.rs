//! The in-memory view of the library that the UI reads from.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use lychee_core::{
    Album, AlbumId, Artist, AudioFormat, Playlist, PlaylistId, Track, TrackId, TrackRef, album_id_for, genre_key,
};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlbumSort {
    #[default]
    Artist,
    Title,
    Year,
    RecentlyAdded,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrackSort {
    #[default]
    Artist,
    Title,
    Album,
    Year,
    Duration,
    Plays,
    RecentlyAdded,
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub tracks: usize,
    pub albums: usize,
    pub artists: usize,
    pub duration_ms: u64,
    pub bytes: u64,
    pub lossless_tracks: usize,
}

#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub albums: Vec<AlbumId>,
    pub artists: Vec<String>,
    pub tracks: Vec<TrackId>,
    pub playlists: Vec<PlaylistId>,
}

#[derive(Clone, Debug, Default)]
pub struct LibraryIndex {
    tracks: HashMap<TrackId, TrackRef>,
    albums: Vec<Album>,
    album_pos: HashMap<AlbumId, usize>,
    artists: Vec<Artist>,
    genres: Vec<(String, Vec<AlbumId>)>,
    playlists: Vec<Playlist>,
    accents: HashMap<String, crate::Colors>,
    stats: Stats,
}

impl LibraryIndex {
    pub fn build(mut tracks: Vec<Track>, playlists: Vec<Playlist>, accents: HashMap<String, crate::Colors>) -> Self {
        assign_album_artists(&mut tracks);
        for t in &mut tracks {
            t.album_id = album_id_for(&t.album_artist, &t.album);
        }

        let mut grouped: HashMap<AlbumId, Vec<&Track>> = HashMap::new();
        for t in tracks.iter().filter(|t| t.in_library) {
            grouped.entry(t.album_id).or_default().push(t);
        }
        let names = genre_names(tracks.iter().filter(|t| t.in_library));
        let mut albums: Vec<Album> = grouped.into_iter().map(|(id, ts)| make_album(id, ts, &accents, &names)).collect();
        albums.sort_by_cached_key(|a| (sort_key(&a.artist), a.year.unwrap_or(i32::MAX), sort_key(&a.title)));
        let album_pos = albums.iter().enumerate().map(|(i, a)| (a.id, i)).collect();

        let mut by_artist: HashMap<String, Artist> = HashMap::new();
        for a in &albums {
            let entry = by_artist.entry(a.artist.to_lowercase()).or_insert_with(|| Artist { name: a.artist.clone(), ..Default::default() });
            entry.albums.push(a.id);
            entry.track_count += a.tracks.len();
        }
        let mut artists: Vec<Artist> = by_artist.into_values().collect();
        artists.sort_by_cached_key(|a| sort_key(&a.name));

        // Album genres already use one spelling per genre, so they group by name.
        let mut by_genre: HashMap<&str, Vec<AlbumId>> = HashMap::new();
        for a in &albums {
            for g in &a.genres {
                by_genre.entry(g).or_default().push(a.id);
            }
        }
        let by_genre: Vec<(String, Vec<AlbumId>)> = by_genre.into_iter().map(|(g, ids)| (g.to_string(), ids)).collect();
        let mut genres = by_genre;
        genres.sort_by_cached_key(|(g, _)| g.to_lowercase());

        let library_tracks = tracks.iter().filter(|t| t.in_library);
        let stats = Stats {
            tracks: library_tracks.clone().count(),
            albums: albums.len(),
            artists: artists.len(),
            duration_ms: library_tracks.clone().map(|t| t.duration_ms).sum(),
            bytes: library_tracks.clone().map(|t| t.file_size).sum(),
            lossless_tracks: library_tracks.filter(|t| t.format.is_lossless()).count(),
        };
        let tracks = tracks.into_iter().map(|t| (t.id, Arc::new(t))).collect();
        LibraryIndex { tracks, albums, album_pos, artists, genres, playlists, accents, stats }
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn track(&self, id: TrackId) -> Option<TrackRef> {
        self.tracks.get(&id).cloned()
    }

    pub fn contains(&self, id: TrackId) -> bool {
        self.tracks.contains_key(&id)
    }

    pub fn album(&self, id: AlbumId) -> Option<&Album> {
        self.album_pos.get(&id).map(|&i| &self.albums[i])
    }

    pub fn album_tracks(&self, id: AlbumId) -> Vec<TrackRef> {
        self.album(id).map(|a| a.tracks.iter().filter_map(|t| self.track(*t)).collect()).unwrap_or_default()
    }

    pub fn albums(&self, sort: AlbumSort) -> Vec<&Album> {
        let mut out: Vec<&Album> = self.albums.iter().collect();
        match sort {
            AlbumSort::Artist => {}
            AlbumSort::Title => out.sort_by_cached_key(|a| sort_key(&a.title)),
            AlbumSort::Year => out.sort_by_key(|a| (std::cmp::Reverse(a.year.unwrap_or(0)), sort_key(&a.artist))),
            AlbumSort::RecentlyAdded => out.sort_by_key(|a| std::cmp::Reverse(a.added_at)),
        }
        out
    }

    pub fn artists(&self) -> &[Artist] {
        &self.artists
    }

    pub fn artist(&self, name: &str) -> Option<&Artist> {
        self.artists.iter().find(|a| a.name.eq_ignore_ascii_case(name))
    }

    pub fn artist_albums(&self, name: &str) -> Vec<&Album> {
        let mut albums: Vec<&Album> = self.artist(name).map(|a| a.albums.iter().filter_map(|id| self.album(*id)).collect()).unwrap_or_default();
        albums.sort_by_key(|a| std::cmp::Reverse(a.year.unwrap_or(0)));
        albums
    }

    /// Every library track by `name` (as track artist or album artist), most played first.
    pub fn artist_tracks(&self, name: &str) -> Vec<TrackRef> {
        let lower = name.to_lowercase();
        let mut out: Vec<TrackRef> = self
            .tracks
            .values()
            .filter(|t| {
                t.in_library
                    && (t.album_artist.to_lowercase() == lower
                        || t.display_main_artist().to_lowercase() == lower
                        || t.artist.to_lowercase().split(", ").any(|a| a == lower))
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| b.play_count.cmp(&a.play_count).then_with(|| sort_key(&a.title).cmp(&sort_key(&b.title))));
        out
    }

    /// Every genre with its albums, by name. "Hip-Hop", "Hip Hop" and "hip-hop" are one genre,
    /// named the way most of the library's songs spell it.
    pub fn genres(&self) -> &[(String, Vec<AlbumId>)] {
        &self.genres
    }

    /// The albums of a genre, matched however `name` is spelled.
    pub fn genre_albums(&self, name: &str) -> Option<&(String, Vec<AlbumId>)> {
        let key = genre_key(name);
        self.genres.iter().find(|(g, _)| genre_key(g) == key)
    }

    pub fn tracks(&self, sort: TrackSort) -> Vec<TrackRef> {
        let mut out: Vec<TrackRef> = self.tracks.values().filter(|t| t.in_library).cloned().collect();
        match sort {
            TrackSort::Artist => out.sort_by_cached_key(|t| {
                let album = self.album(t.album_id);
                (sort_key(&t.album_artist), album.and_then(|a| a.year).unwrap_or(0), sort_key(&t.album), t.disc_number.unwrap_or(1), t.track_number.unwrap_or(0))
            }),
            TrackSort::Title => out.sort_by_cached_key(|t| sort_key(&t.title)),
            TrackSort::Album => out.sort_by_cached_key(|t| (sort_key(&t.album), t.disc_number.unwrap_or(1), t.track_number.unwrap_or(0))),
            TrackSort::Year => out.sort_by_key(|t| (std::cmp::Reverse(t.year.unwrap_or(0)), t.album_id, t.track_number)),
            TrackSort::Duration => out.sort_by_key(|t| std::cmp::Reverse(t.duration_ms)),
            TrackSort::Plays => out.sort_by_key(|t| (std::cmp::Reverse(t.play_count), std::cmp::Reverse(t.last_played))),
            TrackSort::RecentlyAdded => out.sort_by_key(|t| (std::cmp::Reverse(t.added_at), t.album_id, t.track_number)),
        }
        out
    }

    /// Favourites, most recently liked first.
    pub fn liked(&self) -> Vec<TrackRef> {
        let mut out: Vec<TrackRef> = self.tracks.values().filter(|t| t.liked).cloned().collect();
        out.sort_by_key(|t| std::cmp::Reverse(t.liked_at.unwrap_or(0)));
        out
    }

    pub fn recently_played(&self, limit: usize) -> Vec<TrackRef> {
        let mut out: Vec<TrackRef> = self.tracks.values().filter(|t| t.last_played.is_some()).cloned().collect();
        out.sort_by_key(|t| std::cmp::Reverse(t.last_played));
        out.truncate(limit);
        out
    }

    pub fn playlists(&self) -> &[Playlist] {
        &self.playlists
    }

    pub fn playlist(&self, id: PlaylistId) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    pub fn colors(&self, artwork: &str) -> Option<crate::Colors> {
        self.accents.get(artwork).copied()
    }

    pub(crate) fn set_playlists(&mut self, playlists: Vec<Playlist>) {
        self.playlists = playlists;
    }

    pub(crate) fn update_track(&mut self, id: TrackId, f: impl FnOnce(&mut Track)) {
        if let Some(t) = self.tracks.get_mut(&id) {
            f(Arc::make_mut(t));
        }
    }

    /// Fuzzy search across albums, artists, tracks and playlists.
    pub fn search(&self, query: &str, limit: usize) -> SearchResults {
        let query = query.trim();
        if query.is_empty() {
            return SearchResults::default();
        }
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf = Vec::new();
        let mut score = |text: &str| pattern.score(Utf32Str::new(text, &mut buf), &mut matcher);

        fn top<T>(mut scored: Vec<(u32, T)>, limit: usize) -> Vec<T> {
            scored.sort_by(|a, b| b.0.cmp(&a.0));
            scored.into_iter().take(limit).map(|(_, v)| v).collect()
        }

        // Matches on the name itself count double, so "Tidal Memory" the song outranks every
        // song that only matches through its album.
        let mut weighted = |name: &str, context: &str| {
            let direct = score(name).map(|s| s * 2);
            let broad = score(&format!("{name} {context}"));
            direct.max(broad)
        };
        let albums = self.albums.iter().filter_map(|a| weighted(&a.title, &a.artist).map(|s| (s, a.id))).collect();
        let artists = self.artists.iter().filter_map(|a| weighted(&a.name, "").map(|s| (s, a.name.clone()))).collect();
        let tracks = self
            .tracks
            .values()
            .filter(|t| t.in_library)
            .filter_map(|t| weighted(&t.title, &format!("{} {}", t.artist, t.album)).map(|s| (s, t.id)))
            .collect();
        let playlists = self.playlists.iter().filter_map(|p| weighted(&p.name, "").map(|s| (s, p.id))).collect();
        SearchResults {
            albums: top(albums, limit),
            artists: top(artists, limit),
            tracks: top(tracks, limit * 2),
            playlists: top(playlists, limit),
        }
    }
}

/// Fill in album artists the files don't tag. Tracks of one album folder that share a main
/// artist get that artist (so one "A feat. B" track doesn't split an album); mixed folders become
/// "Various Artists" so compilations stay together.
fn assign_album_artists(tracks: &mut [Track]) {
    let mut groups: HashMap<(String, Option<PathBuf>), Vec<usize>> = HashMap::new();
    for (i, t) in tracks.iter().enumerate() {
        if t.album_artist.trim().is_empty() {
            groups.entry((t.album.to_lowercase(), t.path.parent().map(PathBuf::from))).or_default().push(i);
        }
    }
    for indexes in groups.values() {
        let first = tracks[indexes[0]].display_main_artist().to_string();
        let same = indexes.iter().all(|&i| tracks[i].display_main_artist().eq_ignore_ascii_case(&first));
        let effective = if same { first } else { "Various Artists".to_string() };
        for &i in indexes {
            tracks[i].album_artist = effective.clone();
        }
    }
}

/// The library's spelling of each genre, by [`genre_key`]: the one most of its songs use. Ties go
/// to the spelling with more capitals ("Hip-Hop" over "hip-hop"), then the alphabetically first.
fn genre_names<'a>(tracks: impl Iterator<Item = &'a Track>) -> HashMap<String, String> {
    let mut spellings: HashMap<String, HashMap<&str, usize>> = HashMap::new();
    for t in tracks {
        for g in t.genres() {
            *spellings.entry(genre_key(g)).or_default().entry(g).or_default() += 1;
        }
    }
    spellings
        .into_iter()
        .filter_map(|(key, counts)| {
            let capitals = |g: &str| g.chars().filter(|c| c.is_uppercase()).count();
            let best = counts.into_iter().max_by(|(a, na), (b, nb)| {
                na.cmp(nb).then(capitals(a).cmp(&capitals(b))).then(b.cmp(a))
            })?;
            Some((key, best.0.to_string()))
        })
        .collect()
}

fn make_album(
    id: AlbumId,
    mut ts: Vec<&Track>,
    accents: &HashMap<String, crate::Colors>,
    names: &HashMap<String, String>,
) -> Album {
    ts.sort_by_key(|t| (t.disc_number.unwrap_or(1), t.track_number.unwrap_or(u32::MAX), sort_key(&t.title)));
    let first = ts[0];
    let year = ts.iter().filter_map(|t| t.year).min();
    // Every genre of every song, the most common first.
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut keys: Vec<String> = Vec::new();
    for g in ts.iter().flat_map(|t| t.genres()) {
        let key = genre_key(g);
        let c = counts.entry(key.clone()).or_insert(0);
        if *c == 0 {
            keys.push(key);
        }
        *c += 1;
    }
    keys.sort_by_key(|k| std::cmp::Reverse(counts[k]));
    let genres = keys.iter().filter_map(|k| names.get(k).cloned()).collect();
    let artwork = most_common(ts.iter().filter_map(|t| t.artwork.clone()));
    let colors = artwork.as_ref().and_then(|k| accents.get(k).copied()).unwrap_or_default();
    let format = ts.iter().map(|t| t.format.clone()).max_by_key(format_rank).unwrap_or_default();
    Album {
        id,
        title: first.album.clone(),
        artist: first.album_artist.clone(),
        year,
        genres,
        tracks: ts.iter().map(|t| t.id).collect(),
        artwork,
        accent: colors.accent,
        tint: colors.tint,
        added_at: ts.iter().map(|t| t.added_at).min().unwrap_or(0),
        duration_ms: ts.iter().map(|t| t.duration_ms).sum(),
        format,
        folder: first.path.parent().map(PathBuf::from),
    }
}

fn format_rank(f: &AudioFormat) -> (bool, u8, u32, u32) {
    (f.is_lossless(), f.bit_depth.unwrap_or(0), f.sample_rate.unwrap_or(0), f.bitrate_kbps.unwrap_or(0))
}

fn most_common<T: Eq + std::hash::Hash + Clone>(items: impl Iterator<Item = T>) -> Option<T> {
    let mut counts: HashMap<T, usize> = HashMap::new();
    let mut order = Vec::new();
    for item in items {
        let c = counts.entry(item.clone()).or_insert(0);
        if *c == 0 {
            order.push(item);
        }
        *c += 1;
    }
    order.into_iter().max_by_key(|i| counts[i])
}

/// Case-insensitive sort key that ignores a leading "The ".
pub fn sort_key(s: &str) -> String {
    let lower = s.trim().to_lowercase();
    lower.strip_prefix("the ").map(str::to_string).unwrap_or(lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: TrackId, title: &str, artist: &str, album: &str, album_artist: &str, dir: &str, n: u32) -> Track {
        Track {
            id,
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            album_artist: album_artist.into(),
            path: PathBuf::from(format!("/music/{dir}/{n:02} {title}.flac")),
            track_number: Some(n),
            in_library: true,
            duration_ms: 1000,
            ..Default::default()
        }
    }

    #[test]
    fn groups_albums_and_detects_compilations() {
        let tracks = vec![
            track(1, "B", "Hollow Coast", "Tidal Memory", "", "hc", 2),
            track(2, "A", "Hollow Coast", "Tidal Memory", "", "hc", 1),
            track(3, "X", "Artist One", "Summer Mix", "", "mix", 1),
            track(4, "Y", "Artist Two", "Summer Mix", "", "mix", 2),
            track(5, "Z", "Guest", "Glasshouse", "Mira Valen", "mv", 1),
        ];
        let index = LibraryIndex::build(tracks, vec![], HashMap::new());
        assert_eq!(index.stats().albums, 3);
        let tidal = index.albums(AlbumSort::Title).into_iter().find(|a| a.title == "Tidal Memory").unwrap();
        assert_eq!(tidal.artist, "Hollow Coast");
        assert_eq!(tidal.tracks, vec![2, 1], "ordered by track number");
        assert!(index.albums(AlbumSort::Artist).iter().any(|a| a.title == "Summer Mix" && a.artist == "Various Artists"));
        assert!(index.artist("mira valen").is_some());
        assert_eq!(index.artist_tracks("Guest").len(), 1);
    }

    #[test]
    fn featured_artists_dont_make_a_compilation() {
        let mut multi = track(3, "C", "Hollow Coast, Mira Valen", "Tidal Memory", "", "hc", 3);
        multi.main_artist = "Hollow Coast".into();
        let tracks = vec![
            track(1, "A", "Hollow Coast", "Tidal Memory", "", "hc", 1),
            track(2, "B", "Hollow Coast feat. Mira Valen", "Tidal Memory", "", "hc", 2),
            multi,
        ];
        let index = LibraryIndex::build(tracks, vec![], HashMap::new());
        assert_eq!(index.stats().albums, 1);
        assert_eq!(index.albums(AlbumSort::Title)[0].artist, "Hollow Coast");
        assert_eq!(index.artist_tracks("Hollow Coast").len(), 3);
    }

    #[test]
    fn genre_spellings_are_one_genre() {
        let genre = |mut t: Track, g: &str| {
            t.genre = Some(g.into());
            t
        };
        let tracks = vec![
            genre(track(1, "A", "One", "First", "", "a", 1), "hip-hop"),
            genre(track(2, "B", "Two", "Second", "", "b", 1), "Hip Hop"),
            genre(track(3, "C", "Two", "Second", "", "b", 2), "Hip Hop"),
            genre(track(4, "D", "Three", "Third", "", "c", 1), "Hip-Hop; Rap"),
            genre(track(5, "E", "Four", "Fourth", "", "d", 1), "Hip-Hop/Rap"),
        ];
        let index = LibraryIndex::build(tracks, vec![], HashMap::new());
        let names: Vec<&str> = index.genres().iter().map(|(g, _)| g.as_str()).collect();
        assert_eq!(names, ["Hip Hop", "Hip-Hop/Rap", "Rap"], "the most used spelling names the genre");
        assert_eq!(index.genre_albums("hiphop").unwrap().1.len(), 3);
        let third = index.albums(AlbumSort::Title).into_iter().find(|a| a.title == "Third").unwrap();
        assert_eq!(third.genres, ["Hip Hop", "Rap"]);
    }

    #[test]
    fn fuzzy_search_finds_albums_and_tracks() {
        let tracks = vec![track(1, "Saltwater Hymn", "Hollow Coast", "Tidal Memory", "", "hc", 3)];
        let index = LibraryIndex::build(tracks, vec![], HashMap::new());
        let r = index.search("tid mem", 5);
        assert_eq!(r.albums.len(), 1);
        let r = index.search("saltwater", 5);
        assert_eq!(r.tracks, vec![1]);
        assert!(index.search("zzzz", 5).tracks.is_empty());

        let tracks = vec![
            track(1, "Lantern Season", "Hollow Coast", "Tidal Memory", "", "hc", 2),
            track(2, "Tidal Memory", "Hollow Coast", "Tidal Memory", "", "hc", 10),
        ];
        let index = LibraryIndex::build(tracks, vec![], HashMap::new());
        assert_eq!(index.search("tidal", 5).tracks.first(), Some(&2), "a title match ranks first");
    }

    #[test]
    fn sort_key_ignores_leading_the() {
        assert!(sort_key("The Quiet Engines") < sort_key("Rain"));
    }
}
