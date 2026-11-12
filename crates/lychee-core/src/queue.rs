use crate::{RepeatMode, TrackId};

/// One slot in the play queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueEntry {
    pub track: TrackId,
    /// Added with "Play next" / "Add to queue" instead of being part of the playing context.
    pub manual: bool,
    /// Insertion order, used to restore the original order when shuffle is turned off.
    seq: u64,
}

/// The play queue: what has played, what is playing, and what plays next.
///
/// The queue always stores tracks in their *actual* play order, so what the UI shows is exactly
/// what will happen. Shuffling reorders the upcoming part in place; turning shuffle off restores
/// the original order. Tracks added by the user ("Play next" / "Add to queue") sit right after the
/// current track, ahead of the rest of the context, and are never shuffled away.
#[derive(Clone, Debug, Default)]
pub struct Queue {
    entries: Vec<QueueEntry>,
    current: Option<usize>,
    shuffle: bool,
    repeat: RepeatMode,
    context: String,
    next_seq: u64,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[QueueEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current(&self) -> Option<TrackId> {
        self.current.and_then(|i| self.entries.get(i)).map(|e| e.track)
    }

    /// Name of what the queue was started from, e.g. an album title.
    pub fn context(&self) -> &str {
        &self.context
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    fn entry(&mut self, track: TrackId, manual: bool) -> QueueEntry {
        self.next_seq += 1;
        QueueEntry { track, manual, seq: self.next_seq }
    }

    fn upcoming_start(&self) -> usize {
        self.current.map_or(0, |c| c + 1)
    }

    /// Replace the queue with `tracks`, starting playback at `start`.
    pub fn set(&mut self, tracks: &[TrackId], start: usize, context: impl Into<String>) {
        self.entries.clear();
        self.context = context.into();
        if tracks.is_empty() {
            self.current = None;
            return;
        }
        let start = start.min(tracks.len() - 1);
        for &t in tracks {
            let e = self.entry(t, false);
            self.entries.push(e);
        }
        self.current = Some(start);
        if self.shuffle {
            // The chosen track plays first; everything else follows in random order.
            let chosen = self.entries.remove(start);
            self.entries.insert(0, chosen);
            self.current = Some(0);
            self.shuffle_upcoming();
        }
    }

    /// Insert tracks right after the current one (ahead of anything else queued by the user).
    pub fn play_next(&mut self, tracks: &[TrackId]) {
        let at = self.upcoming_start();
        for (i, &t) in tracks.iter().enumerate() {
            let e = self.entry(t, true);
            self.entries.insert(at + i, e);
        }
    }

    /// Append tracks to the end of the user-queued block that follows the current track.
    pub fn add(&mut self, tracks: &[TrackId]) {
        let mut at = self.upcoming_start();
        while at < self.entries.len() && self.entries[at].manual {
            at += 1;
        }
        for (i, &t) in tracks.iter().enumerate() {
            let e = self.entry(t, true);
            self.entries.insert(at + i, e);
        }
    }

    /// The track `advance(auto)` would move to, without changing anything. Used to pre-load the
    /// next file for gapless playback. Returns `None` when the next step can't be known in
    /// advance (for example when the queue will be reshuffled on wrap-around).
    pub fn peek_next(&self, auto: bool) -> Option<TrackId> {
        self.peek_next_index(auto).map(|i| self.entries[i].track)
    }

    /// Where in [`Queue::entries`] `peek_next` finds its track.
    pub fn peek_next_index(&self, auto: bool) -> Option<usize> {
        let cur = self.current?;
        if auto && self.repeat == RepeatMode::One {
            return Some(cur);
        }
        if cur + 1 < self.entries.len() {
            return Some(cur + 1);
        }
        if self.repeat != RepeatMode::Off && !self.shuffle && !self.entries.is_empty() {
            return Some(0);
        }
        None
    }

    /// Move to the next track. `auto` is true when the current track simply ended, false when the
    /// user pressed "next" (which skips even in repeat-one mode). Returns `None` at the end of the
    /// queue with repeat off; the current index is left unchanged in that case.
    pub fn advance(&mut self, auto: bool) -> Option<TrackId> {
        let cur = self.current?;
        if auto && self.repeat == RepeatMode::One {
            return self.current();
        }
        if cur + 1 < self.entries.len() {
            self.current = Some(cur + 1);
        } else if self.repeat != RepeatMode::Off {
            if self.shuffle && self.entries.len() > 2 {
                let last = self.entries[cur].track;
                fastrand::shuffle(&mut self.entries);
                // Avoid playing the same track twice in a row across the reshuffle.
                if self.entries[0].track == last {
                    let n = self.entries.len();
                    self.entries.swap(0, n - 1);
                }
            }
            self.current = Some(0);
        } else {
            return None;
        }
        self.current()
    }

    /// Step back one track (wrapping in repeat-all). The "restart the current track if more than
    /// a few seconds in" rule belongs to the player, not the queue.
    pub fn previous(&mut self) -> Option<TrackId> {
        let cur = self.current?;
        if cur > 0 {
            self.current = Some(cur - 1);
        } else if self.repeat == RepeatMode::All && !self.entries.is_empty() {
            self.current = Some(self.entries.len() - 1);
        }
        self.current()
    }

    pub fn jump(&mut self, index: usize) -> Option<TrackId> {
        if index < self.entries.len() {
            self.current = Some(index);
        }
        self.current()
    }

    pub fn remove(&mut self, index: usize) {
        if index >= self.entries.len() {
            return;
        }
        self.entries.remove(index);
        self.current = match self.current {
            _ if self.entries.is_empty() => None,
            Some(c) if index < c => Some(c - 1),
            Some(c) if index == c => Some(c.min(self.entries.len() - 1)),
            other => other,
        };
    }

    /// Move the entry at `from` so it ends up at index `to`.
    pub fn move_entry(&mut self, from: usize, to: usize) {
        let len = self.entries.len();
        if from >= len || to >= len || from == to {
            return;
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        if let Some(c) = self.current {
            self.current = Some(if c == from {
                to
            } else if from < c && to >= c {
                c - 1
            } else if from > c && to <= c {
                c + 1
            } else {
                c
            });
        }
    }

    /// Remove everything after the current track.
    pub fn clear_upcoming(&mut self) {
        let start = self.upcoming_start();
        self.entries.truncate(start);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
        self.context.clear();
    }

    /// Drop entries whose track no longer exists (e.g. after files were deleted).
    pub fn retain(&mut self, mut keep: impl FnMut(TrackId) -> bool) {
        let mut i = 0;
        while i < self.entries.len() {
            if keep(self.entries[i].track) { i += 1 } else { self.remove(i) }
        }
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if on {
            self.shuffle_upcoming();
        } else {
            // Restore insertion order for the context part; user-queued tracks keep their place.
            let start = self.first_context_index();
            self.entries[start..].sort_by_key(|e| e.seq);
        }
    }

    fn first_context_index(&self) -> usize {
        let mut i = self.upcoming_start();
        while i < self.entries.len() && self.entries[i].manual {
            i += 1;
        }
        i
    }

    fn shuffle_upcoming(&mut self) {
        let start = self.first_context_index();
        if start < self.entries.len() {
            fastrand::shuffle(&mut self.entries[start..]);
        }
    }

    /// Everything needed to restore the queue on the next start.
    pub fn snapshot(&self) -> (Vec<(TrackId, bool)>, Option<usize>) {
        (self.entries.iter().map(|e| (e.track, e.manual)).collect(), self.current)
    }

    pub fn restore(
        &mut self,
        entries: &[(TrackId, bool)],
        current: Option<usize>,
        shuffle: bool,
        repeat: RepeatMode,
        context: String,
    ) {
        self.entries.clear();
        for &(t, manual) in entries {
            let e = self.entry(t, manual);
            self.entries.push(e);
        }
        self.current = current.filter(|&c| c < self.entries.len());
        if self.current.is_none() && !self.entries.is_empty() {
            self.current = Some(0);
        }
        self.shuffle = shuffle;
        self.repeat = repeat;
        self.context = context;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(q: &Queue) -> Vec<TrackId> {
        q.entries().iter().map(|e| e.track).collect()
    }

    #[test]
    fn plays_through_and_stops_without_repeat() {
        let mut q = Queue::new();
        q.set(&[1, 2, 3], 0, "Album");
        assert_eq!(q.current(), Some(1));
        assert_eq!(q.advance(true), Some(2));
        assert_eq!(q.advance(true), Some(3));
        assert_eq!(q.advance(true), None);
        assert_eq!(q.current(), Some(3), "current stays on the last track");
    }

    #[test]
    fn repeat_modes() {
        let mut q = Queue::new();
        q.set(&[1, 2], 1, "Album");
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.peek_next(true), Some(1));
        assert_eq!(q.advance(true), Some(1));
        q.set_repeat(RepeatMode::One);
        assert_eq!(q.advance(true), Some(1), "repeat-one repeats on natural end");
        assert_eq!(q.advance(false), Some(2), "but the next button still skips");
    }

    #[test]
    fn previous_wraps_only_in_repeat_all() {
        let mut q = Queue::new();
        q.set(&[1, 2, 3], 0, "Album");
        assert_eq!(q.previous(), Some(1));
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.previous(), Some(3));
    }

    #[test]
    fn user_queued_tracks_play_first_in_order() {
        let mut q = Queue::new();
        q.set(&[1, 2, 3], 0, "Album");
        q.add(&[10]);
        q.add(&[11]);
        q.play_next(&[12]);
        assert_eq!(ids(&q), vec![1, 12, 10, 11, 2, 3]);
        assert!(q.entries()[1].manual && !q.entries()[4].manual);
    }

    #[test]
    fn shuffle_keeps_current_and_restores_order() {
        let mut q = Queue::new();
        let tracks: Vec<TrackId> = (1..=50).collect();
        q.set(&tracks, 4, "Album");
        q.add(&[100]);
        q.set_shuffle(true);
        assert_eq!(q.current(), Some(5));
        assert_eq!(q.entries()[5].track, 100, "user-queued track is not shuffled away");
        let mut sorted = ids(&q);
        sorted.sort_unstable();
        let mut expected: Vec<TrackId> = (1..=50).collect();
        expected.push(100);
        assert_eq!(sorted, expected, "shuffling never loses or duplicates tracks");
        q.set_shuffle(false);
        assert_eq!(&ids(&q)[6..], &(6..=50).collect::<Vec<_>>()[..]);
    }

    #[test]
    fn starting_with_shuffle_plays_the_chosen_track_first() {
        let mut q = Queue::new();
        q.set_shuffle(true);
        q.set(&[1, 2, 3, 4, 5], 3, "Album");
        assert_eq!(q.current(), Some(4));
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn remove_and_move_keep_current_pointing_at_the_same_track() {
        let mut q = Queue::new();
        q.set(&[1, 2, 3, 4], 2, "Album");
        q.remove(0);
        assert_eq!(q.current(), Some(3));
        q.move_entry(2, 0);
        assert_eq!(ids(&q), vec![4, 2, 3]);
        assert_eq!(q.current(), Some(3));
        q.move_entry(2, 0);
        assert_eq!(q.current(), Some(3));
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn retain_drops_missing_tracks() {
        let mut q = Queue::new();
        q.set(&[1, 2, 3], 1, "Album");
        q.retain(|t| t != 1);
        assert_eq!(ids(&q), vec![2, 3]);
        assert_eq!(q.current(), Some(2));
    }

    #[test]
    fn snapshot_round_trip() {
        let mut q = Queue::new();
        q.set(&[1, 2, 3], 1, "Album");
        q.add(&[9]);
        let (entries, current) = q.snapshot();
        let mut r = Queue::new();
        r.restore(&entries, current, false, RepeatMode::All, "Album".into());
        assert_eq!(ids(&r), ids(&q));
        assert_eq!(r.current(), Some(2));
        assert_eq!(r.repeat(), RepeatMode::All);
    }
}
