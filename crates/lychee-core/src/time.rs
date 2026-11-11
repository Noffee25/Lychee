use std::time::{SystemTime, UNIX_EPOCH};

/// "3:07", or "1:02:07" for anything an hour or longer.
pub fn format_duration(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

pub fn format_duration_ms(ms: u64) -> String {
    format_duration(ms / 1000)
}

pub fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(187), "3:07");
        assert_eq!(format_duration(3727), "1:02:07");
        assert_eq!(format_duration_ms(252_400), "4:12");
    }
}
