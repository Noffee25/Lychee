//! The tag fields the editor knows, and how they map onto lofty's item keys.

use lofty::tag::{ItemKey, TagType};

/// A tag field. The first group is EasyTAG's set; the MusicBrainz identifiers are written by
/// the online lookup so later lookups (and other taggers) can find the exact release again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Date,
    Genre,
    Track,
    TrackTotal,
    Disc,
    DiscTotal,
    Composer,
    Comment,
    OriginalArtist,
    Lyricist,
    Label,
    CatalogNumber,
    Isrc,
    Copyright,
    Url,
    EncodedBy,
    Lyrics,
    MbReleaseId,
    MbReleaseGroupId,
    MbRecordingId,
    MbTrackId,
    MbArtistId,
    MbAlbumArtistId,
}

impl Field {
    pub const ALL: [Field; 27] = [
        Field::Title,
        Field::Artist,
        Field::Album,
        Field::AlbumArtist,
        Field::Date,
        Field::Genre,
        Field::Track,
        Field::TrackTotal,
        Field::Disc,
        Field::DiscTotal,
        Field::Composer,
        Field::Comment,
        Field::OriginalArtist,
        Field::Lyricist,
        Field::Label,
        Field::CatalogNumber,
        Field::Isrc,
        Field::Copyright,
        Field::Url,
        Field::EncodedBy,
        Field::Lyrics,
        Field::MbReleaseId,
        Field::MbReleaseGroupId,
        Field::MbRecordingId,
        Field::MbTrackId,
        Field::MbArtistId,
        Field::MbAlbumArtistId,
    ];

    /// Fields that hold free text, which the clean-up tool may rewrite.
    pub const TEXT: [Field; 12] = [
        Field::Title,
        Field::Artist,
        Field::Album,
        Field::AlbumArtist,
        Field::Genre,
        Field::Composer,
        Field::Comment,
        Field::OriginalArtist,
        Field::Lyricist,
        Field::Label,
        Field::Copyright,
        Field::EncodedBy,
    ];

    /// Fields that are the same for every track of an album.
    pub const ALBUM: [Field; 10] = [
        Field::Album,
        Field::AlbumArtist,
        Field::Date,
        Field::Genre,
        Field::TrackTotal,
        Field::DiscTotal,
        Field::Label,
        Field::CatalogNumber,
        Field::MbReleaseId,
        Field::MbReleaseGroupId,
    ];

    /// Stable identifier used by the UI.
    pub fn id(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Artist => "artist",
            Field::Album => "album",
            Field::AlbumArtist => "album-artist",
            Field::Date => "date",
            Field::Genre => "genre",
            Field::Track => "track",
            Field::TrackTotal => "track-total",
            Field::Disc => "disc",
            Field::DiscTotal => "disc-total",
            Field::Composer => "composer",
            Field::Comment => "comment",
            Field::OriginalArtist => "original-artist",
            Field::Lyricist => "lyricist",
            Field::Label => "label",
            Field::CatalogNumber => "catalog-number",
            Field::Isrc => "isrc",
            Field::Copyright => "copyright",
            Field::Url => "url",
            Field::EncodedBy => "encoded-by",
            Field::Lyrics => "lyrics",
            Field::MbReleaseId => "mb-release",
            Field::MbReleaseGroupId => "mb-release-group",
            Field::MbRecordingId => "mb-recording",
            Field::MbTrackId => "mb-track",
            Field::MbArtistId => "mb-artist",
            Field::MbAlbumArtistId => "mb-album-artist",
        }
    }

    pub fn from_id(id: &str) -> Option<Field> {
        Field::ALL.into_iter().find(|f| f.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Field::Title => "Title",
            Field::Artist => "Artist",
            Field::Album => "Album",
            Field::AlbumArtist => "Album artist",
            Field::Date => "Year",
            Field::Genre => "Genre",
            Field::Track => "Track",
            Field::TrackTotal => "Tracks",
            Field::Disc => "Disc",
            Field::DiscTotal => "Discs",
            Field::Composer => "Composer",
            Field::Comment => "Comment",
            Field::OriginalArtist => "Original artist",
            Field::Lyricist => "Lyricist",
            Field::Label => "Label",
            Field::CatalogNumber => "Catalog number",
            Field::Isrc => "ISRC",
            Field::Copyright => "Copyright",
            Field::Url => "URL",
            Field::EncodedBy => "Encoded by",
            Field::Lyrics => "Lyrics",
            Field::MbReleaseId => "MusicBrainz release",
            Field::MbReleaseGroupId => "MusicBrainz release group",
            Field::MbRecordingId => "MusicBrainz recording",
            Field::MbTrackId => "MusicBrainz track",
            Field::MbArtistId => "MusicBrainz artist",
            Field::MbAlbumArtistId => "MusicBrainz album artist",
        }
    }

    pub fn is_number(self) -> bool {
        matches!(self, Field::Track | Field::TrackTotal | Field::Disc | Field::DiscTotal)
    }

    pub fn is_musicbrainz(self) -> bool {
        matches!(
            self,
            Field::MbReleaseId
                | Field::MbReleaseGroupId
                | Field::MbRecordingId
                | Field::MbTrackId
                | Field::MbArtistId
                | Field::MbAlbumArtistId
        )
    }

    /// The lofty key for this field in a tag of the given type.
    pub(crate) fn key(self, tag_type: TagType) -> ItemKey {
        match self {
            Field::Title => ItemKey::TrackTitle,
            Field::Artist => ItemKey::TrackArtist,
            Field::Album => ItemKey::AlbumTitle,
            Field::AlbumArtist => ItemKey::AlbumArtist,
            Field::Date => ItemKey::RecordingDate,
            Field::Genre => ItemKey::Genre,
            Field::Track => ItemKey::TrackNumber,
            Field::TrackTotal => ItemKey::TrackTotal,
            Field::Disc => ItemKey::DiscNumber,
            Field::DiscTotal => ItemKey::DiscTotal,
            Field::Composer => ItemKey::Composer,
            Field::Comment => ItemKey::Comment,
            Field::OriginalArtist => ItemKey::OriginalArtist,
            Field::Lyricist => ItemKey::Lyricist,
            Field::Label => ItemKey::Label,
            Field::CatalogNumber => ItemKey::CatalogNumber,
            Field::Isrc => ItemKey::Isrc,
            Field::Copyright => ItemKey::CopyrightMessage,
            Field::Url => ItemKey::AudioFileUrl,
            Field::EncodedBy => ItemKey::EncodedBy,
            // ID3v2 keeps plain lyrics in USLT, which lofty calls "unsynchronised".
            Field::Lyrics if tag_type == TagType::Id3v2 => ItemKey::UnsyncLyrics,
            Field::Lyrics => ItemKey::Lyrics,
            Field::MbReleaseId => ItemKey::MusicBrainzReleaseId,
            Field::MbReleaseGroupId => ItemKey::MusicBrainzReleaseGroupId,
            Field::MbRecordingId => ItemKey::MusicBrainzRecordingId,
            Field::MbTrackId => ItemKey::MusicBrainzTrackId,
            Field::MbArtistId => ItemKey::MusicBrainzArtistId,
            Field::MbAlbumArtistId => ItemKey::MusicBrainzReleaseArtistId,
        }
    }

    /// Other keys that hold the same information in files written by other software.
    pub(crate) fn alternate_keys(self) -> &'static [ItemKey] {
        match self {
            Field::Date => &[ItemKey::Year],
            Field::Lyrics => &[ItemKey::Lyrics, ItemKey::UnsyncLyrics],
            _ => &[],
        }
    }

    /// Can a tag of this type store the field?
    pub fn supported_by(self, tag_type: TagType) -> bool {
        self.key(tag_type).map_key(tag_type).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_round_trip() {
        for f in Field::ALL {
            assert_eq!(Field::from_id(f.id()), Some(f));
        }
        let mut ids: Vec<_> = Field::ALL.iter().map(|f| f.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), Field::ALL.len());
    }

    #[test]
    fn common_fields_are_supported_everywhere() {
        for tag_type in [TagType::Id3v2, TagType::VorbisComments, TagType::Mp4Ilst, TagType::Ape] {
            for f in [Field::Title, Field::Artist, Field::Album, Field::Track, Field::Date, Field::Genre, Field::Lyrics] {
                assert!(f.supported_by(tag_type), "{f:?} in {tag_type:?}");
            }
        }
        assert!(!Field::Composer.supported_by(TagType::Id3v1));
    }
}
