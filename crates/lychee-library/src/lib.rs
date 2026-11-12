//! Lychee's music library: scanning folders, reading tags and artwork, storing everything in
//! SQLite, and serving a fast in-memory index to the UI.

mod artwork;
mod db;
mod index;
pub mod lyrics;
mod tags;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use anyhow::Context;
use lychee_core::{PlaylistId, TrackId, now_unix};
use parking_lot::{Mutex, RwLock};
use walkdir::WalkDir;

pub use artwork::{ArtworkCache, Colors, LARGE, THUMB, folder_image};
pub use db::TrackRecord;
pub use index::{AlbumSort, LibraryIndex, SearchResults, Stats, TrackSort, sort_key};

/// Progress reported while scanning.
#[derive(Clone, Copy, Debug)]
pub enum ScanProgress {
    /// Walking folders; `found` audio files so far.
    Discovering { found: usize },
    /// Reading tags of new or changed files.
    Reading { done: usize, total: usize },
}

#[derive(Clone, Debug, Default)]
pub struct ScanSummary {
    pub added_or_updated: usize,
    pub removed: usize,
    pub total: usize,
}

pub struct Library {
    db: Mutex<db::Db>,
    index: RwLock<Arc<LibraryIndex>>,
    artwork: ArtworkCache,
    scanning: AtomicBool,
    measuring: AtomicBool,
}

impl Library {
    /// Open (or create) the library database in `data_dir`, with artwork cached in `cache_dir`.
    pub fn open(data_dir: &Path, cache_dir: &Path) -> anyhow::Result<Library> {
        std::fs::create_dir_all(data_dir).context("creating the data directory")?;
        let db = db::Db::open(&data_dir.join("library.db")).context("opening the library database")?;
        let artwork = ArtworkCache::new(cache_dir.join("artwork")).context("creating the artwork cache")?;
        let library = Library {
            db: Mutex::new(db),
            index: RwLock::new(Arc::default()),
            artwork,
            scanning: AtomicBool::new(false),
            measuring: AtomicBool::new(false),
        };
        library.reload()?;
        Ok(library)
    }

    /// A consistent snapshot of the library. Cheap to call; hold it only briefly.
    pub fn index(&self) -> Arc<LibraryIndex> {
        self.index.read().clone()
    }

    /// Rebuild the in-memory index from the database.
    pub fn reload(&self) -> anyhow::Result<()> {
        let (tracks, playlists, accents) = {
            let db = self.db.lock();
            let accents: HashMap<String, Colors> = db.artwork_colors()?.into_iter().collect();
            (db.all_tracks()?, db.playlists()?, accents)
        };
        let index = LibraryIndex::build(tracks, playlists, accents);
        *self.index.write() = Arc::new(index);
        Ok(())
    }

    pub fn artwork_thumb(&self, key: &str) -> PathBuf {
        self.artwork.thumb_path(key)
    }

    pub fn artwork_large(&self, key: &str) -> PathBuf {
        self.artwork.large_path(key)
    }

    // ---- folders and scanning ------------------------------------------------------------

    pub fn folders(&self) -> Vec<PathBuf> {
        self.db.lock().folders().unwrap_or_default()
    }

    pub fn add_folder(&self, path: &Path) -> anyhow::Result<()> {
        self.db.lock().add_folder(path)?;
        Ok(())
    }

    /// Forget a folder. Its tracks disappear on the next scan.
    pub fn remove_folder(&self, path: &Path) -> anyhow::Result<()> {
        self.db.lock().remove_folder(path)?;
        Ok(())
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning.load(Ordering::Relaxed)
    }

    /// Bring the database in line with the library folders. Only new or changed files are read.
    pub fn scan(&self, progress: &(dyn Fn(ScanProgress) + Sync)) -> anyhow::Result<ScanSummary> {
        if self.scanning.swap(true, Ordering::SeqCst) {
            return Ok(ScanSummary::default());
        }
        let result = self.scan_inner(progress);
        self.scanning.store(false, Ordering::SeqCst);
        result
    }

    fn scan_inner(&self, progress: &(dyn Fn(ScanProgress) + Sync)) -> anyhow::Result<ScanSummary> {
        let folders = self.folders();
        let existing = self.db.lock().file_stamps()?;

        let mut found: Vec<(PathBuf, i64, u64)> = Vec::new();
        for folder in &folders {
            for entry in WalkDir::new(folder).follow_links(true).into_iter().filter_map(Result::ok) {
                if !entry.file_type().is_file() || !is_audio(entry.path()) {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                found.push((entry.into_path(), mtime, meta.len()));
                if found.len() % 250 == 0 {
                    progress(ScanProgress::Discovering { found: found.len() });
                }
            }
        }
        progress(ScanProgress::Discovering { found: found.len() });

        let known: HashMap<&Path, (i64, u64)> = existing.iter().map(|(_, p, m, s)| (p.as_path(), (*m, *s))).collect();
        let found_paths: HashSet<&Path> = found.iter().map(|(p, _, _)| p.as_path()).collect();
        let removed: Vec<TrackId> = existing.iter().filter(|(_, p, _, _)| !found_paths.contains(p.as_path())).map(|(id, ..)| *id).collect();
        let to_read: Vec<&PathBuf> = found
            .iter()
            .filter(|(p, m, s)| known.get(p.as_path()) != Some(&(*m, *s)))
            .map(|(p, _, _)| p)
            .collect();

        let total = to_read.len();
        let done = AtomicUsize::new(0);
        let art = ArtIngest::new(&self.artwork);
        let records: Vec<TrackRecord> = parallel_filter_map(&to_read, all_cores(), |path| {
                let meta = match tags::read(path) {
                    Ok(meta) => meta,
                    Err(e) => {
                        tracing::warn!("skipping {}: {e}", path.display());
                        return None;
                    }
                };
                let mut record = meta.record;
                record.artwork = match meta.picture {
                    Some(bytes) => art.ingest(&bytes),
                    None => path.parent().and_then(|dir| art.folder(dir)),
                };
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                if n % 20 == 0 || n == total {
                    progress(ScanProgress::Reading { done: n, total });
                }
                Some(record)
        });

        {
            let mut db = self.db.lock();
            db.upsert_tracks(&records, true)?;
            db.delete_tracks(&removed)?;
            for (key, colors) in art.into_new() {
                db.put_artwork(&key, colors)?;
            }
        }
        self.reload()?;
        Ok(ScanSummary { added_or_updated: records.len(), removed: removed.len(), total: found.len() })
    }

    pub fn is_measuring(&self) -> bool {
        self.measuring.load(Ordering::Relaxed)
    }

    /// Measure the loudness of library songs that have no ReplayGain tags, for volume levelling.
    /// Uses half the cores so playback and the UI stay smooth; results are saved as they come, so
    /// a stopped run continues where it left off. `progress(done, total)` follows each batch, and
    /// the index is refreshed every few seconds and at the end. Returns how many songs were measured.
    pub fn measure_loudness(&self, stop: &AtomicBool, progress: &(dyn Fn(usize, usize) + Sync)) -> anyhow::Result<usize> {
        if self.measuring.swap(true, Ordering::SeqCst) {
            return Ok(0);
        }
        let result = self.measure_inner(stop, progress);
        self.measuring.store(false, Ordering::SeqCst);
        result
    }

    fn measure_inner(&self, stop: &AtomicBool, progress: &(dyn Fn(usize, usize) + Sync)) -> anyhow::Result<usize> {
        let workers = (all_cores() / 2).max(1);
        let mut measured = 0;
        let mut tried: HashSet<TrackId> = HashSet::new();
        loop {
            // Songs added by a scan during the run are picked up by the next round.
            let todo: Vec<(TrackId, PathBuf)> =
                self.db.lock().loudness_todo()?.into_iter().filter(|(id, _)| !tried.contains(id)).collect();
            if todo.is_empty() || stop.load(Ordering::Relaxed) {
                break;
            }
            tried.extend(todo.iter().map(|(id, _)| *id));
            let total = todo.len();
            let mut last_reload = std::time::Instant::now();
            progress(0, total);
            for (n, batch) in todo.chunks(workers * 4).enumerate() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let results = parallel_filter_map(batch, workers, |(id, path)| {
                    if stop.load(Ordering::Relaxed) {
                        return None;
                    }
                    let m = match lychee_audio::loudness::measure(path) {
                        Ok(l) => Some(lychee_core::Measured { lufs: l.lufs.map(|v| v as f32), peak: Some(l.peak) }),
                        Err(e) => {
                            tracing::debug!("could not measure {}: {e}", path.display());
                            None
                        }
                    };
                    Some((*id, m))
                });
                measured += results.iter().filter(|(_, m)| m.is_some()).count();
                self.db.lock().set_loudness(&results)?;
                progress((n * workers * 4 + batch.len()).min(total), total);
                if last_reload.elapsed() > std::time::Duration::from_secs(10) {
                    self.reload()?;
                    last_reload = std::time::Instant::now();
                }
            }
            self.reload()?;
        }
        Ok(measured)
    }

    /// Make files that aren't in the library playable ("Open with…", drag and drop, CLI).
    /// Folders are expanded. Returns track ids in a sensible play order.
    pub fn import_paths(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<TrackId>> {
        let mut files = Vec::new();
        for p in paths {
            if p.is_dir() {
                let mut inner: Vec<PathBuf> = WalkDir::new(p)
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().is_file() && is_audio(e.path()))
                    .map(|e| e.into_path())
                    .collect();
                inner.sort();
                files.extend(inner);
            } else if p.is_file() {
                files.push(p.clone());
            }
        }
        let mut ids = Vec::with_capacity(files.len());
        let mut to_insert = Vec::new();
        let art = ArtIngest::new(&self.artwork);
        for f in &files {
            if let Some(id) = self.db.lock().track_id_for_path(f)? {
                ids.push(Some(id));
                continue;
            }
            match tags::read(f) {
                Ok(meta) => {
                    let mut record = meta.record;
                    record.artwork = match meta.picture {
                        Some(bytes) => art.ingest(&bytes),
                        None => f.parent().and_then(|dir| art.folder(dir)),
                    };
                    to_insert.push(record);
                    ids.push(None);
                }
                Err(e) => tracing::warn!("cannot open {}: {e}", f.display()),
            }
        }
        if !to_insert.is_empty() {
            let mut db = self.db.lock();
            let mut inserted = db.upsert_tracks(&to_insert, false)?.into_iter();
            for (key, colors) in art.into_new() {
                db.put_artwork(&key, colors)?;
            }
            drop(db);
            for slot in ids.iter_mut().filter(|s| s.is_none()) {
                *slot = inserted.next();
            }
            self.reload()?;
        }
        Ok(ids.into_iter().flatten().collect())
    }

    /// Re-read files whose tags changed outside a scan (the tag editor), following renames.
    /// Each pair is (old path, new path). Track ids, likes, play counts and playlists stay.
    /// Files the library doesn't know are left alone.
    pub fn refresh_files(&self, files: &[(PathBuf, PathBuf)]) -> anyhow::Result<usize> {
        let mut known = Vec::new();
        {
            let db = self.db.lock();
            for (old, new) in files {
                if old != new {
                    if let Err(e) = db.rename_path(old, new) {
                        tracing::warn!("could not move {} in the library: {e}", old.display());
                    }
                }
                if db.track_id_for_path(new)?.is_some() {
                    known.push(new.clone());
                }
            }
        }
        if known.is_empty() {
            return Ok(0);
        }
        let art = ArtIngest::new(&self.artwork);
        let mut records = Vec::with_capacity(known.len());
        for path in &known {
            match tags::read(path) {
                Ok(meta) => {
                    let mut record = meta.record;
                    record.artwork = match meta.picture {
                        Some(bytes) => art.ingest(&bytes),
                        None => path.parent().and_then(|dir| art.folder(dir)),
                    };
                    records.push(record);
                }
                Err(e) => tracing::warn!("could not re-read {}: {e}", path.display()),
            }
        }
        {
            let mut db = self.db.lock();
            db.upsert_tracks(&records, false)?;
            for (key, colors) in art.into_new() {
                db.put_artwork(&key, colors)?;
            }
        }
        self.reload()?;
        Ok(records.len())
    }

    // ---- user data -----------------------------------------------------------------------

    pub fn set_liked(&self, id: TrackId, liked: bool) -> anyhow::Result<()> {
        self.db.lock().set_liked(id, liked)?;
        let now = now_unix();
        let mut index = self.index.write();
        Arc::make_mut(&mut index).update_track(id, |t| {
            t.liked = liked;
            t.liked_at = liked.then_some(now);
        });
        Ok(())
    }

    pub fn record_play(&self, id: TrackId) -> anyhow::Result<()> {
        let now = now_unix();
        self.db.lock().record_play(id, now)?;
        let mut index = self.index.write();
        Arc::make_mut(&mut index).update_track(id, |t| {
            t.play_count += 1;
            t.last_played = Some(now);
        });
        Ok(())
    }

    fn refresh_playlists(&self) -> anyhow::Result<()> {
        let playlists = self.db.lock().playlists()?;
        let mut index = self.index.write();
        Arc::make_mut(&mut index).set_playlists(playlists);
        Ok(())
    }

    pub fn create_playlist(&self, name: &str, tracks: &[TrackId]) -> anyhow::Result<PlaylistId> {
        let id = {
            let mut db = self.db.lock();
            let id = db.create_playlist(name)?;
            db.set_playlist_tracks(id, tracks)?;
            id
        };
        self.refresh_playlists()?;
        Ok(id)
    }

    pub fn rename_playlist(&self, id: PlaylistId, name: &str) -> anyhow::Result<()> {
        self.db.lock().rename_playlist(id, name)?;
        self.refresh_playlists()
    }

    pub fn delete_playlist(&self, id: PlaylistId) -> anyhow::Result<()> {
        self.db.lock().delete_playlist(id)?;
        self.refresh_playlists()
    }

    pub fn set_playlist_tracks(&self, id: PlaylistId, tracks: &[TrackId]) -> anyhow::Result<()> {
        self.db.lock().set_playlist_tracks(id, tracks)?;
        self.refresh_playlists()
    }

    /// Append songs that aren't in the playlist yet. Returns how many were added; the rest were
    /// already there (or repeated in `tracks`).
    pub fn add_to_playlist(&self, id: PlaylistId, tracks: &[TrackId]) -> anyhow::Result<usize> {
        let mut all = self.index().playlist(id).map(|p| p.tracks.clone()).unwrap_or_default();
        let mut present: HashSet<TrackId> = all.iter().copied().collect();
        let before = all.len();
        all.extend(tracks.iter().copied().filter(|t| present.insert(*t)));
        let added = all.len() - before;
        if added > 0 {
            self.set_playlist_tracks(id, &all)?;
        }
        Ok(added)
    }

    /// `base`, or "base 2", "base 3"… if a playlist already has that name (ignoring case).
    pub fn unique_playlist_name(&self, base: &str) -> String {
        let index = self.index();
        let taken = |name: &str| index.playlists().iter().any(|p| p.name.eq_ignore_ascii_case(name));
        let base = base.trim();
        let base = if base.is_empty() { "New playlist" } else { base };
        if !taken(base) {
            return base.to_string();
        }
        (2..).map(|n| format!("{base} {n}")).find(|name| !taken(name)).expect("some number is free")
    }

    /// Move a playlist to `to` (0 = top) in the sidebar.
    pub fn move_playlist(&self, id: PlaylistId, to: usize) -> anyhow::Result<()> {
        let mut order: Vec<PlaylistId> = self.index().playlists().iter().map(|p| p.id).collect();
        let Some(from) = order.iter().position(|p| *p == id) else { return Ok(()) };
        let moved = order.remove(from);
        order.insert(to.min(order.len()), moved);
        self.db.lock().set_playlist_order(&order)?;
        self.refresh_playlists()
    }

    /// Create a playlist with `tracks` at sidebar position `at` (undoing a delete, duplicating).
    pub fn insert_playlist(&self, name: &str, tracks: &[TrackId], at: usize) -> anyhow::Result<PlaylistId> {
        let id = self.create_playlist(name, tracks)?;
        self.move_playlist(id, at)?;
        Ok(id)
    }

    /// Copy a playlist ("Mix copy") and place it right below the original.
    pub fn duplicate_playlist(&self, id: PlaylistId) -> anyhow::Result<PlaylistId> {
        let index = self.index();
        let Some(pos) = index.playlists().iter().position(|p| p.id == id) else {
            anyhow::bail!("the playlist no longer exists");
        };
        let original = index.playlists()[pos].clone();
        drop(index);
        let name = self.unique_playlist_name(&format!("{} copy", original.name));
        self.insert_playlist(&name, &original.tracks, pos + 1)
    }

    pub fn get_kv(&self, key: &str) -> Option<String> {
        self.db.lock().get_kv(key).ok().flatten()
    }

    pub fn set_kv(&self, key: &str, value: &str) {
        if let Err(e) = self.db.lock().set_kv(key, value) {
            tracing::warn!("could not save {key}: {e}");
        }
    }
}

/// Artwork deduplication during a scan: every distinct image is decoded once.
struct ArtIngest<'a> {
    cache: &'a ArtworkCache,
    seen: Mutex<HashSet<String>>,
    folders: Mutex<HashMap<PathBuf, Option<String>>>,
    new: Mutex<Vec<(String, Colors)>>,
}

impl<'a> ArtIngest<'a> {
    fn new(cache: &'a ArtworkCache) -> Self {
        ArtIngest { cache, seen: Mutex::default(), folders: Mutex::default(), new: Mutex::default() }
    }

    fn ingest(&self, bytes: &[u8]) -> Option<String> {
        let key = ArtworkCache::key_for(bytes);
        if !self.seen.lock().insert(key.clone()) {
            return Some(key);
        }
        if self.cache.has(&key) {
            return Some(key);
        }
        match self.cache.store(&key, bytes) {
            Ok(colors) => {
                self.new.lock().push((key.clone(), colors));
                Some(key)
            }
            Err(e) => {
                tracing::debug!("unreadable artwork: {e}");
                None
            }
        }
    }

    fn folder(&self, dir: &Path) -> Option<String> {
        if let Some(cached) = self.folders.lock().get(dir) {
            return cached.clone();
        }
        let key = artwork::folder_image(dir).and_then(|p| std::fs::read(p).ok()).and_then(|bytes| self.ingest(&bytes));
        self.folders.lock().insert(dir.to_path_buf(), key.clone());
        key
    }

    fn into_new(self) -> Vec<(String, Colors)> {
        self.new.into_inner()
    }
}

fn all_cores() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

/// `filter_map` on up to `workers` threads: workers pull the next item from a shared counter, so
/// slow files don't hold up a whole chunk. (Scoped threads; no thread-pool crate needed.)
fn parallel_filter_map<T: Sync, R: Send>(items: &[T], workers: usize, f: impl Fn(&T) -> Option<R> + Sync) -> Vec<R> {
    let workers = workers.min(items.len()).max(1);
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        if let Some(r) = f(item) {
                            out.push(r);
                        }
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).collect()
    })
}

fn is_audio(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(lychee_audio::is_supported_extension)
}
