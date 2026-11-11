//! EasyTAG's scanner masks.
//!
//! A mask mixes literal text with codes such as `%n` (track number) or `%t` (title). It reads
//! tags out of file names ("01 - Intro.flac" with `%n - %t`) and builds new file names from tags.
//! A `/` in a mask stands for a folder level, so `%a/%b/%n - %t` also reads the two folders
//! above the file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::{Field, TagData};

/// The codes EasyTAG understands, with what they stand for.
pub const CODES: [(char, &str); 17] = [
    ('a', "Artist"),
    ('z', "Album artist"),
    ('b', "Album"),
    ('t', "Title"),
    ('n', "Track number"),
    ('l', "Number of tracks"),
    ('d', "Disc number"),
    ('x', "Number of discs"),
    ('y', "Year"),
    ('g', "Genre"),
    ('p', "Composer"),
    ('o', "Original artist"),
    ('c', "Comment"),
    ('r', "Copyright"),
    ('u', "URL"),
    ('e', "Encoded by"),
    ('i', "Ignored"),
];

fn code_field(c: char) -> Option<Field> {
    Some(match c {
        'a' => Field::Artist,
        'z' => Field::AlbumArtist,
        'b' => Field::Album,
        't' => Field::Title,
        'n' => Field::Track,
        'l' => Field::TrackTotal,
        'd' => Field::Disc,
        'x' => Field::DiscTotal,
        'y' => Field::Date,
        'g' => Field::Genre,
        'p' => Field::Composer,
        'o' => Field::OriginalArtist,
        'c' => Field::Comment,
        'r' => Field::Copyright,
        'u' => Field::Url,
        'e' => Field::EncodedBy,
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Text(String),
    /// A field, or `None` for `%i` (matches anything, keeps nothing).
    Code(char, Option<Field>),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MaskError {
    #[error("the mask is empty")]
    Empty,
    #[error("“%{0}” isn't a known code")]
    UnknownCode(char),
    #[error("a % at the end needs a code after it")]
    Dangling,
    #[error("two codes need some text between them, like “%a - %t”")]
    Adjacent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    tokens: Vec<Token>,
}

impl Mask {
    pub fn parse(text: &str) -> Result<Mask, MaskError> {
        let mut tokens = Vec::new();
        let mut literal = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                literal.push(c);
                continue;
            }
            match chars.next() {
                None => return Err(MaskError::Dangling),
                Some('%') => literal.push('%'),
                Some(code) => {
                    let code = code.to_ascii_lowercase();
                    if code != 'i' && code_field(code).is_none() {
                        return Err(MaskError::UnknownCode(code));
                    }
                    if !literal.is_empty() {
                        tokens.push(Token::Text(std::mem::take(&mut literal)));
                    } else if matches!(tokens.last(), Some(Token::Code(..))) {
                        return Err(MaskError::Adjacent);
                    }
                    tokens.push(Token::Code(code, code_field(code)));
                }
            }
        }
        if !literal.is_empty() {
            tokens.push(Token::Text(literal));
        }
        if tokens.is_empty() {
            return Err(MaskError::Empty);
        }
        Ok(Mask { tokens })
    }

    /// Folder levels the mask spans above the file name.
    fn depth(&self) -> usize {
        self.tokens.iter().map(|t| if let Token::Text(s) = t { s.matches('/').count() } else { 0 }).sum()
    }

    /// The fields used by the mask.
    pub fn fields(&self) -> Vec<Field> {
        self.tokens.iter().filter_map(|t| if let Token::Code(_, f) = t { *f } else { None }).collect()
    }

    /// Read field values out of a path. `None` if the path doesn't fit the mask.
    pub fn extract(&self, path: &Path) -> Option<Vec<(Field, String)>> {
        let stem = path.file_stem()?.to_string_lossy().into_owned();
        let mut parts = vec![stem];
        let mut dir = path.parent();
        for _ in 0..self.depth() {
            let d = dir?;
            parts.push(d.file_name()?.to_string_lossy().into_owned());
            dir = d.parent();
        }
        parts.reverse();
        let text = parts.join("/");

        let mut captures = Vec::new();
        let mut failed = HashSet::new();
        if !match_tokens(&self.tokens, &text, 0, &mut captures, &mut failed) {
            return None;
        }
        Some(
            captures
                .into_iter()
                .filter_map(|(field, value)| {
                    let field = field?;
                    let value = value.trim();
                    let value = if field.is_number() { value.trim_start_matches('0') } else { value };
                    let value = if field.is_number() && value.is_empty() { "0" } else { value };
                    Some((field, value.to_string()))
                })
                .collect(),
        )
    }

    /// Build a file name (without extension; may contain `/` for folders) from tags.
    /// `Err` lists the fields that are empty.
    pub fn format(&self, tags: &TagData, track_digits: usize) -> Result<String, Vec<Field>> {
        let mut out = String::new();
        let mut missing = Vec::new();
        for token in &self.tokens {
            match token {
                Token::Text(s) => out.push_str(s),
                Token::Code(_, None) => {}
                Token::Code(_, Some(field)) => {
                    let value = tags.get(*field).trim();
                    if value.is_empty() {
                        missing.push(*field);
                        continue;
                    }
                    let value = match field {
                        Field::Track | Field::TrackTotal => pad(value, track_digits),
                        Field::Date => value.chars().take(4).collect(),
                        _ => value.to_string(),
                    };
                    out.push_str(&sanitize(&value));
                }
            }
        }
        if !missing.is_empty() {
            return Err(missing);
        }
        let cleaned: Vec<String> = out.split('/').map(clean_component).filter(|c| !c.is_empty()).collect();
        if cleaned.is_empty() {
            return Err(self.fields());
        }
        Ok(cleaned.join("/"))
    }
}

/// Where a file goes when renamed with `mask`: relative to its current folder, keeping the
/// extension.
pub fn renamed_path(path: &Path, relative: &str) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut target = dir.to_path_buf();
    for part in relative.split('/') {
        target.push(part);
    }
    match path.extension() {
        Some(ext) => {
            let mut name = target.file_name().map(|n| n.to_os_string()).unwrap_or_default();
            name.push(".");
            name.push(ext);
            target.set_file_name(name);
            target
        }
        None => target,
    }
}

fn pad(value: &str, digits: usize) -> String {
    let number = value.split('/').next().unwrap_or(value).trim();
    match number.parse::<u32>() {
        Ok(n) => format!("{n:0digits$}"),
        Err(_) => number.to_string(),
    }
}

/// Characters that can't appear in file names on Windows (or anywhere, for `/`).
fn sanitize(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '/' | '\\' | ':' | '|' => out.push('-'),
            '"' => out.push('\''),
            '*' | '?' | '<' | '>' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Tidy one path component: no runs of spaces, nothing Windows rejects at the end.
fn clean_component(part: &str) -> String {
    let collapsed = part.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.trim_end_matches(['.', ' ']).trim_start().to_string()
}

/// Backtracking match. Codes take as little text as possible; a failed (token, offset) pair is
/// remembered so ambiguous masks stay fast.
fn match_tokens<'t>(
    tokens: &[Token],
    text: &'t str,
    offset: usize,
    out: &mut Vec<(Option<Field>, &'t str)>,
    failed: &mut HashSet<(usize, usize)>,
) -> bool {
    let rest = &text[offset..];
    let Some((first, tail)) = tokens.split_first() else { return rest.is_empty() };
    let state = (tokens.len(), offset);
    if failed.contains(&state) {
        return false;
    }
    let ok = match first {
        Token::Text(lit) => rest.starts_with(lit.as_str()) && match_tokens(tail, text, offset + lit.len(), out, failed),
        Token::Code(_, field) => {
            let numeric = field.is_some_and(Field::is_number) || *field == Some(Field::Date);
            let mut found = false;
            let ends = rest.char_indices().map(|(i, _)| i).skip(1).chain(std::iter::once(rest.len()));
            for end in ends {
                let capture = &rest[..end];
                if capture.contains('/') {
                    break;
                }
                if numeric && !capture.trim().chars().all(|c| c.is_ascii_digit()) {
                    if capture.trim().is_empty() {
                        continue;
                    }
                    break;
                }
                out.push((*field, capture));
                if match_tokens(tail, text, offset + end, out, failed) {
                    found = true;
                    break;
                }
                out.pop();
            }
            found
        }
    };
    if !ok {
        failed.insert(state);
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(mask: &str, path: &str) -> Option<Vec<(Field, String)>> {
        Mask::parse(mask).unwrap().extract(Path::new(path))
    }

    #[test]
    fn extracts_simple_masks() {
        let r = get("%n - %t", "/music/01 - Intro - Live.flac").unwrap();
        assert_eq!(r, vec![(Field::Track, "1".into()), (Field::Title, "Intro - Live".into())]);
        let r = get("%a - %t", "/x/AC-DC - Back in Black.mp3").unwrap();
        assert_eq!(r, vec![(Field::Artist, "AC-DC".into()), (Field::Title, "Back in Black".into())]);
    }

    #[test]
    fn extracts_folders() {
        let r = get("%a/%b (%y)/%n. %t", "/m/Massive Attack/Mezzanine (1998)/03. Teardrop.flac").unwrap();
        assert_eq!(
            r,
            vec![
                (Field::Artist, "Massive Attack".into()),
                (Field::Album, "Mezzanine".into()),
                (Field::Date, "1998".into()),
                (Field::Track, "3".into()),
                (Field::Title, "Teardrop".into()),
            ]
        );
    }

    #[test]
    fn numbers_must_be_digits_and_ignored_parts_vanish() {
        assert!(get("%n - %t", "/m/Intro - Song.flac").is_none());
        let r = get("%i - %n - %t", "/m/Some Band - 07 - Song.flac").unwrap();
        assert_eq!(r, vec![(Field::Track, "7".into()), (Field::Title, "Song".into())]);
        assert!(get("%a/%t", "/onlyfile.flac").is_none() || get("%a/%t", "onlyfile.flac").is_none());
    }

    #[test]
    fn parse_errors() {
        assert_eq!(Mask::parse("%q"), Err(MaskError::UnknownCode('q')));
        assert_eq!(Mask::parse("%a%t"), Err(MaskError::Adjacent));
        assert_eq!(Mask::parse("abc %"), Err(MaskError::Dangling));
        assert_eq!(Mask::parse(""), Err(MaskError::Empty));
        assert!(Mask::parse("100%% %t").is_ok());
    }

    #[test]
    fn ambiguous_masks_stay_fast() {
        let mask = Mask::parse("%a-%b-%c-%t-%g-%p-%o").unwrap();
        let name = format!("/m/{}.flac", "x".repeat(300));
        let start = std::time::Instant::now();
        assert!(mask.extract(Path::new(&name)).is_none());
        assert!(start.elapsed().as_secs() < 2);
    }

    #[test]
    fn formats_names() {
        let mut tags = TagData::default();
        tags.set(Field::Track, "3");
        tags.set(Field::Title, "What? Why: Who/Where");
        tags.set(Field::Artist, "AC/DC");
        tags.set(Field::Date, "1980-07-25");
        let mask = Mask::parse("%n. %a - %t (%y)").unwrap();
        assert_eq!(mask.format(&tags, 2).unwrap(), "03. AC-DC - What Why- Who-Where (1980)");
        let missing = Mask::parse("%n - %b").unwrap().format(&tags, 2).unwrap_err();
        assert_eq!(missing, vec![Field::Album]);
        let nested = Mask::parse("%a/%n %t").unwrap().format(&tags, 1).unwrap();
        assert_eq!(nested, "AC-DC/3 What Why- Who-Where");
    }

    #[test]
    fn renamed_paths_keep_the_extension() {
        let p = renamed_path(Path::new("/m/a/old.flac"), "03 - New");
        assert_eq!(p, PathBuf::from("/m/a/03 - New.flac"));
        let p = renamed_path(Path::new("/m/a/old.flac"), "Artist/03 - New");
        assert_eq!(p, PathBuf::from("/m/a/Artist/03 - New.flac"));
    }
}
