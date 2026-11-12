//! OS media integration: lets media keys, desktop media widgets and lock screens see and control
//! what Lychee is playing.
//!
//! * Linux/BSD: MPRIS over D-Bus (GNOME and KDE media controls, `playerctl`, media keys).
//! * Windows: System Media Transport Controls (media keys, volume flyout, lock screen).
//! * macOS: Now Playing (media keys, Control Center and menu bar widget, headphone remotes).
//!
//! The UI never talks to these APIs directly. It pushes state with the setters below and
//! receives [`MediaAction`]s on a channel, the same way every other control surface does.
//!
//! [`instance`] keeps one Lychee running per library: later launches hand it their files.

use std::path::PathBuf;

use crossbeam_channel::Sender;

pub mod instance;
#[cfg(all(unix, not(target_os = "macos")))]
mod mpris;
#[cfg(target_os = "macos")]
mod nowplaying;
#[cfg(windows)]
mod smtc;

/// A request coming from the OS.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaAction {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Relative seek in milliseconds.
    SeekBy(i64),
    /// Absolute seek in milliseconds.
    SetPosition(u64),
    SetVolume(f64),
    SetShuffle(bool),
    SetRepeat(Repeat),
    Raise,
    Quit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Playback {
    #[default]
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NowPlaying {
    pub track_id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub duration_ms: u64,
    /// Local cover image to show in the OS widget.
    pub art_path: Option<PathBuf>,
}

/// Updates sent to the backend (its thread, or straight to the OS on macOS).
#[derive(Clone, Debug)]
#[cfg_attr(not(any(windows, unix)), allow(dead_code))]
enum Update {
    Metadata(Option<NowPlaying>),
    Playback(Playback),
    Position(u64),
    Seeked(u64),
    // Windows' and macOS' media controls have no volume of their own; only MPRIS reads this.
    #[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
    Volume(f64),
    // macOS' Now Playing widget shows no shuffle or repeat.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Shuffle(bool),
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Repeat(Repeat),
    #[cfg(windows)]
    Window(isize),
}

pub struct MediaControls {
    #[cfg(all(unix, not(target_os = "macos")))]
    backend: mpris::Backend,
    #[cfg(windows)]
    backend: smtc::Backend,
    #[cfg(target_os = "macos")]
    backend: nowplaying::Backend,
}

impl MediaControls {
    /// Start the integration. Returns `None` where it isn't available (no session bus, etc.).
    #[allow(unused_variables)]
    pub fn new(actions: Sender<MediaAction>) -> Option<MediaControls> {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            return mpris::Backend::start(actions).map(|backend| MediaControls { backend });
        }
        #[cfg(windows)]
        {
            return Some(MediaControls { backend: smtc::Backend::start(actions) });
        }
        #[cfg(target_os = "macos")]
        {
            return Some(MediaControls { backend: nowplaying::Backend::start(actions) });
        }
        #[allow(unreachable_code)]
        None
    }

    #[allow(unused_variables)]
    fn send(&self, update: Update) {
        #[cfg(any(windows, unix))]
        self.backend.send(update);
    }

    /// Windows needs the main window handle (HWND) before the controls can appear.
    #[allow(unused_variables)]
    pub fn attach_window(&self, hwnd: isize) {
        #[cfg(windows)]
        self.send(Update::Window(hwnd));
    }

    pub fn set_metadata(&self, now_playing: Option<NowPlaying>) {
        self.send(Update::Metadata(now_playing));
    }

    pub fn set_playback(&self, playback: Playback) {
        self.send(Update::Playback(playback));
    }

    /// Call periodically (about once a second) while playing.
    pub fn set_position(&self, ms: u64) {
        self.send(Update::Position(ms));
    }

    /// Call after a user-initiated seek or track restart.
    pub fn seeked(&self, ms: u64) {
        self.send(Update::Seeked(ms));
    }

    pub fn set_volume(&self, volume: f64) {
        self.send(Update::Volume(volume));
    }

    pub fn set_shuffle(&self, shuffle: bool) {
        self.send(Update::Shuffle(shuffle));
    }

    pub fn set_repeat(&self, repeat: Repeat) {
        self.send(Update::Repeat(repeat));
    }
}

/// Open a URL, file or folder with the desktop's default handler, without blocking.
pub fn open(target: impl AsRef<std::ffi::OsStr>) -> std::io::Result<()> {
    use std::process::Command;
    let target = target.as_ref();
    #[cfg(windows)]
    let child = Command::new("explorer").arg(target).spawn()?;
    #[cfg(target_os = "macos")]
    let child = Command::new("open").arg(target).spawn()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    let child = match Command::new("xdg-open").arg(target).spawn() {
        Ok(child) => child,
        // GNOME systems without xdg-utils still have gio.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Command::new("gio").arg("open").arg(target).spawn()?,
        Err(e) => return Err(e),
    };
    // Reap it in the background so no zombie is left behind.
    let mut child = child;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
