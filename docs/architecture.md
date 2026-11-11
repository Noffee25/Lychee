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
└─ lychee            The app: Slint UI (ui/*.slint), controller, settings, image cache, mini player
```

Only `lychee` depends on Slint, and only `lychee-platform` has OS-specific code. Everything else is plain,
testable Rust: `cargo test --workspace` runs 34 tests, including an end-to-end scan of a temporary
music folder.

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

## Extension points

| To add… | Implement / touch |
|---|---|
| A decoder (FFmpeg, libopus, DSD) | a new variant in `lychee-audio`'s `Decoder` (see `source.rs`) |
| A DSP stage (ReplayGain, EQ, crossfade) | the decode → push path in `engine.rs::fill` (before the ring buffer) |
| A library source (Subsonic, Jellyfin) | a sibling of `Library` producing `Track`s; the index and UI are source-agnostic |
| A scrobbler (ListenBrainz) | a sibling of `lychee-lastfm::Scrobbler`; the controller already tracks listens (`lastfm.rs::Listen`) |
| A lyrics or artwork provider | `lychee-library::lyrics::load` / `ArtIngest` |
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
- **Platform:** `examples/mpris_check.rs` registers the MPRIS service for a quick check with `playerctl`.
- **UI:** headless screenshots (`LYCHEE_SCREENSHOT`, see `crates/lychee/src/screenshot.rs`).
