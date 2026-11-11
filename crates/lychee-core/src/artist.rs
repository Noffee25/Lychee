//! Telling the main artist apart from featured ones.

/// Words that introduce featured artists. "with" counts only inside brackets ("A (with B)"),
/// because plenty of band names contain it.
const FEATURE_WORDS: [&str; 5] = ["featuring", "feat.", "feat", "ft.", "ft"];

/// The main artist of an artist tag: the part before the first `;` (the separator taggers like
/// foobar2000 write between artists), cut before "feat." / "ft." / "featuring" (also in brackets).
/// `&`, `,` and `/` are left alone: they belong to names like "Simon & Garfunkel",
/// "Tyler, The Creator" and "AC/DC".
pub fn main_artist(artist: &str) -> &str {
    let artist = artist.trim();
    let artist = match artist.split_once(';') {
        Some((first, _)) if !first.trim().is_empty() => first.trim(),
        _ => artist,
    };
    for (i, _) in artist.match_indices(char::is_whitespace) {
        let rest = artist[i..].trim_start();
        let (bracketed, rest) = match rest.strip_prefix(['(', '[']) {
            Some(inner) => (true, inner.trim_start()),
            None => (false, rest),
        };
        let mut words = FEATURE_WORDS.iter().copied().chain(bracketed.then_some("with"));
        let found = words.any(|word| {
            let Some(head) = rest.get(..word.len()) else { return false };
            let after = &rest[word.len()..];
            head.eq_ignore_ascii_case(word)
                && (word.ends_with('.') || after.is_empty() || after.starts_with(char::is_whitespace))
        });
        let main = artist[..i].trim_end();
        if found && !main.is_empty() {
            return main;
        }
    }
    artist
}

#[cfg(test)]
mod tests {
    use super::main_artist;

    #[test]
    fn cuts_at_feature_words() {
        assert_eq!(main_artist("Daft Punk feat. Pharrell Williams"), "Daft Punk");
        assert_eq!(main_artist("Daft Punk Feat. Pharrell Williams"), "Daft Punk");
        assert_eq!(main_artist("Daft Punk ft. Pharrell Williams, Nile Rodgers"), "Daft Punk");
        assert_eq!(main_artist("Daft Punk ft Pharrell Williams"), "Daft Punk");
        assert_eq!(main_artist("Daft Punk featuring Pharrell Williams"), "Daft Punk");
        assert_eq!(main_artist("Daft Punk (feat. Pharrell Williams)"), "Daft Punk");
        assert_eq!(main_artist("Daft Punk [ft. Pharrell Williams]"), "Daft Punk");
        assert_eq!(main_artist("Daft Punk (with Pharrell Williams)"), "Daft Punk");
        assert_eq!(main_artist("Simon & Garfunkel feat. Someone"), "Simon & Garfunkel");
        assert_eq!(main_artist("  Björk feat. Thom Yorke "), "Björk");
        assert_eq!(main_artist("Daft Punk; Pharrell Williams"), "Daft Punk");
        assert_eq!(main_artist("Daft Punk;Pharrell Williams; Nile Rodgers"), "Daft Punk");
        assert_eq!(main_artist("Simon & Garfunkel; Someone"), "Simon & Garfunkel");
        assert_eq!(main_artist("Daft Punk feat. Pharrell Williams; Nile Rodgers"), "Daft Punk");
    }

    #[test]
    fn leaves_band_names_alone() {
        for name in [
            "Simon & Garfunkel",
            "Tyler, The Creator",
            "AC/DC",
            "Earth, Wind & Fire",
            "Nothing But Thieves",
            "Death with Dignity",
            "Fifth Harmony",
            "The Fts",
            "Feat",
            "feat. Someone",
        ] {
            assert_eq!(main_artist(name), name);
        }
    }
}
