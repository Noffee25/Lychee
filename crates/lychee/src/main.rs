//! Lychee: a modern music player for Linux, Windows and macOS.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod editor;
mod eq;
mod images;
mod lastfm;
mod mini;
mod models;
mod palette;
mod screenshot;
mod settings;
mod themes;

slint::include_modules!();

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use app::{Action, App, Dispatcher, Services};
use slint::ComponentHandle;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::prelude::*;

fn main() -> anyhow::Result<()> {
    // RUST_LOG takes `level` and `target=level` directives, e.g. `info,lychee=debug`.
    let filter: Targets = std::env::var("RUST_LOG")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "info,symphonia=warn,lofty=warn,winit=warn,zbus=warn".parse().expect("valid default filter"));
    tracing_subscriber::registry().with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr)).with(filter).init();
    // The always-on-top mini player helper (see `mini.rs`); its stdout is a pipe to the player.
    if std::env::args_os().nth(1).is_some_and(|a| a == "--mini-player") {
        return mini::run_helper();
    }

    let mut open: Vec<PathBuf> = Vec::new();
    let mut edit: Vec<PathBuf> = Vec::new();
    let mut editing = false;
    for arg in std::env::args_os().skip(1) {
        let s = arg.to_string_lossy();
        match s.as_ref() {
            "--version" | "-V" => {
                println!("lychee {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!(
                    "Usage: lychee [FILE|FOLDER]...\n       lychee --edit FILE|FOLDER...\n\n\
                     Opens Lychee and plays the given files or folders, or with --edit opens them in the tag editor."
                );
                return Ok(());
            }
            "--edit" => editing = true,
            _ if editing => edit.push(arg_path(arg)),
            _ => open.push(arg_path(arg)),
        }
    }
    let screenshot = std::env::var_os("LYCHEE_SCREENSHOT").map(PathBuf::from);
    if screenshot.is_some() {
        screenshot::install_platform();
    }

    let paths = settings::Paths::discover();
    // One Lychee per library: if it's already running, hand it the files and stop here.
    // (Screenshot runs are tools, not players.)
    let instance = if screenshot.is_some() {
        None
    } else {
        let request = lychee_platform::instance::Request { open: open.clone(), edit: edit.clone() };
        match lychee_platform::instance::claim(&paths.data_dir, &request) {
            Ok(lychee_platform::instance::Claim::Forwarded) => {
                tracing::info!("Lychee is already running; handed the request to it");
                return Ok(());
            }
            Ok(lychee_platform::instance::Claim::Primary(primary)) => Some(primary),
            Err(e) => {
                tracing::warn!("couldn't reach the running Lychee ({e}); starting another one");
                None
            }
        }
    };
    let settings = settings::Settings::load(&paths.config_file);
    let library = Arc::new(lychee_library::Library::open(&paths.data_dir, &paths.cache_dir)?);
    let silent = screenshot.is_some() || settings.muted;
    let engine = lychee_audio::Engine::new(settings.output_device.clone(), if silent { 0.0 } else { settings.volume });
    let (lastfm_tx, lastfm_rx) = crossbeam_channel::unbounded();
    let status_tx = lastfm_tx.clone();
    let scrobbler = lychee_lastfm::Scrobbler::start(
        paths.data_dir.join("scrobbles.json"),
        settings.lastfm_credentials(),
        settings.lastfm_session(),
        move |status| {
            let _ = status_tx.send(lastfm::LastfmMsg::Status(status));
        },
    );
    let (media_tx, media_rx) = crossbeam_channel::unbounded();
    let media = if screenshot.is_some() { None } else { lychee_platform::MediaControls::new(media_tx) };

    #[cfg(target_os = "macos")]
    let on_exit = {
        let on_exit = OnExit::default();
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .with_winit_custom_application_handler(on_exit.clone())
            .select()?;
        on_exit
    };
    let ui = AppWindow::new()?;
    // Wayland app id / X11 class: lets docks match the window to lychee.desktop (icon, name,
    // pinning). It has to come after the first window is created (before that Slint has no
    // platform, so the call fails with NoPlatform) and before the window is shown. Without it,
    // GNOME shows a generic icon named "Unknown".
    if let Err(err) = slint::set_xdg_app_id("lychee") {
        tracing::warn!("couldn't set the app id ({err}); docks won't show Lychee's icon");
    }
    let dispatcher = Dispatcher::new();
    app::wire_main(&ui, &dispatcher);
    editor::wire(&ui, &dispatcher);
    let services = Services {
        library,
        engine,
        scrobbler,
        lastfm_rx,
        lastfm_tx,
        media,
        media_rx,
        instance,
        settings,
        paths,
        synchronous_images: screenshot.is_some(),
        system_dark: true,
    };
    let app = App::new(&ui, services, &dispatcher);
    dispatcher.set_app(app);

    if let Some(out) = screenshot {
        return screenshot::capture(&ui, &dispatcher, &out);
    }

    let ticker = slint::Timer::default();
    {
        let d = dispatcher.clone();
        ticker.start(slint::TimerMode::Repeated, Duration::from_millis(50), move || d.dispatch(Action::Tick));
    }
    {
        let d = dispatcher.clone();
        ui.window().on_close_requested(move || {
            // Unsaved tag edits: ask first (the app shows a prompt).
            if d.with_app(|a| a.request_close()) == Some(false) {
                return slint::CloseRequestResponse::KeepWindowShown;
            }
            d.dispatch(Action::MainWindowClosed);
            let _ = slint::quit_event_loop();
            slint::CloseRequestResponse::HideWindow
        });
    }
    let _drop_timer = install_window_hooks(&ui, &dispatcher);

    if !open.is_empty() {
        dispatcher.dispatch(Action::OpenPaths(open));
    }
    if !edit.is_empty() {
        dispatcher.dispatch(Action::EditPaths(edit));
    }

    ui.show()?;
    // The native window exists now: read the system theme and hand it to the OS media controls.
    {
        let weak = ui.as_weak();
        let d = dispatcher.clone();
        slint::Timer::single_shot(Duration::from_millis(30), move || {
            let Some(ui) = weak.upgrade() else { return };
            use slint::winit_030::{WinitWindowAccessor, winit};
            if let Some(theme) = ui.window().with_winit_window(|w| w.theme()).flatten() {
                d.dispatch(Action::SystemThemeChanged(theme == winit::window::Theme::Dark));
            }
            ui.global::<AppState>().set_wayland(mini::is_wayland(ui.window()));
            #[cfg(windows)]
            attach_media_window(&ui, &d);
        });
    }
    #[cfg(target_os = "macos")]
    {
        let d = dispatcher.clone();
        on_exit.set(move || {
            d.with_app(|a| a.shutdown());
        });
    }
    slint::run_event_loop_until_quit()?;
    #[cfg(target_os = "macos")]
    on_exit.run();
    #[cfg(not(target_os = "macos"))]
    dispatcher.with_app(|a| a.shutdown());
    Ok(())
}

/// A command-line argument as a path: absolute (a running Lychee may get it, from another working
/// directory), and `file://` URIs (what some launchers pass) decoded.
fn arg_path(arg: std::ffi::OsString) -> PathBuf {
    let path = match arg.to_str().and_then(|s| s.strip_prefix("file://")) {
        Some(rest) => file_uri_path(rest),
        None => PathBuf::from(arg),
    };
    std::path::absolute(&path).unwrap_or(path)
}

/// The path in a `file://` URI (after the scheme): an optional host (only the local one), then
/// a percent-encoded path.
fn file_uri_path(rest: &str) -> PathBuf {
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    let mut bytes = Vec::with_capacity(path.len());
    let raw = path.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let (Some(h), Some(l)) = (hex(raw[i + 1]), hex(raw[i + 2])) {
                bytes.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        bytes.push(raw[i]);
        i += 1;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    }
    #[cfg(not(unix))]
    {
        // "file:///C:/Music/a.flac": no leading slash before the drive letter.
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let text = match text.strip_prefix('/') {
            Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
            _ => text,
        };
        PathBuf::from(text)
    }
}

/// The shutdown (saving the session and settings) for when the event loop ends without returning.
///
/// On macOS, Quit in the app menu (⌘Q) or the Dock, and logging out, end the process from inside
/// the event loop, so `run_event_loop_until_quit` never returns. winit still reports `exiting`
/// first, so the shutdown runs from there. It runs once: after a normal quit, `run` finds it done.
#[cfg(target_os = "macos")]
#[derive(Clone, Default)]
struct OnExit(Rc<RefCell<Option<Box<dyn FnOnce()>>>>);

#[cfg(target_os = "macos")]
impl OnExit {
    fn set(&self, shutdown: impl FnOnce() + 'static) {
        *self.0.borrow_mut() = Some(Box::new(shutdown));
    }

    fn run(&self) {
        let shutdown = self.0.borrow_mut().take();
        if let Some(shutdown) = shutdown {
            shutdown();
        }
    }
}

#[cfg(target_os = "macos")]
impl slint::winit_030::CustomApplicationHandler for OnExit {
    fn exiting(
        &mut self,
        _event_loop: &slint::winit_030::winit::event_loop::ActiveEventLoop,
    ) -> slint::winit_030::EventResult {
        self.run();
        slint::winit_030::EventResult::Propagate
    }
}

/// Theme changes and files dropped onto the window. Returns the timer that batches drops.
fn install_window_hooks(ui: &AppWindow, d: &Rc<Dispatcher>) -> slint::Timer {
    use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
    let dropped: Rc<RefCell<(Vec<PathBuf>, Option<Instant>)>> = Rc::default();
    {
        let d = d.clone();
        let dropped = dropped.clone();
        ui.window().on_winit_window_event(move |_, event| {
            match event {
                winit::event::WindowEvent::ThemeChanged(theme) => {
                    d.dispatch(Action::SystemThemeChanged(*theme == winit::window::Theme::Dark));
                }
                winit::event::WindowEvent::DroppedFile(path) => {
                    let mut drop = dropped.borrow_mut();
                    drop.0.push(path.clone());
                    drop.1 = Some(Instant::now());
                }
                _ => {}
            }
            EventResult::Propagate
        });
    }
    let timer = slint::Timer::default();
    let d = d.clone();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(150), move || {
        let batch = {
            let mut drop = dropped.borrow_mut();
            match drop.1 {
                Some(at) if at.elapsed() > Duration::from_millis(120) => {
                    drop.1 = None;
                    std::mem::take(&mut drop.0)
                }
                _ => Vec::new(),
            }
        };
        if !batch.is_empty() {
            d.dispatch(Action::OpenPaths(batch));
        }
    });
    timer
}

#[cfg(windows)]
fn attach_media_window(ui: &AppWindow, d: &Rc<Dispatcher>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = ui.window().window_handle();
    if let Ok(h) = handle.window_handle() {
        if let RawWindowHandle::Win32(win) = h.as_raw() {
            let hwnd = win.hwnd.get();
            d.with_app(|a| a.attach_media_window(hwnd));
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(arg_path("file:///home/me/Music/01%20Caf%C3%A9.flac".into()), PathBuf::from("/home/me/Music/01 Café.flac"));
        assert_eq!(arg_path("file://localhost/music/a%2Fb".into()), PathBuf::from("/music/a/b"));
        assert_eq!(arg_path("file:///music/100%".into()), PathBuf::from("/music/100%"));
        assert_eq!(arg_path("/music/plain.mp3".into()), PathBuf::from("/music/plain.mp3"));
        let relative = arg_path("song.flac".into());
        assert!(relative.is_absolute() && relative.ends_with("song.flac"));
    }
}
