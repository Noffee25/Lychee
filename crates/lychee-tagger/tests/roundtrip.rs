//! Write tags into real files of every supported container and read them back.

use std::fs::File;
use std::path::{Path, PathBuf};

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::id3::v2::{Frame, Id3v2Tag, PrivateFrame};
use lofty::mpeg::MpegFile;
use lychee_tagger::session::{EditFile, Session};
use lychee_tagger::{Field, Mask, Picture, PictureKind, file};

const FORMATS: [&str; 7] = ["flac", "mp3", "ogg", "opus", "m4a", "wav", "aiff"];

fn fixture(dir: &Path, ext: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(format!("silence.{ext}"));
    let dst = dir.join(format!("track.{ext}"));
    std::fs::copy(&src, &dst).unwrap();
    dst
}

fn jpeg(fill: u8) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0];
    v.extend(std::iter::repeat_n(fill, 300));
    v.extend([0xFF, 0xD9]);
    v
}

const VALUES: [(Field, &str); 14] = [
    (Field::Title, "Tïtle — with “quotes”"),
    (Field::Artist, "Ärtist"),
    (Field::Album, "Album"),
    (Field::AlbumArtist, "Various Artists"),
    (Field::Date, "1999"),
    (Field::Genre, "Jazz"),
    (Field::Track, "3"),
    (Field::TrackTotal, "12"),
    (Field::Disc, "1"),
    (Field::DiscTotal, "2"),
    (Field::Composer, "Composer"),
    (Field::Comment, "A comment"),
    (Field::Lyrics, "[00:01.00]Hello\n[00:02.00]World"),
    (Field::MbReleaseId, "5b11f4ce-a62d-471e-81fc-a69a8278c7da"),
];

#[test]
fn every_format_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    for ext in FORMATS {
        let path = fixture(dir.path(), ext);
        let read = file::read(&path).unwrap_or_else(|e| panic!("{ext}: {e}"));
        assert!(read.info.duration_ms > 0 || ext == "opus", "{ext}: no duration");
        let before = read.tags.clone();
        let mut after = before.clone();
        for (field, value) in VALUES {
            if read.supported.contains(&field) {
                after.set(field, value);
            }
        }
        after.set_picture(Picture::new(PictureKind::Front, jpeg(7)));
        file::write(&path, &before, &after).unwrap_or_else(|e| panic!("{ext}: {e}"));

        let back = file::read(&path).unwrap();
        for (field, value) in VALUES {
            if read.supported.contains(&field) {
                assert_eq!(back.tags.get(field), value, "{ext}: {field:?}");
            }
        }
        assert_eq!(back.tags.pictures.len(), 1, "{ext}: pictures");
        assert_eq!(&*back.tags.pictures[0].data, jpeg(7).as_slice(), "{ext}: picture data");
        assert_eq!(back.tags.pictures[0].kind, PictureKind::Front, "{ext}: picture kind");

        // Clearing a field and the pictures leaves everything else alone.
        let mut cleared = back.tags.clone();
        cleared.set(Field::Comment, "");
        cleared.pictures.clear();
        file::write(&path, &back.tags, &cleared).unwrap();
        let again = file::read(&path).unwrap();
        assert_eq!(again.tags.get(Field::Comment), "", "{ext}: comment cleared");
        assert!(again.tags.pictures.is_empty(), "{ext}: pictures cleared");
        assert_eq!(again.tags.get(Field::Title), VALUES[0].1, "{ext}: title kept");
        assert_eq!(again.tags.get(Field::Track), "3", "{ext}: track kept");
    }
}

#[test]
fn several_genres_are_stored_as_a_list() {
    let dir = tempfile::tempdir().unwrap();
    for ext in FORMATS {
        let path = fixture(dir.path(), ext);
        let read = file::read(&path).unwrap();
        if !read.supported.contains(&Field::Genre) {
            continue;
        }
        let mut after = read.tags.clone();
        after.set(Field::Genre, "Hip-Hop; Rap");
        file::write(&path, &read.tags, &after).unwrap_or_else(|e| panic!("{ext}: {e}"));
        assert_eq!(file::read(&path).unwrap().tags.get(Field::Genre), "Hip-Hop; Rap", "{ext}");

        let tagged = lofty::read_from_path(&path).unwrap();
        let tag = tagged.primary_tag().unwrap();
        let stored: Vec<&str> = tag.get_strings(lofty::tag::ItemKey::Genre).collect();
        let expected: &[&str] = match tag.tag_type() {
            lofty::tag::TagType::VorbisComments | lofty::tag::TagType::Id3v2 => &["Hip-Hop", "Rap"],
            _ => &["Hip-Hop; Rap"],
        };
        assert_eq!(stored, expected, "{ext}");
    }
}

#[test]
fn unknown_id3_frames_survive_an_edit() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "mp3");
    {
        let mut f = MpegFile::read_from(&mut File::open(&path).unwrap(), ParseOptions::new()).unwrap();
        let mut tag = Id3v2Tag::new();
        tag.insert(Frame::Private(PrivateFrame::new("serato-like", vec![1u8, 2, 3, 4])));
        f.set_id3v2(tag);
        f.save_to_path(&path, WriteOptions::default()).unwrap();
    }
    let read = file::read(&path).unwrap();
    let mut after = read.tags.clone();
    after.set(Field::Title, "Edited");
    file::write(&path, &read.tags, &after).unwrap();

    let f = MpegFile::read_from(&mut File::open(&path).unwrap(), ParseOptions::new()).unwrap();
    let tag = f.id3v2().expect("ID3v2 tag");
    let private = tag.into_iter().any(|frame| matches!(frame, Frame::Private(p) if p.owner == "serato-like" && *p.private_data == [1, 2, 3, 4]));
    assert!(private, "the private frame was lost");
    assert_eq!(file::read(&path).unwrap().tags.get(Field::Title), "Edited");
}

#[test]
fn session_saves_and_renames() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "flac");
    std::fs::write(path.with_extension("lrc"), "[00:01.00]hi").unwrap();
    let mut session = Session::new(vec![EditFile::open(&path)]);
    session.set_field(&[0], Field::Title, "Intro");
    session.set_field(&[0], Field::Track, "1");
    session.rename_from_tags(&[0], &Mask::parse("%n - %t").unwrap(), 2);
    assert_eq!(session.modified_count(), 1);

    let jobs = session.save_jobs();
    assert_eq!(jobs.len(), 1);
    let final_path = jobs[0].run().unwrap();
    assert_eq!(final_path, dir.path().join("01 - Intro.flac"));
    assert!(!path.exists());
    assert!(dir.path().join("01 - Intro.lrc").exists(), "lyrics follow the file");
    session.finish_save(&jobs[0], Ok(EditFile::open(&final_path)));
    assert_eq!(session.modified_count(), 0);
    assert_eq!(session.files()[0].tags.get(Field::Title), "Intro");
}

#[test]
fn malformed_dates_from_other_software_are_survivable() {
    // ffmpeg (like several taggers) writes free text into ID3v2.4's TDRC, which must be a
    // timestamp; lofty's default mode then rejects the whole file.
    let dir = tempfile::tempdir().unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/baddate.mp3");
    let path = dir.path().join("baddate.mp3");
    std::fs::copy(&src, &path).unwrap();

    let read = file::read(&path).expect("readable despite the bad date");
    assert_eq!(read.tags.get(Field::Title), "Bad Date");
    let mut after = read.tags.clone();
    after.set(Field::Album, "Fixed");
    after.set(Field::Date, "c. 2006");
    file::write(&path, &read.tags, &after).unwrap();

    let back = file::read(&path).unwrap();
    assert_eq!(back.tags.get(Field::Date), "2006", "free text becomes a valid year");
    assert_eq!(back.tags.get(Field::Album), "Fixed");
    assert_eq!(back.tags.get(Field::Title), "Bad Date");
}

#[test]
fn a_date_without_a_year_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "mp3");
    let read = file::read(&path).unwrap();
    let mut after = read.tags.clone();
    after.set(Field::Date, "spring");
    let err = file::write(&path, &read.tags, &after).unwrap_err();
    assert!(err.to_string().contains("isn't a year"), "{err}");
    assert!(file::read(&path).is_ok(), "the file is untouched");
}

#[test]
fn read_only_files_say_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "mp3");
    let read = file::read(&path).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms).unwrap();
    if std::fs::OpenOptions::new().write(true).open(&path).is_ok() {
        return; // running as root (or on a filesystem that ignores the flag): nothing to check
    }
    let mut after = read.tags.clone();
    after.set(Field::Title, "Nope");
    let err = file::write(&path, &read.tags, &after).unwrap_err();
    assert!(err.to_string().contains("isn't allowed"), "{err}");
}
