//! Reading tags and technical properties from audio files.

use std::path::Path;
use std::time::UNIX_EPOCH;

use lofty::file::FileType;
use lofty::picture::PictureType;
use lofty::prelude::*;
use lofty::tag::Tag;
use lychee_core::{AudioFormat, ReplayGain};

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

    match read_tagged(path) {
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
            record.replay_gain = replay_gain(tagged.primary_tag().into_iter().chain(tagged.tags()));
            if tagged.file_type() == FileType::Opus {
                opus_gain(path, &mut record.replay_gain);
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

/// lofty's default mode rejects a whole file over one malformed frame (dates like "c. 2006" are
/// common); retry in relaxed mode, which skips just the broken parts.
fn read_tagged(path: &Path) -> Result<lofty::file::TaggedFile, lofty::error::FileParseError> {
    use lofty::config::{ParseOptions, ParsingMode};
    lofty::read_from_path(path).or_else(|first| {
        let relaxed = ParseOptions::new().parsing_mode(ParsingMode::Relaxed);
        lofty::probe::Probe::open(path).ok().and_then(|p| p.options(relaxed).read().ok()).ok_or(first)
    })
}

fn apply_tag(tag: &Tag, record: &mut TrackRecord) {
    if let Some(title) = tag.title().map(|t| clean(&t)).filter(|t| !t.is_empty()) {
        record.title = title;
    }
    let artists: Vec<String> = tag.get_strings(ItemKey::TrackArtist).map(clean).filter(|a| !a.is_empty()).collect();
    if !artists.is_empty() {
        record.artist = artists.join(", ");
    }
    // Picard-style taggers keep the credit ("A feat. B") in ARTIST and the names in ARTISTS.
    let credited: Vec<String> = tag.get_strings(ItemKey::TrackArtists).map(clean).filter(|a| !a.is_empty()).collect();
    if let Some(first) = credited.first().or(artists.first()) {
        record.main_artist = lychee_core::main_artist(first).to_string();
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
    // Several genres arrive as repeated fields, an ID3v2.4 list or one "A; B" string.
    let genres: Vec<String> = tag.get_strings(ItemKey::Genre).flat_map(lychee_core::split_genres).map(clean).collect();
    record.genre = Some(lychee_core::join_genres(genres.iter().map(String::as_str))).filter(|g| !g.is_empty());
    record.composer = tag.get_string(ItemKey::Composer).map(clean).filter(|c| !c.is_empty());
}

/// ReplayGain values from the first tag that has them (MP3s often keep them in an APE tag, as
/// mp3gain writes them, next to the ID3v2 tag).
fn replay_gain<'a>(tags: impl Iterator<Item = &'a Tag> + Clone) -> ReplayGain {
    use lychee_core::gain::{parse_gain, parse_peak};
    let find = |key: ItemKey, parse: fn(&str) -> Option<f32>| tags.clone().find_map(|t| t.get_string(key).and_then(parse));
    ReplayGain {
        track_gain: find(ItemKey::ReplayGainTrackGain, parse_gain),
        track_peak: find(ItemKey::ReplayGainTrackPeak, parse_peak),
        album_gain: find(ItemKey::ReplayGainAlbumGain, parse_gain),
        album_peak: find(ItemKey::ReplayGainAlbumPeak, parse_peak),
    }
}

/// Opus keeps its gains in `R128_TRACK_GAIN`/`R128_ALBUM_GAIN` (relative to −23 LUFS), on top
/// of the output gain in the stream header. Lychee's Opus decoder doesn't apply that header gain,
/// so it is added to every gain here; Lychee's own measurements see the same undecorated signal.
fn opus_gain(path: &Path, rg: &mut ReplayGain) {
    use lofty::config::{ParseOptions, ParsingMode};
    use lofty::file::AudioFile;
    use lychee_core::gain::parse_r128;
    let Ok(mut file) = std::fs::File::open(path) else { return };
    let options = ParseOptions::new().parsing_mode(ParsingMode::Relaxed).read_cover_art(false);
    let Ok(opus) = lofty::ogg::OpusFile::read_from(&mut file, options) else { return };
    let comments = opus.vorbis_comments();
    rg.track_gain = rg.track_gain.or_else(|| comments.get("R128_TRACK_GAIN").and_then(parse_r128));
    rg.album_gain = rg.album_gain.or_else(|| comments.get("R128_ALBUM_GAIN").and_then(parse_r128));
    let header = opus.properties().output_gain_db();
    if header != 0.0 {
        rg.track_gain = rg.track_gain.map(|g| g + header);
        rg.album_gain = rg.album_gain.map(|g| g + header);
    }
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
    let tagged = read_tagged(path).ok()?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::tag::{ItemKey, ItemValue, TagItem, TagType};

    fn read_artists(values: &[(ItemKey, &str)]) -> (String, String) {
        let mut tag = Tag::new(TagType::VorbisComments);
        for (key, value) in values {
            tag.push(TagItem::new(*key, ItemValue::Text((*value).into())));
        }
        let mut record = TrackRecord::default();
        apply_tag(&tag, &mut record);
        (record.artist, record.main_artist)
    }

    #[test]
    fn reads_replay_gain_from_any_tag() {
        let mut id3 = Tag::new(TagType::Id3v2);
        id3.push(TagItem::new(ItemKey::TrackTitle, ItemValue::Text("Song".into())));
        let mut ape = Tag::new(TagType::Ape);
        ape.push(TagItem::new(ItemKey::ReplayGainTrackGain, ItemValue::Text("-7.25 dB".into())));
        ape.push(TagItem::new(ItemKey::ReplayGainTrackPeak, ItemValue::Text("0.977".into())));
        ape.push(TagItem::new(ItemKey::ReplayGainAlbumGain, ItemValue::Text("+1.5 dB".into())));
        let tags = [id3, ape];
        let rg = replay_gain(tags.iter());
        assert_eq!(rg.track_gain, Some(-7.25));
        assert_eq!(rg.track_peak, Some(0.977));
        assert_eq!(rg.album_gain, Some(1.5));
        assert_eq!(rg.album_peak, None);
    }

    #[test]
    fn finds_the_main_artist() {
        use ItemKey::{TrackArtist as A, TrackArtists as AS};
        let pair = |a: &str, m: &str| (a.to_string(), m.to_string());
        assert_eq!(read_artists(&[(A, "Daft Punk"), (A, "Pharrell Williams")]), pair("Daft Punk, Pharrell Williams", "Daft Punk"));
        assert_eq!(read_artists(&[(A, "Daft Punk feat. Pharrell Williams")]), pair("Daft Punk feat. Pharrell Williams", "Daft Punk"));
        assert_eq!(
            read_artists(&[(A, "Daft Punk & Pharrell"), (AS, "Daft Punk"), (AS, "Pharrell")]),
            pair("Daft Punk & Pharrell", "Daft Punk")
        );
        assert_eq!(read_artists(&[(A, "Simon & Garfunkel")]), pair("Simon & Garfunkel", "Simon & Garfunkel"));
        assert_eq!(read_artists(&[(A, "AC/DC")]), pair("AC/DC", "AC/DC"));
        assert_eq!(read_artists(&[(A, "Daft Punk; Pharrell")]), pair("Daft Punk; Pharrell", "Daft Punk"));
        assert_eq!(read_artists(&[]), pair("", ""));
    }
}
