# Lychee: architecture

Lychee will be extended a lot (more scrobblers, remote libraries, plugins, new UIs), so every subsystem
lives in its own crate with a small API. The UI never talks to services directly.

## Principles

1. **One controller, many clients.** Every UI callback becomes an `Action` handled by a single
   controller (`crates/lychee/src/app.rs`). OS media controls send `MediaAction`s that the controller
   handles the same way, and a future CLI, remote API or plugin can do so too.
2. **Real-time safety.** The audio callback never allocates, locks (except a non-blocking `try_lock`) or
   touches the disk.
3. **Services report back through channels.** The engine, scanner, image decoder, Last.fm worker and
   media controls run on their own threads. The UI thread polls their channels 20 times a second.
4. **The database is a cache.** Files on disk are the truth. Deleting the database only loses likes, play
   counts and playlists; everything else comes back with a rescan.

## Crates

```
crates/
├─ lychee-core       Track/Album/Playlist model, the play Queue (shuffle, repeat, play-next), helpers
├─ lychee-audio      Engine: Symphonia demux/decode (+ pure-Rust Opus), remix, rubato resample,
│                    lock-free ring buffer, cpal output, gapless hand-over, clickless fades
├─ lychee-library    Scanner (walkdir + rayon + lofty), artwork cache and colours, SQLite store,
│                    in-memory LibraryIndex with fuzzy search, LRC parsing
├─ lychee-lastfm     Last.fm client (auth, now playing, scrobble, love) and a background Scrobbler
│                    with a persistent offline queue
├─ lychee-platform   MediaControls: MPRIS (mpris-server) on Linux, SMTC (windows crate) on Windows
├─ lychee-tagger     Tag editor engine: read/write with lofty (lossless split/merge), EasyTAG masks,
│                    text clean-up, a Session with pending changes, multi-selection and undo
├─ lychee-lookup     MusicBrainz (rate-limited), Cover Art Archive and LRCLIB clients; album
│                    detection from tags and folder names; candidate scoring and track matching
└─ lychee            The app: Slint UI (ui/*.slint), controller, settings, image cache, mini player,
                     tag editor page (editor.rs)
```

Only `lychee` depends on Slint, and only `lychee-platform` has OS-specific code. Everything else is plain,
testable Rust: `cargo test --workspace` runs 73 tests. They include an end-to-end scan of a temporary
music folder and tag round trips through seven container formats.

## Runtime

```
 Slint UI thread ── Action ──► Dispatcher ──► App (controller) ──► Engine handle ──► lychee-audio thread
        ▲                                        │   │   │              decode ► remix ► resample ► ring buffer
        │                                        │   │   │                                       ▼
        └── models / globals ◄───────────────────┘   │   │                               cpal callback (RT)
                                                     │   ├─► scan thread (rayon pool) ─► SQLite
              20 Hz tick: engine events, position,   │   ├─► image decode threads (covers for visible rows)
              scan progress, Last.fm status,          │   ├─► Last.fm Scrobbler thread (offline queue on disk)
              media actions, decoded images           │   └─► MPRIS / SMTC thread ◄── media keys, desktop widgets
```

- **Dispatcher.** Slint can invoke callbacks while Rust is setting properties (re-entrancy). Actions go
  into a queue and run when the controller is free, so nested callbacks never cause borrow panics.
- **Lazy models.** Grids and long lists use `LazyModel`, which builds rows only when the view asks for
  them and refreshes a row when its cover finishes decoding. A 50,000-song list costs only what's on screen.
- **Engine tokens.** Each loaded or pre-loaded track gets a token. The engine reports `Loaded`, `Started`
  (audible, which is how gapless hand-overs are detected), `Ended` and `Error` per token, and exposes the
  audible position through atomics.
- **Album handles.** Album ids are 63-bit hashes and Slint ints are 32-bit, so the UI uses session-local
  handles that map back to ids.

## Tag editor

`EditorCtl` (`crates/lychee/src/editor.rs`) owns a `lychee_tagger::Session` and mirrors it into the
`Editor` global. Reading and saving run on worker threads, and so do lookups. Their results come back
through a channel polled on the tick, and they refer to files by path because indices change when files
are added or closed. Edits live in memory until saved:

```
 select / type ─► Session::set_field (one undo step per field while typing)
 Scanner ───────► Session::fill_from_names / rename_from_tags / cleanup
 Look up ───────► matching::guess ─► find_candidates (MusicBrainz) ─► Session::apply
                   └─► Cover Art Archive / LRCLIB downloads ─► Session::set_pictures / apply
 Save ──────────► SaveJob::run (write tags, then rename) ─► Library::refresh_files ─► views refresh
```

- **Lossless writing.** lofty's generic `Tag` can't hold every ID3v2 frame or MP4 atom, so MP3, WAV, AIFF,
  AAC and MP4 files are edited through `SplitTag`/`MergeTag`. The editor changes the part it understands
  and the rest goes back untouched. The main tag is made to match the editor exactly; secondary tags
  (ID3v1, APE) only receive the fields that changed.
- **Library identity.** `Library::refresh_files` moves the database row to the new path before re-reading
  the tags, so track ids (and with them likes, play counts and playlists) survive renames.
- **Inputs.** Text inputs are filled from Rust only when `Editor.generation` changes (selection, undo, a
  tool), never while the user types, so model updates can't fight with the cursor.

## Mini player

On X11 and Windows the mini player is a second Slint window. Wayland has no way for an app to stay above
other windows, so on a Wayland session Lychee starts itself again as `lychee --mini-player`. That helper
forces winit's X11 backend (XWayland), where the window manager honours "keep above". The parent sends
the player state as JSON lines on the helper's stdin and gets button presses back as words on its stdout.
When the parent goes away, the helper's stdin closes and it quits. If XWayland isn't there, the app falls
back to the in-process window. Two Wayland limits shape the rest: a minimised window can't be restored
by the app, and an app can't focus itself. So the main window is hidden rather than minimised while the
mini player is open, and showing it again maps a fresh window.

## Extension points

| To add… | Implement / touch |
|---|---|
| A decoder (FFmpeg, libopus, DSD) | a new variant in `lychee-audio`'s `Decoder` (see `source.rs`) |
| A DSP stage (ReplayGain, EQ, crossfade) | the decode → push path in `engine.rs::fill` (before the ring buffer) |
| A library source (Subsonic, Jellyfin) | a sibling of `Library` producing `Track`s; the index and UI are source-agnostic |
| A scrobbler (ListenBrainz) | a sibling of `lychee-lastfm::Scrobbler`; the controller already tracks listens (`lastfm.rs::Listen`) |
| A lyrics or artwork provider | `lychee-library::lyrics::load` / `ArtIngest`; online ones in `lychee-lookup` |
| A metadata source (Discogs, AcoustID) | a sibling of `musicbrainz.rs` producing `Release`s; `matching` scores any source |
| A tag editor tool | a `Session` method (one undo step through `Session::edit`) and a scanner mode |
| OS integration (macOS Now Playing) | a backend in `lychee-platform` |
| A new page or panel | a `.slint` view, a `Page` variant and a `refresh_page` arm |
| A command | `palette::Command` + `App::run_command` (it then appears in Ctrl+K) |

Later, third-party plugins are planned as sandboxed WASM components (wasmtime + WIT) for providers
(lyrics, metadata, artwork, scrobbling), and theme packs as token files loaded into the `Theme` global.

## Theming

Views read every colour, size and duration from the `Theme` global (`ui/theme.slint`). `Theme.dark`
switches the palette; `Theme.accent` and `Theme.tint` come from the album artwork. The accent is the
cover's most vivid colour; the tint is its dominant one. Both are computed once at scan time.

## Storage

| What | Where (Linux) | Notes |
|---|---|---|
| Settings | `~/.config/lychee/config.toml` | written with user-only permissions (holds the Last.fm session key) |
| Library | `~/.local/share/lychee/library.db` | SQLite (WAL): tracks, artwork colours, playlists, plays, session |
| Scrobble queue | `~/.local/share/lychee/scrobbles.json` | survives restarts while offline |
| Artwork | `~/.cache/lychee/artwork/` | 320 px and 720 px JPEGs per distinct image (MD5 of the bytes) |

## Testing

- **Core:** queue rules (shuffle keeps the current track, user-queued tracks stay put, repeat modes, moves).
- **Audio:** remix, exact-length resampling; `examples/decode.rs` and `examples/engine_check.rs` exercise
  every format and a silent gapless hand-over on a real device.
- **Library:** schema, upserts that keep user data, playlists, LRC parsing, accent/tint, fuzzy ranking,
  and an end-to-end scan of a generated folder.
- **Last.fm:** request signatures and error classification.
- **Tagger:** masks, renaming, case rules, undo, and round trips through FLAC, MP3, Ogg Vorbis, Opus, M4A,
  WAV and AIFF fixtures (every field, pictures, clearing). Also checks that a private ID3 frame survives
  an edit and that a save renames the file and moves its `.lrc`. `examples/tag.rs` prints or edits a file.
- **Lookup:** MusicBrainz JSON parsing, folder-name guesses, duration/title matching and scoring, LRCLIB
  hit selection (offline fixtures). `examples/lookup.rs` runs a live search.
- **Platform:** `examples/mpris_check.rs` registers the MPRIS service for a quick check with `playerctl`.
- **UI:** headless screenshots (`LYCHEE_SCREENSHOT`, see `crates/lychee/src/screenshot.rs`).
