//! Field clean-up, after EasyTAG's "Process fields" scanner: case conversion, underscores and
//! `%20` to spaces, tidy whitespace, plain find-and-replace.

/// Case conversion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Case {
    #[default]
    Keep,
    /// "The Dark Side of the Moon": small words stay lower case, acronyms stay as they are.
    Title,
    /// "The dark side of the moon"
    Sentence,
    Upper,
    Lower,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cleanup {
    pub underscores_to_spaces: bool,
    pub percent20_to_spaces: bool,
    pub tidy_spaces: bool,
    pub case: Case,
    /// Replace every occurrence of the first string with the second.
    pub replace: Option<(String, String)>,
}

impl Cleanup {
    pub fn is_noop(&self) -> bool {
        *self == Cleanup::default() || (self.case == Case::Keep && !self.has_text_steps())
    }

    fn has_text_steps(&self) -> bool {
        self.underscores_to_spaces
            || self.percent20_to_spaces
            || self.tidy_spaces
            || self.replace.as_ref().is_some_and(|(from, _)| !from.is_empty())
    }

    pub fn apply(&self, text: &str) -> String {
        let mut s = text.to_string();
        if let Some((from, to)) = &self.replace {
            if !from.is_empty() {
                s = s.replace(from.as_str(), to);
            }
        }
        if self.percent20_to_spaces {
            s = s.replace("%20", " ");
        }
        if self.underscores_to_spaces {
            s = s.replace('_', " ");
        }
        if self.tidy_spaces {
            s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        }
        match self.case {
            Case::Keep => s,
            Case::Title => title_case(&s),
            Case::Sentence => sentence_case(&s),
            Case::Upper => s.to_uppercase(),
            Case::Lower => s.to_lowercase(),
        }
    }
}

const SMALL_WORDS: [&str; 25] = [
    "a", "an", "and", "as", "at", "but", "by", "de", "feat", "feat.", "for", "from", "ft.", "in", "into", "nor", "of", "on",
    "or", "the", "to", "vs", "vs.", "via", "with",
];

fn is_roman(word: &str) -> bool {
    // I to XXXIX; longer numerals are rare in titles and collide with words ("mix").
    let upper = word.to_ascii_uppercase();
    if upper.is_empty() || !upper.chars().all(|c| matches!(c, 'I' | 'V' | 'X')) {
        return false;
    }
    let tens = upper.chars().take_while(|c| *c == 'X').count();
    let rest = &upper[tens..];
    tens <= 3 && matches!(rest, "" | "I" | "II" | "III" | "IV" | "V" | "VI" | "VII" | "VIII" | "IX")
}

fn is_acronym(word: &str) -> bool {
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase())
}

/// Upper-case the first letter, lower-case the rest; keeps "McCartney"-style inner capitals.
fn capitalize(word: &str) -> String {
    let has_inner_upper = word.chars().skip(1).any(char::is_uppercase) && word.chars().any(char::is_lowercase);
    let mut out = String::with_capacity(word.len());
    let mut first = true;
    for c in word.chars() {
        if first && c.is_alphanumeric() {
            out.extend(c.to_uppercase());
            first = false;
        } else if has_inner_upper || first {
            out.push(c);
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// Words with their separators, so the original spacing survives.
fn words(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut gap = String::new();
    for c in text.chars() {
        if c.is_whitespace() {
            gap.push(c);
        } else {
            if !gap.is_empty() {
                out.push((std::mem::take(&mut word), std::mem::take(&mut gap)));
            }
            word.push(c);
        }
    }
    out.push((word, gap));
    out
}

pub fn title_case(text: &str) -> String {
    let all_caps = !text.chars().any(char::is_lowercase);
    let parts = words(text);
    let last = parts.len().saturating_sub(1);
    let mut out = String::with_capacity(text.len());
    let mut after_break = true;
    for (i, (word, gap)) in parts.iter().enumerate() {
        let core = word.trim_matches(|c: char| !c.is_alphanumeric());
        let lower = word.to_lowercase();
        let converted = if word.is_empty() {
            String::new()
        } else if is_roman(core) {
            word.to_uppercase()
        } else if !all_caps && is_acronym(word) {
            word.clone()
        } else if !after_break && i != last && SMALL_WORDS.contains(&lower.trim_start_matches(['(', '[', '"', '\''])) {
            lower
        } else {
            // "self-destruct" -> "Self-Destruct", "don't" -> "Don't"
            word.split('-').map(capitalize).collect::<Vec<_>>().join("-")
        };
        out.push_str(&converted);
        out.push_str(gap);
        after_break = word.ends_with([':', '-', '(', '[', '/', '.', '!', '?']) || word == "-" || word == "–";
    }
    out
}

pub fn sentence_case(text: &str) -> String {
    let all_caps = !text.chars().any(char::is_lowercase);
    let mut out = String::with_capacity(text.len());
    let mut start = true;
    for (word, gap) in words(text) {
        let core = word.trim_matches(|c: char| !c.is_alphanumeric());
        let converted = if is_roman(core) && core.len() > 1 || core == "I" || core == "i" {
            word.to_uppercase()
        } else if !all_caps && is_acronym(&word) {
            word.clone()
        } else if start {
            capitalize(&word.to_lowercase())
        } else {
            word.to_lowercase()
        };
        start = word.ends_with(['.', '!', '?', ':']) || (start && converted.chars().all(|c| !c.is_alphanumeric()));
        out.push_str(&converted);
        out.push_str(&gap);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_case_rules() {
        assert_eq!(title_case("the dark side of the moon"), "The Dark Side of the Moon");
        assert_eq!(title_case("WE WILL ROCK YOU"), "We Will Rock You");
        assert_eq!(title_case("live at the BBC"), "Live at the BBC");
        assert_eq!(title_case("rocky ii: the return of the man"), "Rocky II: The Return of the Man");
        assert_eq!(title_case("self-destruct don't stop"), "Self-Destruct Don't Stop");
        assert_eq!(title_case("paul mcCartney"), "Paul McCartney");
        assert_eq!(title_case("what you're waiting for"), "What You're Waiting For");
        assert_eq!(title_case("song (feat. someone)"), "Song (feat. Someone)");
        assert_eq!(title_case("a mix of things"), "A Mix of Things");
    }

    #[test]
    fn sentence_case_rules() {
        assert_eq!(sentence_case("THE DARK SIDE OF THE MOON"), "The dark side of the moon");
        assert_eq!(sentence_case("hello World. how are You"), "Hello world. How are you");
        assert_eq!(sentence_case("i know what i want"), "I know what I want");
        assert_eq!(sentence_case("live at the BBC"), "Live at the BBC");
    }

    #[test]
    fn cleanup_steps() {
        let c = Cleanup { underscores_to_spaces: true, percent20_to_spaces: true, tidy_spaces: true, ..Default::default() };
        assert_eq!(c.apply("  01_Some%20Song__name "), "01 Some Song name");
        let c = Cleanup { replace: Some((" feat ".into(), " feat. ".into())), case: Case::Upper, ..Default::default() };
        assert_eq!(c.apply("a feat b"), "A FEAT. B");
        assert!(Cleanup::default().is_noop());
        assert!(!c.is_noop());
    }

    #[test]
    fn spacing_is_kept() {
        assert_eq!(title_case("one  two\tthree"), "One  Two\tThree");
    }
}
