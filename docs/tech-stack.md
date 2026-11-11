# Lychee: library research and recommended stack

Status: **decided and implemented in v0.1** (November 2026). The Flow design direction was chosen and is
built with Slint. Lychee is licensed Apache-2.0; Slint is used under its Royalty-free License 2.0, with the
required attribution in Settings → About. Everything below targets **Linux (Wayland and X11) and Windows**.

## Summary

| Concern | Recommendation | Why |
|---|---|---|
| UI toolkit | **Slint 1.18+** (Skia renderer) | Native and lightweight. Declarative `.slint` markup with a live preview, so the design can be iterated quickly. Stable 1.x API. |
| Decoding | **Symphonia 0.6** | Pure Rust: MP3, FLAC, WAV, AIFF, CAF, OGG Vorbis, AAC-LC, ALAC, MP4/M4A, MKV/WebM |
| Opus | **opus-decoder** (pure Rust), or **symphonia-adapter-libopus** | Symphonia 0.6 demuxes Opus but cannot decode it |
| Rarer formats | Optional **FFmpeg** backend (cargo feature) | WMA, APE, WavPack, DSD, Musepack, TTA… behind the same decoder trait |
| Audio output | **cpal 0.18** | Native PipeWire, PulseAudio, ALSA and JACK hosts on Linux; WASAPI on Windows |
| Resampling | **rubato** | High-quality conversion when the device rate differs from the file's rate |
| Engine | Our own small engine: decode thread → DSP → **rtrb** ring buffer → cpal callback | Needed for gapless playback, sample-accurate position, DSP and visualizer taps, and device switching |
| Tags and embedded art | **lofty 0.25** | Reads and writes ID3v1/v2, Vorbis comments, MP4 atoms, APE and RIFF INFO; also pictures and audio properties |
| Library database | **SQLite** via **rusqlite** (`bundled`, FTS5) | Embedded, fast, full-text search, single file |
| Scanning and watching | **walkdir / jwalk + rayon**, **notify** (+ debouncer) | Parallel tag reading; live updates when files change |
| Fuzzy search | **nucleo** | The matcher used by the Helix editor; powers Ctrl+K |
| Images | **image** + **fast_image_resize** + a fast blur | Thumbnail cache, blurred backdrops, accent-color extraction |
| Linux media controls | **MPRIS** over **zbus 6** (or **mpris-server 0.11**) | Media keys, GNOME and KDE media widgets, `playerctl` |
| Windows media controls | **SMTC** via the **windows** crate | Media keys, volume flyout, lock screen |
| Tray and notifications | **ksni** (Linux), **tray-icon** (Windows), **notify-rust** | |
| Plumbing | tracing, thiserror/anyhow, serde + toml, directories, crossbeam-channel, interprocess (single instance), rfd (file dialogs) | |
| Packaging | Flatpak + AppImage (+ .deb/.rpm); MSI or NSIS on Windows; GitHub Actions CI | |

---

## 1. UI toolkit

UI quality is the priority for this project, so the toolkit matters most. Candidates, as of November 2026:

| | Slint | Iced | Dioxus (desktop) | Tauri 2 | GPUI | egui | GTK4 + libadwaita |
|---|---|---|---|---|---|---|---|
| Version | 1.18.1 (Sep 2026), 1.19 in testing | 0.14 (Dec 2025) | 0.7.x | 2.x | crates.io stale (0.2.2); community forks | 0.x | 4.x |
| UI written in | `.slint` markup + Rust | Rust | Rust (RSX) + CSS | HTML/CSS/JS (or Rust→WASM) | Rust | Rust | Rust (+ XML/Blueprint) |
| Rendering | Native (Skia, FemtoVG, software; Vello experimental) | wgpu | System webview (WebKitGTK / WebView2); native "Blitz" renderer still early | System webview | Own GPU renderer | GPU, immediate mode | GTK |
| Fully custom look | Yes | Yes, more work | Yes (CSS) | Yes (CSS) | Yes | Hard to make it look premium | Bound to the Adwaita look |
| Design iteration | Live preview and viewer, no recompile | Recompile | Hot reload + hot-patching | Web HMR | Recompile | Recompile | Recompile |
| Footprint | Small | Small | Webview (heavier) | Webview (heavier) | Small | Small | GTK runtime |
| Windows story | Good | Good | Good (WebView2) | Good | Usable | Good | Awkward to ship |

### Recommendation: Slint

1. **Fast design iteration.** `.slint` files can be edited in the live preview without recompiling Rust. Since visual polish is the whole point of Lychee, that loop matters more than anything else.
2. **It can render these designs.** Slint has rounded clipping, drop shadows, gradients, transforms, property animations and states, custom fonts, and (since 1.18) `FlexboxLayout` and runtime z-order. Element-level `backdrop-blur` was merged in October 2026 and is listed for testing in 1.19 (Skia renderer), which covers Aurora's frosted glass. The blurred album backdrop never needed it anyway: we blur the image once in Rust and display it.
3. **It is light enough to leave running.** A music player is open all day. Native rendering keeps memory low and idle CPU near zero. Webview options carry a browser engine, and WebKitGTK is the weak spot on Linux.
4. **The API is stable.** 1.x has been stable since 2023, with regular releases from a company that works on it full time. The MSRV is Rust 1.92.
5. **Logic stays in Rust.** `.slint` markup compiles to Rust at build time and is type-checked against our models.
6. **Licensing.** Slint is offered under GPLv3, a royalty-free license (which requires attribution), or a commercial license. If Lychee is GPL-3.0, everything is simple. If Lychee is MIT or Apache, we use Slint's royalty-free license with its attribution.

**Runner-up: Iced 0.14.** Pure Rust with an Elm architecture that suits a player well, plus custom wgpu shaders, which are great for visualizers. Pre-1.0 API churn and more effort for a polished look put it second.

**Choose Dioxus instead if** you want unlimited CSS freedom and accept the webview cost. These HTML mockups would port almost one-to-one.

**Not recommended for this project.** GPUI: the official crate is stale and the community forks are fragmented, though the Hummingbird player shows what it can do. egui: its immediate-mode look works against an aesthetics-first goal. GTK4/libadwaita: it locks the app into GNOME's look and is painful on Windows. Qt/QML: C++ toolchain and heavy runtime.

**Prior art worth studying.** Hummingbird (GPUI, alpha), RustMusic (Tauri 2 + Svelte), Amberol (GTK4), Tributary (GTK4). They show that the audio stack below is a sound choice in Rust.

### How the mockup effects map to Slint

| Effect | Implementation |
|---|---|
| Blurred, drifting album backdrop | Blur once in Rust (downscale + fast blur), show as an `Image` with a slow transform animation, crossfade on track change |
| Frosted glass panels | Translucent fills; `backdrop-blur` on Skia (1.19+), plain translucency elsewhere |
| Dynamic accent color | Extracted at scan time, stored in the DB, bound to a `Theme` global with animated color transitions |
| Waveform seek bar | Precomputed peak data → a `for` loop of rectangles (about 100 bars) |
| Synced lyrics | `Flickable` with an animated `viewport-y`; the current line comes from the engine's position events |
| Dot-matrix art / spectrum (Console) | Dot art pre-rendered as an image per album; spectrum is a model of bar heights updated at ~30 fps |
| Custom title bar | Frameless window; drag and resize handled through winit |

---

## 2. Audio

### Decoding: Symphonia 0.6

Symphonia 0.6.0 is out (0.6.1 is current). It reworked the architecture, enabled SIMD by default, rewrote the Matroska demuxer, and added a priority system for decoders and format readers. That priority system lets us register extra decoders cleanly. MSRV is 1.85. By default only royalty-free codecs are enabled, so we turn on the MP3, AAC, ALAC and MP4 features explicitly.

| Format | Handled by |
|---|---|
| MP3 (and MP1/MP2), FLAC, WAV, AIFF, CAF (PCM/ADPCM), OGG Vorbis, AAC-LC in M4A/MP4, ALAC, MKV/WebM audio | Symphonia |
| Opus (`.opus`, Ogg, WebM) | Symphonia demuxer + **opus-decoder** (pure Rust, passes the RFC 8251 conformance vectors). Fallback: **symphonia-adapter-libopus** (C libopus) |
| WMA, APE, WavPack, DSD (DSF/DFF), Musepack, TTA, other AAC profiles | Optional **FFmpeg** backend behind a cargo feature, using the same `DecoderBackend` trait |

That covers "plays essentially anything" while the default build stays pure Rust with no system codec dependencies.

### Output: cpal 0.18

cpal 0.18 added native **PipeWire** and **PulseAudio** hosts on Linux, alongside ALSA and JACK. It also unified error types and reports accurate timestamps on every platform. On Windows it uses **WASAPI** (ASIO is optional). Accurate timestamps matter for lyric sync and visualizers.

### Engine design: why not just rodio?

rodio 0.23 (cpal + Symphonia) is excellent for simple playback, and we may use it in tests. A serious player needs more control:

- **Gapless playback:** open and pre-roll the next track before the current one ends.
- **Sample-accurate position:** for synced lyrics, the waveform and scrobbling.
- **A DSP chain:** ReplayGain, EQ, crossfade, limiter.
- **Analysis taps:** spectrum and VU meters.
- **Output switching:** change device without a restart, and later exclusive or bit-perfect modes.

So Lychee gets a small engine of its own (on the order of 1–2k lines):

```
file ─► Symphonia reader ─► decoder ─► rubato (if rates differ) ─► DSP chain ─► rtrb ring buffer ─► cpal callback ─► device
              ▲                                                    │
   next track pre-opened (gapless)                                 └─► analysis tap ─► realfft ─► spectrum / VU events
```

Rules: the real-time callback never allocates, locks or does I/O. Decoding runs on its own thread, a little ahead.

Supporting crates: **rubato** (resampling), **rtrb** (lock-free single-producer single-consumer ring buffer), **realfft** (spectrum), **ebur128** (loudness and ReplayGain analysis), **biquad** (EQ filters).

---

## 3. Metadata, artwork and color

- **lofty 0.25**: one API for tags and audio properties across MP3, FLAC, WAV, MP4, Ogg, APE, AIFF and others. It reads and writes, which a future tag editor needs. Symphonia's metadata reader is the fallback for anything lofty can't handle.
- **Artwork lookup order:** embedded picture, then a folder image (`cover`, `folder`, `front`, `album` in `.jpg/.png/.webp`), then a **generated placeholder** (the procedural covers in these mockups, ported to Rust and seeded by album name).
- **Cache:** thumbnails at 64/128/256/512 px plus a pre-blurred backdrop per album, stored in the cache dir. Uses **image** and **fast_image_resize**.
- **Accent color:** extracted once at scan time from a 64×64 thumbnail and stored in the DB, so theming costs nothing at runtime. Either a small in-house k-means/median-cut with WCAG contrast rules, or **material-colors** (a port of Google's material-color-utilities; it currently needs Rust 1.97).

## 4. Library and search

- **rusqlite** with the `bundled` feature (SQLite + FTS5), schema migrations via `rusqlite_migration`. A single writer thread; readers use WAL mode.
- **Scanning:** walk folders in parallel (`jwalk` or `walkdir`), read tags with `rayon`, and rescan incrementally by (mtime, size). Large libraries stay fast.
- **Watching:** `notify` plus `notify-debouncer-full` applies file changes live.
- **Search:** FTS5 for full-text search; **nucleo** ranks the fuzzy matches shown in the Ctrl+K palette, over an in-memory index of titles, artists and albums.

## 5. Desktop integration

| Feature | Linux | Windows |
|---|---|---|
| Media keys and OS media widgets | MPRIS over **zbus 6**. The interface is small enough to implement directly; **mpris-server 0.11** is the ready-made option (0.10 pulled in an unmaintained `zvariant_derive`, so check the zbus version) | System Media Transport Controls via the **windows** crate |
| Tray icon | **ksni** (StatusNotifierItem over D-Bus, pure Rust) | **tray-icon** |
| Notifications | **notify-rust** | **notify-rust** (toasts) |
| Custom global hotkeys | XDG GlobalShortcuts portal via **ashpd** (Wayland), **global-hotkey** (X11) | **global-hotkey** |
| File dialogs | **rfd** (uses XDG portals) | **rfd** |
| Single instance / "Open with" | **interprocess** local socket | **interprocess** named pipe |

Avoid **souvlaki**, the usual cross-platform media-controls crate. It has been unmaintained for over a year and pins an old `windows` crate. **playwire** is a new rewrite worth watching, but it is unproven. Two thin backends of our own, behind a `MediaControls` trait, are safer and also serve as an extension point.

## 6. Plumbing, packaging and tooling

- **Plumbing:** `tracing` for logs, `thiserror` in libraries and `anyhow` in the app, `serde` + `toml` for config, `directories` for XDG and AppData paths, `crossbeam-channel` for messages.
- **Linux packaging:** Flatpak is the primary target (Flathub). Also AppImage, plus `.deb`/`.rpm` via `cargo-deb` and `cargo-generate-rpm`.
- **Windows packaging:** MSI (`cargo-wix`) or an NSIS installer; `cargo-packager` can produce several formats from one config.
- **CI:** GitHub Actions on Linux and Windows running `fmt`, `clippy`, tests and release builds.
- **Toolchain:** Rust ≥ 1.92 (Slint 1.18's MSRV). On Linux the build needs the ALSA and fontconfig
  development packages (`alsa-lib-devel fontconfig-devel` on Fedora, `libasound2-dev libfontconfig1-dev` on
  Debian/Ubuntu). cpal's native PipeWire host is not enabled: it needs libclang for bindgen, and ALSA
  reaches PipeWire through `pipewire-alsa` anyway.

## Sources

- Slint 1.18 release: <https://slint.dev/blog/slint-1.18-released>. Backdrop blur: <https://github.com/slint-ui/slint/pull/13821>, 1.19 testing: <https://github.com/slint-ui/slint/discussions/14688>. Changelog / MSRV: <https://github.com/slint-ui/slint/blob/master/CHANGELOG.md>
- Iced 0.14: <https://github.com/iced-rs/iced/releases/tag/0.14.0>
- Dioxus 0.7: <https://dioxuslabs.com/blog/release-070/>. Blitz: <https://github.com/DioxusLabs/blitz>
- GPUI on crates.io: <https://github.com/zed-industries/zed/issues/66275>, <https://crates.io/crates/gpui-ce>
- Hummingbird: <https://hummingbird.mailliw.org/>. RustMusic: <https://github.com/larevuegeek/rustmusic-desktop>
- Symphonia 0.6: <https://github.com/pdeljanov/Symphonia/releases/tag/v0.6.0>
- Opus: <https://lib.rs/crates/opus-decoder>, <https://lib.rs/crates/symphonia-adapter-libopus>
- cpal releases: <https://github.com/rustaudio/cpal/releases>. rodio changelog: <https://github.com/RustAudio/rodio/blob/master/CHANGELOG.md>
- lofty: <https://github.com/Serial-ATA/lofty-rs>
- Media controls: <https://github.com/SFG5453/playwire>, <https://github.com/esoltys/luminous/issues/576>, <https://crates.io/crates/mpris-server>, <https://github.com/SeaDve/mpris-server/issues/20>
- material-colors: <https://github.com/Aiving/material-colors>

## What v0.1 actually uses

- `slint 1.18` (winit backend, FemtoVG renderer; the software renderer is only used for headless screenshots)
- `symphonia 0.6` (`all` features) + `opus-decoder 0.1` for Opus. It is correct but slow: about 20% of a CPU
  core while playing, so a libopus option is planned.
- `cpal 0.18`, `rubato 5` (FFT resampler), `rtrb 0.4`
- `lofty 0.25`, `rusqlite 0.41` (bundled SQLite), `image 0.25`, `nucleo-matcher 0.3`, `walkdir`, `rayon`
- `ureq 3` (rustls) + `md-5` for Last.fm
- `mpris-server 0.11` (zbus 6) on Linux, `windows 0.62` (SMTC) on Windows. souvlaki was avoided as planned.
- `rfd` (folder picker via XDG portals), `open`, `directories`, `toml`, `tracing`
