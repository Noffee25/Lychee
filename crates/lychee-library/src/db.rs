//! The SQLite store. It is a cache of what is on disk plus the user's own data (likes, play
//! counts, playlists). Everything except that user data can be rebuilt with a rescan.

use std::path::{Path, PathBuf};

use lychee_core::{AudioFormat, Playlist, PlaylistId, Track, TrackId, now_unix};
use rusqlite::{Connection, OptionalExtension, params};

const SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = "
CREATE TABLE folders (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    added_at INTEGER NOT NULL
);
CREATE TABLE tracks (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    in_library INTEGER NOT NULL DEFAULT 1,
    mtime INTEGER NOT NULL DEFAULT 0,
    size INTEGER NOT NULL DEFAULT 0,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    album TEXT NOT NULL,
    album_artist TEXT NOT NULL,
    track_no INTEGER,
    disc_no INTEGER,
    year INTEGER,
    genre TEXT,
    composer TEXT,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    codec TEXT NOT NULL DEFAULT '',
    sample_rate INTEGER,
    bit_depth INTEGER,
    channels INTEGER,
    bitrate INTEGER,
    artwork TEXT,
    added_at INTEGER NOT NULL,
    play_count INTEGER NOT NULL DEFAULT 0,
    last_played INTEGER,
    liked INTEGER NOT NULL DEFAULT 0,
    liked_at INTEGER
);
CREATE TABLE artwork (
    key TEXT PRIMARY KEY,
    accent INTEGER,
    tint INTEGER
);
CREATE TABLE playlists (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    PRIMARY KEY (playlist_id, position)
);
CREATE TABLE plays (
    id INTEGER PRIMARY KEY,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    played_at INTEGER NOT NULL
);
CREATE INDEX plays_by_time ON plays(played_at);
CREATE TABLE kv (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

/// Metadata for one file, as written by the scanner.
#[derive(Clone, Debug, Default)]
pub struct TrackRecord {
    pub path: PathBuf,
    pub mtime: i64,
    pub size: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The album-artist *tag*; empty when the file has none.
    pub album_artist: String,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub duration_ms: u64,
    pub format: AudioFormat,
    pub artwork: Option<String>,
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Db> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Db> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)?;
        }
        if version < SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Db { conn })
    }

    // ---- folders -------------------------------------------------------------------------

    pub fn folders(&self) -> rusqlite::Result<Vec<PathBuf>> {
        let mut stmt = self.conn.prepare("SELECT path FROM folders ORDER BY path")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| r.map(PathBuf::from)).collect()
    }

    pub fn add_folder(&self, path: &Path) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO folders (path, added_at) VALUES (?1, ?2)",
            params![path.to_string_lossy(), now_unix()],
        )?;
        Ok(())
    }

    pub fn remove_folder(&self, path: &Path) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM folders WHERE path = ?1", params![path.to_string_lossy()])?;
        Ok(())
    }

    // ---- tracks --------------------------------------------------------------------------

    /// path → (id, mtime, size) for every in-library track, so a rescan only reads changed files.
    pub fn file_stamps(&self) -> rusqlite::Result<Vec<(TrackId, PathBuf, i64, u64)>> {
        let mut stmt = self.conn.prepare("SELECT id, path, mtime, size FROM tracks WHERE in_library = 1")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get(0)?, PathBuf::from(r.get::<_, String>(1)?), r.get(2)?, r.get::<_, i64>(3)? as u64))
        })?;
        rows.collect()
    }

    /// Insert or update tracks by path. User data (likes, plays, date added) is preserved.
    pub fn upsert_tracks(&mut self, records: &[TrackRecord], in_library: bool) -> rusqlite::Result<Vec<TrackId>> {
        let tx = self.conn.transaction()?;
        let mut ids = Vec::with_capacity(records.len());
        {
            let mut stmt = tx.prepare(
                "INSERT INTO tracks (path, in_library, mtime, size, title, artist, album, album_artist, track_no,
                    disc_no, year, genre, composer, duration_ms, codec, sample_rate, bit_depth, channels, bitrate,
                    artwork, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
                 ON CONFLICT(path) DO UPDATE SET
                    in_library = max(in_library, excluded.in_library), mtime = excluded.mtime, size = excluded.size,
                    title = excluded.title, artist = excluded.artist, album = excluded.album,
                    album_artist = excluded.album_artist, track_no = excluded.track_no, disc_no = excluded.disc_no,
                    year = excluded.year, genre = excluded.genre, composer = excluded.composer,
                    duration_ms = excluded.duration_ms, codec = excluded.codec, sample_rate = excluded.sample_rate,
                    bit_depth = excluded.bit_depth, channels = excluded.channels, bitrate = excluded.bitrate,
                    artwork = excluded.artwork
                 RETURNING id",
            )?;
            let now = now_unix();
            for r in records {
                let f = &r.format;
                let id: TrackId = stmt.query_row(
                    params![
                        r.path.to_string_lossy(),
                        in_library as i64,
                        r.mtime,
                        r.size as i64,
                        r.title,
                        r.artist,
                        r.album,
                        r.album_artist,
                        r.track_number,
                        r.disc_number,
                        r.year,
                        r.genre,
                        r.composer,
                        r.duration_ms as i64,
                        f.codec,
                        f.sample_rate,
                        f.bit_depth,
                        f.channels,
                        f.bitrate_kbps,
                        r.artwork,
                        now,
                    ],
                    |row| row.get(0),
                )?;
                ids.push(id);
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    pub fn delete_tracks(&mut self, ids: &[TrackId]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare("DELETE FROM tracks WHERE id = ?1")?;
            for id in ids {
                stmt.execute(params![id])?;
            }
        }
        tx.commit()
    }

    pub fn all_tracks(&self) -> rusqlite::Result<Vec<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, in_library, size, title, artist, album, album_artist, track_no, disc_no, year, genre,
                    composer, duration_ms, codec, sample_rate, bit_depth, channels, bitrate, artwork, added_at,
                    play_count, last_played, liked, liked_at
             FROM tracks",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Track {
                id: r.get(0)?,
                path: PathBuf::from(r.get::<_, String>(1)?),
                in_library: r.get::<_, i64>(2)? != 0,
                file_size: r.get::<_, i64>(3)? as u64,
                title: r.get(4)?,
                artist: r.get(5)?,
                album: r.get(6)?,
                album_artist: r.get(7)?,
                track_number: r.get(8)?,
                disc_number: r.get(9)?,
                year: r.get(10)?,
                genre: r.get(11)?,
                composer: r.get(12)?,
                duration_ms: r.get::<_, i64>(13)? as u64,
                format: AudioFormat {
                    codec: r.get(14)?,
                    sample_rate: r.get(15)?,
                    bit_depth: r.get(16)?,
                    channels: r.get(17)?,
                    bitrate_kbps: r.get(18)?,
                },
                artwork: r.get(19)?,
                added_at: r.get(20)?,
                play_count: r.get::<_, i64>(21)? as u32,
                last_played: r.get(22)?,
                liked: r.get::<_, i64>(23)? != 0,
                liked_at: r.get(24)?,
                album_id: 0,
            })
        })?;
        rows.collect()
    }

    /// A file moved: keep its row (and so its id, likes and plays) under the new path.
    pub fn rename_path(&self, old: &Path, new: &Path) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE tracks SET path = ?2 WHERE path = ?1",
            params![old.to_string_lossy(), new.to_string_lossy()],
        )?;
        Ok(())
    }

    pub fn track_id_for_path(&self, path: &Path) -> rusqlite::Result<Option<TrackId>> {
        self.conn
            .query_row("SELECT id FROM tracks WHERE path = ?1", params![path.to_string_lossy()], |r| r.get(0))
            .optional()
    }

    pub fn set_liked(&self, id: TrackId, liked: bool) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE tracks SET liked = ?2, liked_at = CASE WHEN ?2 = 1 THEN ?3 ELSE NULL END WHERE id = ?1",
            params![id, liked as i64, now_unix()],
        )?;
        Ok(())
    }

    /// Count a completed listen.
    pub fn record_play(&self, id: TrackId, at: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE tracks SET play_count = play_count + 1, last_played = ?2 WHERE id = ?1",
            params![id, at],
        )?;
        self.conn.execute("INSERT INTO plays (track_id, played_at) VALUES (?1, ?2)", params![id, at])?;
        Ok(())
    }

    // ---- artwork -------------------------------------------------------------------------

    pub fn artwork_colors(&self) -> rusqlite::Result<Vec<(String, crate::Colors)>> {
        let mut stmt = self.conn.prepare("SELECT key, accent, tint FROM artwork")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get(0)?,
                crate::Colors {
                    accent: r.get::<_, Option<i64>>(1)?.map(|v| v as u32),
                    tint: r.get::<_, Option<i64>>(2)?.map(|v| v as u32),
                },
            ))
        })?;
        rows.collect()
    }

    pub fn put_artwork(&self, key: &str, colors: crate::Colors) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO artwork (key, accent, tint) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET accent = excluded.accent, tint = excluded.tint",
            params![key, colors.accent.map(i64::from), colors.tint.map(i64::from)],
        )?;
        Ok(())
    }

    // ---- playlists -----------------------------------------------------------------------

    pub fn playlists(&self) -> rusqlite::Result<Vec<Playlist>> {
        let mut stmt = self.conn.prepare("SELECT id, name, updated_at FROM playlists ORDER BY name COLLATE NOCASE")?;
        let mut playlists: Vec<Playlist> = stmt
            .query_map([], |r| Ok(Playlist { id: r.get(0)?, name: r.get(1)?, updated_at: r.get(2)?, tracks: vec![] }))?
            .collect::<rusqlite::Result<_>>()?;
        let mut items = self.conn.prepare("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position")?;
        for p in &mut playlists {
            p.tracks = items.query_map(params![p.id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        }
        Ok(playlists)
    }

    pub fn create_playlist(&self, name: &str) -> rusqlite::Result<PlaylistId> {
        let now = now_unix();
        self.conn.execute(
            "INSERT INTO playlists (name, created_at, updated_at) VALUES (?1, ?2, ?2)",
            params![name, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_playlist(&self, id: PlaylistId, name: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE playlists SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, now_unix()],
        )?;
        Ok(())
    }

    pub fn delete_playlist(&self, id: PlaylistId) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM playlists WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Replace a playlist's contents.
    pub fn set_playlist_tracks(&mut self, id: PlaylistId, tracks: &[TrackId]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", params![id])?;
        {
            let mut stmt = tx.prepare("INSERT INTO playlist_tracks (playlist_id, position, track_id) VALUES (?1, ?2, ?3)")?;
            for (pos, t) in tracks.iter().enumerate() {
                stmt.execute(params![id, pos as i64, t])?;
            }
        }
        tx.execute("UPDATE playlists SET updated_at = ?2 WHERE id = ?1", params![id, now_unix()])?;
        tx.commit()
    }

    // ---- key/value -----------------------------------------------------------------------

    pub fn get_kv(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn.query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| r.get(0)).optional()
    }

    pub fn set_kv(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_preserves_user_data() {
        let mut db = Db::open_in_memory().unwrap();
        let rec = TrackRecord { path: "/m/a.flac".into(), title: "A".into(), artist: "X".into(), album: "Al".into(), ..Default::default() };
        let id = db.upsert_tracks(&[rec.clone()], true).unwrap()[0];
        db.set_liked(id, true).unwrap();
        db.record_play(id, 100).unwrap();
        let changed = TrackRecord { title: "A (remaster)".into(), ..rec };
        assert_eq!(db.upsert_tracks(&[changed], true).unwrap()[0], id, "same row for the same path");
        let t = &db.all_tracks().unwrap()[0];
        assert_eq!(t.title, "A (remaster)");
        assert!(t.liked && t.liked_at.is_some());
        assert_eq!(t.play_count, 1);
    }

    #[test]
    fn playlists_keep_order() {
        let mut db = Db::open_in_memory().unwrap();
        let recs: Vec<TrackRecord> = (0..3).map(|i| TrackRecord { path: format!("/m/{i}.mp3").into(), title: i.to_string(), ..Default::default() }).collect();
        let ids = db.upsert_tracks(&recs, true).unwrap();
        let p = db.create_playlist("Mix").unwrap();
        db.set_playlist_tracks(p, &[ids[2], ids[0]]).unwrap();
        assert_eq!(db.playlists().unwrap()[0].tracks, vec![ids[2], ids[0]]);
        db.delete_tracks(&[ids[0]]).unwrap();
        assert_eq!(db.playlists().unwrap()[0].tracks, vec![ids[2]], "deleted tracks leave playlists");
    }
}
