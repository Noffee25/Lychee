//! Genres: several per file, spelled many ways.
//!
//! Files store several genres as repeated fields (Vorbis comments), an ID3v2.4 list, or one string
//! joined with `;` (the convention where a format has no lists). Lychee keeps them as one string
//! joined with "; ". `/` is not a separator: Apple's store genres contain it ("Hip-Hop/Rap",
//! "R&B/Soul", "Singer/Songwriter").

/// The separate genres of a tag value, trimmed, without empty ones.
pub fn split_genres(genre: &str) -> impl Iterator<Item = &str> {
    genre.split([';', '\0']).map(str::trim).filter(|g| !g.is_empty())
}

/// What makes two spellings the same genre: case, spaces, hyphens, underscores, dots and
/// apostrophes don't count, so "Hip-Hop", "Hip Hop", "hip-hop" and "HipHop" share a key.
pub fn genre_key(genre: &str) -> String {
    genre
        .chars()
        .filter(|c| !(c.is_whitespace() || matches!(c, '-' | '_' | '.' | '\'' | '’' | '‐' | '–')))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Genre values joined the way Lychee stores them ("Hip-Hop; Rap"), dropping repeats.
pub fn join_genres<'a>(genres: impl IntoIterator<Item = &'a str>) -> String {
    let mut seen = std::collections::HashSet::new();
    let kept: Vec<&str> =
        genres.into_iter().map(str::trim).filter(|g| !g.is_empty() && seen.insert(genre_key(g))).collect();
    kept.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_at_semicolons_and_nulls_only() {
        assert_eq!(split_genres("Hip-Hop; Rap").collect::<Vec<_>>(), ["Hip-Hop", "Rap"]);
        assert_eq!(split_genres("Hip-Hop\0Rap;;").collect::<Vec<_>>(), ["Hip-Hop", "Rap"]);
        assert_eq!(split_genres("Hip-Hop/Rap").collect::<Vec<_>>(), ["Hip-Hop/Rap"]);
        assert_eq!(split_genres(" ; ").count(), 0);
    }

    #[test]
    fn spellings_share_a_key() {
        for g in ["Hip-Hop", "Hip Hop", "hip-hop", "HipHop", "HIP_HOP", "Hip‐Hop"] {
            assert_eq!(genre_key(g), "hiphop", "{g}");
        }
        assert_ne!(genre_key("Hip-Hop/Rap"), genre_key("Rap/Hip Hop"));
        assert_eq!(genre_key("R&B"), "r&b");
        assert_eq!(genre_key("Drum 'n' Bass"), genre_key("Drum n Bass"));
    }

    #[test]
    fn joins_without_repeats() {
        assert_eq!(join_genres(["Hip-Hop", "Rap", "hip hop", " "]), "Hip-Hop; Rap");
    }
}
