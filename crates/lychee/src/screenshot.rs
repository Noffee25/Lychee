//! Headless screenshots for documentation and visual checks, rendered with Slint's software
//! renderer. Set `LYCHEE_SCREENSHOT=out.png` and optionally:
//!
//! * `LYCHEE_SCREENSHOT_SIZE=1440x900`
//! * `LYCHEE_SCREENSHOT_SCENE="play-album:Tidal Memory;page:album;palette:tid"` (see `App::run_scene`)
//! * `LYCHEE_SCREENSHOT_MINI=mini.png` to also render the mini player.
//! * `LYCHEE_SCREENSHOT_HOVER=700,872` to put the pointer somewhere first (hover states).

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::ComponentHandle;
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter};

use crate::AppWindow;
use crate::app::{Action, Dispatcher};

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(Vec::new()) };
}

struct ShotPlatform {
    start: Instant,
}

impl Platform for ShotPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        WINDOWS.with(|w| w.borrow_mut().push(window.clone()));
        Ok(window)
    }

    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}

pub fn install_platform() {
    slint::platform::set_platform(Box::new(ShotPlatform { start: Instant::now() })).expect("a platform was already set");
}

pub fn capture(ui: &AppWindow, dispatcher: &Rc<Dispatcher>, out: &Path) -> anyhow::Result<()> {
    let (w, h) = std::env::var("LYCHEE_SCREENSHOT_SIZE")
        .ok()
        .and_then(|s| s.split_once('x').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))))
        .unwrap_or((1440u32, 900u32));
    ui.window().set_size(slint::PhysicalSize::new(w, h));
    ui.show()?;
    settle(dispatcher, 0);
    if let Ok(scene) = std::env::var("LYCHEE_SCREENSHOT_SCENE") {
        for step in scene.split(';').filter(|s| !s.trim().is_empty()) {
            // "wait:2000" lets background work (reading tags, lookups) finish.
            if let Some(ms) = step.trim().strip_prefix("wait:").and_then(|ms| ms.parse::<u64>().ok()) {
                let until = Instant::now() + Duration::from_millis(ms);
                while Instant::now() < until {
                    settle(dispatcher, 0);
                }
                continue;
            }
            dispatcher.with_app(|a| a.run_scene(step.trim()));
            settle(dispatcher, 0);
        }
    }
    if let Some((x, y)) = std::env::var("LYCHEE_SCREENSHOT_HOVER").ok().and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?))
    }) {
        if let Some(win) = WINDOWS.with(|ws| ws.borrow().first().cloned()) {
            let position = slint::LogicalPosition::new(x, y);
            win.dispatch_event(slint::platform::WindowEvent::PointerMoved { position });
        }
    }
    settle(dispatcher, 0);
    render(0, w, h, out)?;
    if let Ok(mini_out) = std::env::var("LYCHEE_SCREENSHOT_MINI") {
        dispatcher.dispatch(Action::OpenMini);
        settle(dispatcher, 1);
        // Debug aid: LYCHEE_SCREENSHOT_MINI_CLICKS="368,44;300,44" clicks inside the mini player.
        if let Ok(clicks) = std::env::var("LYCHEE_SCREENSHOT_MINI_CLICKS") {
            let win = WINDOWS.with(|ws| ws.borrow().get(1).cloned());
            for point in clicks.split(';') {
                let Some((x, y)) = point.split_once(',') else { continue };
                let (Ok(x), Ok(y)) = (x.trim().parse::<f32>(), y.trim().parse::<f32>()) else { continue };
                if let Some(win) = &win {
                    use slint::platform::{PointerEventButton, WindowEvent};
                    let position = slint::LogicalPosition::new(x, y);
                    win.dispatch_event(WindowEvent::PointerMoved { position });
                    // Hover like a person would, long enough for tooltips to appear.
                    for _ in 0..8 {
                        std::thread::sleep(Duration::from_millis(100));
                        slint::platform::update_timers_and_animations();
                    }
                    win.dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
                    win.dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
                    tracing::info!("clicked mini player at {x},{y}");
                }
                settle(dispatcher, 1);
            }
        }
        render(1, 400, 88, Path::new(&mini_out))?;
    }
    Ok(())
}

/// Let layouts, lazy models and `changed` handlers run a few times.
fn settle(dispatcher: &Rc<Dispatcher>, window: usize) {
    for _ in 0..4 {
        slint::platform::update_timers_and_animations();
        dispatcher.dispatch(Action::Tick);
        if let Some(win) = WINDOWS.with(|ws| ws.borrow().get(window).cloned()) {
            win.request_redraw();
            let size = win.size();
            let mut scratch = vec![PremultipliedRgbaColor::default(); (size.width * size.height).max(1) as usize];
            win.draw_if_needed(|r| {
                r.render(&mut scratch, size.width.max(1) as usize);
            });
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn render(window: usize, w: u32, h: u32, out: &Path) -> anyhow::Result<()> {
    let win = WINDOWS.with(|ws| ws.borrow().get(window).cloned()).ok_or_else(|| anyhow::anyhow!("no window {window}"))?;
    win.set_size(slint::PhysicalSize::new(w, h));
    slint::platform::update_timers_and_animations();
    win.request_redraw();
    let mut buffer = vec![PremultipliedRgbaColor::default(); (w * h) as usize];
    win.draw_if_needed(|r| {
        r.render(&mut buffer, w as usize);
    });
    let mut img = image::RgbaImage::new(w, h);
    for (i, px) in buffer.iter().enumerate() {
        let a = px.alpha.max(1) as f32 / 255.0;
        let un = |c: u8| ((c as f32 / a).round().min(255.0)) as u8;
        img.put_pixel(i as u32 % w, i as u32 / w, image::Rgba([un(px.red), un(px.green), un(px.blue), 255]));
    }
    img.save(out)?;
    println!("wrote {}", out.display());
    Ok(())
}
