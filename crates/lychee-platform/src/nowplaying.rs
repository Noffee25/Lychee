//! macOS Now Playing: media keys, the Now Playing widget in Control Center and the menu bar, and
//! headphone and Bluetooth remote buttons.
//!
//! macOS routes remote commands to the app that last published `MPNowPlayingInfoCenter` info and
//! a playback state. Everything here runs on the main thread: `MediaControls` is called from the
//! UI thread, and Slint's event loop runs the main run loop that delivers the commands.
//!
//! macOS works out the elapsed time itself from the playback rate, so the info is only published
//! when the track, the playback state or the position (after a seek) changes.

use std::cell::RefCell;
use std::path::Path;
use std::ptr::NonNull;

use block2::RcBlock;
use crossbeam_channel::Sender;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGSize;
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemArtwork, MPMediaItemPropertyAlbumArtist,
    MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist, MPMediaItemPropertyArtwork,
    MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle, MPNowPlayingInfoCenter,
    MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState,
    MPRemoteCommand, MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};

use crate::{MediaAction, NowPlaying, Playback, Update};

pub struct Backend {
    state: RefCell<State>,
    /// Tokens for the registered command handlers (the command center keeps the handlers).
    _targets: Vec<Retained<AnyObject>>,
}

#[derive(Default)]
struct State {
    now: Option<NowPlaying>,
    artwork: Option<Retained<MPMediaItemArtwork>>,
    playback: Playback,
    position_ms: u64,
}

impl Backend {
    pub fn start(actions: Sender<MediaAction>) -> Backend {
        let center = unsafe { MPRemoteCommandCenter::sharedCommandCenter() };
        let mut targets = Vec::new();
        let mut on = |command: &MPRemoteCommand, action: MediaAction| {
            let actions = actions.clone();
            let handler = RcBlock::new(move |_event: NonNull<MPRemoteCommandEvent>| {
                let _ = actions.send(action.clone());
                MPRemoteCommandHandlerStatus::Success
            });
            unsafe {
                command.setEnabled(true);
                targets.push(command.addTargetWithHandler(&handler));
            }
        };
        unsafe {
            on(&center.playCommand(), MediaAction::Play);
            on(&center.pauseCommand(), MediaAction::Pause);
            on(&center.togglePlayPauseCommand(), MediaAction::PlayPause);
            on(&center.stopCommand(), MediaAction::Stop);
            on(&center.nextTrackCommand(), MediaAction::Next);
            on(&center.previousTrackCommand(), MediaAction::Previous);
        }

        // Dragging the position slider in the Now Playing widget.
        let seek = unsafe { center.changePlaybackPositionCommand() };
        let handler = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| {
            let event = unsafe { event.as_ref() };
            match event.downcast_ref::<MPChangePlaybackPositionCommandEvent>() {
                Some(event) => {
                    let seconds = unsafe { event.positionTime() };
                    let _ = actions.send(MediaAction::SetPosition((seconds.max(0.0) * 1000.0) as u64));
                    MPRemoteCommandHandlerStatus::Success
                }
                None => MPRemoteCommandHandlerStatus::CommandFailed,
            }
        });
        unsafe {
            seek.setEnabled(true);
            targets.push(seek.addTargetWithHandler(&handler));
        }

        Backend { state: RefCell::new(State::default()), _targets: targets }
    }

    pub fn send(&self, update: Update) {
        let mut state = self.state.borrow_mut();
        match update {
            Update::Metadata(now) => {
                state.artwork = now.as_ref().and_then(|now| now.art_path.as_deref()).and_then(artwork);
                state.now = now;
                state.position_ms = 0;
            }
            Update::Playback(playback) => state.playback = playback,
            Update::Seeked(ms) => state.position_ms = ms,
            // Kept for the next state change; macOS keeps the clock running in between.
            Update::Position(ms) => {
                state.position_ms = ms;
                return;
            }
            // macOS has no per-app volume, shuffle or repeat in its media controls.
            Update::Volume(_) | Update::Shuffle(_) | Update::Repeat(_) => return,
        }
        publish(&state);
    }
}

fn publish(state: &State) {
    let center = unsafe { MPNowPlayingInfoCenter::defaultCenter() };
    let Some(now) = &state.now else {
        unsafe {
            center.setNowPlayingInfo(None);
            center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
        }
        return;
    };
    let playing = state.playback == Playback::Playing;
    let title = NSString::from_str(&now.title);
    let artist = NSString::from_str(&now.artist);
    let album = NSString::from_str(&now.album);
    let album_artist = NSString::from_str(&now.album_artist);
    let duration = NSNumber::new_f64(now.duration_ms as f64 / 1000.0);
    let elapsed = NSNumber::new_f64(state.position_ms as f64 / 1000.0);
    let rate = NSNumber::new_f64(if playing { 1.0 } else { 0.0 });

    let keys: [&NSString; 7] = unsafe {
        [
            MPMediaItemPropertyTitle,
            MPMediaItemPropertyArtist,
            MPMediaItemPropertyAlbumTitle,
            MPMediaItemPropertyAlbumArtist,
            MPMediaItemPropertyPlaybackDuration,
            MPNowPlayingInfoPropertyElapsedPlaybackTime,
            MPNowPlayingInfoPropertyPlaybackRate,
        ]
    };
    let values: [&AnyObject; 7] = [&title, &artist, &album, &album_artist, &duration, &elapsed, &rate];
    let mut keys = Vec::from(keys);
    let mut values = Vec::from(values);
    if let Some(artwork) = &state.artwork {
        keys.push(unsafe { MPMediaItemPropertyArtwork });
        values.push(artwork);
    }
    let info = NSDictionary::from_slices(&keys, &values);
    let playback = match state.playback {
        Playback::Playing => MPNowPlayingPlaybackState::Playing,
        Playback::Paused => MPNowPlayingPlaybackState::Paused,
        Playback::Stopped => MPNowPlayingPlaybackState::Stopped,
    };
    unsafe {
        center.setNowPlayingInfo(Some(&info));
        center.setPlaybackState(playback);
    }
}

/// The cover for the Now Playing widget, from Lychee's artwork cache.
fn artwork(path: &Path) -> Option<Retained<MPMediaItemArtwork>> {
    let path = NSString::from_str(path.to_str()?);
    let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &path)?;
    let size = image.size();
    // macOS asks for the image at whatever size it needs; NSImage scales it when it's drawn.
    let handler = RcBlock::new(move |_size: CGSize| NonNull::from(&*image));
    Some(unsafe { MPMediaItemArtwork::initWithBoundsSize_requestHandler(MPMediaItemArtwork::alloc(), size, &handler) })
}
