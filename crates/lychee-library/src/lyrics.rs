//! Lyrics: sidecar `.lrc` files and embedded lyrics, synced or plain.

use std::path::Path;

use lychee_core::{LyricLine, Lyrics};

/// Find lyrics for an audio file: `song.lrc` next to `song.flac` first, then embedded tags.
pub fn load(audio_path: &Path) -> Option<Lyrics> {
    let lrc_path = audio_path.with_extension("lrc");
    if let Ok(text) = std::fs::read_to_string(&lrc_path) {
        let name = lrc_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(lyrics) = parse(&text, name) {
            return Some(lyrics);
        }
    }
    crate::tags::embedded_lyrics(audio_path).and_then(|text| parse(&text, "Embedded in file".into()))
}

/// Parse LRC (`[mm:ss.xx]line`, several timestamps per line, `[offset:±ms]`) or plain text.
pub fn parse(text: &str, source: String) -> Option<Lyrics> {
    let mut offset_ms: i64 = 0;
    let mut timed: Vec<(i64, String)> = Vec::new();
    let mut plain: Vec<String> = Vec::new();

    for raw in text.lines() {
        let mut line = raw.trim();
        let mut stamps = Vec::new();
        while let Some(rest) = line.strip_prefix('[') {
            let Some(end) = rest.find(']') else { break };
            let inside = &rest[..end];
            if let Some(ms) = parse_timestamp(inside) {
                stamps.push(ms);
            } else if let Some(value) = inside.strip_prefix("offset:") {
                offset_ms = value.trim().parse().unwrap_or(0);
            } else if !stamps.is_empty() {
                break;
            }
            // Other ID tags ([ar:], [ti:], …) are skipped.
            line = rest[end + 1..].trim_start();
        }
        if stamps.is_empty() {
            if !raw.trim_start().starts_with('[') {
                plain.push(raw.trim().to_string());
            }
        } else {
            for ms in stamps {
                timed.push((ms, line.to_string()));
            }
        }
    }

    if !timed.is_empty() {
        timed.sort_by_key(|(ms, _)| *ms);
        // A positive offset means the lyrics should appear earlier.
        let lines = timed
            .into_iter()
            .map(|(ms, text)| LyricLine { time_ms: Some((ms - offset_ms).max(0) as u64), text })
            .collect();
        return Some(Lyrics { lines, synced: true, source });
    }
    while plain.last().is_some_and(|l| l.is_empty()) {
        plain.pop();
    }
    while plain.first().is_some_and(|l| l.is_empty()) {
        plain.remove(0);
    }
    if plain.is_empty() {
        return None;
    }
    Some(Lyrics { lines: plain.into_iter().map(|text| LyricLine { time_ms: None, text }).collect(), synced: false, source })
}

fn parse_timestamp(s: &str) -> Option<i64> {
    let (min, rest) = s.split_once(':')?;
    let min: i64 = min.trim().parse().ok()?;
    let (sec, frac) = match rest.split_once(['.', ':']) {
        Some((sec, frac)) => (sec, frac),
        None => (rest, "0"),
    };
    let sec: i64 = sec.trim().parse().ok()?;
    let frac_digits = frac.trim();
    if !frac_digits.chars().all(|c| c.is_ascii_digit()) || frac_digits.len() > 3 {
        return None;
    }
    let frac_ms = match frac_digits.len() {
        0 => 0,
        1 => frac_digits.parse::<i64>().ok()? * 100,
        2 => frac_digits.parse::<i64>().ok()? * 10,
        _ => frac_digits.parse::<i64>().ok()?,
    };
    Some(min * 60_000 + sec * 1000 + frac_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synced_lrc_with_multiple_stamps_and_offset() {
        let text = "[ar:Someone]\n[offset:500]\n[00:12.00]First line\n[00:20.50][01:02.25]Chorus\n\n";
        let lyrics = parse(text, "x.lrc".into()).unwrap();
        assert!(lyrics.synced);
        let times: Vec<_> = lyrics.lines.iter().map(|l| l.time_ms.unwrap()).collect();
        assert_eq!(times, vec![11_500, 20_000, 61_750]);
        assert_eq!(lyrics.lines[1].text, "Chorus");
    }

    #[test]
    fn plain_text_is_unsynced() {
        let lyrics = parse("\nHello\nWorld\n", "Embedded".into()).unwrap();
        assert!(!lyrics.synced);
        assert_eq!(lyrics.lines.len(), 2);
    }

    #[test]
    fn empty_text_is_no_lyrics() {
        assert!(parse("  \n\n", "x".into()).is_none());
    }
}
