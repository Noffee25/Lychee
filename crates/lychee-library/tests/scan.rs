//! End-to-end scan of a temporary music folder.
use std::path::Path;

use lychee_library::{AlbumSort, Library};

fn write_wav(path: &Path, seconds: f32) {
    write_sine(path, seconds, 8000.0);
}

fn write_sine(path: &Path, seconds: f32, amplitude: f32) {
    let rate = 44_100u32;
    let frames = (rate as f32 * seconds) as u32;
    let data_len = frames * 4;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&2u16.to_le_bytes()); // stereo
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        let s = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * amplitude) as i16;
        bytes.extend_from_slice(&s.to_le_bytes());
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn scans_rescans_and_keeps_user_data() {
    let root = tempfile::tempdir().unwrap();
    let music = root.path().join("Music");
    let album = music.join("Test Album");
    std::fs::create_dir_all(&album).unwrap();
    write_wav(&album.join("01 First.wav"), 0.5);
    write_wav(&album.join("02 Second.wav"), 0.5);
    let cover = image::RgbImage::from_fn(64, 64, |x, _| if x < 40 { image::Rgb([210, 60, 40]) } else { image::Rgb([30, 30, 30]) });
    cover.save(album.join("cover.png")).unwrap();

    let lib = Library::open(&root.path().join("data"), &root.path().join("cache")).unwrap();
    lib.add_folder(&music).unwrap();
    let summary = lib.scan(&|_| {}).unwrap();
    assert_eq!(summary.added_or_updated, 2);

    let index = lib.index();
    assert_eq!(index.stats().tracks, 2);
    let albums = index.albums(AlbumSort::Title);
    assert_eq!(albums.len(), 1);
    let a = albums[0];
    assert_eq!(a.title, "Test Album", "untagged files are grouped by folder");
    assert_eq!(a.artist, "Unknown Artist");
    let key = a.artwork.clone().expect("cover.png is picked up");
    assert!(lib.artwork_thumb(&key).exists() && lib.artwork_large(&key).exists());
    assert!(a.accent.is_some(), "a red cover yields an accent colour");
    let first = index.album_tracks(a.id)[0].clone();
    assert_eq!(first.title, "01 First");
    assert!(first.duration_ms >= 450 && first.duration_ms <= 550, "{}", first.duration_ms);
    drop(index);

    lib.set_liked(first.id, true).unwrap();
    // Unchanged files are not read again; removed files disappear; likes survive.
    std::fs::remove_file(album.join("02 Second.wav")).unwrap();
    let summary = lib.scan(&|_| {}).unwrap();
    assert_eq!(summary.added_or_updated, 0);
    assert_eq!(summary.removed, 1);
    let index = lib.index();
    assert_eq!(index.stats().tracks, 1);
    assert!(index.track(first.id).unwrap().liked);

    // Files opened directly are playable but stay out of the library views.
    let outside = root.path().join("loose.wav");
    write_wav(&outside, 0.2);
    let ids = lib.import_paths(&[outside]).unwrap();
    assert_eq!(ids.len(), 1);
    let index = lib.index();
    assert!(index.track(ids[0]).is_some());
    assert_eq!(index.stats().tracks, 1);
}

#[test]
fn edited_and_renamed_files_keep_their_identity() {
    use lofty::config::WriteOptions;
    use lofty::file::TaggedFileExt;
    use lofty::prelude::{Accessor, TagExt};
    use lofty::tag::Tag;

    let root = tempfile::tempdir().unwrap();
    let music = root.path().join("Music");
    let album = music.join("Album");
    std::fs::create_dir_all(&album).unwrap();
    let old = album.join("track.wav");
    write_wav(&old, 0.3);

    let lib = Library::open(&root.path().join("data"), &root.path().join("cache")).unwrap();
    lib.add_folder(&music).unwrap();
    lib.scan(&|_| {}).unwrap();
    let id = lib.index().tracks(lychee_library::TrackSort::Title).first().map(|t| t.id).unwrap();
    lib.set_liked(id, true).unwrap();

    // What the tag editor does: write tags, then rename.
    let tagged = lofty::read_from_path(&old).unwrap();
    let mut tag = Tag::new(tagged.primary_tag_type());
    tag.set_title("New Title".into());
    tag.set_artist("Someone".into());
    tag.save_to_path(&old, WriteOptions::default()).unwrap();
    let new = album.join("01 - New Title.wav");
    std::fs::rename(&old, &new).unwrap();

    assert_eq!(lib.refresh_files(&[(old.clone(), new.clone())]).unwrap(), 1);
    let index = lib.index();
    let t = index.track(id).expect("same id after the rename");
    assert_eq!(t.title, "New Title");
    assert_eq!(t.artist, "Someone");
    assert_eq!(t.path, new);
    assert!(t.liked, "likes survive");
    assert_eq!(index.stats().tracks, 1);
    drop(index);

    // A rescan sees nothing new: the stored file stamps are current.
    assert_eq!(lib.scan(&|_| {}).unwrap().added_or_updated, 0);
    // Files outside the library are ignored.
    assert_eq!(lib.refresh_files(&[(root.path().join("x.wav"), root.path().join("y.wav"))]).unwrap(), 0);
}

#[test]
fn measures_loudness_for_volume_levelling() {
    let root = tempfile::tempdir().unwrap();
    let album = root.path().join("Music").join("Levels");
    std::fs::create_dir_all(&album).unwrap();
    write_sine(&album.join("01 Loud.wav"), 2.0, 16_000.0);
    write_sine(&album.join("02 Quiet.wav"), 2.0, 4_000.0); // 12 dB quieter

    let lib = Library::open(&root.path().join("data"), &root.path().join("cache")).unwrap();
    lib.add_folder(&root.path().join("Music")).unwrap();
    lib.scan(&|_| {}).unwrap();
    let stop = std::sync::atomic::AtomicBool::new(false);
    assert_eq!(lib.measure_loudness(&stop, &|_, _| {}).unwrap(), 2);
    assert_eq!(lib.measure_loudness(&stop, &|_, _| {}).unwrap(), 0, "nothing left to measure");

    let index = lib.index();
    let album = index.albums(AlbumSort::Title)[0];
    let tracks = index.album_tracks(album.id);
    let gain = |i: usize| tracks[i].track_gain_db().unwrap().0;
    // A stereo 440 Hz sine at −6.2 dBFS reads about −6.9 LUFS (K-weighting dips below 1 kHz),
    // 11.1 dB above the −18 LUFS reference.
    assert!((gain(0) + 11.1).abs() < 0.2, "{}", gain(0));
    assert!((gain(1) - gain(0) - 12.04).abs() < 0.05, "{} vs {}", gain(0), gain(1));
    assert!((tracks[0].track_peak().unwrap() - 16_000.0 / 32_768.0).abs() < 1e-3);
    let album_gain = album.gain.expect("every song is measured");
    assert!(album_gain < gain(0) + 3.0 && album_gain > gain(0), "{album_gain}");
}
