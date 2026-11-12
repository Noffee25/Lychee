# Lychee: feature list

Lychee is a local-library music player for Linux, Windows and macOS, written in Rust, with a modern custom UI
(the "Flow" design, see `design/`). Features are grouped by milestone.
`[x]` means done in the current code; `[ ]` means planned.

## Principles

- **Beautiful by default.** A custom-designed interface with smooth, restrained animation.
- **Plays essentially anything.** Common formats in pure Rust; rarer ones through an optional backend later.
- **Fast with large libraries.** Lists and grids are virtualised and covers load lazily in the background.
- **Keyboard-friendly.** Everything is reachable from the keyboard and the Ctrl+K command palette.
- **Your files are yours.** Lychee only writes to your files when you save in the tag editor. Edits stay
  pending until then, and tag frames it doesn't understand are kept. The database is a cache that can always be rebuilt.
- **Your library, not a streaming service.** No recommendation feeds, "You might also like" rows or home screens.
- **Built to be extended.** Each subsystem lives in its own crate behind a small API (see `docs/architecture.md`).

---

## v0.1 "First Light" (current)

### Playback
- [x] Formats: **MP3, FLAC, WAV, AIFF, Ogg Vorbis, AAC/M4A, ALAC, Opus**, plus CAF and MKV/WebM audio
- [x] Play / pause; next / previous (previous restarts the track if more than 3 s in)
- [x] Seek by clicking or dragging the progress bar; while you hover, the time label shows where a click would go
- [x] Skip back / forward 10 s (`←` / `→`); relative seeks from media keys and MPRIS
- [ ] Hold-to-scrub fast-forward / rewind buttons
- [x] Volume with a perceptual curve; mute; scroll the volume slider with the mouse wheel
- [x] Shuffle (the current track stays, user-queued tracks are never shuffled away) and repeat off / all / one
- [x] **Gapless playback**: the next track is pre-loaded and starts on the exact sample
- [x] High-quality resampling (rubato) when the device rate differs from the file's
- [x] Output device selection
- [x] Clickless pause, resume and seek (short fades)
- [x] Resume where you left off: queue, track, position, volume, shuffle and repeat
- [x] **Volume levelling** (ReplayGain 2.0, on by default in Settings → Playback): every song plays at about
  the same loudness (−18 LUFS). Gains come from ReplayGain tags (also APE tags on MP3s and Opus's `R128_*`
  tags); songs without them are measured in the background with Lychee's own EBU R128 / BS.1770 meter.
  Album gain while an album plays (the songs beside it in the queue are from the same album), track gain
  otherwise; peaks keep quiet songs from clipping when they're raised. A pre-amp (0 to +6 dB) plays everything
  louder for people who find −18 LUFS too quiet. The Details panel shows the gain in use

### Library
- [x] Add and remove music folders; parallel first scan; fast incremental rescans (only changed files are read)
- [x] Rescan on startup and on demand
- [ ] Live file watching
- [x] Tags: title, artist, album, album artist, track and disc number, year, genres (several per song), composer
- [x] Technical info: duration, codec, bitrate, sample rate, bit depth, channels, file size
- [x] **Album art**: embedded, then folder images (`cover`/`folder`/`front`…), then a generated colour tile
- [x] Accent and tint colours extracted from every cover
- [x] Browse **Albums** (grid with four sort orders), **Artists**, **Songs** (five sort orders), **Genres** (spellings like "Hip-Hop" and "Hip Hop" are one genre; an album is listed under each of its genres), **Recently Added**, **Recently Played**
- [ ] Folder browser
- [x] Multi-disc albums (disc headers) and compilations ("Various Artists"); a guest on one track
      ("A feat. B") doesn't turn an album into a compilation
- [x] Instant fuzzy search across albums, artists, songs and playlists
- [x] Play files without importing them: command-line arguments, "Open with" (via the desktop file), drag and drop

### Now Playing
- [x] Large album art, track title, artist, album, format badge (e.g. `FLAC 24/96`), position and duration
- [x] Click the cover or title in the player bar to open the album (the playing song is highlighted there).
  A credit like "A feat. B, C" shows as separate artist links (A, B, C) there and in the Queue panel
- [x] The playing song and album are marked with three bars, which move while music plays and rest when
  it's paused
- [x] While an album plays, its Play button (on the album page and on its tile) turns into Pause; pressing
  it again resumes where you left off
- [x] **Dynamic colour**: buttons and highlights take the album's accent; headers its dominant colour (can be turned off)
- [x] **Lyrics**: `.lrc` sidecar files and embedded lyrics, synced highlighting with auto-scroll, click a line to seek
- [x] Details panel: tags, format, decoder, output device, file size, play count, path

### Queue and playlists
- [x] Up Next with "Queued by you" and "Next from …" sections: play now, play next, add to queue, remove, clear
- [x] Drag songs anywhere on their row to reorder the queue and playlists; the context menu also
  moves them (queue: up / down; playlists: to the top, up, down, to the bottom)
- [x] Playlists: create, rename, delete, add albums or songs, remove songs
- [x] Undo from the toast (or Ctrl+Z) after deleting a playlist, removing a song, sorting or removing duplicates
- [x] Adding songs that are already in a playlist skips them and says so; new playlists get unique names
- [x] Save the queue as a playlist (Queue panel, Ctrl+K); add the playing song to a playlist from the player
  bar's right-click menu or Ctrl+K
- [x] Playlist covers are a 2×2 mosaic of their first four albums; drag playlists to reorder the sidebar
- [x] Right-click a sidebar playlist: play, shuffle, play next, add to queue, rename, duplicate, delete.
  The playlist page's "⋯" menu: play next, add to queue, sort once (title, artist, album, length, reverse),
  remove duplicates, duplicate the playlist
- [ ] Import and export M3U/M3U8
- [x] Favorites (♥), ordered by when you liked them
- [x] Play counts and "Recently Played" history

### Interface
- [x] The Flow design: sidebar, content, Queue / Lyrics / Details panel, player bar
- [x] Back and forward buttons in the top left, like a browser's; each is greyed out when there's nowhere to go
- [x] Narrow windows: the side panel opens over the content instead of squeezing it
- [x] Dark, light and system themes
- [x] Colour themes: a dark and a light pick in Settings → Appearance (Flow, Blackout, Catppuccin Mocha,
  Tokyo Night, Gruvbox, Rosé Pine, Moss, Afterglow; Flow Light, Liner Notes, Lychee). Pale album-art accents
  are darkened on light themes so they stay readable
- [x] **Command palette (Ctrl+K)** for search and commands (`>` filters to commands)
- [x] Keyboard shortcuts (see the README)
- [x] Right-click menus on albums, songs and queue items
- [x] Toast notifications for actions such as "Added to the queue"
- [x] Welcome screen for an empty library
- [x] **Mini player** (Ctrl+M): compact, drag to move, back to the full window with one click. It stays on top,
  and on Wayland (which has no "always on top") it runs as a small XWayland helper process to manage that.
- [ ] Custom title bar (native window decorations for now)
- [ ] Respect the system "reduce motion" setting

### Tag editor (modelled on EasyTAG)
- [x] Open a folder (recursively) or single files, use "Edit tags…" on any album or song, drop files onto the editor, or run `lychee --edit FOLDER`
- [x] Add albums from your library without leaving the editor: a searchable picker (the file list's "+",
  the empty editor, or Ctrl+K) where you tick several albums at once; albums already open are marked
- [x] File list with markers for unsaved changes, pending renames and read or save errors
- [x] The file list sets each album apart in its own card (same album tag in the same folder; untagged files group
  by folder), headed by the album, artist, file count and unsaved changes. Click a header to select the whole
  album, so Look up searches for it
- [x] Multi-selection (Ctrl/Shift-click, Ctrl+A). A field reads "Multiple values" when the files disagree, and typing sets it for all of them
- [x] Fields: title, artist, album, album artist, year, genre, track and disc numbers with totals, composer, comment,
  plus original artist, lyricist, label, catalog number, ISRC, copyright, URL, encoded by and the MusicBrainz IDs
- [x] Number the selected tracks (per disc) and set the totals (EasyTAG's "#" button)
- [x] Artwork: add, replace, remove, export, change the picture type, use the folder image
- [x] Lyrics: edit plain or synced (LRC) lyrics per track
- [x] File info: format, sample rate, bit depth, channels, bitrate, duration, size, tag format
- [x] Scanner, "Fill tags": read tags from file and folder names with EasyTAG masks (`%a %b %t %n %y %g`…, `/` for a folder), with a live preview
- [x] Scanner, "Rename files": new names from tags. Renames wait until you save; `.lrc` lyrics move along; names are safe on Windows
- [x] Scanner, "Clean up": Title Case (keeps acronyms and Roman numerals), Sentence case, UPPER, lower, underscores and `%20` to spaces, tidy spaces, find and replace
- [x] Scanner, "Artists": write artist credits one way ("A feat. B & C", "A & B feat. C", "A, B & C" or "A; B; C";
  optionally with a comma before the last name: "A feat. B, C, D").
  `;` and feat./ft./featuring always separate artists; `,`, `&` and `/` do when switched on; artists you have
  albums by ("Simon & Garfunkel") stay whole
- [x] Scanner, "Genres": one spelling per genre (the library's most common, or Capitalised Words), with `;` (and
  optionally `,` or `/`) splitting several genres, which are saved as a real list (MP4 keeps one "A; B" line)
- [x] Undo and redo for every step, and revert per file or for everything
- [x] Lossless writing: MP3, WAV, AIFF, AAC and MP4 tags are edited in place, so frames the editor doesn't know about survive (Serato/Traktor data, private frames, chapters)
- [x] Saved files are re-read into the library, keeping play counts, Favorites and playlists, even after renames
- [x] Asks before quitting with unsaved changes
- [x] Says why a file can't be saved (no permission, read-only drive, moved), keeps free-text years valid
  ("c. 2006" → 2006), and still opens files whose tags other software left malformed
- [ ] Playlist generator, "load file names from a text file", converting between ID3v2.3 and v2.4, stripping tags

### Online lookup
- [x] Works out the album from tags, MusicBrainz IDs or folder names ("Artist - Album (1998) [FLAC]"); untagged file names count as titles
- [x] Loose songs (a few files, or folders such as "Singles") are looked up by title, which finds the singles and albums they appeared on
- [x] Shows what it is doing while it searches, and lets a contradicting title overrule a matching length
- [x] MusicBrainz release search. The track lists of the best candidates are compared with the files (durations, titles, track counts) and scored
- [x] Each file is matched to its track by MusicBrainz ID, then disc and track number, title, and finally duration
- [x] Fill in what's missing, or replace everything: title, artists, album, year (of the original release), genre,
  track and disc numbers with totals, label, catalog number, ISRC and MusicBrainz IDs
- [x] Front cover from the Cover Art Archive (1200 px), falling back to the release group's cover
- [x] Synced (or plain) lyrics from LRCLIB, matched by duration, for the whole album or any selection
- [x] Keeps to MusicBrainz's limit of one request per second and identifies itself with a proper User-Agent
- [ ] AcoustID fingerprinting for files with neither tags nor useful names
- [ ] Discogs as a second source

### Desktop integration
- [x] **Linux: MPRIS** (media keys, GNOME and KDE media controls, `playerctl`) and a `.desktop` file
- [x] **Windows: System Media Transport Controls** (media keys, volume flyout, lock screen). Written but not yet tested on Windows.
- [x] **macOS: Now Playing** (media keys, Control Center and menu bar widget, headphone remotes), `⌘` shortcut
  labels, ⌘Q / Dock / logout quits that save the session, and a universal app (Apple silicon + Intel) in a
  `.dmg`. Type-checked and packaged by the release workflow, but not yet run on a Mac.
- [ ] macOS: opening audio files from Finder ("Open With", double-click). winit doesn't pass them to the app.
- [ ] macOS: ⌘Q quits without asking about unsaved tag edits (the system ends the app directly)
- [ ] Single instance: opening a file while Lychee runs hands it to the running window
- [ ] Desktop notifications on track change

### Last.fm
- [x] Connect through the browser (desktop auth flow; no password stored)
- [x] "Now playing" updates
- [x] Scrobbling by Last.fm's rules (half the track or 4 minutes, tracks over 30 s)
- [x] Offline queue saved to disk, retried with back-off; status icon in the bottom-right corner
- [x] Optionally love tracks on Last.fm when you add them to Favorites
- [x] Scrobbles the main artist ("A feat. B", "A; B" or a song tagged with artists A and B counts for A; titles
      are sent as tagged), or the artist tag exactly as written (Settings → Last.fm → Artist to scrobble)
- [x] Your own API key in Settings, or one built in at compile time

---

## v0.2 "Daily Driver"
- [ ] System tray icon with controls; optionally close to tray
- [ ] Optional crossfade (never between gapless album tracks)
- [ ] Waveform seek bar
- [ ] Ratings and smart playlists (rules: genre, year, rating, play count, date added, format…)
- [ ] Live folder watching, folder browser, M3U import/export
- [ ] Single instance and desktop notifications
- [ ] A faster Opus decoder: the pure-Rust one uses about 20% of a CPU core; libopus would make it optional

## v0.3 "Audiophile & Power"
- [ ] Parametric / graphic EQ with presets
- [ ] Spectrum analyzer and level meters
- [ ] Bit-perfect / exclusive output (WASAPI exclusive, direct ALSA) with sample-rate switching
- [ ] Rarer formats through an optional FFmpeg backend: WMA, APE, WavPack, DSD, Musepack, TTA
- [ ] CUE sheets; library statistics

## Extensions (the architecture is ready for these)
- Lyrics and artwork lookups for the playing track outside the tag editor; AcoustID fingerprints
- ListenBrainz scrobbling alongside Last.fm
- Remote libraries: Subsonic / Navidrome, Jellyfin
- Internet radio; podcasts
- Remote control API; casting (UPnP/DLNA, Chromecast)
- Plugins (sandboxed WASM) and theme packs; a terminal UI on the same core

## Not planned
- Streaming-service features: recommendations, "You might also like", algorithmic home screens
- Clients for DRM streaming services, video playback, CD ripping, mobile apps
