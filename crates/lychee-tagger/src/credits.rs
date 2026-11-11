//! Standardising artist credits: "A ft. B", "A, B" and "A; B" all written one way.
//!
//! A credit is split into lead artists and featured ones (after "feat.", "ft.", "featuring", or
//! "with" in brackets). `;` always separates artists; `,`, `&` and `/` do when asked, because they
//! also appear inside names ("Earth, Wind & Fire", "AC/DC"). Names in [`Credits::keep`] are never
//! split.

use std::collections::HashSet;

/// How a standardised credit is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CreditStyle {
    /// "A feat. B & C": the first artist leads, everyone else is featured.
    #[default]
    FirstFeaturing,
    /// "A & B feat. C": who was featured stays featured; the rest are leads.
    KeepFeatured,
    /// "A, B & C": everyone listed equally.
    List,
    /// "A; B; C"
    Semicolons,
}

#[derive(Clone, Debug, Default)]
pub struct Credits {
    pub style: CreditStyle,
    pub split_commas: bool,
    pub split_ampersands: bool,
    pub split_slashes: bool,
    /// Names kept whole even though they contain a separator, lower-cased with single spaces
    /// ("simon & garfunkel").
    pub keep: HashSet<String>,
}

/// The artists of one credit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub leads: Vec<String>,
    pub featured: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sep {
    /// `;`, `,`, `&`, `/`: between artists of the same kind.
    Between,
    /// "feat." and friends: what follows is featured.
    Feature,
}

const FEATURE_WORDS: [&str; 5] = ["featuring", "feat.", "feat", "ft.", "ft"];

/// A name as it is compared with [`Credits::keep`].
pub fn keep_key(name: &str) -> String {
    tidy(name).to_lowercase()
}

impl Credits {
    /// Rewrite `artist` in the chosen style. Text with no artists in it is returned unchanged.
    pub fn apply(&self, artist: &str) -> String {
        let p = self.parse(artist);
        let all: Vec<&str> = p.leads.iter().chain(&p.featured).map(String::as_str).collect();
        if all.is_empty() {
            return artist.to_string();
        }
        match self.style {
            CreditStyle::FirstFeaturing => featuring(&all[..1], &all[1..]),
            CreditStyle::KeepFeatured => {
                let leads: Vec<&str> = p.leads.iter().map(String::as_str).collect();
                let featured: Vec<&str> = p.featured.iter().map(String::as_str).collect();
                if leads.is_empty() { featuring(&featured[..1], &featured[1..]) } else { featuring(&leads, &featured) }
            }
            CreditStyle::List => join_list(&all),
            CreditStyle::Semicolons => all.join("; "),
        }
    }

    pub fn parse(&self, artist: &str) -> Parsed {
        let s = artist.trim();
        // Pieces of `s` between separators, and the separator after each piece but the last.
        let mut pieces: Vec<(usize, usize)> = Vec::new();
        let mut seps: Vec<Sep> = Vec::new();
        let mut start = 0;
        let mut i = 0;
        while i < s.len() {
            if let Some((len, sep)) = self.separator_at(s, i) {
                pieces.push((start, i));
                seps.push(sep);
                i += len;
                start = i;
            } else {
                i += s[i..].chars().next().map_or(1, char::len_utf8);
            }
        }
        pieces.push((start, s.len()));

        let mut parsed = Parsed::default();
        let mut seen: HashSet<String> = HashSet::new();
        let mut featured = false;
        let mut a = 0;
        while a < pieces.len() {
            // The longest run of pieces that together make a name to keep whole ("Simon & Garfunkel").
            let mut end = a;
            for b in (a + 1..pieces.len()).rev() {
                if seps[a..b].contains(&Sep::Feature) {
                    continue;
                }
                if self.keep.contains(&keep_key(clean(&s[pieces[a].0..pieces[b].1]))) {
                    end = b;
                    break;
                }
            }
            let name = tidy(clean(&s[pieces[a].0..pieces[end].1]));
            if !name.is_empty() && seen.insert(name.to_lowercase()) {
                if featured { parsed.featured.push(name) } else { parsed.leads.push(name) }
            }
            if seps.get(end) == Some(&Sep::Feature) {
                featured = true;
            }
            a = end + 1;
        }
        parsed
    }

    /// The separator starting at byte `i` of `s`, with its length.
    fn separator_at(&self, s: &str, i: usize) -> Option<(usize, Sep)> {
        let c = s[i..].chars().next()?;
        match c {
            ';' => return Some((1, Sep::Between)),
            ',' if self.split_commas => return Some((1, Sep::Between)),
            '&' if self.split_ampersands => return Some((1, Sep::Between)),
            '/' if self.split_slashes => return Some((1, Sep::Between)),
            _ => {}
        }
        // A feature word needs something before it ("feat. B" alone is a name) and a gap or a
        // bracket in front: " feat. ", " (ft. ", ",feat ".
        if i == 0 || !(c.is_whitespace() || c == '(' || c == '[') {
            return None;
        }
        let rest = s[i..].trim_start();
        let (bracketed, rest) = match rest.strip_prefix(['(', '[']) {
            Some(inner) => (true, inner.trim_start()),
            None => (false, rest),
        };
        let mut words = FEATURE_WORDS.iter().copied().chain(bracketed.then_some("with"));
        let word = words.find(|w| {
            let Some(head) = rest.get(..w.len()) else { return false };
            let after = &rest[w.len()..];
            head.eq_ignore_ascii_case(w) && (w.ends_with('.') || after.is_empty() || after.starts_with(char::is_whitespace))
        })?;
        let after = &rest[word.len()..];
        let consumed = s.len() - i - after.trim_start().len();
        Some((consumed, Sep::Feature))
    }
}

/// "A", "A & B", "A, B & C"
fn join_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} & {last}", rest.join(", ")),
    }
}

fn featuring(leads: &[&str], featured: &[&str]) -> String {
    if featured.is_empty() { join_list(leads) } else { format!("{} feat. {}", join_list(leads), join_list(featured)) }
}

/// Trim a piece, and the closing bracket a "(feat. B)" leaves behind.
fn clean(piece: &str) -> &str {
    let p = piece.trim();
    let unbalanced = |open, close| p.ends_with(close) && !p.contains(open);
    if unbalanced('(', ')') || unbalanced('[', ']') { p[..p.len() - 1].trim_end() } else { p }
}

fn tidy(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credits(style: CreditStyle) -> Credits {
        Credits { style, split_commas: true, split_ampersands: true, split_slashes: false, keep: HashSet::new() }
    }

    #[test]
    fn parses_leads_and_featured_artists() {
        let c = credits(CreditStyle::FirstFeaturing);
        let p = |s| c.parse(s);
        let parsed = |leads: &[&str], featured: &[&str]| Parsed {
            leads: leads.iter().map(|s| s.to_string()).collect(),
            featured: featured.iter().map(|s| s.to_string()).collect(),
        };
        assert_eq!(p("Daft Punk ft. Pharrell Williams, Nile Rodgers"), parsed(&["Daft Punk"], &["Pharrell Williams", "Nile Rodgers"]));
        assert_eq!(p("Daft Punk (feat. Pharrell Williams)"), parsed(&["Daft Punk"], &["Pharrell Williams"]));
        assert_eq!(p("Daft Punk [Featuring Pharrell Williams & Nile Rodgers]"), parsed(&["Daft Punk"], &["Pharrell Williams", "Nile Rodgers"]));
        assert_eq!(p("Daft Punk & Pharrell Williams feat. Nile Rodgers"), parsed(&["Daft Punk", "Pharrell Williams"], &["Nile Rodgers"]));
        assert_eq!(p("Daft Punk;Pharrell Williams ; "), parsed(&["Daft Punk", "Pharrell Williams"], &[]));
        assert_eq!(p("Daft Punk, feat. Pharrell Williams"), parsed(&["Daft Punk"], &["Pharrell Williams"]));
        assert_eq!(p("Daft Punk (with Pharrell Williams)"), parsed(&["Daft Punk"], &["Pharrell Williams"]));
        assert_eq!(p("Daft  Punk, daft punk"), parsed(&["Daft Punk"], &[]), "duplicates and double spaces go");
        assert_eq!(p("AC/DC"), parsed(&["AC/DC"], &[]), "/ only splits when asked");
        assert_eq!(p("Nothing But Thieves"), parsed(&["Nothing But Thieves"], &[]));
    }

    #[test]
    fn writes_each_style() {
        let input = "Daft Punk & Pharrell Williams ft. Nile Rodgers";
        assert_eq!(credits(CreditStyle::FirstFeaturing).apply(input), "Daft Punk feat. Pharrell Williams & Nile Rodgers");
        assert_eq!(credits(CreditStyle::KeepFeatured).apply(input), "Daft Punk & Pharrell Williams feat. Nile Rodgers");
        assert_eq!(credits(CreditStyle::List).apply(input), "Daft Punk, Pharrell Williams & Nile Rodgers");
        assert_eq!(credits(CreditStyle::Semicolons).apply(input), "Daft Punk; Pharrell Williams; Nile Rodgers");
        assert_eq!(credits(CreditStyle::FirstFeaturing).apply("Daft Punk, Pharrell Williams"), "Daft Punk feat. Pharrell Williams");
        assert_eq!(credits(CreditStyle::KeepFeatured).apply("Daft Punk, Pharrell Williams"), "Daft Punk & Pharrell Williams");
        assert_eq!(credits(CreditStyle::FirstFeaturing).apply("Daft Punk"), "Daft Punk");
        assert_eq!(credits(CreditStyle::FirstFeaturing).apply(""), "");
        assert_eq!(credits(CreditStyle::FirstFeaturing).apply(" , "), " , ");
    }

    #[test]
    fn kept_names_stay_whole() {
        let mut c = credits(CreditStyle::FirstFeaturing);
        c.keep = ["Simon & Garfunkel", "Earth, Wind & Fire"].iter().map(|n| keep_key(n)).collect();
        assert_eq!(c.apply("Simon & Garfunkel"), "Simon & Garfunkel");
        assert_eq!(c.apply("simon  & garfunkel, Earth, Wind & Fire"), "simon & garfunkel feat. Earth, Wind & Fire");
        assert_eq!(c.apply("Earth, Wind & Fire ft. Someone & Other"), "Earth, Wind & Fire feat. Someone & Other");
        assert_eq!(credits(CreditStyle::FirstFeaturing).apply("Simon & Garfunkel"), "Simon feat. Garfunkel", "without the list");
    }
}
