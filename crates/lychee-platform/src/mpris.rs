//! MPRIS 2 over D-Bus, via `mpris-server`, on a dedicated thread.

use std::thread;

use crossbeam_channel::Sender;
use futures_lite::future;
use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time, TrackId};

use crate::{MediaAction, NowPlaying, Playback, Repeat, Update};

pub struct Backend {
    tx: async_channel::Sender<Update>,
}

impl Backend {
    pub fn start(actions: Sender<MediaAction>) -> Option<Backend> {
        let (tx, rx) = async_channel::unbounded::<Update>();
        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<bool>(1);
        thread::Builder::new()
            .name("lychee-mpris".into())
            .spawn(move || {
                future::block_on(async move {
                    let player = match Player::builder("Lychee")
                        .identity("Lychee")
                        .desktop_entry("lychee")
                        .can_play(true)
                        .can_pause(true)
                        .can_go_next(true)
                        .can_go_previous(true)
                        .can_seek(true)
                        .can_control(true)
                        .can_raise(true)
                        .can_quit(true)
                        .build()
                        .await
                    {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::warn!("MPRIS unavailable: {e}");
                            let _ = ready_tx.send(false);
                            return;
                        }
                    };
                    let _ = ready_tx.send(true);
                    connect(&player, &actions);
                    let updates = async {
                        while let Ok(update) = rx.recv().await {
                            if let Err(e) = apply(&player, update).await {
                                tracing::debug!("MPRIS update failed: {e}");
                            }
                        }
                    };
                    future::or(player.run(), updates).await;
                });
            })
            .ok()?;
        match ready_rx.recv() {
            Ok(true) => Some(Backend { tx }),
            _ => None,
        }
    }

    pub fn send(&self, update: Update) {
        let _ = self.tx.send_blocking(update);
    }
}

fn connect(player: &Player, actions: &Sender<MediaAction>) {
    let a = actions.clone();
    player.connect_play(move |_| drop(a.send(MediaAction::Play)));
    let a = actions.clone();
    player.connect_pause(move |_| drop(a.send(MediaAction::Pause)));
    let a = actions.clone();
    player.connect_play_pause(move |_| drop(a.send(MediaAction::PlayPause)));
    let a = actions.clone();
    player.connect_stop(move |_| drop(a.send(MediaAction::Stop)));
    let a = actions.clone();
    player.connect_next(move |_| drop(a.send(MediaAction::Next)));
    let a = actions.clone();
    player.connect_previous(move |_| drop(a.send(MediaAction::Previous)));
    let a = actions.clone();
    player.connect_seek(move |_, offset| drop(a.send(MediaAction::SeekBy(offset.as_millis()))));
    let a = actions.clone();
    player.connect_set_position(move |_, _, pos| drop(a.send(MediaAction::SetPosition(pos.as_millis().max(0) as u64))));
    let a = actions.clone();
    player.connect_set_volume(move |_, v| drop(a.send(MediaAction::SetVolume(v))));
    let a = actions.clone();
    player.connect_set_shuffle(move |_, s| drop(a.send(MediaAction::SetShuffle(s))));
    let a = actions.clone();
    player.connect_set_loop_status(move |_, status| {
        let repeat = match status {
            LoopStatus::None => Repeat::Off,
            LoopStatus::Track => Repeat::One,
            LoopStatus::Playlist => Repeat::All,
        };
        drop(a.send(MediaAction::SetRepeat(repeat)));
    });
    let a = actions.clone();
    player.connect_raise(move |_| drop(a.send(MediaAction::Raise)));
    let a = actions.clone();
    player.connect_quit(move |_| drop(a.send(MediaAction::Quit)));
}

async fn apply(player: &Player, update: Update) -> mpris_server::zbus::Result<()> {
    match update {
        Update::Metadata(np) => player.set_metadata(metadata(np)).await?,
        Update::Playback(p) => {
            let status = match p {
                Playback::Playing => PlaybackStatus::Playing,
                Playback::Paused => PlaybackStatus::Paused,
                Playback::Stopped => PlaybackStatus::Stopped,
            };
            player.set_playback_status(status).await?;
        }
        Update::Position(ms) => player.set_position(Time::from_millis(ms as i64)),
        Update::Seeked(ms) => {
            player.set_position(Time::from_millis(ms as i64));
            player.seeked(Time::from_millis(ms as i64)).await?;
        }
        Update::Volume(v) => player.set_volume(v).await?,
        Update::Shuffle(s) => player.set_shuffle(s).await?,
        Update::Repeat(r) => {
            let status = match r {
                Repeat::Off => LoopStatus::None,
                Repeat::All => LoopStatus::Playlist,
                Repeat::One => LoopStatus::Track,
            };
            player.set_loop_status(status).await?;
        }
    }
    Ok(())
}

fn metadata(np: Option<NowPlaying>) -> Metadata {
    let Some(np) = np else { return Metadata::new() };
    let mut builder = Metadata::builder()
        .title(np.title)
        .artist([np.artist])
        .album(np.album)
        .album_artist([np.album_artist])
        .length(Time::from_millis(np.duration_ms as i64));
    if let Ok(id) = TrackId::try_from(format!("/org/lychee/track/{}", np.track_id.max(0))) {
        builder = builder.trackid(id);
    }
    if let Some(path) = np.art_path {
        builder = builder.art_url(format!("file://{}", path.display()));
    }
    builder.build()
}
