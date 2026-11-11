//! Reading and writing tags and pictures.
//!
//! lofty's format-independent [`Tag`] can't represent every ID3v2 frame or MP4 atom (Serato and
//! Traktor data, private frames, chapters...). Converting a whole file to it and back would drop
//! them, so MP3, WAV, AIFF, AAC and MP4 files are edited through their concrete tag types:
//! [`SplitTag`] hands us the parts the editor understands and [`MergeTag`] puts the rest back.
//! Formats whose tags are plain key/value text (FLAC, Ogg, APE, WavPack...) use the generic path.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use lofty::aac::AacFile;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::iff::aiff::AiffFile;
use lofty::iff::wav::WavFile;
use lofty::mp4::Mp4File;
use lofty::mpeg::MpegFile;
use lofty::picture::{MimeType, Picture as LoftyPicture, PictureType};
use lofty::probe::Probe;
use lofty::tag::{MergeTag, SplitTag, Tag, TagType};

use crate::{Error, Field};

/// What a picture shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PictureKind {
    Front,
    Back,
    Booklet,
    Media,
    Artist,
    Other,
}

impl PictureKind {
    pub const ALL: [PictureKind; 6] =
        [PictureKind::Front, PictureKind::Back, PictureKind::Booklet, PictureKind::Media, PictureKind::Artist, PictureKind::Other];

    pub fn label(self) -> &'static str {
        match self {
            PictureKind::Front => "Front cover",
            PictureKind::Back => "Back cover",
            PictureKind::Booklet => "Booklet",
            PictureKind::Media => "Disc",
            PictureKind::Artist => "Artist",
            PictureKind::Other => "Other",
        }
    }

    fn from_lofty(t: PictureType) -> PictureKind {
        match t {
            PictureType::CoverFront => PictureKind::Front,
            PictureType::CoverBack => PictureKind::Back,
            PictureType::Leaflet => PictureKind::Booklet,
            PictureType::Media => PictureKind::Media,
            PictureType::Artist | PictureType::LeadArtist | PictureType::Band | PictureType::Conductor => PictureKind::Artist,
            _ => PictureKind::Other,
        }
    }

    fn to_lofty(self) -> PictureType {
        match self {
            PictureKind::Front => PictureType::CoverFront,
            PictureKind::Back => PictureType::CoverBack,
            PictureKind::Booklet => PictureType::Leaflet,
            PictureKind::Media => PictureType::Media,
            PictureKind::Artist => PictureType::Artist,
            PictureKind::Other => PictureType::Other,
        }
    }
}

/// An embedded picture. The bytes are shared, so copies (undo steps, many files) are cheap.
#[derive(Clone, Debug, Eq)]
pub struct Picture {
    pub kind: PictureKind,
    pub mime: String,
    pub description: String,
    pub data: Arc<[u8]>,
}

impl PartialEq for Picture {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.mime == other.mime
            && self.description == other.description
            && (Arc::ptr_eq(&self.data, &other.data) || self.data == other.data)
    }
}

impl Picture {
    pub fn new(kind: PictureKind, data: Vec<u8>) -> Picture {
        Picture { kind, mime: sniff_mime(&data).to_string(), description: String::new(), data: data.into() }
    }

    fn from_lofty(p: &LoftyPicture) -> Picture {
        Picture {
            kind: PictureKind::from_lofty(p.pic_type()),
            mime: p.mime_type().map(|m| m.as_str().to_string()).unwrap_or_else(|| sniff_mime(p.data()).to_string()),
            description: p.description().unwrap_or_default().to_string(),
            data: p.data().into(),
        }
    }

    fn to_lofty(&self) -> LoftyPicture {
        let mut builder = LoftyPicture::unchecked(self.data.to_vec()).pic_type(self.kind.to_lofty()).mime_type(MimeType::from_str(&self.mime));
        if !self.description.is_empty() {
            builder = builder.description(self.description.clone());
        }
        builder.build()
    }

    /// File extension matching the image data.
    pub fn extension(&self) -> &'static str {
        match sniff_mime(&self.data) {
            "image/png" => "png",
            "image/gif" => "gif",
            "image/bmp" => "bmp",
            "image/webp" => "webp",
            _ => "jpg",
        }
    }
}

/// MIME type from the first bytes of an image.
pub fn sniff_mime(data: &[u8]) -> &'static str {
    match data {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'G', b'I', b'F', b'8', ..] => "image/gif",
        [b'B', b'M', ..] => "image/bmp",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        _ => "image/jpeg",
    }
}

/// The values of all fields plus the pictures. Empty values mean "not set".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagData {
    values: BTreeMap<Field, String>,
    pub pictures: Vec<Picture>,
}

impl TagData {
    pub fn get(&self, field: Field) -> &str {
        self.values.get(&field).map_or("", String::as_str)
    }

    pub fn set(&mut self, field: Field, value: impl Into<String>) {
        let value = value.into();
        if value.is_empty() {
            self.values.remove(&field);
        } else {
            self.values.insert(field, value);
        }
    }

    pub fn front_cover(&self) -> Option<&Picture> {
        self.pictures.iter().find(|p| p.kind == PictureKind::Front).or(self.pictures.first())
    }

    /// Put a picture in, replacing any other picture of the same kind.
    pub fn set_picture(&mut self, picture: Picture) {
        match self.pictures.iter().position(|p| p.kind == picture.kind) {
            Some(i) => {
                self.pictures[i] = picture;
                let kind = self.pictures[i].kind;
                let mut seen = false;
                self.pictures.retain(|p| p.kind != kind || !std::mem::replace(&mut seen, true));
            }
            None => self.pictures.push(picture),
        }
    }

    /// Fields whose values differ.
    pub fn changed_fields(&self, other: &TagData) -> Vec<Field> {
        Field::ALL.into_iter().filter(|f| self.get(*f) != other.get(*f)).collect()
    }
}

/// Technical facts about a file, for the editor's File tab.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileInfo {
    pub format: String,
    pub tag_format: String,
    pub duration_ms: u64,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    pub bitrate_kbps: Option<u32>,
    pub size: u64,
}

pub struct ReadFile {
    pub tags: TagData,
    pub info: FileInfo,
    /// Fields the file's main tag can store.
    pub supported: Vec<Field>,
    pub supports_pictures: bool,
}

/// Read every field the editor knows. The main tag wins; other tags (an MP3's ID3v1, say) only
/// fill fields it lacks.
pub fn read(path: &Path) -> Result<ReadFile, Error> {
    let size = std::fs::metadata(path)?.len();
    let tagged = Probe::open(path)?.options(ParseOptions::new()).read()?;
    let primary = tagged.primary_tag_type();
    let mut ordered: Vec<&Tag> = tagged.tag(primary).into_iter().collect();
    ordered.extend(tagged.tags().iter().filter(|t| t.tag_type() != primary));

    let mut tags = TagData::default();
    for tag in ordered {
        for field in Field::ALL {
            if tags.get(field).is_empty() {
                let value = read_field(tag, field);
                tags.set(field, value);
            }
        }
        if tags.pictures.is_empty() {
            tags.pictures = stored_pictures(tag);
        }
    }
    split_totals(&mut tags, Field::Track, Field::TrackTotal);
    split_totals(&mut tags, Field::Disc, Field::DiscTotal);

    let p = tagged.properties();
    let info = FileInfo {
        format: format_name(tagged.file_type(), p.bit_depth()).to_string(),
        tag_format: tag_format_name(primary).to_string(),
        duration_ms: p.duration().as_millis() as u64,
        sample_rate: p.sample_rate(),
        bit_depth: p.bit_depth(),
        channels: p.channels(),
        bitrate_kbps: p.audio_bitrate().or(p.overall_bitrate()).filter(|b| *b > 0),
        size,
    };
    let supported = Field::ALL.into_iter().filter(|f| f.supported_by(primary)).collect();
    let supports_pictures = matches!(primary, TagType::Id3v2 | TagType::VorbisComments | TagType::Mp4Ilst | TagType::Ape);
    Ok(ReadFile { tags, info, supported, supports_pictures })
}

/// "3/12" in a track number field becomes track 3 of 12.
fn split_totals(tags: &mut TagData, number: Field, total: Field) {
    let value = tags.get(number).to_string();
    if let Some((n, t)) = value.split_once('/') {
        tags.set(number, n.trim());
        if tags.get(total).is_empty() {
            tags.set(total, t.trim());
        }
    }
}

fn read_field(tag: &Tag, field: Field) -> String {
    let main = field.key(tag.tag_type());
    for key in std::iter::once(&main).chain(field.alternate_keys()) {
        let values: Vec<&str> =
            tag.get_strings(key.clone()).map(|s| s.trim_matches('\0').trim()).filter(|s| !s.is_empty()).collect();
        if !values.is_empty() {
            return values.join("; ");
        }
    }
    String::new()
}

/// Write the changes from `before` to `after`.
///
/// The main tag is made to match `after` exactly (it is created if the file has none), touching
/// only fields whose stored value differs, so untouched multi-value fields keep their values.
/// Other tags in the file get the fields that changed, so they don't contradict the main one.
pub fn write(path: &Path, before: &TagData, after: &TagData) -> Result<(), Error> {
    if before == after {
        return Ok(());
    }
    let plan = Plan { after, changed: before.changed_fields(after) };
    let file_type = Probe::open(path)?.guess_file_type()?.file_type().ok_or(Error::Unsupported)?;
    let options = WriteOptions::default();
    match file_type {
        FileType::Mpeg => {
            let mut f: MpegFile = open(path)?;
            let tag = plan.primary(f.remove_id3v2().unwrap_or_default());
            f.set_id3v2(tag);
            if let Some(t) = f.remove_id3v1() {
                f.set_id3v1(plan.secondary(t));
            }
            if let Some(t) = f.remove_ape() {
                f.set_ape(plan.secondary(t));
            }
            f.save_to_path(path, options)?;
        }
        FileType::Wav => {
            let mut f: WavFile = open(path)?;
            let tag = plan.primary(f.remove_id3v2().unwrap_or_default());
            f.set_id3v2(tag);
            if let Some(t) = f.remove_riff_info() {
                f.set_riff_info(plan.secondary(t));
            }
            f.save_to_path(path, options)?;
        }
        FileType::Aiff => {
            let mut f: AiffFile = open(path)?;
            let tag = plan.primary(f.remove_id3v2().unwrap_or_default());
            f.set_id3v2(tag);
            if let Some(t) = f.remove_text_chunks() {
                f.set_text_chunks(plan.secondary(t));
            }
            f.save_to_path(path, options)?;
        }
        FileType::Aac => {
            let mut f: AacFile = open(path)?;
            let tag = plan.primary(f.remove_id3v2().unwrap_or_default());
            f.set_id3v2(tag);
            if let Some(t) = f.remove_id3v1() {
                f.set_id3v1(plan.secondary(t));
            }
            f.save_to_path(path, options)?;
        }
        FileType::Mp4 => {
            let mut f: Mp4File = open(path)?;
            let tag = plan.primary(f.remove_ilst().unwrap_or_default());
            f.set_ilst(tag);
            f.save_to_path(path, options)?;
        }
        _ => {
            let mut tagged = Probe::open(path)?.options(ParseOptions::new()).read()?;
            let primary = tagged.primary_tag_type();
            if tagged.tag(primary).is_none() {
                tagged.insert_tag(Tag::new(primary));
            }
            let types: Vec<TagType> = tagged.tags().iter().map(Tag::tag_type).collect();
            for tag_type in types {
                if let Some(tag) = tagged.tag_mut(tag_type) {
                    if tag_type == primary {
                        plan.sync_primary(tag);
                    } else {
                        plan.sync_secondary(tag);
                    }
                }
            }
            tagged.save_to_path(path, options)?;
        }
    }
    Ok(())
}

fn open<T: AudioFile>(path: &Path) -> Result<T, Error> {
    let mut file = File::open(path)?;
    Ok(T::read_from(&mut file, ParseOptions::new())?)
}

struct Plan<'a> {
    after: &'a TagData,
    changed: Vec<Field>,
}

impl Plan<'_> {
    fn primary<T: SplitTag>(&self, tag: T) -> <T::Remainder as MergeTag>::Merged {
        let (rest, mut generic) = tag.split_tag();
        self.sync_primary(&mut generic);
        rest.merge_tag(generic)
    }

    fn secondary<T: SplitTag>(&self, tag: T) -> <T::Remainder as MergeTag>::Merged {
        let (rest, mut generic) = tag.split_tag();
        self.sync_secondary(&mut generic);
        rest.merge_tag(generic)
    }

    fn sync_primary(&self, tag: &mut Tag) {
        let tag_type = tag.tag_type();
        for field in Field::ALL {
            if !field.supported_by(tag_type) {
                continue;
            }
            let wanted = self.after.get(field);
            let stored = read_field(tag, field);
            let stored_split = match field {
                // A "3/12" track number counts as 3 when the total is stored separately.
                Field::Track | Field::Disc => stored.split_once('/').map_or(stored.as_str(), |(n, _)| n.trim()),
                _ => stored.as_str(),
            };
            if stored_split != wanted || (stored.contains('/') && field.is_number()) {
                put(tag, field, wanted);
            }
        }
        if stored_pictures(tag) != normalized(tag_type, self.after.pictures.clone()) {
            while !tag.pictures().is_empty() {
                tag.remove_picture(0);
            }
            for p in &self.after.pictures {
                tag.push_picture(p.to_lofty());
            }
        }
    }

    /// Secondary tags (ID3v1, APE on MP3, RIFF INFO) only follow the edited text fields.
    fn sync_secondary(&self, tag: &mut Tag) {
        let tag_type = tag.tag_type();
        for field in &self.changed {
            if *field != Field::Lyrics && !field.is_musicbrainz() && field.supported_by(tag_type) {
                put(tag, *field, self.after.get(*field));
            }
        }
    }
}

fn stored_pictures(tag: &Tag) -> Vec<Picture> {
    normalized(tag.tag_type(), tag.pictures().iter().map(Picture::from_lofty).collect())
}

/// MP4 cover atoms have no picture type or description: the first one is the front cover.
fn normalized(tag_type: TagType, mut pictures: Vec<Picture>) -> Vec<Picture> {
    if tag_type == TagType::Mp4Ilst {
        for (i, p) in pictures.iter_mut().enumerate() {
            p.kind = if i == 0 { PictureKind::Front } else { PictureKind::Other };
            p.description.clear();
        }
    }
    pictures
}

fn put(tag: &mut Tag, field: Field, value: &str) {
    let key = field.key(tag.tag_type());
    tag.remove_key(key.clone());
    for alt in field.alternate_keys() {
        if *alt != key && alt.map_key(tag.tag_type()).is_some() {
            tag.remove_key(alt.clone());
        }
    }
    if !value.is_empty() && !tag.insert_text(key, value.to_string()) {
        tracing::debug!("{:?} can't store {field:?}", tag.tag_type());
    }
}

fn format_name(file_type: FileType, bit_depth: Option<u8>) -> &'static str {
    match file_type {
        FileType::Flac => "FLAC",
        FileType::Mpeg => "MP3",
        FileType::Mp4 => {
            if bit_depth.is_some() {
                "ALAC"
            } else {
                "AAC (MP4)"
            }
        }
        FileType::Aac => "AAC",
        FileType::Opus => "Opus",
        FileType::Vorbis => "Ogg Vorbis",
        FileType::Speex => "Speex",
        FileType::Wav => "WAV",
        FileType::Aiff => "AIFF",
        FileType::Ape => "Monkey's Audio",
        FileType::WavPack => "WavPack",
        FileType::Mpc => "Musepack",
        _ => "Audio",
    }
}

fn tag_format_name(tag_type: TagType) -> &'static str {
    match tag_type {
        TagType::Id3v2 => "ID3v2",
        TagType::Id3v1 => "ID3v1",
        TagType::VorbisComments => "Vorbis comments",
        TagType::Mp4Ilst => "MP4 metadata",
        TagType::Ape => "APEv2",
        TagType::RiffInfo => "RIFF INFO",
        TagType::AiffText => "AIFF text",
        _ => "Tags",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_picture_replaces_same_kind() {
        let mut t = TagData::default();
        t.set_picture(Picture::new(PictureKind::Front, vec![0xFF, 0xD8, 0xFF, 1]));
        t.set_picture(Picture::new(PictureKind::Back, vec![0x89, b'P', b'N', b'G']));
        t.set_picture(Picture::new(PictureKind::Front, vec![0xFF, 0xD8, 0xFF, 2]));
        assert_eq!(t.pictures.len(), 2);
        assert_eq!(&*t.front_cover().unwrap().data, &[0xFF, 0xD8, 0xFF, 2]);
        assert_eq!(t.pictures[1].mime, "image/png");
    }

    #[test]
    fn totals_split_off() {
        let mut t = TagData::default();
        t.set(Field::Track, "3/12");
        split_totals(&mut t, Field::Track, Field::TrackTotal);
        assert_eq!((t.get(Field::Track), t.get(Field::TrackTotal)), ("3", "12"));
    }
}
