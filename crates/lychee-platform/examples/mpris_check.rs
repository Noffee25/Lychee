//! Registers the media controls briefly so tools like `playerctl` can see them.
fn main() {
    let (tx, rx) = crossbeam_channel::unbounded();
    let Some(controls) = lychee_platform::MediaControls::new(tx) else {
        eprintln!("media controls unavailable");
        return;
    };
    controls.set_metadata(Some(lychee_platform::NowPlaying {
        track_id: 3,
        title: "Saltwater Hymn".into(),
        artist: "Hollow Coast".into(),
        album: "Tidal Memory".into(),
        album_artist: "Hollow Coast".into(),
        duration_ms: 252_000,
        art_path: None,
    }));
    controls.set_playback(lychee_platform::Playback::Paused);
    controls.set_position(134_000);
    println!("registered");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if let Ok(action) = rx.recv_timeout(std::time::Duration::from_millis(100)) {
            println!("action from the OS: {action:?}");
        }
    }
}
