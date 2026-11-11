# CLAUDE.md: working on Lychee

Lychee is a local-library music player for **Linux and Windows**, written in **Rust**, with a custom
**Slint** UI in the "Flow" design. These notes are for agents picking the project up. Read them
before changing code: they record decisions the owner made and pitfalls that already cost time.

## What the owner wants

- **The UI is the point.** The owner finds Strawberry, Clementine and similar players dated. New UI
  must match the Flow look: tokens from `crates/lychee/ui/theme.slint`, shared widgets from
  `ui/widgets.slint`, and generous spacing. Check every UI change with a screenshot (see Testing).
- **A local player, not a streaming service.** Don't add recommendations, "You might also like" rows or
  algorithmic home screens. Factual navigation is fine ("More by {artist}" for albums the user owns,
  Recently Added, play counts).
- **Tag editor = EasyTAG, modernised.** The editor follows EasyTAG's features (multi-file editing,
  scanner masks, renaming, case clean-up, pictures, undo) with Lychee's UX.
- **Last.fm** is a core feature, with its status icon in the bottom-right of the player bar.
- **Licences:** the project is Apache-2.0. Slint is used under its Royalty-free 2.0 licence, which
  requires keeping `AboutSlint` in Settings → About and the "Made with Slint" badge in the README.
- **Delivery:** when asked to "push", the owner expects a commit on `main` pushed to
  `origin` (git@github.com:Noffee25/Lychee.git). Commits so far end with a `Co-Authored-By:` trailer
  for the AI model that wrote them; follow your harness's attribution rules. Keep `README.md` and
  `docs/` current when features change.

## Repository map

```
crates/
  lychee-core      model (Track, Album, ids), the play Queue (shuffle/repeat/play-next), formatting,
                   MD5 (md5.rs: Last.fm signatures and artwork cache keys)
  lychee-audio     Engine: Symphonia decode (+ pure-Rust Opus), remix, rubato resample, rtrb ring
                   buffer, cpal output, gapless hand-over, fades. examples/decode.rs, engine_check.rs
  lychee-library   scanner (walkdir + scoped threads + lofty), SQLite store (db.rs), artwork cache + accent/tint
                   colours, in-memory LibraryIndex + fuzzy search, LRC lyrics, refresh_files()
  lychee-lastfm    Last.fm client and background Scrobbler with an offline queue
  lychee-platform  MPRIS (Linux), SMTC (Windows; type-checked, never run here), `open()` for URLs/folders
  lychee-tagger    tag editor engine: lossless read/write (file.rs), EasyTAG masks (mask.rs), clean-up
                   (process.rs), Session with pending edits, undo, saving (session.rs)
  lychee-lookup    MusicBrainz / Cover Art Archive / LRCLIB clients, album guessing and track matching
  lychee           the app (bin). src/: main.rs (startup, CLI), app.rs (controller), editor.rs (tag
                   editor page), mini.rs (mini player + XWayland helper), models.rs (LazyModel),
                   images.rs, palette.rs, lastfm.rs, settings.rs, screenshot.rs (headless renders)
                   ui/: app.slint (window, routing, shortcuts), state.slint (AppState/Player/Actions),
                   editor-state.slint (Editor), theme.slint, widgets.slint, icons.slint, views/*.slint
design/            HTML prototypes; the app implements direction D "Flow" and its compact mini player
docs/              features.md (done/planned), architecture.md, tech-stack.md, screenshots/
assets/            desktop file, icons; windows/lychee.ico + lychee.res (from packaging/windows/make-icon.py)
packaging/         linux/install.sh + build-packages.sh, windows/lychee.iss, release-notes.md,
                   ci-report.sh (writes CI/release results to the status branches)
```

## Build, run, test

- Rust ≥ 1.92 (the toolchain here is 1.99 from Fedora's dnf: no rustup, clippy, rustfmt or extra targets).
- Linux needs the ALSA and fontconfig development files (`alsa-lib-devel fontconfig-devel` /
  `libasound2-dev libfontconfig1-dev`). If they're missing and you can't install packages, a
  `PKG_CONFIG_PATH` directory with hand-written `alsa.pc`/`fontconfig.pc` files pointing at the runtime
  `.so.N` libraries works. `RUST_FONTCONFIG_DLOPEN` breaks the fontique build, so don't use it.
- `cargo build`, `cargo run --release -- [FILES…]` (plays them), `cargo run -- --edit FOLDER` (opens the
  tag editor), `cargo test --workspace` (all tests are offline). The dev profile builds dependencies
  at opt-level 2.
- Code style: `rustfmt.toml` (max_width 120). rustfmt and clippy aren't installed here, so match the
  surrounding style by hand. CI (`.github/workflows/ci.yml`) builds and tests Ubuntu and Windows
  with `--locked`, so commit `Cargo.lock`. Clippy is advisory there.
- Handy tools: `cargo run -p lychee-tagger --example tag -- FILE [field=value]…` prints or edits tags;
  `cargo run -p lychee-lookup --example lookup -- "Artist" "Album"` runs a live MusicBrainz search.
- **Type-checking Windows code on Linux.** Install a throwaway rustup toolchain with the
  `x86_64-pc-windows-gnu` target, setting `RUSTUP_HOME` and `CARGO_HOME` to a temp dir. Run its
  `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu` with `RUSTC` pointing at that
  toolchain's rustc. `CC_x86_64_pc_windows_gnu` and `AR_x86_64_pc_windows_gnu` can point at stub
  scripts that just create their output files, because `check` never links the C parts (SQLite, ring).
  Do this before releasing: CI is the only place Windows gets built.
- **Release profile:** fat LTO, one codegen unit, and `opt-level = "s"` for the `lychee` crate only. That
  crate is mostly UI code generated from `.slint`; the audio and decoding crates stay at full speed.
  This cut the binary from 53 to 44 MB.

## How the app is put together

- **One controller.** Every Slint callback becomes an `Action` (`app.rs`) and goes through the
  `Dispatcher`, which queues actions so re-entrant callbacks can't cause borrow panics. To add an
  action: add a variant, wire it in `wire_actions!` (or `editor::wire` for the `Editor` global), and add a
  `handle()` arm. Commands for the Ctrl+K palette are `palette::Command` + `App::run_command`.
- **Ticks and threads.** A 50 ms timer dispatches `Action::Tick`, which drains channels: engine events,
  scan progress, Last.fm, media keys, decoded images, editor workers and the mini helper. Slow work (scans,
  tag I/O, network, file dialogs via `rfd`) runs on threads and reports back through channels. Never block
  the UI thread.
- **Pages.** These need a `Page` variant (with a `key`), a `refresh_page` arm, and an
  `if AppState.page == "…": View {}` line in `app.slint`. The right panel is hidden on the editor page.
- **Lazy models.** `LazyModel` builds rows on demand, and `refresh()` re-runs the builder. **Builders
  must read live state**: share ids through `Rc<Cell<…>>` (see `NowIds`) and look tracks up in
  `library.index()` at build time. Values captured by value at page-build time go stale. That bug once
  stopped the album page from highlighting the playing song.
- **The tag editor** (`editor.rs`, `EditorCtl`) mirrors a `lychee_tagger::Session` into the `Editor` global.
  - Worker results identify files by *path*, because indices change when files are added or closed.
  - Text inputs reload only when `Editor.generation` changes, never while the user types.
  - `Outcome` tells the app to toast, refresh views after a save, or quit.
  - Saving writes tags, then renames, then calls `Library::refresh_files`, which keeps track ids, likes
    and play counts across renames.
- **Tag writing must stay lossless.** MP3, WAV, AIFF, AAC and MP4 are edited through lofty's
  `SplitTag`/`MergeTag`, so frames the editor doesn't know survive (Serato, PRIV, chapters). The main tag
  is made to match the editor; secondary tags (ID3v1, APE) only get changed fields. The round-trip tests
  in `crates/lychee-tagger/tests` cover seven formats; extend them when you touch `file.rs`.
- **Reading is forgiving, writing is strict.** lofty's default mode rejects a whole file over one
  malformed frame, so reads (tagger and library scanner) retry in `ParsingMode::Relaxed`. The Year field
  is normalised to an ID3v2.4 timestamp before writing ("c. 2006" → "2006", no year → refused), because
  lofty writes free-text dates but can't read them back. Errors shown to the user go through
  `lychee_tagger::io_message`, which turns lofty's generic "failed to write" into the real cause.
- **Online lookups** (`lychee-lookup`):
  - MusicBrainz allows one request per second. `Client` spaces requests by 1.1 s and sends a
    User-Agent with the project URL. Keep one shared `Client`, and never fire requests in a loop
    without it.
  - `matching::guess` reads tags, then folder names ("Artist - Album (1998) [FLAC]"). Folders named
    "Singles", "Misc", etc. aren't albums: those files are looked up song by song.
  - `find_candidates` scores releases on durations, titles, track count and names. A contradicting
    title vetoes a duration-only match.
  - Unit tests use JSON fixtures, never the network.
- **Mini player** (`mini.rs`):
  - On X11 and Windows it is a second Slint window, kept on top.
  - Wayland has no "always on top", so there the app re-launches itself as `lychee --mini-player` on
    XWayland. The parent sends JSON state lines on stdin and gets button words back on stdout; the
    helper quits on EOF.
  - winit sends X11's `_NET_WM_STATE_ABOVE` too early, so `keep_on_top` re-sends it after mapping.
  - On Wayland an app can't un-minimise or focus its own window, so the main window is *hidden* (and
    later re-shown) rather than minimised.
  - Drags start only after a few pixels of movement, and Slint's pointer state is reset after the
    window manager takes over.
  - `LYCHEE_MINI=inprocess|external` forces either mode.

## Slint pitfalls (1.18, winit + FemtoVG)

- **No text inside rounded clips or faded containers.** FemtoVG draws `clip: true` with a
  `border-radius`, and `opacity < 1` on an element with children, through an offscreen layer. The
  layer is resampled at fractional positions, so text comes out blurry. This made the tag editor and
  dialogs look out of focus.
  - Only put images in rounded clips (e.g. `Cover`).
  - Show disabled states with colours (see `PillButton`, `FieldInput`).
  - Let `ListView`/`ScrollView` clip themselves (rectangular) and inset them inside rounded cards.
- `color` and `row` can't be used as property names. Use names like `bar-color`, `tiles`, `track`.
- Add `cross-axis-alignment: center` to `HorizontalLayout`s, or icons and text sit off-centre.
- Ids inside `if`/`for` aren't visible outside them. Use a small component with its own `changed`/`init`
  handlers (e.g. `LyricsBox`), or `init => { self.focus(); }`.
- Use `content-x/y/width/height` (`viewport-*` is deprecated) and `transform-rotation`.
- Binding loops: a label sized by its text can't depend on a value derived from a sibling's layout
  width. Store such values from pointer events instead (see `SeekBar.hover-fraction`).
- Columns with stretch factors need `preferred-width: 0px; min-width: 0px`, or long text squeezes
  neighbours.
- `Segmented`/`Toggle`/`CheckChip` set their own `selected`/`checked`, so bind them to globals with `<=>`.
- Group elements that share a condition under one `if` (a half-drawn player bar was reported once).
- Keyboard shortcuts are `KeyBinding`s on the window-level `FocusScope` (`keys` in `app.slint`). Dialogs
  take focus on `init`, and `app.slint` refocuses `keys` when editor dialogs close.
- `IconButton` shows a tooltip only when `tooltip` isn't empty. Tooltips misplace themselves in tiny
  windows, so the mini player has none.
- std widgets use the `fluent` style (`build.rs`); `Palette.color-scheme` follows `Theme.dark`.
- `slint::set_xdg_app_id` only works after the first window is created and before it's shown.
  - Called earlier, it fails with `NoPlatform`, because Slint has no platform yet. The Wayland
    window then has no app id, and GNOME shows a generic icon called "Unknown" instead of
    matching `lychee.desktop`.
  - Check it in a nested mutter with `WAYLAND_DEBUG=1`: look for `xdg_toplevel…set_app_id("lychee")`.

## Testing and checking your work

- **Headless screenshots** (Slint software renderer). Use `LYCHEE_HOME` to keep config and library out
  of your real profile:
  `LYCHEE_HOME=/tmp/home LYCHEE_SCREENSHOT=out.png LYCHEE_SCREENSHOT_SCENE="add-folder:/path/Music;album:Some Album" cargo run`
  - Scene steps are `;`-separated (see `App::run_scene` and `screenshot.rs`):
    - pages: `page:`, `album:`, `artist:`
    - playback: `play-album:`, `track:N`, `seek:MS`, `pause`
    - UI: `palette:`, `tab:`, `theme:N`, `like`, `queue:`, `toast:`
    - library: `add-folder:`, `like-album:`
    - tag editor: `edit-album:`, `editor-open:PATH`, `editor-select:0,1`, `editor-tab:`,
      `editor-field:id=value`, `editor-more`, `editor-scanner:N`, `editor-mask:`, `editor-lookup`,
      `editor-lookup-apply`, `editor-save`, `editor-quit`
    - waiting: `wait:MS`, to let threads and network finish
  - Extras: `LYCHEE_SCREENSHOT_SIZE=WxH`, `LYCHEE_SCREENSHOT_HOVER=x,y` (hover states),
    `LYCHEE_SCREENSHOT_MINI=mini.png`, and `LYCHEE_SCREENSHOT_MINI_CLICKS="x,y;x,y"`.
  - The software renderer can't show FemtoVG issues (layers and blur), rounded clipping, or compositor
    behaviour.
- **Real compositor tests without touching the owner's desktop.** Run a nested headless mutter:
  `dbus-run-session` then `mutter --headless --wayland --virtual-monitor 1280x800 --wayland-display=NAME`.
  It also starts Xwayland, so pass its XAUTHORITY to X clients.
  - Drive it with `org.gnome.Mutter.RemoteDesktop` on the private bus (pointer and keys) and capture
    frames with `org.gnome.Mutter.ScreenCast`.
  - Observe with `RUST_LOG=lychee=debug` (every action is logged), MPRIS on the private bus,
    `WAYLAND_DEBUG=1` and `xprop`.
  - Never inject input into or open windows on the owner's live session (DISPLAY=:0 / wayland-0).
- Test fixtures: `crates/lychee-tagger/tests/fixtures` (tiny silent files in seven formats); the
  library test writes WAVs on the fly. Make larger test media with ffmpeg in a temp dir, never in the repo.

## Environment notes (owner's machine)

- Fedora 44, GNOME on Wayland, Xwayland at `DISPLAY=:0`. ffmpeg, ImageMagick (`magick`), xprop, mutter
  and gdbus are installed; Xvfb, xdotool and gh are not.
- `~/.ssh/known_hosts` had no github.com entry. Pushing worked with a temporary known_hosts built from
  <https://api.github.com/meta> (check the fingerprints against GitHub's published ones) passed through
  `GIT_SSH_COMMAND="ssh -o UserKnownHostsFile=… -o StrictHostKeyChecking=yes"`. Don't edit `~/.ssh`.
- The GitHub repo is private: its API (and the Actions page) needs credentials you probably don't
  have. CI and release runs therefore report to branches. Read them with
  `git fetch origin status/ci && git show FETCH_HEAD:STATUS.md`; use `status/release` for releases.
  - The report (`packaging/ci-report.sh`) comes from GitHub's jobs API: every job's result, and for
    failed jobs only, the failed step, the `##[error]` lines and the log leading up to the last error.
    Passing jobs get no log, so clippy warnings never show up there.
  - Check the commit line: the branch holds only the latest run. If a report is missing or stale,
    ask the owner for the run's Report job log.
- The owner wants the workflows kept small: no retry wrappers or extra test matrices. A runner's network
  outage (e.g. apt "Temporary failure resolving") is waited out and re-run, not scripted around.
- Workflow tokens are read-only in this repo. Publish declares `contents: write`; the Report jobs declare
  `contents: write` and `actions: read`.

## Releasing

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`), update `packaging/release-notes.md`
   (its file names include the version), and commit.
2. Push `main` and wait for `status/ci` to report success. That is the first real Windows build of new
   code.
3. `git tag vX.Y.Z && git push origin vX.Y.Z`. `.github/workflows/release.yml` then builds:
   - on Ubuntu 22.04 (glibc 2.35): the `.deb`, `.rpm` and `.tar.xz` with `install.sh`;
   - on Windows: the Inno Setup installer and a portable zip.

   It publishes them with `SHA256SUMS.txt` as a GitHub release and reports to `status/release`.
4. Packaging notes:
   - Linux packages declare the libraries Lychee `dlopen`s (Wayland, X11, xkbcommon, EGL/GL), which no
     scanner can detect. Keep them in sync with `install.sh`'s list.
   - On Windows the C runtime is static (`.cargo/config.toml`), so there's nothing to redistribute. The
     exe icon is linked from `assets/windows/lychee.res`; `LYCHEE_NO_EXE_ICON=1` skips it.
5. If a job fails, nothing is published: Publish needs both builds.
   - If the cause is outside the code (a runner's network, for example), the owner can click *Re-run
     failed jobs*.
   - Or delete the tag (`git push origin :refs/tags/vX.Y.Z`), tag the fixed commit and push the tag again.

## Dependencies

The owner asked for a lean dependency tree that loses no features. The Linux build compiles about
457 crates (it was 512); `Cargo.lock` also lists crates for platforms we don't build (macOS, Android,
web). Decisions so far:
- **Removed:**
  - Slint's `image-default-formats` (it pulled in an AV1 *encoder* and EXR); the `image` crate keeps
    jpeg/png/webp/gif/bmp/tiff.
  - `md-5` (replaced by `lychee_core::md5`, checked against RFC 1321 and the old crate's output);
  - `rayon` (scoped threads in `parallel_filter_map`);
  - `open` (`lychee_platform::open`);
  - `tracing-subscriber`'s `env-filter` (`Targets` parses the same `RUST_LOG` syntax);
  - several unused declarations.
- **Downgraded / unified:** `mpris-server` 0.10, which uses zbus 5 like Slint, so there's one D-Bus stack
  instead of two; `toml` 1.x, which shares its parser with the build tools.
- **Kept on purpose:**
  - `rfd` for its zenity fallback when there's no desktop portal;
  - Slint's `renderer-software`, its fallback when OpenGL fails (VMs, broken drivers);
  - `accessibility`;
  - winit's `sctk-adwaita`, which draws title bars on GNOME;
  - the rustls/ring TLS stack.
- Check new dependencies with `cargo tree -p lychee --target x86_64-unknown-linux-gnu -e normal,build`.
  `cargo tree -d` finds duplicate versions, and `--prune <crate>` shows what a dependency costs.

## Status and next steps

Done and planned work is tracked in `docs/features.md`. Notable gaps:
- Windows type-checks and CI builds it, but nobody has run it on a real Windows machine yet.
- EasyTAG features not done yet: playlist generator, loading file names from a text file, ID3v2.3/2.4
  conversion, stripping tags.
- Also planned: AcoustID fingerprints, ReplayGain, live folder watching, and a system tray.
