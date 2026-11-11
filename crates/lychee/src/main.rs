//! Lychee: a modern music player for Linux and Windows.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod images;
mod lastfm;
mod models;
mod palette;
mod screenshot;
mod settings;

slint::include_modules!();

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use app::{Action, App, Dispatcher, Services};
use slint::ComponentHandle;
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,symphonia=warn,lofty=warn,winit=warn,zbus=warn")),
        )
        .init();

    let mut open: Vec<PathBuf> = Vec::new();
    for arg in std::env::args_os().skip(1) {
        let s = arg.to_string_lossy();
        match s.as_ref() {
            "--version" | "-V" => {
                println!("lychee {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("Usage: lychee [FILE|FOLDER]...\n\nOpens Lychee and plays the given files or folders.");
                return Ok(());
            }
            _ => open.push(PathBuf::from(arg)),
        }
    }
    let screenshot = std::env::var_os("LYCHEE_SCREENSHOT").map(PathBuf::from);
    if screenshot.is_some() {
        screenshot::install_platform();
    }

    let paths = settings::Paths::discover();
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

    let ui = AppWindow::new()?;
    let dispatcher = Dispatcher::new();
    app::wire_main(&ui, &dispatcher);
    let services = Services {
        library,
        engine,
        scrobbler,
        lastfm_rx,
        lastfm_tx,
        media,
        media_rx,
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
            d.dispatch(Action::MainWindowClosed);
            let _ = slint::quit_event_loop();
            slint::CloseRequestResponse::HideWindow
        });
    }
    let _drop_timer = install_window_hooks(&ui, &dispatcher);

    if !open.is_empty() {
        dispatcher.dispatch(Action::OpenPaths(open));
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
            #[cfg(windows)]
            attach_media_window(&ui, &d);
        });
    }
    slint::run_event_loop_until_quit()?;
    dispatcher.with_app(|a| a.shutdown());
    Ok(())
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
