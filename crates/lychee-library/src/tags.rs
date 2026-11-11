//! Reading tags and technical properties from audio files.

use std::path::Path;
use std::time::UNIX_EPOCH;

use lofty::file::FileType;
use lofty::picture::PictureType;
use lofty::prelude::*;
use lofty::tag::Tag;
use lychee_core::AudioFormat;

use crate::db::TrackRecord;

/// Everything the scanner learns from one file.
pub struct FileMeta {
    pub record: TrackRecord,
    /// Embedded front cover, if any.
    pub picture: Option<Vec<u8>>,
}

/// Read a file's metadata. Never fails: unreadable tags fall back to file and folder names, and
/// formats lofty can't parse (CAF, Matroska) are probed with the decoder instead.
pub fn read(path: &Path) -> std::io::Result<FileMeta> {
    let fs_meta = std::fs::metadata(path)?;
    let mtime = fs_meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
    let mut record = TrackRecord {
        path: path.to_path_buf(),
        mtime,
        size: fs_meta.len(),
        title: file_stem(path),
        album: folder_name(path),
        ..Default::default()
    };
    let mut picture = None;

    match lofty::read_from_path(path) {
        Ok(tagged) => {
            let props = tagged.properties();
            record.duration_ms = props.duration().as_millis() as u64;
            record.format = AudioFormat {
                codec: codec_name(tagged.file_type(), props.bit_depth()).to_string(),
                sample_rate: props.sample_rate(),
                bit_depth: props.bit_depth(),
                channels: props.channels(),
                bitrate_kbps: props.audio_bitrate().or(props.overall_bitrate()),
            };
            if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
                apply_tag(tag, &mut record);
            }
            picture = tagged.tags().iter().find_map(front_cover);
        }
        Err(err) => {
            tracing::debug!("lofty could not read {}: {err}", path.display());
            if let Ok(source) = lychee_audio::Source::open(path) {
                record.duration_ms = source.duration_ms.unwrap_or(0);
                record.format = AudioFormat {
                    codec: friendly_codec(&source.info.codec),
                    sample_rate: Some(source.info.sample_rate),
                    bit_depth: source.info.bits_per_sample.and_then(|b| u8::try_from(b).ok()),
                    channels: Some(source.info.channels as u8),
                    bitrate_kbps: None,
                };
            }
        }
    }
    if record.format.codec.is_empty() {
        record.format.codec = path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
    }
    Ok(FileMeta { record, picture })
}

fn apply_tag(tag: &Tag, record: &mut TrackRecord) {
    if let Some(title) = tag.title().map(|t| clean(&t)).filter(|t| !t.is_empty()) {
        record.title = title;
    }
    let artists: Vec<String> = tag.get_strings(ItemKey::TrackArtist).map(clean).filter(|a| !a.is_empty()).collect();
    if !artists.is_empty() {
        record.artist = artists.join(", ");
    }
    if let Some(album) = tag.album().map(|a| clean(&a)).filter(|a| !a.is_empty()) {
        record.album = album;
    }
    record.album_artist = tag.get_string(ItemKey::AlbumArtist).map(clean).unwrap_or_default();
    record.track_number = tag.track();
    record.disc_number = tag.disk();
    record.year = tag
        .date()
        .map(|d| i32::from(d.year))
        .or_else(|| tag.get_string(ItemKey::Year).and_then(leading_year))
        .or_else(|| tag.get_string(ItemKey::RecordingDate).and_then(leading_year))
        .filter(|y| *y > 0);
    record.genre = tag.genre().map(|g| clean(&g)).filter(|g| !g.is_empty());
    record.composer = tag.get_string(ItemKey::Composer).map(clean).filter(|c| !c.is_empty());
}

fn front_cover(tag: &Tag) -> Option<Vec<u8>> {
    let pictures = tag.pictures();
    pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .map(|p| p.data().to_vec())
        .filter(|d| !d.is_empty())
}

/// Lyrics stored inside the file (synced LRC text or plain text).
pub fn embedded_lyrics(path: &Path) -> Option<String> {
    let tagged = lofty::read_from_path(path).ok()?;
    tagged.tags().iter().find_map(|tag| {
        tag.get_string(ItemKey::Lyrics)
            .or_else(|| tag.get_string(ItemKey::UnsyncLyrics))
            .map(str::to_string)
            .filter(|l| !l.trim().is_empty())
    })
}

fn codec_name(file_type: FileType, bit_depth: Option<u8>) -> &'static str {
    match file_type {
        FileType::Flac => "FLAC",
        FileType::Mpeg => "MP3",
        FileType::Mp4 => {
            if bit_depth.is_some() { "ALAC" } else { "AAC" }
        }
        FileType::Aac => "AAC",
        FileType::Opus => "Opus",
        FileType::Vorbis => "Vorbis",
        FileType::Speex => "Speex",
        FileType::Wav => "WAV",
        FileType::Aiff => "AIFF",
        FileType::Ape => "APE",
        FileType::WavPack => "WavPack",
        FileType::Mpc => "Musepack",
        _ => "",
    }
}

fn friendly_codec(symphonia_name: &str) -> String {
    let upper = symphonia_name.to_uppercase();
    if upper.starts_with("PCM") {
        "PCM".into()
    } else if upper == "VORBIS" {
        "Vorbis".into()
    } else if upper == "OPUS" {
        "Opus".into()
    } else {
        upper
    }
}

fn clean(s: impl AsRef<str>) -> String {
    s.as_ref().trim().trim_matches('\0').to_string()
}

fn leading_year(s: &str) -> Option<i32> {
    s.trim().get(0..4).and_then(|y| y.parse().ok())
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Unknown".into())
}

fn folder_name(path: &Path) -> String {
    path.parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Unknown Album".into())
}
