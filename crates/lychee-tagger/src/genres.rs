//! Standardising genres: one spelling per genre, several genres as a list.
//!
//! `;` always separates genres (it's how Lychee shows a list). `,` and `/` do only when asked,
//! because Apple's store genres contain slashes ("Hip-Hop/Rap", "R&B/Soul").

use std::collections::HashMap;

use lychee_core::{genre_key, join_genres};

#[derive(Clone, Debug, Default)]
pub struct Genres {
    pub split_commas: bool,
    pub split_slashes: bool,
    /// Capitalise the first letter of each word ("hip hop" → "Hip Hop"); other letters are kept,
    /// so "EDM" and "R&B" stay as they are.
    pub capitalise: bool,
    /// Spellings to use, by [`genre_key`]: usually the library's ("Hip-Hop" for "hip hop").
    pub names: HashMap<String, String>,
}

impl Genres {
    /// Rewrite a Genre field. Text with no genres in it is returned unchanged.
    pub fn apply(&self, genre: &str) -> String {
        let separator =
            |c: char| c == ';' || c == '\0' || (self.split_commas && c == ',') || (self.split_slashes && c == '/');
        let pieces: Vec<String> = genre
            .split(separator)
            .map(|g| g.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|g| !g.is_empty())
            .map(|g| {
                let named = self.names.get(&genre_key(&g)).cloned().unwrap_or(g);
                if self.capitalise { capitalise(&named) } else { named }
            })
            .collect();
        if pieces.is_empty() {
            return genre.to_string();
        }
        join_genres(pieces.iter().map(String::as_str))
    }
}

/// Upper-case the first letter of each word; words start after a space, `-`, `/` or `&`.
fn capitalise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if start && c.is_alphabetic() {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        start = c.is_whitespace() || matches!(c, '-' | '/' | '&' | '(');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn genres() -> Genres {
        Genres { capitalise: true, ..Default::default() }
    }

    #[test]
    fn tidies_and_dedupes() {
        let g = genres();
        assert_eq!(g.apply("hip hop;  rap ; Hip-Hop"), "Hip Hop; Rap");
        assert_eq!(g.apply("r&b"), "R&B");
        assert_eq!(g.apply("EDM"), "EDM");
        assert_eq!(g.apply("Hip-Hop/Rap"), "Hip-Hop/Rap", "/ only splits when asked");
        assert_eq!(g.apply(" ; "), " ; ", "nothing to standardise");
    }

    #[test]
    fn splits_when_asked() {
        let g = Genres { split_commas: true, split_slashes: true, ..genres() };
        assert_eq!(g.apply("Rap/Hip Hop, Trap"), "Rap; Hip Hop; Trap");
    }

    #[test]
    fn uses_the_library_spelling() {
        let mut g = genres();
        g.names.insert(genre_key("Hip-Hop"), "Hip-Hop".into());
        assert_eq!(g.apply("hip hop; HipHop; rap"), "Hip-Hop; Rap");
        g.capitalise = false;
        assert_eq!(g.apply("HIP HOP; jazz"), "Hip-Hop; jazz");
    }
}
