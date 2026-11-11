# Lychee: feature list

Lychee is a local-library music player for Linux and Windows, written in Rust, with a modern custom UI
(the "Flow" design, see `design/`). Features are grouped by milestone.
`[x]` means done in the current code; `[ ]` means planned.

## Principles

- **Beautiful by default.** A custom-designed interface with smooth, restrained animation.
- **Plays essentially anything.** Common formats in pure Rust; rarer ones through an optional backend later.
- **Fast with large libraries.** Lists and grids are virtualised and covers load lazily in the background.
- **Keyboard-friendly.** Everything is reachable from the keyboard and the Ctrl+K command palette.
- **Your files are yours.** Lychee never edits your files. The database is a cache that can always be rebuilt.
- **Your library, not a streaming service.** No recommendation feeds, "You might also like" rows or home screens.
- **Built to be extended.** Each subsystem lives in its own crate behind a small API (see `docs/architecture.md`).

---

## v0.1 "First Light" (current)

### Playback
- [x] Formats: **MP3, FLAC, WAV, AIFF, Ogg Vorbis, AAC/M4A, ALAC, Opus**, plus CAF and MKV/WebM audio
- [x] Play / pause; next / previous (previous restarts the track if more than 3 s in)
- [x] Seek by clicking or dragging the progress bar (with a time preview on hover)
- [x] Skip back / forward 10 s (`←` / `→`); relative seeks from media keys and MPRIS
- [ ] Hold-to-scrub fast-forward / rewind buttons
- [x] Volume with a perceptual curve; mute; scroll the volume slider with the mouse wheel
- [x] Shuffle (the current track stays, user-queued tracks are never shuffled away) and repeat off / all / one
- [x] **Gapless playback**: the next track is pre-loaded and starts on the exact sample
- [x] High-quality resampling (rubato) when the device rate differs from the file's
- [x] Output device selection
- [x] Clickless pause, resume and seek (short fades)
- [x] Resume where you left off: queue, track, position, volume, shuffle and repeat

### Library
- [x] Add and remove music folders; parallel first scan; fast incremental rescans (only changed files are read)
- [x] Rescan on startup and on demand
- [ ] Live file watching
- [x] Tags: title, artist, album, album artist, track and disc number, year, genre, composer
- [x] Technical info: duration, codec, bitrate, sample rate, bit depth, channels, file size
- [x] **Album art**: embedded, then folder images (`cover`/`folder`/`front`…), then a generated colour tile
- [x] Accent and tint colours extracted from every cover
- [x] Browse **Albums** (grid with four sort orders), **Artists**, **Songs** (five sort orders), **Genres**, **Recently Added**, **Recently Played**
- [ ] Folder browser
- [x] Multi-disc albums (disc headers) and compilations ("Various Artists")
- [x] Instant fuzzy search across albums, artists, songs and playlists
- [x] Play files without importing them: command-line arguments, "Open with" (via the desktop file), drag and drop

### Now Playing
- [x] Large album art, track title, artist, album, format badge (e.g. `FLAC 24/96`), position and duration
- [x] **Dynamic colour**: buttons and highlights take the album's accent; headers its dominant colour (can be turned off)
- [x] **Lyrics**: `.lrc` sidecar files and embedded lyrics, synced highlighting with auto-scroll, click a line to seek
- [x] Details panel: tags, format, decoder, output device, file size, play count, path

### Queue and playlists
- [x] Up Next with "Queued by you" and "Next from …" sections: play now, play next, add to queue, remove, clear
- [x] Reorder via the context menu (move up / down)
- [ ] Drag to reorder
- [x] Playlists: create, rename, delete, add albums or songs, remove songs
- [ ] Reorder playlists; import and export M3U/M3U8
- [x] Favorites (♥), ordered by when you liked them
- [x] Play counts and "Recently Played" history

### Interface
- [x] The Flow design: sidebar, content, Queue / Lyrics / Details panel, player bar
- [x] Dark, light and system themes
- [x] **Command palette (Ctrl+K)** for search and commands (`>` filters to commands)
- [x] Keyboard shortcuts (see the README)
- [x] Right-click menus on albums, songs and queue items
- [x] Toast notifications for actions such as "Added to the queue"
- [x] Welcome screen for an empty library
- [x] **Mini player** (Ctrl+M): compact, always-on-top, drag to move
- [ ] Custom title bar (native window decorations for now)
- [ ] Respect the system "reduce motion" setting

### Desktop integration
- [x] **Linux: MPRIS** (media keys, GNOME and KDE media controls, `playerctl`) and a `.desktop` file
- [x] **Windows: System Media Transport Controls** (media keys, volume flyout, lock screen). Written but not yet tested on Windows.
- [ ] Single instance: opening a file while Lychee runs hands it to the running window
- [ ] Desktop notifications on track change

### Last.fm
- [x] Connect through the browser (desktop auth flow; no password stored)
- [x] "Now playing" updates
- [x] Scrobbling by Last.fm's rules (half the track or 4 minutes, tracks over 30 s)
- [x] Offline queue saved to disk, retried with back-off; status icon in the bottom-right corner
- [x] Optionally love tracks on Last.fm when you add them to Favorites
- [x] Your own API key in Settings, or one built in at compile time

---

## v0.2 "Daily Driver"
- [ ] System tray icon with controls; optionally close to tray
- [ ] **ReplayGain** (track/album) from tags, plus EBU R128 analysis for untagged files
- [ ] Optional crossfade (never between gapless album tracks)
- [ ] Waveform seek bar
- [ ] Ratings and smart playlists (rules: genre, year, rating, play count, date added, format…)
- [ ] Live folder watching, folder browser, M3U import/export, drag-to-reorder
- [ ] Single instance and desktop notifications
- [ ] A faster Opus decoder: the pure-Rust one uses about 20% of a CPU core; libopus would make it optional

## v0.3 "Audiophile & Power"
- [ ] Parametric / graphic EQ with presets
- [ ] Spectrum analyzer and level meters
- [ ] Bit-perfect / exclusive output (WASAPI exclusive, direct ALSA) with sample-rate switching
- [ ] Rarer formats through an optional FFmpeg backend: WMA, APE, WavPack, DSD, Musepack, TTA
- [ ] CUE sheets; tag editor; library statistics

## Extensions (the architecture is ready for these)
- Online lyrics (e.g. LRCLIB), online artwork, MusicBrainz lookup
- ListenBrainz scrobbling alongside Last.fm
- Remote libraries: Subsonic / Navidrome, Jellyfin
- Internet radio; podcasts
- Remote control API; casting (UPnP/DLNA, Chromecast)
- Plugins (sandboxed WASM) and theme packs; a terminal UI on the same core; macOS

## Not planned
- Streaming-service features: recommendations, "You might also like", algorithmic home screens
- Clients for DRM streaming services, video playback, CD ripping, mobile apps
