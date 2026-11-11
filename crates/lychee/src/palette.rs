//! The Ctrl+K command palette: fuzzy search over the library plus app commands.

use lychee_core::{AlbumId, PlaylistId, TrackId};
use lychee_library::LibraryIndex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
    Shuffle,
    Repeat,
    ClearQueue,
    Mini,
    Lyrics,
    SidePanel,
    Theme,
    Albums,
    Artists,
    Songs,
    Favorites,
    Settings,
    AddFolder,
    Rescan,
    Lastfm,
    TagEditor,
}

pub struct CommandInfo {
    pub command: Command,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub hint: &'static str,
    pub icon: &'static str,
}

pub const COMMANDS: &[CommandInfo] = &[
    CommandInfo { command: Command::PlayPause, title: "Play / pause", subtitle: "Playback", hint: "Space", icon: "play" },
    CommandInfo { command: Command::Next, title: "Next track", subtitle: "Playback", hint: "Ctrl →", icon: "next" },
    CommandInfo { command: Command::Previous, title: "Previous track", subtitle: "Playback", hint: "Ctrl ←", icon: "prev" },
    CommandInfo { command: Command::Shuffle, title: "Toggle shuffle", subtitle: "Playback", hint: "", icon: "shuffle" },
    CommandInfo { command: Command::Repeat, title: "Cycle repeat (off / all / one)", subtitle: "Playback", hint: "", icon: "repeat" },
    CommandInfo { command: Command::ClearQueue, title: "Clear the queue", subtitle: "Removes everything after the current song", hint: "", icon: "trash" },
    CommandInfo { command: Command::Mini, title: "Open mini player", subtitle: "Compact, always-on-top window", hint: "Ctrl M", icon: "mini" },
    CommandInfo { command: Command::Lyrics, title: "Show lyrics", subtitle: "Side panel", hint: "", icon: "lyrics" },
    CommandInfo { command: Command::SidePanel, title: "Toggle the side panel", subtitle: "Queue, lyrics and details", hint: "Ctrl Q", icon: "sidebar" },
    CommandInfo { command: Command::Theme, title: "Switch light / dark theme", subtitle: "Appearance", hint: "", icon: "moon" },
    CommandInfo { command: Command::Albums, title: "Go to Albums", subtitle: "Library", hint: "", icon: "disc" },
    CommandInfo { command: Command::Artists, title: "Go to Artists", subtitle: "Library", hint: "", icon: "user" },
    CommandInfo { command: Command::Songs, title: "Go to Songs", subtitle: "Library", hint: "", icon: "music" },
    CommandInfo { command: Command::Favorites, title: "Go to Favorites", subtitle: "Library", hint: "", icon: "heart" },
    CommandInfo { command: Command::Settings, title: "Open Settings", subtitle: "Preferences", hint: "Ctrl ,", icon: "settings" },
    CommandInfo { command: Command::AddFolder, title: "Add music folder…", subtitle: "Library", hint: "", icon: "folder-plus" },
    CommandInfo { command: Command::Rescan, title: "Rescan library", subtitle: "Pick up new and changed files", hint: "", icon: "refresh" },
    CommandInfo { command: Command::Lastfm, title: "Connect Last.fm", subtitle: "Scrobbling", hint: "", icon: "scrobble" },
    CommandInfo { command: Command::TagEditor, title: "Open the tag editor", subtitle: "Edit tags, artwork and lyrics; look albums up online", hint: "", icon: "edit" },
];

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Album(AlbumId),
    Artist(String),
    Track(TrackId),
    Playlist(PlaylistId),
    Command(Command),
}

/// Results in display order, each with the section label to show above it (or "").
pub fn search(index: &LibraryIndex, query: &str) -> Vec<(&'static str, Entry)> {
    let q = query.trim();
    let mut out: Vec<(&'static str, Entry)> = Vec::new();
    let push_section = |label: &'static str, entries: Vec<Entry>, out: &mut Vec<(&'static str, Entry)>| {
        for (i, e) in entries.into_iter().enumerate() {
            out.push((if i == 0 { label } else { "" }, e));
        }
    };

    if let Some(cmd_query) = q.strip_prefix('>') {
        let cmds = matching_commands(cmd_query.trim(), 12);
        push_section("Commands", cmds, &mut out);
        return out;
    }
    if q.is_empty() {
        let cmds = COMMANDS.iter().take(6).map(|c| Entry::Command(c.command)).collect();
        push_section("Suggested", cmds, &mut out);
        return out;
    }

    let results = index.search(q, 4);
    push_section("Albums", results.albums.into_iter().map(Entry::Album).collect(), &mut out);
    push_section("Artists", results.artists.into_iter().take(3).map(Entry::Artist).collect(), &mut out);
    push_section("Songs", results.tracks.into_iter().take(5).map(Entry::Track).collect(), &mut out);
    push_section("Playlists", results.playlists.into_iter().take(3).map(Entry::Playlist).collect(), &mut out);
    push_section("Commands", matching_commands(q, 3), &mut out);
    out
}

fn matching_commands(q: &str, limit: usize) -> Vec<Entry> {
    let q = q.to_lowercase();
    COMMANDS
        .iter()
        .filter(|c| q.is_empty() || c.title.to_lowercase().contains(&q) || c.subtitle.to_lowercase().contains(&q))
        .take(limit)
        .map(|c| Entry::Command(c.command))
        .collect()
}

pub fn info(command: Command) -> &'static CommandInfo {
    COMMANDS.iter().find(|c| c.command == command).expect("every command is listed")
}
