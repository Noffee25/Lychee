//! The mini player window.
//!
//! On X11 and Windows it is a second Slint window in this process. Wayland gives applications
//! no way to keep a window above the others, so on a Wayland session the mini player runs as a
//! small helper process (`lychee --mini-player`) on XWayland, where the window manager honours
//! "always on top". The two talk over the helper's stdin and stdout: the player state goes down
//! as JSON lines and button presses come back as single words.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use slint::{Color, ComponentHandle, Image};

use crate::{Actions, MiniWindow, NowPlaying, Player, Theme};

/// Everything the mini player shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MiniState {
    pub has_track: bool,
    pub title: String,
    pub artist: String,
    pub format: String,
    pub position: String,
    pub duration: String,
    pub progress: f32,
    pub playing: bool,
    pub cover: Option<PathBuf>,
    /// ARGB colour of the placeholder cover.
    pub seed: u32,
    /// Theme id (see `themes.rs`).
    pub theme: String,
    pub dark: bool,
    /// RGB accent colour.
    pub accent: u32,
}

/// A button pressed in the helper process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MiniCommand {
    TogglePlay,
    Next,
    Previous,
    /// Back to the full window: closes the mini player.
    Close,
}

impl MiniCommand {
    const ALL: [MiniCommand; 4] = [MiniCommand::TogglePlay, MiniCommand::Next, MiniCommand::Previous, MiniCommand::Close];

    fn word(self) -> &'static str {
        match self {
            MiniCommand::TogglePlay => "toggle-play",
            MiniCommand::Next => "next",
            MiniCommand::Previous => "previous",
            MiniCommand::Close => "close",
        }
    }

    fn parse(word: &str) -> Option<MiniCommand> {
        MiniCommand::ALL.into_iter().find(|c| c.word() == word)
    }
}

#[derive(Debug, PartialEq)]
pub enum MiniEvent {
    /// The helper's window is on screen.
    Ready,
    Command(MiniCommand),
    /// The helper quit (or could not start).
    Exited,
}

/// Start moving a frameless window with the mouse.
///
/// While the window manager moves the window it owns the pointer, and on X11 and Windows the
/// button release that ends the move never reaches Slint. The drag area would then keep its
/// pointer grab and swallow every later click, so reset Slint's pointer state right away.
pub fn start_drag(mini: &MiniWindow) {
    use slint::winit_030::WinitWindowAccessor;
    mini.window().with_winit_window(|w| {
        if let Err(e) = w.drag_window() {
            tracing::debug!("drag_window: {e}");
        }
    });
    let weak = mini.as_weak();
    slint::Timer::single_shot(Duration::ZERO, move || {
        if let Some(mini) = weak.upgrade() {
            mini.window().dispatch_event(slint::platform::WindowEvent::PointerExited);
        }
    });
}

/// Ask the window manager to keep the window above others.
///
/// winit sends the X11 "above" request right after mapping the window, before the window
/// manager has started managing it, and mutter drops it. Repeat it once the window is up (the
/// request is idempotent, so a few tries cost nothing).
pub fn keep_on_top(mini: &MiniWindow) {
    for delay in [120, 600, 1500] {
        let weak = mini.as_weak();
        slint::Timer::single_shot(Duration::from_millis(delay), move || {
            let Some(mini) = weak.upgrade() else { return };
            use slint::winit_030::{WinitWindowAccessor, winit::window::WindowLevel};
            mini.window().with_winit_window(|w| w.set_window_level(WindowLevel::AlwaysOnTop));
        });
    }
}

/// True if the window is a native Wayland surface.
pub fn is_wayland(window: &slint::Window) -> bool {
    use slint::winit_030::WinitWindowAccessor;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    window
        .with_winit_window(|w| w.window_handle().is_ok_and(|h| matches!(h.as_raw(), RawWindowHandle::Wayland(_))))
        .unwrap_or(false)
}

// -------------------------------------------------------------------------------------------
// Parent side

/// A running helper process.
pub struct External {
    child: Option<Child>,
    lines: Sender<String>,
    events: Receiver<MiniEvent>,
    last: Option<MiniState>,
    pub ready: bool,
}

impl External {
    pub fn spawn() -> std::io::Result<External> {
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--mini-player")
            // Without a Wayland display winit uses X11, which XWayland provides.
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("WAYLAND_SOCKET")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut stdin = child.stdin.take().ok_or_else(|| std::io::Error::other("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("no stdout"))?;

        // Writes happen on their own thread so a stuck helper can never block the UI.
        let (lines, lines_rx) = crossbeam_channel::unbounded::<String>();
        std::thread::Builder::new().name("lychee-mini-out".into()).spawn(move || {
            while let Ok(line) = lines_rx.recv() {
                if writeln!(stdin, "{line}").and_then(|()| stdin.flush()).is_err() {
                    break;
                }
            }
        })?;
        let (events_tx, events) = crossbeam_channel::unbounded();
        std::thread::Builder::new().name("lychee-mini-in".into()).spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let event = match line.trim() {
                    "ready" => MiniEvent::Ready,
                    word => match MiniCommand::parse(word) {
                        Some(c) => MiniEvent::Command(c),
                        None => continue,
                    },
                };
                if events_tx.send(event).is_err() {
                    return;
                }
            }
            let _ = events_tx.send(MiniEvent::Exited);
        })?;
        Ok(External { child: Some(child), lines, events, last: None, ready: false })
    }

    /// Send the state if it changed since last time.
    pub fn send(&mut self, state: &MiniState) {
        if self.last.as_ref() == Some(state) {
            return;
        }
        if let Ok(json) = serde_json::to_string(state) {
            let _ = self.lines.send(json);
        }
        self.last = Some(state.clone());
    }

    pub fn poll(&self) -> Vec<MiniEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for External {
    fn drop(&mut self) {
        // Closing the helper's stdin tells it to quit; make sure it does.
        let (closed, _) = crossbeam_channel::bounded(0);
        drop(std::mem::replace(&mut self.lines, closed));
        if let Some(mut child) = self.child.take() {
            let _ = std::thread::Builder::new().name("lychee-mini-reap".into()).spawn(move || {
                for _ in 0..40 {
                    if matches!(child.try_wait(), Ok(Some(_))) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                let _ = child.kill();
                let _ = child.wait();
            });
        }
    }
}

// -------------------------------------------------------------------------------------------
// Helper process

thread_local! {
    static COVER: RefCell<Option<(PathBuf, Option<Image>)>> = const { RefCell::new(None) };
}

/// Entry point of `lychee --mini-player`.
pub fn run_helper() -> anyhow::Result<()> {
    select_x11_backend()?;
    let mini = MiniWindow::new()?;
    let actions = mini.global::<Actions>();
    actions.on_toggle_play(|| say(MiniCommand::TogglePlay.word()));
    actions.on_next(|| say(MiniCommand::Next.word()));
    actions.on_previous(|| say(MiniCommand::Previous.word()));
    actions.on_raise_main(|| say(MiniCommand::Close.word()));
    actions.on_close_mini(|| say(MiniCommand::Close.word()));
    {
        let weak = mini.as_weak();
        actions.on_start_window_drag(move || {
            if let Some(mini) = weak.upgrade() {
                start_drag(&mini);
            }
        });
    }
    mini.window().on_close_requested(|| {
        say(MiniCommand::Close.word());
        slint::CloseRequestResponse::KeepWindowShown
    });

    let weak = mini.as_weak();
    std::thread::Builder::new().name("lychee-mini-state".into()).spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            let Ok(state) = serde_json::from_str::<MiniState>(&line) else { continue };
            let _ = weak.upgrade_in_event_loop(move |mini| apply(&mini, &state));
        }
        // The player went away (or closed us): quit.
        let _ = slint::invoke_from_event_loop(|| {
            let _ = slint::quit_event_loop();
        });
    })?;

    mini.show()?;
    keep_on_top(&mini);
    say("ready");
    slint::run_event_loop_until_quit()?;
    Ok(())
}

fn say(word: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{word}");
    let _ = out.flush();
}

fn select_x11_backend() -> anyhow::Result<()> {
    use slint::winit_030::winit;
    #[allow(unused_mut)]
    let mut builder = winit::event_loop::EventLoop::<slint::winit_030::SlintEvent>::with_user_event();
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "ios", target_os = "android"))))]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    let selector = slint::BackendSelector::new().backend_name("winit".into()).with_winit_event_loop_builder(builder);
    // Same window class as the main window, so docks group the two.
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "ios", target_os = "android"))))]
    let selector = selector.with_winit_window_attributes_hook(|attributes| {
        use winit::platform::x11::WindowAttributesExtX11;
        attributes.with_name("lychee", "lychee")
    });
    selector.select()?;
    Ok(())
}

fn apply(mini: &MiniWindow, s: &MiniState) {
    let theme = mini.global::<Theme>();
    theme.set_colors(crate::themes::get(&s.theme, s.dark).slint_colors());
    theme.set_accent(Color::from_argb_encoded(0xFF00_0000 | s.accent));
    theme.set_on_accent(crate::themes::on_accent(s.accent));
    let cover = s.cover.as_ref().and_then(|path| {
        COVER.with(|c| {
            let mut c = c.borrow_mut();
            if c.as_ref().is_none_or(|(p, _)| p != path) {
                *c = Some((path.clone(), Image::load_from_path(path).ok()));
            }
            c.as_ref().and_then(|(_, img)| img.clone())
        })
    });
    let player = mini.global::<Player>();
    let now = NowPlaying {
        has_track: s.has_track,
        title: s.title.as_str().into(),
        artist: s.artist.as_str().into(),
        format: s.format.as_str().into(),
        has_cover: cover.is_some(),
        cover: cover.unwrap_or_default(),
        seed: Color::from_argb_encoded(s.seed),
        ..NowPlaying::default()
    };
    if player.get_now() != now {
        player.set_now(now);
    }
    player.set_playing(s.playing);
    player.set_progress(s.progress);
    player.set_position_text(s.position.as_str().into());
    player.set_duration_text(s.duration.as_str().into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_round_trip() {
        for c in MiniCommand::ALL {
            assert_eq!(MiniCommand::parse(c.word()), Some(c));
        }
        assert_eq!(MiniCommand::parse("ready"), None);
    }

    #[test]
    fn state_is_json_lines() {
        let s = MiniState { title: "Line\nbreak".into(), progress: 0.5, ..MiniState::default() };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains('\n'));
        assert_eq!(serde_json::from_str::<MiniState>(&json).unwrap(), s);
    }
}
