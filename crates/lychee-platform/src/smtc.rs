//! Windows System Media Transport Controls (media keys, volume flyout, lock screen).
//!
//! The controls belong to a window, so nothing appears until `Update::Window(hwnd)` arrives.
//! Everything runs on one dedicated thread.

use std::thread;

use crossbeam_channel::{Receiver, Sender};
use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
use windows::Media::{
    MediaPlaybackAutoRepeatMode, MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::RandomAccessStreamReference;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::core::{HSTRING, Ref, factory};

use crate::{MediaAction, NowPlaying, Playback, Repeat, Update};

pub struct Backend {
    tx: Sender<Update>,
}

impl Backend {
    pub fn start(actions: Sender<MediaAction>) -> Backend {
        let (tx, rx) = crossbeam_channel::unbounded();
        let _ = thread::Builder::new().name("lychee-smtc".into()).spawn(move || run(rx, actions));
        Backend { tx }
    }

    pub fn send(&self, update: Update) {
        let _ = self.tx.send(update);
    }
}

#[derive(Default)]
struct Pending {
    metadata: Option<Option<NowPlaying>>,
    playback: Option<Playback>,
}

fn run(rx: Receiver<Update>, actions: Sender<MediaAction>) {
    let mut smtc: Option<SystemMediaTransportControls> = None;
    let mut pending = Pending::default();
    let mut duration_ms = 0u64;
    while let Ok(update) = rx.recv() {
        let result = match (&smtc, update) {
            (_, Update::Window(hwnd)) => match attach(hwnd, &actions) {
                Ok(controls) => {
                    if let Some(meta) = pending.metadata.take() {
                        duration_ms = meta.as_ref().map(|m| m.duration_ms).unwrap_or(0);
                        let _ = set_metadata(&controls, meta);
                    }
                    if let Some(p) = pending.playback.take() {
                        let _ = set_playback(&controls, p);
                    }
                    smtc = Some(controls);
                    Ok(())
                }
                Err(e) => Err(e),
            },
            (None, Update::Metadata(m)) => {
                pending.metadata = Some(m);
                Ok(())
            }
            (None, Update::Playback(p)) => {
                pending.playback = Some(p);
                Ok(())
            }
            (None, _) => Ok(()),
            (Some(c), Update::Metadata(m)) => {
                duration_ms = m.as_ref().map(|m| m.duration_ms).unwrap_or(0);
                set_metadata(c, m)
            }
            (Some(c), Update::Playback(p)) => set_playback(c, p),
            (Some(c), Update::Position(ms) | Update::Seeked(ms)) => set_timeline(c, ms, duration_ms),
            (Some(c), Update::Shuffle(s)) => c.SetShuffleEnabled(s),
            (Some(c), Update::Repeat(r)) => c.SetAutoRepeatMode(match r {
                Repeat::Off => MediaPlaybackAutoRepeatMode::None,
                Repeat::All => MediaPlaybackAutoRepeatMode::List,
                Repeat::One => MediaPlaybackAutoRepeatMode::Track,
            }),
            (Some(_), Update::Volume(_)) => Ok(()),
        };
        if let Err(e) = result {
            tracing::debug!("SMTC update failed: {e}");
        }
    }
}

fn attach(hwnd: isize, actions: &Sender<MediaAction>) -> windows::core::Result<SystemMediaTransportControls> {
    let interop: ISystemMediaTransportControlsInterop =
        factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
    let controls: SystemMediaTransportControls = unsafe { interop.GetForWindow(HWND(hwnd as *mut core::ffi::c_void))? };
    controls.SetIsEnabled(true)?;
    controls.SetIsPlayEnabled(true)?;
    controls.SetIsPauseEnabled(true)?;
    controls.SetIsStopEnabled(true)?;
    controls.SetIsNextEnabled(true)?;
    controls.SetIsPreviousEnabled(true)?;

    let tx = actions.clone();
    controls.ButtonPressed(&TypedEventHandler::new(
        move |_: Ref<SystemMediaTransportControls>, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
            if let Some(args) = args.as_ref() {
                let action = match args.Button()? {
                    SystemMediaTransportControlsButton::Play => Some(MediaAction::Play),
                    SystemMediaTransportControlsButton::Pause => Some(MediaAction::Pause),
                    SystemMediaTransportControlsButton::Stop => Some(MediaAction::Stop),
                    SystemMediaTransportControlsButton::Next => Some(MediaAction::Next),
                    SystemMediaTransportControlsButton::Previous => Some(MediaAction::Previous),
                    _ => None,
                };
                if let Some(action) = action {
                    let _ = tx.send(action);
                }
            }
            Ok(())
        },
    ))?;
    let tx = actions.clone();
    controls.PlaybackPositionChangeRequested(&TypedEventHandler::new(
        move |_: Ref<SystemMediaTransportControls>, args: Ref<PlaybackPositionChangeRequestedEventArgs>| {
            if let Some(args) = args.as_ref() {
                let span = args.RequestedPlaybackPosition()?;
                let _ = tx.send(MediaAction::SetPosition((span.Duration.max(0) / 10_000) as u64));
            }
            Ok(())
        },
    ))?;
    Ok(controls)
}

fn set_metadata(c: &SystemMediaTransportControls, np: Option<NowPlaying>) -> windows::core::Result<()> {
    let updater = c.DisplayUpdater()?;
    let Some(np) = np else {
        updater.ClearAll()?;
        return updater.Update();
    };
    updater.SetType(MediaPlaybackType::Music)?;
    let music = updater.MusicProperties()?;
    music.SetTitle(&HSTRING::from(np.title.as_str()))?;
    music.SetArtist(&HSTRING::from(np.artist.as_str()))?;
    music.SetAlbumTitle(&HSTRING::from(np.album.as_str()))?;
    music.SetAlbumArtist(&HSTRING::from(np.album_artist.as_str()))?;
    if let Some(path) = np.art_path {
        let url = format!("file:///{}", path.display().to_string().replace('\\', "/").replace(' ', "%20"));
        if let Ok(uri) = Uri::CreateUri(&HSTRING::from(url)) {
            if let Ok(stream) = RandomAccessStreamReference::CreateFromUri(&uri) {
                updater.SetThumbnail(&stream)?;
            }
        }
    }
    updater.Update()
}

fn set_playback(c: &SystemMediaTransportControls, p: Playback) -> windows::core::Result<()> {
    c.SetPlaybackStatus(match p {
        Playback::Playing => MediaPlaybackStatus::Playing,
        Playback::Paused => MediaPlaybackStatus::Paused,
        Playback::Stopped => MediaPlaybackStatus::Stopped,
    })
}

fn set_timeline(c: &SystemMediaTransportControls, position_ms: u64, duration_ms: u64) -> windows::core::Result<()> {
    let span = |ms: u64| TimeSpan { Duration: ms as i64 * 10_000 };
    let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
    timeline.SetStartTime(span(0))?;
    timeline.SetMinSeekTime(span(0))?;
    timeline.SetEndTime(span(duration_ms))?;
    timeline.SetMaxSeekTime(span(duration_ms))?;
    timeline.SetPosition(span(position_ms.min(duration_ms.max(position_ms))))?;
    c.UpdateTimelineProperties(&timeline)
}
