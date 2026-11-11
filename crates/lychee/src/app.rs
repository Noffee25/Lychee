//! The controller. It owns the services and the play queue, reacts to UI actions and service
//! events, and keeps the Slint models up to date.
//!
//! Every UI callback becomes an [`Action`] that goes through the [`Dispatcher`], so callbacks
//! that fire while the app is busy (re-entrancy) are queued instead of causing borrow panics.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use lychee_audio::{Engine, EngineEvent, StreamInfo};
use lychee_core::{
    Album, AlbumId, AudioFormat, Lyrics, PlaybackStatus, PlaylistId, Queue, RepeatMode, TrackId, TrackRef, format_duration,
    format_duration_ms,
};
use lychee_lastfm::{Scrobble, Scrobbler};
use lychee_library::{AlbumSort, Library, LibraryIndex, ScanProgress, ScanSummary, TrackSort};
use lychee_platform::{MediaAction, MediaControls, NowPlaying as OsNowPlaying, Playback, Repeat};
use slint::{Color, ComponentHandle, Image, ModelRc, SharedString, VecModel};

use crate::editor::{EditorAction, EditorCtl};
use crate::images::{ImageCache, Size};
use crate::lastfm::{LastfmMsg, Listen, ListenEvent, start_auth, track_info};
use crate::mini::{self, MiniCommand, MiniEvent, MiniState};
use crate::models::LazyModel;
use crate::palette::{self, Command, Entry};
use crate::settings::{Paths, Settings, ThemeMode};
use crate::{
    Actions, AlbumCard, AlbumHeader, AlbumRow, AppState, AppWindow, ArtistCard, Choice, GenreCard, GenreRow, Icons, InfoRow,
    LastfmUi, ListHeader, Lyric, MiniWindow, NavPlaylist, NowPlaying, PaletteItem, Player, QueueItem, ScanUi, Theme, TrackRow,
};

const DEFAULT_ACCENT: u32 = 0xFF6B7D;
const QUEUE_VISIBLE: usize = 120;

#[derive(Debug)]
pub enum Action {
    Tick,
    Navigate(String),
    GoBack,
    OpenAlbum(i32),
    OpenArtist(String),
    OpenGenre(String),
    OpenPlaylist(i32),
    PlayAlbum(i32, bool),
    AlbumAction(String, i32),
    PlayFrom(String, i32),
    PlayList(bool),
    TrackAction(String, i32),
    AddToPlaylist(i32, String, i32),
    TogglePlay,
    Next,
    Previous,
    Seek(f32),
    SeekBy(i32),
    SetVolume(f32),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    ToggleLikeCurrent,
    QueueJump(i32),
    QueueRemove(i32),
    QueueMove(i32, i32),
    QueueClear,
    LyricSeek(i32),
    SetGridColumns(i32),
    SetAlbumSort(i32),
    SetTrackSort(i32),
    OpenPalette,
    PaletteQuery(String),
    PaletteActivate(i32, bool),
    AddFolder,
    RemoveFolder(String),
    Rescan,
    SetDevice(String),
    SetThemeMode(i32),
    SetDynamicAccent(bool),
    SetMiniOnTop(bool),
    LastfmConnect,
    LastfmCancel,
    LastfmDisconnect,
    LastfmRetry,
    LastfmSetKeys(String, String),
    LastfmSetLoveSync(bool),
    LastfmOpenProfile,
    CreatePlaylist(String),
    RenamePlaylist(i32, String),
    DeletePlaylist(i32),
    OpenMini,
    CloseMini,
    RaiseMain,
    StartWindowDrag,
    OpenUrl(String),
    OpenPaths(Vec<PathBuf>),
    SystemThemeChanged(bool),
    MainWindowClosed,
    Editor(EditorAction),
    QuitSave,
    QuitDiscard,
}

/// Serialises actions into the app, tolerating re-entrant callbacks.
#[derive(Default)]
pub struct Dispatcher {
    queue: RefCell<VecDeque<Action>>,
    app: RefCell<Option<App>>,
}

impl Dispatcher {
    pub fn new() -> Rc<Dispatcher> {
        Rc::new(Dispatcher::default())
    }

    pub fn set_app(&self, app: App) {
        *self.app.borrow_mut() = Some(app);
        self.drain();
    }

    pub fn dispatch(&self, action: Action) {
        if !matches!(action, Action::Tick) {
            tracing::debug!("action: {action:?}");
        }
        self.queue.borrow_mut().push_back(action);
        self.drain();
    }

    fn drain(&self) {
        let Ok(mut guard) = self.app.try_borrow_mut() else { return };
        let Some(app) = guard.as_mut() else { return };
        loop {
            let next = self.queue.borrow_mut().pop_front();
            match next {
                Some(action) => app.handle(action),
                None => break,
            }
        }
    }

    pub fn with_app<R>(&self, f: impl FnOnce(&mut App) -> R) -> Option<R> {
        let mut guard = self.app.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    }
}

/// Connect every `Actions` callback of a window to the dispatcher.
macro_rules! wire_actions {
    ($actions:expr, $d:expr) => {{
        let a = $actions;
        let d = $d.clone();
        a.on_navigate({ let d = d.clone(); move |p| d.dispatch(Action::Navigate(p.into())) });
        a.on_go_back({ let d = d.clone(); move || d.dispatch(Action::GoBack) });
        a.on_open_album({ let d = d.clone(); move |id| d.dispatch(Action::OpenAlbum(id)) });
        a.on_open_artist({ let d = d.clone(); move |n| d.dispatch(Action::OpenArtist(n.into())) });
        a.on_open_genre({ let d = d.clone(); move |n| d.dispatch(Action::OpenGenre(n.into())) });
        a.on_open_playlist({ let d = d.clone(); move |id| d.dispatch(Action::OpenPlaylist(id)) });
        a.on_play_album({ let d = d.clone(); move |id, s| d.dispatch(Action::PlayAlbum(id, s)) });
        a.on_album_action({ let d = d.clone(); move |what, id| d.dispatch(Action::AlbumAction(what.into(), id)) });
        a.on_play_from({ let d = d.clone(); move |ctx, i| d.dispatch(Action::PlayFrom(ctx.into(), i)) });
        a.on_play_list({ let d = d.clone(); move |s| d.dispatch(Action::PlayList(s)) });
        a.on_track_action({ let d = d.clone(); move |what, id| d.dispatch(Action::TrackAction(what.into(), id)) });
        a.on_add_to_playlist({ let d = d.clone(); move |p, what, id| d.dispatch(Action::AddToPlaylist(p, what.into(), id)) });
        a.on_toggle_play({ let d = d.clone(); move || d.dispatch(Action::TogglePlay) });
        a.on_next({ let d = d.clone(); move || d.dispatch(Action::Next) });
        a.on_previous({ let d = d.clone(); move || d.dispatch(Action::Previous) });
        a.on_seek({ let d = d.clone(); move |f| d.dispatch(Action::Seek(f)) });
        a.on_seek_by({ let d = d.clone(); move |ms| d.dispatch(Action::SeekBy(ms)) });
        a.on_set_volume({ let d = d.clone(); move |v| d.dispatch(Action::SetVolume(v)) });
        a.on_toggle_mute({ let d = d.clone(); move || d.dispatch(Action::ToggleMute) });
        a.on_toggle_shuffle({ let d = d.clone(); move || d.dispatch(Action::ToggleShuffle) });
        a.on_cycle_repeat({ let d = d.clone(); move || d.dispatch(Action::CycleRepeat) });
        a.on_toggle_like_current({ let d = d.clone(); move || d.dispatch(Action::ToggleLikeCurrent) });
        a.on_queue_jump({ let d = d.clone(); move |i| d.dispatch(Action::QueueJump(i)) });
        a.on_queue_remove({ let d = d.clone(); move |i| d.dispatch(Action::QueueRemove(i)) });
        a.on_queue_move({ let d = d.clone(); move |f, t| d.dispatch(Action::QueueMove(f, t)) });
        a.on_queue_clear({ let d = d.clone(); move || d.dispatch(Action::QueueClear) });
        a.on_lyric_seek({ let d = d.clone(); move |ms| d.dispatch(Action::LyricSeek(ms)) });
        a.on_set_grid_columns({ let d = d.clone(); move |c| d.dispatch(Action::SetGridColumns(c)) });
        a.on_set_album_sort({ let d = d.clone(); move |s| d.dispatch(Action::SetAlbumSort(s)) });
        a.on_set_track_sort({ let d = d.clone(); move |s| d.dispatch(Action::SetTrackSort(s)) });
        a.on_open_palette({ let d = d.clone(); move || d.dispatch(Action::OpenPalette) });
        a.on_palette_query_changed({ let d = d.clone(); move |q| d.dispatch(Action::PaletteQuery(q.into())) });
        a.on_palette_activate({ let d = d.clone(); move |i, n| d.dispatch(Action::PaletteActivate(i, n)) });
        a.on_add_folder({ let d = d.clone(); move || d.dispatch(Action::AddFolder) });
        a.on_remove_folder({ let d = d.clone(); move |p| d.dispatch(Action::RemoveFolder(p.into())) });
        a.on_rescan({ let d = d.clone(); move || d.dispatch(Action::Rescan) });
        a.on_set_device({ let d = d.clone(); move |n| d.dispatch(Action::SetDevice(n.into())) });
        a.on_set_theme_mode({ let d = d.clone(); move |m| d.dispatch(Action::SetThemeMode(m)) });
        a.on_set_dynamic_accent({ let d = d.clone(); move |on| d.dispatch(Action::SetDynamicAccent(on)) });
        a.on_set_mini_on_top({ let d = d.clone(); move |on| d.dispatch(Action::SetMiniOnTop(on)) });
        a.on_lastfm_connect({ let d = d.clone(); move || d.dispatch(Action::LastfmConnect) });
        a.on_lastfm_cancel({ let d = d.clone(); move || d.dispatch(Action::LastfmCancel) });
        a.on_lastfm_disconnect({ let d = d.clone(); move || d.dispatch(Action::LastfmDisconnect) });
        a.on_lastfm_retry({ let d = d.clone(); move || d.dispatch(Action::LastfmRetry) });
        a.on_lastfm_set_keys({ let d = d.clone(); move |k, s| d.dispatch(Action::LastfmSetKeys(k.into(), s.into())) });
        a.on_lastfm_set_love_sync({ let d = d.clone(); move |on| d.dispatch(Action::LastfmSetLoveSync(on)) });
        a.on_lastfm_open_profile({ let d = d.clone(); move || d.dispatch(Action::LastfmOpenProfile) });
        a.on_create_playlist({ let d = d.clone(); move |n| d.dispatch(Action::CreatePlaylist(n.into())) });
        a.on_rename_playlist({ let d = d.clone(); move |id, n| d.dispatch(Action::RenamePlaylist(id, n.into())) });
        a.on_delete_playlist({ let d = d.clone(); move |id| d.dispatch(Action::DeletePlaylist(id)) });
        a.on_open_mini({ let d = d.clone(); move || d.dispatch(Action::OpenMini) });
        a.on_close_mini({ let d = d.clone(); move || d.dispatch(Action::CloseMini) });
        a.on_raise_main({ let d = d.clone(); move || d.dispatch(Action::RaiseMain) });
        a.on_start_window_drag({ let d = d.clone(); move || d.dispatch(Action::StartWindowDrag) });
        a.on_open_url({ let d = d.clone(); move |u| d.dispatch(Action::OpenUrl(u.into())) });
    }};
}

pub fn wire_main(ui: &AppWindow, d: &Rc<Dispatcher>) {
    wire_actions!(ui.global::<Actions>(), d);
}

fn wire_mini(mini: &MiniWindow, d: &Rc<Dispatcher>) {
    wire_actions!(mini.global::<Actions>(), d);
}

#[derive(Clone, Debug, PartialEq)]
enum Page {
    Albums,
    Recent,
    Genre(String),
    Album(AlbumId),
    Artists,
    Artist(String),
    Songs,
    Genres,
    Favorites,
    Playlist(PlaylistId),
    History,
    Settings,
    Editor,
}

impl Page {
    fn key(&self) -> &'static str {
        match self {
            Page::Albums => "albums",
            Page::Recent => "recent",
            Page::Genre(_) => "genre",
            Page::Album(_) => "album",
            Page::Artists => "artists",
            Page::Artist(_) => "artist",
            Page::Songs => "songs",
            Page::Genres => "genres",
            Page::Favorites => "favorites",
            Page::Playlist(_) => "playlist",
            Page::History => "history",
            Page::Settings => "settings",
            Page::Editor => "editor",
        }
    }
}

enum ScanMsg {
    Progress(ScanProgress),
    Done(Result<ScanSummary, String>),
}

/// Album ids are 63-bit hashes; Slint ints are 32-bit. Hand out small handles per session.
#[derive(Default)]
struct Handles {
    to_handle: HashMap<AlbumId, i32>,
    from_handle: Vec<AlbumId>,
}

impl Handles {
    fn get(&mut self, id: AlbumId) -> i32 {
        if let Some(h) = self.to_handle.get(&id) {
            return *h;
        }
        let h = self.from_handle.len() as i32;
        self.from_handle.push(id);
        self.to_handle.insert(id, h);
        h
    }

    fn album(&self, handle: i32) -> Option<AlbumId> {
        usize::try_from(handle).ok().and_then(|i| self.from_handle.get(i).copied())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SessionState {
    entries: Vec<(TrackId, bool)>,
    current: Option<usize>,
    position_ms: u64,
    context: String,
}

struct Models {
    album_rows: Rc<LazyModel<AlbumRow>>,
    more_albums: Rc<LazyModel<AlbumRow>>,
    artist_albums: Rc<LazyModel<AlbumRow>>,
    tracks: Rc<LazyModel<TrackRow>>,
    album_tracks: Rc<LazyModel<TrackRow>>,
    artists: Rc<LazyModel<ArtistCard>>,
    genre_rows: Rc<LazyModel<GenreRow>>,
    queue_manual: Rc<LazyModel<QueueItem>>,
    queue_context: Rc<LazyModel<QueueItem>>,
    playlists: Rc<LazyModel<NavPlaylist>>,
}

impl Models {
    fn new() -> Self {
        Models {
            album_rows: Rc::new(LazyModel::new()),
            more_albums: Rc::new(LazyModel::new()),
            artist_albums: Rc::new(LazyModel::new()),
            tracks: Rc::new(LazyModel::new()),
            album_tracks: Rc::new(LazyModel::new()),
            artists: Rc::new(LazyModel::new()),
            genre_rows: Rc::new(LazyModel::new()),
            queue_manual: Rc::new(LazyModel::new()),
            queue_context: Rc::new(LazyModel::new()),
            playlists: Rc::new(LazyModel::new()),
        }
    }

    fn images_ready(&self, keys: &[String]) {
        self.album_rows.images_ready(keys);
        self.more_albums.images_ready(keys);
        self.artist_albums.images_ready(keys);
        self.tracks.images_ready(keys);
        self.album_tracks.images_ready(keys);
        self.artists.images_ready(keys);
        self.genre_rows.images_ready(keys);
        self.queue_manual.images_ready(keys);
        self.queue_context.images_ready(keys);
        self.playlists.images_ready(keys);
    }
}

pub struct Services {
    pub library: Arc<Library>,
    pub engine: Engine,
    pub scrobbler: Scrobbler,
    pub lastfm_rx: Receiver<LastfmMsg>,
    pub lastfm_tx: Sender<LastfmMsg>,
    pub media: Option<MediaControls>,
    pub media_rx: Receiver<MediaAction>,
    pub settings: Settings,
    pub paths: Paths,
    pub synchronous_images: bool,
    pub system_dark: bool,
}

pub struct App {
    ui: slint::Weak<AppWindow>,
    /// The in-process mini player (X11, Windows, or Wayland without XWayland).
    mini: Option<MiniWindow>,
    /// The always-on-top helper process used on Wayland.
    mini_helper: Option<mini::External>,
    mini_helper_failed: bool,
    wayland: Option<bool>,
    dispatcher: std::rc::Weak<Dispatcher>,
    library: Arc<Library>,
    engine: Engine,
    engine_events: Receiver<EngineEvent>,
    scrobbler: Scrobbler,
    lastfm_rx: Receiver<LastfmMsg>,
    lastfm_tx: Sender<LastfmMsg>,
    lastfm_cancel: Option<Arc<AtomicBool>>,
    lastfm_status: lychee_lastfm::Status,
    lastfm_message: String,
    media: Option<MediaControls>,
    media_rx: Receiver<MediaAction>,
    settings: Settings,
    paths: Paths,
    images: Rc<ImageCache>,
    handles: Rc<RefCell<Handles>>,
    models: Models,
    editor: EditorCtl,

    queue: Queue,
    status: PlaybackStatus,
    current: Option<TrackRef>,
    engine_loaded: bool,
    token: u64,
    token_seq: u64,
    preloaded: Option<(u64, TrackId)>,
    position_ms: u64,
    duration_ms: u64,
    stream_info: Option<StreamInfo>,
    output: String,
    errors_in_row: u32,
    listen: Option<Listen>,
    lyrics: Option<Lyrics>,
    lyric_index: i32,

    page: Page,
    history: Vec<Page>,
    grid_columns: usize,
    list_ids: Vec<TrackId>,
    list_play: Vec<TrackId>,
    list_name: String,
    album_ids: Vec<TrackId>,
    palette_entries: Vec<Entry>,
    system_dark: bool,

    scan_tx: Sender<ScanMsg>,
    scan_rx: Receiver<ScanMsg>,
    folder_tx: Sender<PathBuf>,
    folder_rx: Receiver<PathBuf>,
    devices_rx: Receiver<Vec<String>>,
    devices: Vec<String>,
    toast_until: Option<Instant>,
    last_position_tick: Instant,
    last_media_position: Instant,
    settings_dirty: bool,
    last_settings_save: Instant,
    last_session_save: Instant,
}

impl App {
    pub fn new(ui: &AppWindow, s: Services, dispatcher: &Rc<Dispatcher>) -> App {
        let images = Rc::new(ImageCache::new(s.library.clone(), s.synchronous_images));
        let models = Models::new();
        let state = ui.global::<AppState>();
        state.set_album_rows(ModelRc::from(models.album_rows.clone()));
        state.set_more_albums(ModelRc::from(models.more_albums.clone()));
        state.set_artist_album_rows(ModelRc::from(models.artist_albums.clone()));
        state.set_tracks(ModelRc::from(models.tracks.clone()));
        state.set_album_tracks(ModelRc::from(models.album_tracks.clone()));
        state.set_artists(ModelRc::from(models.artists.clone()));
        state.set_genre_rows(ModelRc::from(models.genre_rows.clone()));
        state.set_queue_manual(ModelRc::from(models.queue_manual.clone()));
        state.set_queue_context(ModelRc::from(models.queue_context.clone()));
        state.set_playlists(ModelRc::from(models.playlists.clone()));
        state.set_version(env!("CARGO_PKG_VERSION").into());

        let (scan_tx, scan_rx) = crossbeam_channel::unbounded();
        let (folder_tx, folder_rx) = crossbeam_channel::unbounded();
        let (devices_tx, devices_rx) = crossbeam_channel::unbounded();
        std::thread::spawn(move || {
            let _ = devices_tx.send(Engine::device_names());
        });

        let engine_events = s.engine.events();
        let editor = EditorCtl::new(ui, s.library.clone());
        let mut app = App {
            ui: ui.as_weak(),
            mini: None,
            mini_helper: None,
            mini_helper_failed: false,
            wayland: None,
            dispatcher: Rc::downgrade(dispatcher),
            library: s.library,
            engine: s.engine,
            engine_events,
            scrobbler: s.scrobbler,
            lastfm_rx: s.lastfm_rx,
            lastfm_tx: s.lastfm_tx,
            lastfm_cancel: None,
            lastfm_status: lychee_lastfm::Status::default(),
            lastfm_message: String::new(),
            media: s.media,
            media_rx: s.media_rx,
            settings: s.settings,
            paths: s.paths,
            images,
            handles: Rc::default(),
            models,
            editor,
            queue: Queue::new(),
            status: PlaybackStatus::Stopped,
            current: None,
            engine_loaded: false,
            token: 0,
            token_seq: 0,
            preloaded: None,
            position_ms: 0,
            duration_ms: 0,
            stream_info: None,
            output: String::new(),
            errors_in_row: 0,
            listen: None,
            lyrics: None,
            lyric_index: -1,
            page: Page::Albums,
            history: Vec::new(),
            grid_columns: 5,
            list_ids: Vec::new(),
            list_play: Vec::new(),
            list_name: String::new(),
            album_ids: Vec::new(),
            palette_entries: Vec::new(),
            system_dark: s.system_dark,
            scan_tx,
            scan_rx,
            folder_tx,
            folder_rx,
            devices_rx,
            devices: Vec::new(),
            toast_until: None,
            last_position_tick: Instant::now(),
            last_media_position: Instant::now(),
            settings_dirty: false,
            last_settings_save: Instant::now(),
            last_session_save: Instant::now(),
        };
        app.init_ui();
        app
    }

    fn init_ui(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let state = ui.global::<AppState>();
        state.set_theme_mode(self.settings.theme.index());
        state.set_dynamic_accent(self.settings.dynamic_accent);
        state.set_mini_on_top(self.settings.mini_on_top);
        state.set_right_panel(self.settings.right_panel);
        state.set_right_tab(self.settings.right_tab.clone().into());
        state.set_album_sort(self.settings.album_sort);
        state.set_track_sort(self.settings.track_sort);
        self.apply_theme();
        self.queue.set_shuffle(self.settings.shuffle);
        self.queue.set_repeat(self.settings.repeat);
        self.engine.set_volume(if self.settings.muted { 0.0 } else { self.settings.volume });
        self.restore_session();
        self.refresh_library_views();
        self.update_player_state();
        self.update_lastfm_ui();
        self.update_settings_ui();
        self.show_page(Page::Albums, false);
        if !self.library.folders().is_empty() {
            self.start_scan();
        }
    }

    // ---------------------------------------------------------------------------------------
    // Action handling

    pub fn handle(&mut self, action: Action) {
        match action {
            Action::Tick => self.tick(),
            Action::Navigate(page) => {
                let p = match page.as_str() {
                    "recent" => Page::Recent,
                    "artists" => Page::Artists,
                    "songs" => Page::Songs,
                    "genres" => Page::Genres,
                    "favorites" => Page::Favorites,
                    "history" => Page::History,
                    "settings" => Page::Settings,
                    "editor" => Page::Editor,
                    _ => Page::Albums,
                };
                self.show_page(p, true);
            }
            Action::GoBack => {
                if self.ui.upgrade().is_some_and(|ui| ui.global::<AppState>().get_palette_open()) {
                    if let Some(ui) = self.ui.upgrade() {
                        ui.global::<AppState>().set_palette_open(false);
                    }
                } else if let Some(prev) = self.history.pop() {
                    self.show_page(prev, false);
                }
            }
            Action::OpenAlbum(handle) => {
                let id = self.handles.borrow().album(handle);
                if let Some(id) = id {
                    self.show_page(Page::Album(id), true);
                }
            }
            Action::OpenArtist(name) => {
                let index = self.library.index();
                // Track artists like "A, B" map to the first album artist that exists.
                let resolved = index
                    .artist(&name)
                    .map(|a| a.name.clone())
                    .or_else(|| name.split(", ").find_map(|n| index.artist(n).map(|a| a.name.clone())));
                match resolved {
                    Some(n) => self.show_page(Page::Artist(n), true),
                    None => self.toast(&format!("No albums by {name} in your library")),
                }
            }
            Action::OpenGenre(name) => self.show_page(Page::Genre(name), true),
            Action::OpenPlaylist(id) => self.show_page(Page::Playlist(i64::from(id)), true),
            Action::PlayAlbum(handle, shuffle) => {
                let id = self.handles.borrow().album(handle);
                if let Some(id) = id {
                    self.play_album(id, shuffle);
                }
            }
            Action::AlbumAction(what, handle) => {
                let Some(id) = self.handles.borrow().album(handle) else { return };
                let index = self.library.index();
                let Some(album) = index.album(id) else { return };
                match what.as_str() {
                    "play-next" => self.enqueue(album.tracks.clone(), true, &album.title),
                    "add-queue" => self.enqueue(album.tracks.clone(), false, &album.title),
                    "go-artist" => {
                        let name = album.artist.clone();
                        self.show_page(Page::Artist(name), true);
                    }
                    "show-folder" => {
                        if let Some(folder) = &album.folder {
                            let _ = open::that(folder);
                        }
                    }
                    "edit-tags" => {
                        let paths: Vec<PathBuf> = album.tracks.iter().filter_map(|id| index.track(*id)).map(|t| t.path.clone()).collect();
                        let title = album.title.clone();
                        drop(index);
                        self.edit_tags(paths, Vec::new(), title);
                    }
                    _ => {}
                }
            }
            Action::PlayFrom(context, index) => {
                let index = index.max(0) as usize;
                if context == "album" {
                    let name = self.album_title();
                    let ids = self.album_ids.clone();
                    self.play_ids(ids, index, name, None);
                } else {
                    let ids = self.list_ids.clone();
                    let name = self.list_name.clone();
                    self.play_ids(ids, index, name, None);
                }
            }
            Action::PlayList(shuffle) => {
                let ids = if self.list_play.is_empty() { self.list_ids.clone() } else { self.list_play.clone() };
                let name = self.list_name.clone();
                self.play_ids(ids, 0, name, Some(shuffle));
            }
            Action::TrackAction(what, id) => self.track_action(&what, i64::from(id)),
            Action::AddToPlaylist(playlist, what, id) => self.add_to_playlist(playlist, &what, id),
            Action::TogglePlay => self.toggle_play(),
            Action::Next => self.next(),
            Action::Previous => self.previous(),
            Action::Seek(fraction) => {
                if self.duration_ms > 0 {
                    let ms = (f64::from(fraction.clamp(0.0, 1.0)) * self.duration_ms as f64) as u64;
                    self.seek_to(ms);
                }
            }
            Action::SeekBy(delta) => {
                let target = (self.position_ms as i64 + i64::from(delta)).clamp(0, self.duration_ms as i64) as u64;
                self.seek_to(target);
            }
            Action::SetVolume(v) => self.set_volume(v),
            Action::ToggleMute => {
                self.settings.muted = !self.settings.muted;
                self.engine.set_volume(if self.settings.muted { 0.0 } else { self.settings.volume });
                self.settings_dirty = true;
                self.update_player_state();
            }
            Action::ToggleShuffle => self.set_shuffle(!self.queue.shuffle()),
            Action::CycleRepeat => self.set_repeat(self.queue.repeat().cycle()),
            Action::ToggleLikeCurrent => {
                if let Some(t) = self.current.clone() {
                    self.toggle_like(t.id);
                }
            }
            Action::QueueJump(i) => {
                if self.queue.jump(i.max(0) as usize).is_some() {
                    self.start_current(true, 0);
                }
            }
            Action::QueueRemove(i) => {
                let i = i.max(0) as usize;
                if Some(i) != self.queue.current_index() {
                    self.queue.remove(i);
                    self.queue_changed();
                }
            }
            Action::QueueMove(from, to) => {
                let len = self.queue.len() as i32;
                let first_upcoming = self.queue.current_index().map_or(0, |c| c as i32 + 1);
                if (first_upcoming..len).contains(&from) && (first_upcoming..len).contains(&to) {
                    self.queue.move_entry(from as usize, to as usize);
                    self.queue_changed();
                }
            }
            Action::QueueClear => {
                self.queue.clear_upcoming();
                self.queue_changed();
            }
            Action::LyricSeek(ms) => self.seek_to(ms.max(0) as u64),
            Action::SetGridColumns(cols) => {
                let cols = cols.clamp(1, 20) as usize;
                if cols != self.grid_columns {
                    self.grid_columns = cols;
                    if let Some(ui) = self.ui.upgrade() {
                        ui.global::<AppState>().set_grid_columns(cols as i32);
                    }
                    self.refresh_page();
                }
            }
            Action::SetAlbumSort(s) => {
                self.settings.album_sort = s;
                self.settings_dirty = true;
                self.refresh_page();
            }
            Action::SetTrackSort(s) => {
                self.settings.track_sort = s;
                self.settings_dirty = true;
                self.refresh_page();
            }
            Action::OpenPalette => self.open_palette(),
            Action::PaletteQuery(q) => self.palette_search(&q),
            Action::PaletteActivate(i, play_next) => self.palette_activate(i, play_next),
            Action::AddFolder => self.pick_folder(),
            Action::RemoveFolder(path) => {
                if let Err(e) = self.library.remove_folder(std::path::Path::new(&path)) {
                    self.toast(&format!("Could not remove folder: {e}"));
                }
                self.update_settings_ui();
                self.start_scan();
            }
            Action::Rescan => self.start_scan(),
            Action::SetDevice(name) => {
                let device = (name != "Default Audio Device" && !name.is_empty()).then_some(name);
                self.settings.output_device = device.clone();
                self.settings_dirty = true;
                self.engine.set_device(device);
                self.update_settings_ui();
            }
            Action::SetThemeMode(mode) => {
                self.settings.theme = ThemeMode::from_index(mode);
                self.settings_dirty = true;
                if let Some(ui) = self.ui.upgrade() {
                    ui.global::<AppState>().set_theme_mode(mode);
                }
                self.apply_theme();
            }
            Action::SetDynamicAccent(on) => {
                self.settings.dynamic_accent = on;
                self.settings_dirty = true;
                if let Some(ui) = self.ui.upgrade() {
                    ui.global::<AppState>().set_dynamic_accent(on);
                }
                self.apply_accent();
            }
            Action::SetMiniOnTop(on) => {
                self.settings.mini_on_top = on;
                self.settings_dirty = true;
                // A failed helper start shouldn't stick once the user changes their mind.
                self.mini_helper_failed = false;
                if let Some(ui) = self.ui.upgrade() {
                    ui.global::<AppState>().set_mini_on_top(on);
                }
            }
            Action::LastfmConnect => self.lastfm_connect(),
            Action::LastfmCancel => {
                if let Some(cancel) = self.lastfm_cancel.take() {
                    cancel.store(true, Ordering::Relaxed);
                }
                self.update_lastfm_ui();
            }
            Action::LastfmDisconnect => {
                self.settings.lastfm.session_key = None;
                self.settings.lastfm.username = None;
                self.settings_dirty = true;
                self.scrobbler.configure(self.settings.lastfm_credentials(), None);
                self.toast("Disconnected from Last.fm");
            }
            Action::LastfmRetry => self.scrobbler.retry(),
            Action::LastfmSetKeys(key, secret) => {
                self.settings.lastfm.api_key = key.trim().to_string();
                self.settings.lastfm.api_secret = secret.trim().to_string();
                // A session belongs to the API key that created it.
                self.settings.lastfm.session_key = None;
                self.settings.lastfm.username = None;
                self.settings_dirty = true;
                self.scrobbler.configure(self.settings.lastfm_credentials(), None);
                self.toast("Last.fm API key saved");
                self.update_lastfm_ui();
            }
            Action::LastfmSetLoveSync(on) => {
                self.settings.lastfm.love_sync = on;
                self.settings_dirty = true;
                self.update_lastfm_ui();
            }
            Action::LastfmOpenProfile => {
                if let Some(user) = &self.settings.lastfm.username {
                    let _ = open::that(format!("https://www.last.fm/user/{user}"));
                }
            }
            Action::CreatePlaylist(name) => match self.library.create_playlist(&name, &[]) {
                Ok(id) => {
                    self.update_playlists_nav();
                    self.show_page(Page::Playlist(id), true);
                }
                Err(e) => self.toast(&format!("Could not create the playlist: {e}")),
            },
            Action::RenamePlaylist(id, name) => {
                if self.library.rename_playlist(i64::from(id), &name).is_ok() {
                    self.update_playlists_nav();
                    self.refresh_page();
                }
            }
            Action::DeletePlaylist(id) => {
                let name = self.library.index().playlist(i64::from(id)).map(|p| p.name.clone()).unwrap_or_default();
                if self.library.delete_playlist(i64::from(id)).is_ok() {
                    self.update_playlists_nav();
                    self.history.retain(|p| *p != Page::Playlist(i64::from(id)));
                    self.show_page(Page::Albums, false);
                    self.toast(&format!("Deleted “{name}”"));
                }
            }
            Action::OpenMini => self.open_mini(),
            Action::CloseMini => {
                if let Some(mini) = &self.mini {
                    // The window is destroyed while the pointer is over it, so it would never
                    // hear that the pointer left: clear the hover state for next time.
                    mini.window().dispatch_event(slint::platform::WindowEvent::PointerExited);
                    let _ = mini.hide();
                }
                // Dropping the helper closes its stdin, which makes it quit.
                self.mini_helper = None;
                if let Some(ui) = self.ui.upgrade() {
                    ui.global::<AppState>().set_mini_open(false);
                }
                self.handle(Action::RaiseMain);
            }
            Action::RaiseMain => {
                let wayland = self.main_is_wayland();
                if let Some(ui) = self.ui.upgrade() {
                    let _ = ui.show();
                    // Wayland can't un-minimise or focus a window from the app; there the main
                    // window was hidden instead and `show` maps it again.
                    if !wayland {
                        use slint::winit_030::WinitWindowAccessor;
                        ui.window().with_winit_window(|w| {
                            w.set_minimized(false);
                            w.focus_window();
                        });
                    }
                }
            }
            Action::StartWindowDrag => {
                if let Some(mini) = &self.mini {
                    mini::start_drag(mini);
                }
            }
            Action::OpenUrl(url) => {
                let _ = open::that(url);
            }
            Action::OpenPaths(paths) => self.open_paths(paths),
            Action::SystemThemeChanged(dark) => {
                self.system_dark = dark;
                self.apply_theme();
            }
            Action::MainWindowClosed => self.shutdown(),
            Action::Editor(action) => {
                self.editor.handle(action);
                self.apply_editor_outcome();
            }
            Action::QuitSave => {
                if let Some(ui) = self.ui.upgrade() {
                    ui.global::<crate::Editor>().set_quit_prompt(false);
                }
                self.editor.save_then_quit();
                self.apply_editor_outcome();
            }
            Action::QuitDiscard => {
                self.shutdown();
                let _ = slint::quit_event_loop();
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Periodic work

    fn tick(&mut self) {
        let ready = self.images.poll();
        if !ready.is_empty() {
            self.models.images_ready(&ready);
        }
        while let Ok(event) = self.engine_events.try_recv() {
            self.on_engine_event(event);
        }
        if self.last_position_tick.elapsed() >= Duration::from_millis(240) {
            self.last_position_tick = Instant::now();
            self.update_position();
        }
        while let Ok(msg) = self.scan_rx.try_recv() {
            self.on_scan(msg);
        }
        while let Ok(msg) = self.lastfm_rx.try_recv() {
            self.on_lastfm(msg);
        }
        while let Ok(action) = self.media_rx.try_recv() {
            self.on_media_action(action);
        }
        self.editor.poll();
        self.apply_editor_outcome();
        let mini_events = self.mini_helper.as_ref().map(|m| m.poll()).unwrap_or_default();
        for event in mini_events {
            self.on_mini_event(event);
        }
        while let Ok(folder) = self.folder_rx.try_recv() {
            match self.library.add_folder(&folder) {
                Ok(()) => {
                    self.update_settings_ui();
                    self.start_scan();
                }
                Err(e) => self.toast(&format!("Could not add the folder: {e}")),
            }
        }
        if let Ok(devices) = self.devices_rx.try_recv() {
            self.devices = devices;
            self.update_settings_ui();
        }
        if self.toast_until.is_some_and(|t| Instant::now() >= t) {
            self.toast_until = None;
            if let Some(ui) = self.ui.upgrade() {
                ui.global::<AppState>().set_toast(SharedString::new());
            }
        }
        if self.settings_dirty && self.last_settings_save.elapsed() > Duration::from_secs(1) {
            self.settings.save(&self.paths.config_file);
            self.settings_dirty = false;
            self.last_settings_save = Instant::now();
        }
        if self.last_session_save.elapsed() > Duration::from_secs(15) {
            self.save_session();
        }
    }

    fn on_engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Loaded { token, info, duration_ms } => {
                if token == self.token {
                    self.stream_info = Some(info);
                    if self.duration_ms == 0 {
                        if let Some(d) = duration_ms {
                            self.duration_ms = d;
                        }
                    }
                    self.update_details();
                }
            }
            EngineEvent::Started { token } => {
                if token == self.token {
                    self.errors_in_row = 0;
                } else if self.preloaded.is_some_and(|(t, _)| t == token) {
                    // Gapless hand-over to the pre-loaded track.
                    let (_, next_id) = self.preloaded.take().expect("checked");
                    let advanced = self.queue.advance(true);
                    if advanced != Some(next_id) {
                        // The queue changed after pre-loading; find the track we actually hear.
                        let pos = self.queue.entries().iter().position(|e| e.track == next_id);
                        if let Some(pos) = pos {
                            self.queue.jump(pos);
                        }
                    }
                    self.token = token;
                    self.errors_in_row = 0;
                    if let Some(track) = self.library.index().track(next_id) {
                        self.set_current(track, 0, true);
                    }
                    self.preload_next();
                }
            }
            EngineEvent::Ended { token } => {
                if token == self.token {
                    if self.queue.advance(true).is_some() {
                        self.start_current(true, 0);
                    } else {
                        self.finish_queue();
                    }
                }
            }
            EngineEvent::Error { token, message } => {
                if token == Some(self.token) {
                    let title = self.current.as_ref().map(|t| t.title.clone()).unwrap_or_default();
                    self.toast(&format!("Can't play “{title}”: {message}"));
                    self.errors_in_row += 1;
                    if self.errors_in_row < 5 && self.queue.advance(false).is_some() {
                        self.start_current(true, 0);
                    } else {
                        self.status = PlaybackStatus::Paused;
                        self.engine_loaded = false;
                        self.update_player_state();
                    }
                } else if token.is_some() && self.preloaded.is_some_and(|(t, _)| Some(t) == token) {
                    self.preloaded = None;
                } else if token.is_none() {
                    self.toast(&format!("Audio output problem: {message}"));
                }
            }
            EngineEvent::OutputChanged { device, sample_rate, channels } => {
                self.output = format!("{device} · {} kHz · {} ch", sample_rate as f32 / 1000.0, channels);
                if let Some(ui) = self.ui.upgrade() {
                    ui.global::<AppState>().set_device_name(device.into());
                }
                self.update_details();
                self.update_settings_ui();
            }
        }
    }

    fn update_position(&mut self) {
        if let Some((token, ms)) = self.engine.position() {
            if token == self.token && self.engine_loaded {
                self.position_ms = if self.duration_ms > 0 { ms.min(self.duration_ms) } else { ms };
            }
        }
        let playing = self.status == PlaybackStatus::Playing;
        if let Some(event) = self.listen.as_mut().and_then(|l| l.update(self.position_ms, playing)) {
            let ListenEvent::Counted { scrobble } = event;
            if let Some(listen) = &self.listen {
                let track = listen.track.clone();
                if let Err(e) = self.library.record_play(track.id) {
                    tracing::warn!("could not record a play: {e}");
                }
                if scrobble && self.settings.lastfm_session().is_some() {
                    self.scrobbler.scrobble(Scrobble { track: track_info(&track), timestamp: listen.started_at });
                }
            }
        }
        self.update_lyric_index();
        self.update_progress_ui();
        if playing && self.last_media_position.elapsed() > Duration::from_secs(1) {
            self.last_media_position = Instant::now();
            if let Some(m) = &self.media {
                m.set_position(self.position_ms);
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Playback

    fn next_token(&mut self) -> u64 {
        self.token_seq += 1;
        self.token_seq
    }

    fn play_album(&mut self, id: AlbumId, shuffle: bool) {
        let index = self.library.index();
        let Some(album) = index.album(id) else { return };
        let (ids, title) = (album.tracks.clone(), album.title.clone());
        drop(index);
        self.play_ids(ids, 0, title, Some(shuffle));
    }

    fn play_ids(&mut self, ids: Vec<TrackId>, start: usize, context: String, shuffle: Option<bool>) {
        if ids.is_empty() {
            return;
        }
        if let Some(s) = shuffle {
            self.queue.set_shuffle(s);
            self.settings.shuffle = s;
            self.settings_dirty = true;
        }
        self.queue.set(&ids, start, context);
        self.start_current(true, 0);
    }

    fn start_current(&mut self, play: bool, start_ms: u64) {
        let Some(id) = self.queue.current() else {
            self.stop();
            return;
        };
        let Some(track) = self.library.index().track(id) else {
            self.queue.remove(self.queue.current_index().unwrap_or(0));
            if self.queue.current().is_some() {
                self.start_current(play, 0);
            }
            return;
        };
        self.token = self.next_token();
        self.engine.load(self.token, track.path.clone(), start_ms, play);
        self.engine_loaded = true;
        self.preloaded = None;
        self.set_current(track, start_ms, play);
        self.preload_next();
    }

    fn set_current(&mut self, track: TrackRef, position_ms: u64, playing: bool) {
        self.current = Some(track.clone());
        self.position_ms = position_ms;
        self.duration_ms = track.duration_ms;
        self.status = if playing { PlaybackStatus::Playing } else { PlaybackStatus::Paused };
        self.listen = Some(Listen::new(track.clone(), position_ms));
        self.stream_info = None;
        self.load_lyrics();
        self.update_now_playing();
        self.update_player_state();
        self.update_details();
        self.queue_changed_ui();
        self.apply_accent();
        self.models.tracks.refresh();
        self.models.album_tracks.refresh();
        self.models.album_rows.refresh();
        if let Some(m) = &self.media {
            let art_path = track.artwork.as_ref().map(|k| self.library.artwork_large(k));
            m.set_metadata(Some(OsNowPlaying {
                track_id: track.id,
                title: track.title.clone(),
                artist: track.display_artist().to_string(),
                album: track.album.clone(),
                album_artist: track.album_artist.clone(),
                duration_ms: track.duration_ms,
                art_path,
            }));
            m.seeked(position_ms);
        }
        if playing && self.settings.lastfm_session().is_some() {
            self.scrobbler.now_playing(track_info(&track));
        }
    }

    fn preload_next(&mut self) {
        let next = self.queue.peek_next(true);
        match next {
            Some(id) => {
                if self.preloaded.is_some_and(|(_, t)| t == id) {
                    return;
                }
                let Some(track) = self.library.index().track(id) else { return };
                let token = self.next_token();
                self.engine.preload(token, track.path.clone());
                self.preloaded = Some((token, id));
            }
            None => {
                if self.preloaded.take().is_some() {
                    self.engine.cancel_preload();
                }
            }
        }
    }

    fn toggle_play(&mut self) {
        match self.status {
            PlaybackStatus::Playing => {
                self.engine.pause();
                self.status = PlaybackStatus::Paused;
            }
            PlaybackStatus::Paused if self.engine_loaded => {
                self.engine.play();
                self.status = PlaybackStatus::Playing;
                if let Some(t) = &self.current {
                    if self.settings.lastfm_session().is_some() {
                        self.scrobbler.now_playing(track_info(t));
                    }
                }
            }
            _ => {
                if self.queue.current().is_some() {
                    let at = self.position_ms;
                    self.start_current(true, at);
                } else {
                    let ids: Vec<TrackId> = self.library.index().tracks(TrackSort::Artist).iter().map(|t| t.id).collect();
                    self.play_ids(ids, 0, "Songs".into(), None);
                }
                return;
            }
        }
        self.update_player_state();
    }

    fn next(&mut self) {
        if self.queue.advance(false).is_some() {
            self.start_current(true, 0);
        } else if self.current.is_some() {
            self.toast("End of the queue");
        }
    }

    fn previous(&mut self) {
        let at_start = self.queue.current_index() == Some(0) && self.queue.repeat() != RepeatMode::All;
        if self.position_ms > 3_000 || at_start {
            self.seek_to(0);
        } else if self.queue.previous().is_some() {
            let playing = self.status == PlaybackStatus::Playing;
            self.start_current(playing || !self.engine_loaded, 0);
        }
    }

    fn seek_to(&mut self, ms: u64) {
        if self.current.is_none() {
            return;
        }
        if !self.engine_loaded {
            self.start_current(false, ms);
            return;
        }
        self.engine.seek(ms);
        self.position_ms = ms;
        if let Some(l) = &mut self.listen {
            l.seeked(ms);
        }
        if let Some(m) = &self.media {
            m.seeked(ms);
        }
        self.update_lyric_index();
        self.update_progress_ui();
    }

    fn stop(&mut self) {
        self.engine.stop();
        self.engine_loaded = false;
        self.status = PlaybackStatus::Stopped;
        self.current = None;
        self.position_ms = 0;
        self.duration_ms = 0;
        self.listen = None;
        self.lyrics = None;
        self.update_now_playing();
        self.update_player_state();
        self.load_lyrics();
        self.update_details();
        self.queue_changed_ui();
        if let Some(m) = &self.media {
            m.set_metadata(None);
        }
    }

    /// The queue played to the end: go back to its first track, paused, so Play starts it over.
    fn finish_queue(&mut self) {
        self.engine_loaded = false;
        self.preloaded = None;
        let first = self.queue.jump(0).and_then(|id| self.library.index().track(id));
        match first {
            Some(track) => self.set_current(track, 0, false),
            None => {
                self.status = PlaybackStatus::Paused;
                self.position_ms = 0;
                self.listen = None;
                self.update_player_state();
            }
        }
    }

    fn set_volume(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.settings.volume = v;
        self.settings.muted = false;
        self.engine.set_volume(v);
        self.settings_dirty = true;
        if let Some(m) = &self.media {
            m.set_volume(f64::from(v));
        }
        self.update_player_state();
    }

    fn set_shuffle(&mut self, on: bool) {
        self.queue.set_shuffle(on);
        self.settings.shuffle = on;
        self.settings_dirty = true;
        if let Some(m) = &self.media {
            m.set_shuffle(on);
        }
        self.queue_changed();
        self.update_player_state();
    }

    fn set_repeat(&mut self, mode: RepeatMode) {
        self.queue.set_repeat(mode);
        self.settings.repeat = mode;
        self.settings_dirty = true;
        if let Some(m) = &self.media {
            m.set_repeat(match mode {
                RepeatMode::Off => Repeat::Off,
                RepeatMode::All => Repeat::All,
                RepeatMode::One => Repeat::One,
            });
        }
        self.queue_changed();
        self.update_player_state();
    }

    fn enqueue(&mut self, ids: Vec<TrackId>, next: bool, label: &str) {
        if ids.is_empty() {
            return;
        }
        if self.queue.current().is_none() {
            self.play_ids(ids, 0, label.to_string(), None);
            return;
        }
        if next {
            self.queue.play_next(&ids);
        } else {
            self.queue.add(&ids);
        }
        self.queue_changed();
        let what = if ids.len() == 1 { format!("“{label}”") } else { format!("{} songs", ids.len()) };
        self.toast(&if next { format!("Playing {what} next") } else { format!("Added {what} to the queue") });
    }

    fn queue_changed(&mut self) {
        self.queue_changed_ui();
        self.preload_next();
        self.save_session();
    }

    fn on_media_action(&mut self, action: MediaAction) {
        match action {
            MediaAction::Play => {
                if self.status != PlaybackStatus::Playing {
                    self.toggle_play();
                }
            }
            MediaAction::Pause | MediaAction::Stop => {
                if self.status == PlaybackStatus::Playing {
                    self.toggle_play();
                }
            }
            MediaAction::PlayPause => self.toggle_play(),
            MediaAction::Next => self.next(),
            MediaAction::Previous => self.previous(),
            MediaAction::SeekBy(ms) => {
                let target = (self.position_ms as i64 + ms).clamp(0, self.duration_ms as i64) as u64;
                self.seek_to(target);
            }
            MediaAction::SetPosition(ms) => self.seek_to(ms),
            MediaAction::SetVolume(v) => self.set_volume(v as f32),
            MediaAction::SetShuffle(s) => self.set_shuffle(s),
            MediaAction::SetRepeat(r) => self.set_repeat(match r {
                Repeat::Off => RepeatMode::Off,
                Repeat::All => RepeatMode::All,
                Repeat::One => RepeatMode::One,
            }),
            MediaAction::Raise => self.handle(Action::RaiseMain),
            MediaAction::Quit => {
                self.shutdown();
                let _ = slint::quit_event_loop();
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Library actions

    fn track_action(&mut self, what: &str, id: TrackId) {
        if what == "remove-from-playlist" {
            // `id` is the row index on the playlist page.
            if let Page::Playlist(pid) = self.page {
                let mut tracks = self.list_ids.clone();
                if (id as usize) < tracks.len() {
                    tracks.remove(id as usize);
                    if self.library.set_playlist_tracks(pid, &tracks).is_ok() {
                        self.update_playlists_nav();
                        self.refresh_page();
                    }
                }
            }
            return;
        }
        let Some(track) = self.library.index().track(id) else { return };
        match what {
            "like" => self.toggle_like(id),
            "play-next" => self.enqueue(vec![id], true, &track.title),
            "add-queue" => self.enqueue(vec![id], false, &track.title),
            "go-album" => self.show_page(Page::Album(track.album_id), true),
            "go-artist" => self.handle(Action::OpenArtist(track.album_artist.clone())),
            "show-folder" => {
                if let Some(dir) = track.path.parent() {
                    let _ = open::that(dir);
                }
            }
            "edit-tags" => {
                // The whole album, with this song selected: lookups work best on albums.
                let index = self.library.index();
                let mut paths: Vec<PathBuf> = index
                    .album(track.album_id)
                    .map(|a| a.tracks.iter().filter_map(|id| index.track(*id)).map(|t| t.path.clone()).collect())
                    .unwrap_or_default();
                if paths.is_empty() {
                    paths.push(track.path.clone());
                }
                drop(index);
                self.edit_tags(paths, vec![track.path.clone()], track.album.clone());
            }
            _ => {}
        }
    }

    /// Open files in the tag editor and go there.
    fn edit_tags(&mut self, paths: Vec<PathBuf>, select: Vec<PathBuf>, title: String) {
        self.editor.open(paths, false, select, Some(title));
        self.apply_editor_outcome();
        self.show_page(Page::Editor, true);
    }

    fn apply_editor_outcome(&mut self) {
        let outcome = self.editor.take_outcome();
        if let Some(text) = outcome.toast {
            self.toast(&text);
        }
        if outcome.library_changed {
            self.after_tags_saved();
        }
        if outcome.ready_to_quit {
            self.shutdown();
            let _ = slint::quit_event_loop();
        }
    }

    /// The editor wrote files: show the new tags, covers and names everywhere.
    fn after_tags_saved(&mut self) {
        self.images.clear_missing();
        let refreshed = self.current.as_ref().and_then(|c| self.library.index().track(c.id));
        if let Some(track) = refreshed {
            self.current = Some(track);
            self.update_now_playing();
            self.update_details();
        }
        self.refresh_library_views();
        if self.page != Page::Editor {
            self.refresh_page();
        }
        self.queue_changed_ui();
        self.apply_accent();
    }

    /// The window is closing. `false` keeps it open (unsaved tag changes: ask first).
    pub fn request_close(&mut self) -> bool {
        if !self.editor.has_unsaved() {
            return true;
        }
        self.show_page(Page::Editor, true);
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<crate::Editor>().set_quit_prompt(true);
        }
        false
    }

    fn toggle_like(&mut self, id: TrackId) {
        let Some(track) = self.library.index().track(id) else { return };
        let liked = !track.liked;
        if let Err(e) = self.library.set_liked(id, liked) {
            self.toast(&format!("Could not update Favorites: {e}"));
            return;
        }
        if self.current.as_ref().is_some_and(|c| c.id == id) {
            self.current = self.library.index().track(id);
            self.update_now_playing();
        }
        if self.settings.lastfm.love_sync && self.settings.lastfm_session().is_some() {
            self.scrobbler.set_loved(track.display_artist().to_string(), track.title.clone(), liked);
        }
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<AppState>().set_favorites_count(self.library.index().liked().len() as i32);
        }
        if self.page == Page::Favorites {
            self.refresh_page();
        } else {
            self.models.tracks.refresh();
            self.models.album_tracks.refresh();
        }
    }

    fn add_to_playlist(&mut self, playlist: i32, what: &str, id: i32) {
        let (ids, label) = {
            let index = self.library.index();
            if what == "album" {
                let Some(album) = self.handles.borrow().album(id).and_then(|a| index.album(a).cloned()) else { return };
                (album.tracks.clone(), album.title.clone())
            } else {
                let Some(t) = index.track(i64::from(id)) else { return };
                (vec![t.id], t.title.clone())
            }
        };
        if playlist < 0 {
            match self.library.create_playlist(&label, &ids) {
                Ok(_) => self.toast(&format!("Created the playlist “{label}”")),
                Err(e) => self.toast(&format!("Could not create the playlist: {e}")),
            }
        } else {
            let name = self.library.index().playlist(i64::from(playlist)).map(|p| p.name.clone()).unwrap_or_default();
            match self.library.add_to_playlist(i64::from(playlist), &ids) {
                Ok(()) => self.toast(&format!("Added to “{name}”")),
                Err(e) => self.toast(&format!("Could not add to the playlist: {e}")),
            }
        }
        self.update_playlists_nav();
        if matches!(self.page, Page::Playlist(_)) {
            self.refresh_page();
        }
    }

    fn open_paths(&mut self, paths: Vec<PathBuf>) {
        if self.page == Page::Editor {
            // Dropped onto the tag editor: edit them instead of playing them.
            self.editor.open(paths, true, Vec::new(), None);
            self.apply_editor_outcome();
            return;
        }
        match self.library.import_paths(&paths) {
            Ok(ids) if !ids.is_empty() => {
                if self.status == PlaybackStatus::Playing {
                    let n = ids.len();
                    self.queue.add(&ids);
                    self.queue_changed();
                    self.toast(&format!("Added {n} song{} to the queue", if n == 1 { "" } else { "s" }));
                } else {
                    self.play_ids(ids, 0, "Opened files".into(), None);
                }
            }
            Ok(_) => self.toast("Nothing playable in what you opened"),
            Err(e) => self.toast(&format!("Could not open: {e}")),
        }
    }

    fn pick_folder(&mut self) {
        let tx = self.folder_tx.clone();
        std::thread::spawn(move || {
            if let Some(folder) = rfd::FileDialog::new().set_title("Add a music folder").pick_folder() {
                let _ = tx.send(folder);
            }
        });
    }

    fn start_scan(&mut self) {
        if self.library.is_scanning() {
            return;
        }
        let library = self.library.clone();
        let tx = self.scan_tx.clone();
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<AppState>().set_scan(ScanUi { active: true, text: "Looking for music…".into(), progress: 0.0 });
        }
        std::thread::Builder::new()
            .name("lychee-scan".into())
            .spawn(move || {
                let progress_tx = tx.clone();
                let result = library.scan(&move |p| {
                    let _ = progress_tx.send(ScanMsg::Progress(p));
                });
                let _ = tx.send(ScanMsg::Done(result.map_err(|e| e.to_string())));
            })
            .ok();
    }

    fn on_scan(&mut self, msg: ScanMsg) {
        let Some(ui) = self.ui.upgrade() else { return };
        match msg {
            ScanMsg::Progress(ScanProgress::Discovering { found }) => {
                ui.global::<AppState>().set_scan(ScanUi {
                    active: true,
                    text: format!("Looking for music… {}", thousands(found)).into(),
                    progress: 0.0,
                });
            }
            ScanMsg::Progress(ScanProgress::Reading { done, total }) => {
                ui.global::<AppState>().set_scan(ScanUi {
                    active: true,
                    text: format!("Scanning {} / {}", thousands(done), thousands(total)).into(),
                    progress: done as f32 / total.max(1) as f32,
                });
            }
            ScanMsg::Done(result) => {
                ui.global::<AppState>().set_scan(ScanUi { active: false, text: SharedString::new(), progress: 1.0 });
                match result {
                    Ok(summary) => {
                        if summary.added_or_updated > 0 || summary.removed > 0 {
                            self.images.clear_missing();
                            let index = self.library.index();
                            self.queue.retain(|id| index.contains(id));
                            drop(index);
                            self.refresh_library_views();
                            self.refresh_page();
                            self.queue_changed_ui();
                            if summary.added_or_updated > 0 {
                                self.toast(&format!("Library updated: {} new or changed", thousands(summary.added_or_updated)));
                            }
                        }
                    }
                    Err(e) => self.toast(&format!("Scan failed: {e}")),
                }
                self.update_settings_ui();
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Last.fm

    fn lastfm_connect(&mut self) {
        let Some(credentials) = self.settings.lastfm_credentials() else {
            self.toast("Add a Last.fm API key in Settings first");
            self.show_page(Page::Settings, true);
            return;
        };
        if let Some(cancel) = self.lastfm_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.lastfm_message.clear();
        self.lastfm_cancel = Some(start_auth(credentials, self.lastfm_tx.clone()));
        self.update_lastfm_ui();
        self.toast("Approve Lychee in the Last.fm page that just opened");
    }

    fn on_lastfm(&mut self, msg: LastfmMsg) {
        match msg {
            LastfmMsg::Status(status) => {
                self.lastfm_status = status;
            }
            LastfmMsg::Connected(session) => {
                self.lastfm_cancel = None;
                self.lastfm_message.clear();
                self.settings.lastfm.username = Some(session.username.clone());
                self.settings.lastfm.session_key = Some(session.key.clone());
                self.settings_dirty = true;
                self.scrobbler.configure(self.settings.lastfm_credentials(), Some(session.clone()));
                self.toast(&format!("Connected to Last.fm as {}", session.username));
                if let Some(t) = &self.current {
                    if self.status == PlaybackStatus::Playing {
                        self.scrobbler.now_playing(track_info(t));
                    }
                }
            }
            LastfmMsg::ConnectFailed(message) => {
                self.lastfm_cancel = None;
                self.lastfm_message = message.clone();
                self.toast(&format!("Last.fm: {message}"));
            }
            LastfmMsg::Cancelled => {
                self.lastfm_cancel = None;
            }
        }
        self.update_lastfm_ui();
    }

    fn update_lastfm_ui(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let s = &self.lastfm_status;
        let connected = self.settings.lastfm_session().is_some();
        let state = if self.lastfm_cancel.is_some() {
            "connecting"
        } else if !connected {
            "disconnected"
        } else {
            match s.state {
                lychee_lastfm::State::Submitting => "submitting",
                lychee_lastfm::State::Offline => "offline",
                lychee_lastfm::State::AuthError => "error",
                _ => "idle",
            }
        };
        let message = if !self.lastfm_message.is_empty() {
            self.lastfm_message.clone()
        } else if state == "error" || state == "offline" {
            s.last_error.clone().unwrap_or_default()
        } else {
            String::new()
        };
        ui.global::<Player>().set_lastfm(LastfmUi {
            state: state.into(),
            username: self.settings.lastfm.username.clone().unwrap_or_default().into(),
            pending: s.pending as i32,
            scrobbled: s.scrobbled_this_session as i32,
            last: s.last_scrobbled.clone().unwrap_or_default().into(),
            message: message.into(),
            love_sync: self.settings.lastfm.love_sync,
            builtin_key: lychee_lastfm::Credentials::built_in().is_some(),
            api_key: self.settings.lastfm.api_key.clone().into(),
            api_secret: self.settings.lastfm.api_secret.clone().into(),
        });
    }

    // ---------------------------------------------------------------------------------------
    // Command palette

    fn open_palette(&mut self) {
        if let Some(ui) = self.ui.upgrade() {
            let state = ui.global::<AppState>();
            state.set_palette_query(SharedString::new());
            state.set_palette_selected(0);
            state.set_palette_open(true);
        }
        self.palette_search("");
    }

    fn palette_search(&mut self, query: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let index = self.library.index();
        let results = palette::search(&index, query);
        let icons = ui.global::<Icons>();
        let mut items = Vec::with_capacity(results.len());
        self.palette_entries.clear();
        for (section, entry) in results {
            let mut item = PaletteItem { section: section.into(), ..Default::default() };
            match &entry {
                Entry::Album(id) => {
                    let Some(a) = index.album(*id) else { continue };
                    item.kind = "album".into();
                    item.title = a.title.clone().into();
                    item.subtitle = format!("Album · {}{}", a.artist, a.year.map(|y| format!(" · {y}")).unwrap_or_default()).into();
                    item.hint = "Open".into();
                    self.fill_cover(&mut item.cover, &mut item.has_cover, a.artwork.as_deref());
                    item.seed = seed_color(&a.title);
                }
                Entry::Artist(name) => {
                    let Some(artist) = index.artist(name) else { continue };
                    item.kind = "artist".into();
                    item.title = artist.name.clone().into();
                    item.subtitle = format!("Artist · {} album{}", artist.albums.len(), plural(artist.albums.len())).into();
                    item.hint = "Open".into();
                    let art = artist.albums.first().and_then(|a| index.album(*a)).and_then(|a| a.artwork.clone());
                    self.fill_cover(&mut item.cover, &mut item.has_cover, art.as_deref());
                    item.seed = seed_color(&artist.name);
                }
                Entry::Track(id) => {
                    let Some(t) = index.track(*id) else { continue };
                    item.kind = "track".into();
                    item.title = t.title.clone().into();
                    item.subtitle = format!("{} · {} · {}", t.display_artist(), t.album, format_duration_ms(t.duration_ms)).into();
                    item.hint = "Play".into();
                    self.fill_cover(&mut item.cover, &mut item.has_cover, t.artwork.as_deref());
                    item.seed = seed_color(&t.album);
                }
                Entry::Playlist(id) => {
                    let Some(p) = index.playlist(*id) else { continue };
                    item.kind = "playlist".into();
                    item.title = p.name.clone().into();
                    item.subtitle = format!("Playlist · {} song{}", p.tracks.len(), plural(p.tracks.len())).into();
                    item.hint = "Open".into();
                    let art = p.tracks.first().and_then(|t| index.track(*t)).and_then(|t| t.artwork.clone());
                    self.fill_cover(&mut item.cover, &mut item.has_cover, art.as_deref());
                    item.seed = seed_color(&p.name);
                }
                Entry::Command(cmd) => {
                    let info = palette::info(*cmd);
                    item.kind = "command".into();
                    item.title = info.title.into();
                    item.subtitle = info.subtitle.into();
                    item.hint = info.hint.into();
                    item.icon = icon_by_name(&icons, info.icon);
                }
            }
            items.push(item);
            self.palette_entries.push(entry);
        }
        let state = ui.global::<AppState>();
        state.set_palette_items(ModelRc::new(VecModel::from(items)));
        if state.get_palette_selected() >= self.palette_entries.len() as i32 {
            state.set_palette_selected(0);
        }
    }

    fn fill_cover(&self, cover: &mut Image, has: &mut bool, key: Option<&str>) {
        if let Some(img) = key.and_then(|k| self.images.get_now(k, Size::Thumb)) {
            *cover = img;
            *has = true;
        }
    }

    fn palette_activate(&mut self, i: i32, play_next: bool) {
        let Some(entry) = usize::try_from(i).ok().and_then(|i| self.palette_entries.get(i).cloned()) else { return };
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<AppState>().set_palette_open(false);
            ui.invoke_focus_shortcuts();
        }
        let index = self.library.index();
        match entry {
            Entry::Album(id) => {
                if play_next {
                    if let Some(a) = index.album(id) {
                        let (ids, title) = (a.tracks.clone(), a.title.clone());
                        drop(index);
                        self.enqueue(ids, true, &title);
                    }
                } else {
                    drop(index);
                    self.show_page(Page::Album(id), true);
                }
            }
            Entry::Artist(name) => {
                drop(index);
                self.show_page(Page::Artist(name), true);
            }
            Entry::Track(id) => {
                let Some(t) = index.track(id) else { return };
                if play_next {
                    drop(index);
                    self.enqueue(vec![id], true, &t.title);
                } else {
                    let album = index.album(t.album_id).cloned();
                    drop(index);
                    match album {
                        Some(a) => {
                            let start = a.tracks.iter().position(|x| *x == id).unwrap_or(0);
                            self.play_ids(a.tracks.clone(), start, a.title.clone(), None);
                        }
                        None => self.play_ids(vec![id], 0, t.title.clone(), None),
                    }
                }
            }
            Entry::Playlist(id) => {
                drop(index);
                self.show_page(Page::Playlist(id), true);
            }
            Entry::Command(cmd) => {
                drop(index);
                self.run_command(cmd);
            }
        }
    }

    fn run_command(&mut self, cmd: Command) {
        match cmd {
            Command::PlayPause => self.toggle_play(),
            Command::Next => self.next(),
            Command::Previous => self.previous(),
            Command::Shuffle => self.set_shuffle(!self.queue.shuffle()),
            Command::Repeat => self.set_repeat(self.queue.repeat().cycle()),
            Command::ClearQueue => self.handle(Action::QueueClear),
            Command::Mini => self.open_mini(),
            Command::Lyrics | Command::SidePanel => {
                if let Some(ui) = self.ui.upgrade() {
                    let state = ui.global::<AppState>();
                    if cmd == Command::Lyrics {
                        state.set_right_panel(true);
                        state.set_right_tab("lyrics".into());
                    } else {
                        state.set_right_panel(!state.get_right_panel());
                    }
                }
            }
            Command::Theme => {
                let dark = self.is_dark();
                self.handle(Action::SetThemeMode(if dark { 2 } else { 1 }));
            }
            Command::Albums => self.show_page(Page::Albums, true),
            Command::Artists => self.show_page(Page::Artists, true),
            Command::Songs => self.show_page(Page::Songs, true),
            Command::Favorites => self.show_page(Page::Favorites, true),
            Command::Settings => self.show_page(Page::Settings, true),
            Command::AddFolder => self.pick_folder(),
            Command::Rescan => self.start_scan(),
            Command::TagEditor => self.show_page(Page::Editor, true),
            Command::Lastfm => self.lastfm_connect(),
        }
    }

    // ---------------------------------------------------------------------------------------
    // Pages

    fn show_page(&mut self, page: Page, push: bool) {
        if push && self.page != page {
            self.history.push(self.page.clone());
            if self.history.len() > 64 {
                self.history.remove(0);
            }
        }
        self.page = page;
        if let Some(ui) = self.ui.upgrade() {
            let state = ui.global::<AppState>();
            state.set_page(self.page.key().into());
            state.set_can_go_back(!self.history.is_empty());
            state.set_content_scrolled(false);
        }
        self.refresh_page();
        self.apply_accent();
    }

    fn refresh_page(&mut self) {
        let index = self.library.index();
        match self.page.clone() {
            Page::Albums => {
                let sort = match self.settings.album_sort {
                    1 => AlbumSort::Title,
                    2 => AlbumSort::Year,
                    3 => AlbumSort::RecentlyAdded,
                    _ => AlbumSort::Artist,
                };
                let ids: Vec<AlbumId> = index.albums(sort).iter().map(|a| a.id).collect();
                let s = index.stats();
                let subtitle = format!(
                    "{} albums · {} songs · {}",
                    thousands(s.albums),
                    thousands(s.tracks),
                    human_duration(s.duration_ms)
                );
                self.set_albums_header("Albums", &subtitle, true);
                self.set_album_grid(&self.models.album_rows.clone(), ids, &index);
            }
            Page::Recent => {
                let ids: Vec<AlbumId> = index.albums(AlbumSort::RecentlyAdded).iter().map(|a| a.id).collect();
                self.set_albums_header("Recently Added", "Newest albums in your library first", false);
                self.set_album_grid(&self.models.album_rows.clone(), ids, &index);
            }
            Page::Genre(name) => {
                let ids: Vec<AlbumId> =
                    index.genres().iter().find(|(g, _)| g.eq_ignore_ascii_case(&name)).map(|(_, a)| a.clone()).unwrap_or_default();
                let subtitle = format!("{} album{}", ids.len(), plural(ids.len()));
                self.set_albums_header(&name, &subtitle, false);
                self.set_album_grid(&self.models.album_rows.clone(), ids, &index);
            }
            Page::Album(id) => self.show_album(id, &index),
            Page::Artists => {
                let artists: Vec<(String, String, Option<String>)> = index
                    .artists()
                    .iter()
                    .map(|a| {
                        let detail = format!("{} album{} · {} song{}", a.albums.len(), plural(a.albums.len()), a.track_count, plural(a.track_count));
                        let art = a.albums.iter().filter_map(|id| index.album(*id)).find_map(|al| al.artwork.clone());
                        (a.name.clone(), detail, art)
                    })
                    .collect();
                let images = self.images.clone();
                self.models.artists.set(artists.len(), move |row, need| {
                    let (name, detail, art) = &artists[row];
                    let (cover, has_cover) = thumb(&images, art.as_deref(), need);
                    ArtistCard { name: name.into(), detail: detail.into(), cover, has_cover, seed: seed_color(name) }
                });
            }
            Page::Artist(name) => {
                let albums: Vec<AlbumId> = index.artist_albums(&name).iter().map(|a| a.id).collect();
                let tracks = index.artist_tracks(&name);
                let mut play: Vec<&Album> = index.artist_albums(&name);
                play.sort_by_key(|a| a.year.unwrap_or(0));
                self.list_play = play.iter().flat_map(|a| a.tracks.iter().copied()).collect();
                let top: Vec<TrackRef> = tracks.iter().filter(|t| t.play_count > 0).take(10).cloned().collect();
                let art = albums.iter().filter_map(|id| index.album(*id)).find_map(|a| a.artwork.clone());
                let (cover, has_cover) = match art.as_deref().and_then(|k| self.images.get_now(k, Size::Large)) {
                    Some(img) => (img, true),
                    None => (Image::default(), false),
                };
                self.set_list_header(ListHeader {
                    kind: "Artist".into(),
                    title: name.clone().into(),
                    subtitle: format!("{} album{} · {} song{}", albums.len(), plural(albums.len()), tracks.len(), plural(tracks.len())).into(),
                    cover,
                    has_cover,
                    seed: seed_color(&name),
                    id: 0,
                    editable: false,
                });
                self.list_name = name.clone();
                self.set_track_list(top.iter().map(|t| t.id).collect(), &index, true);
                self.set_album_grid(&self.models.artist_albums.clone(), albums, &index);
            }
            Page::Songs => {
                let sort = match self.settings.track_sort {
                    1 => TrackSort::Title,
                    2 => TrackSort::Album,
                    3 => TrackSort::Plays,
                    4 => TrackSort::RecentlyAdded,
                    _ => TrackSort::Artist,
                };
                let ids: Vec<TrackId> = index.tracks(sort).iter().map(|t| t.id).collect();
                let s = index.stats();
                self.set_list_header(ListHeader {
                    kind: "Library".into(),
                    title: "Songs".into(),
                    subtitle: format!("{} songs · {}", thousands(s.tracks), human_duration(s.duration_ms)).into(),
                    ..Default::default()
                });
                self.list_name = "Songs".into();
                self.list_play.clear();
                self.set_track_list(ids, &index, true);
            }
            Page::Genres => {
                let genres: Vec<(String, String, Option<String>)> = index
                    .genres()
                    .iter()
                    .map(|(g, albums)| {
                        let art = albums.iter().filter_map(|id| index.album(*id)).find_map(|a| a.artwork.clone());
                        (g.clone(), format!("{} album{}", albums.len(), plural(albums.len())), art)
                    })
                    .collect();
                let cols = self.grid_columns.max(1);
                let images = self.images.clone();
                let rows = genres.len().div_ceil(cols);
                self.models.genre_rows.set(rows, move |row, need| {
                    let items: Vec<GenreCard> = genres[row * cols..((row + 1) * cols).min(genres.len())]
                        .iter()
                        .map(|(name, detail, art)| {
                            let (cover, has_cover) = thumb(&images, art.as_deref(), need);
                            GenreCard { name: name.into(), detail: detail.into(), cover, has_cover, seed: genre_color(name) }
                        })
                        .collect();
                    GenreRow { items: ModelRc::new(VecModel::from(items)) }
                });
            }
            Page::Favorites => {
                let liked = index.liked();
                let total: u64 = liked.iter().map(|t| t.duration_ms).sum();
                self.set_list_header(ListHeader {
                    kind: "Playlist".into(),
                    title: "Favorites".into(),
                    subtitle: format!("{} song{} · {}", liked.len(), plural(liked.len()), human_duration(total)).into(),
                    ..Default::default()
                });
                self.list_name = "Favorites".into();
                self.list_play.clear();
                self.set_track_list(liked.iter().map(|t| t.id).collect(), &index, true);
            }
            Page::Playlist(id) => {
                let Some(p) = index.playlist(id).cloned() else {
                    drop(index);
                    self.show_page(Page::Albums, false);
                    return;
                };
                let total: u64 = p.tracks.iter().filter_map(|t| index.track(*t)).map(|t| t.duration_ms).sum();
                let art = p.tracks.iter().filter_map(|t| index.track(*t)).find_map(|t| t.artwork.clone());
                let (cover, has_cover) = match art.as_deref().and_then(|k| self.images.get_now(k, Size::Thumb)) {
                    Some(img) => (img, true),
                    None => (Image::default(), false),
                };
                self.set_list_header(ListHeader {
                    kind: "Playlist".into(),
                    title: p.name.clone().into(),
                    subtitle: format!("{} song{} · {}", p.tracks.len(), plural(p.tracks.len()), human_duration(total)).into(),
                    cover,
                    has_cover,
                    seed: seed_color(&p.name),
                    id: p.id as i32,
                    editable: true,
                });
                self.list_name = p.name.clone();
                self.list_play.clear();
                self.set_track_list(p.tracks.clone(), &index, true);
            }
            Page::History => {
                let recent = index.recently_played(300);
                self.set_list_header(ListHeader {
                    kind: "History".into(),
                    title: "Recently Played".into(),
                    subtitle: "Songs you listened to, newest first".into(),
                    ..Default::default()
                });
                self.list_name = "Recently Played".into();
                self.list_play.clear();
                self.set_track_list(recent.iter().map(|t| t.id).collect(), &index, true);
            }
            Page::Settings => self.update_settings_ui(),
            Page::Editor => {}
        }
    }

    fn set_albums_header(&self, title: &str, subtitle: &str, sortable: bool) {
        if let Some(ui) = self.ui.upgrade() {
            let state = ui.global::<AppState>();
            state.set_albums_title(title.into());
            state.set_albums_subtitle(subtitle.into());
            state.set_albums_sortable(sortable);
            state.set_album_sort(self.settings.album_sort);
        }
    }

    fn set_list_header(&self, header: ListHeader) {
        if let Some(ui) = self.ui.upgrade() {
            let state = ui.global::<AppState>();
            state.set_list_header(header);
            state.set_track_sort(self.settings.track_sort);
        }
    }

    fn set_album_grid(&self, model: &Rc<LazyModel<AlbumRow>>, ids: Vec<AlbumId>, index: &Arc<LibraryIndex>) {
        let cols = self.grid_columns.max(1);
        let index = index.clone();
        let images = self.images.clone();
        let handles = self.handles.clone();
        let playing = self.current.as_ref().map(|t| t.album_id);
        let rows = ids.len().div_ceil(cols);
        model.set(rows, move |row, need| {
            let items: Vec<AlbumCard> = ids[row * cols..((row + 1) * cols).min(ids.len())]
                .iter()
                .filter_map(|id| index.album(*id))
                .map(|a| {
                    let (cover, has_cover) = thumb(&images, a.artwork.as_deref(), need);
                    AlbumCard {
                        id: handles.borrow_mut().get(a.id),
                        title: a.title.clone().into(),
                        artist: a.artist.clone().into(),
                        year: a.year.map(|y| y.to_string()).unwrap_or_default().into(),
                        cover,
                        has_cover,
                        hires: a.format.is_hires(),
                        playing: playing == Some(a.id),
                        seed: seed_color(&a.title),
                    }
                })
                .collect();
            AlbumRow { items: ModelRc::new(VecModel::from(items)) }
        });
    }

    fn set_track_list(&mut self, ids: Vec<TrackId>, index: &Arc<LibraryIndex>, show_album: bool) {
        self.list_ids = ids.clone();
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<AppState>().set_tracks_show_album(show_album);
        }
        let index = index.clone();
        let images = self.images.clone();
        let current = self.current.as_ref().map(|t| t.id);
        self.models.tracks.set(ids.len(), move |row, need| {
            let Some(t) = index.track(ids[row]) else { return TrackRow::default() };
            let (cover, has_cover) = thumb(&images, t.artwork.as_deref(), need);
            TrackRow {
                id: t.id as i32,
                index: row as i32,
                number: (row + 1).to_string().into(),
                title: t.title.clone().into(),
                artist: t.display_artist().into(),
                album: t.album.clone().into(),
                album_id: 0,
                year: t.year.map(|y| y.to_string()).unwrap_or_default().into(),
                duration: format_duration_ms(t.duration_ms).into(),
                plays: if t.play_count > 0 { t.play_count.to_string().into() } else { SharedString::new() },
                liked: t.liked,
                current: current == Some(t.id),
                cover,
                has_cover,
                disc_header: SharedString::new(),
            }
        });
    }

    fn show_album(&mut self, id: AlbumId, index: &Arc<LibraryIndex>) {
        let Some(album) = index.album(id).cloned() else { return };
        let tracks = index.album_tracks(id);
        self.album_ids = tracks.iter().map(|t| t.id).collect();
        let handle = self.handles.borrow_mut().get(album.id);
        let (cover, has_cover) = match album.artwork.as_deref().and_then(|k| self.images.get_now(k, Size::Large)) {
            Some(img) => (img, true),
            None => (Image::default(), false),
        };
        let mut meta = Vec::new();
        if let Some(y) = album.year {
            meta.push(y.to_string());
        }
        meta.push(format!("{} song{}, {}", tracks.len(), plural(tracks.len()), human_duration(album.duration_ms)));
        if let Some(g) = &album.genre {
            meta.push(g.clone());
        }
        let f = &album.format;
        let mut tags = vec![f.codec.clone()];
        let quality = f.long_quality();
        if !quality.is_empty() {
            tags.push(quality);
        }
        if f.is_lossless() {
            if let Some(kbps) = f.bitrate_kbps.filter(|k| *k >= 100) {
                tags.push(format!("{} kbps", thousands(kbps as usize)));
            }
        }
        let note = format!(
            "Added {} · {}",
            relative_day(album.added_at),
            album.folder.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
        );
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<AppState>().set_album(AlbumHeader {
                id: handle,
                title: album.title.clone().into(),
                artist: album.artist.clone().into(),
                meta: format!("· {}", meta.join(" · ")).into(),
                cover,
                has_cover,
                seed: seed_color(&album.title),
                tags: ModelRc::new(VecModel::from(tags.into_iter().map(SharedString::from).collect::<Vec<_>>())),
                hires: f.is_hires(),
                note: note.into(),
            });
        }
        let multi_disc = tracks.iter().filter_map(|t| t.disc_number).max().unwrap_or(1) > 1;
        let images = self.images.clone();
        let current = self.current.as_ref().map(|t| t.id);
        let album_artist = album.artist.clone();
        let rows: Vec<TrackRef> = tracks.clone();
        self.models.album_tracks.set(rows.len(), move |row, _need| {
            let t = &rows[row];
            let disc_header = if multi_disc && (row == 0 || rows[row - 1].disc_number != t.disc_number) {
                format!("Disc {}", t.disc_number.unwrap_or(1))
            } else {
                String::new()
            };
            let _ = &images;
            TrackRow {
                id: t.id as i32,
                index: row as i32,
                number: t.track_number.map(|n| n.to_string()).unwrap_or_else(|| (row + 1).to_string()).into(),
                title: t.title.clone().into(),
                artist: if t.artist.is_empty() || t.artist == album_artist { SharedString::new() } else { t.artist.clone().into() },
                album: t.album.clone().into(),
                album_id: 0,
                year: SharedString::new(),
                duration: format_duration_ms(t.duration_ms).into(),
                plays: if t.play_count > 0 { t.play_count.to_string().into() } else { SharedString::new() },
                liked: t.liked,
                current: current == Some(t.id),
                cover: Image::default(),
                has_cover: false,
                disc_header: disc_header.into(),
            }
        });
        let others: Vec<AlbumId> = index.artist_albums(&album.artist).iter().filter(|a| a.id != album.id).map(|a| a.id).collect();
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<AppState>().set_more_title(format!("More by {}", album.artist).into());
        }
        self.set_album_grid(&self.models.more_albums.clone(), others, index);
    }

    fn album_title(&self) -> String {
        match &self.page {
            Page::Album(id) => self.library.index().album(*id).map(|a| a.title.clone()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    // ---------------------------------------------------------------------------------------
    // UI state

    fn refresh_library_views(&mut self) {
        let index = self.library.index();
        let s = index.stats().clone();
        if let Some(ui) = self.ui.upgrade() {
            let state = ui.global::<AppState>();
            state.set_count_albums(if s.albums > 0 { thousands(s.albums).into() } else { SharedString::new() });
            state.set_count_artists(if s.artists > 0 { thousands(s.artists).into() } else { SharedString::new() });
            state.set_count_songs(if s.tracks > 0 { thousands(s.tracks).into() } else { SharedString::new() });
            state.set_favorites_count(index.liked().len() as i32);
            state.set_library_empty(self.library.folders().is_empty() && s.tracks == 0);
            state.set_library_summary(
                format!("{} songs · {}", thousands(s.tracks), human_bytes(s.bytes)).into(),
            );
        }
        drop(index);
        self.update_playlists_nav();
    }

    fn update_playlists_nav(&mut self) {
        let index = self.library.index();
        let playlists: Vec<(i64, String, usize, Option<String>)> = index
            .playlists()
            .iter()
            .map(|p| {
                let art = p.tracks.iter().filter_map(|t| index.track(*t)).find_map(|t| t.artwork.clone());
                (p.id, p.name.clone(), p.tracks.len(), art)
            })
            .collect();
        let images = self.images.clone();
        self.models.playlists.set(playlists.len(), move |row, need| {
            let (id, name, count, art) = &playlists[row];
            let (cover, has_cover) = thumb(&images, art.as_deref(), need);
            NavPlaylist { id: *id as i32, name: name.into(), count: *count as i32, cover, has_cover, seed: seed_color(name) }
        });
    }

    fn queue_changed_ui(&mut self) {
        let index = self.library.index();
        let entries = self.queue.entries().to_vec();
        let start = self.queue.current_index().map_or(0, |c| c + 1);
        let mut manual = Vec::new();
        let mut context = Vec::new();
        let mut i = start;
        while i < entries.len() && entries[i].manual {
            manual.push(i);
            i += 1;
        }
        while i < entries.len() && manual.len() + context.len() < QUEUE_VISIBLE {
            context.push(i);
            i += 1;
        }
        let hidden = entries.len().saturating_sub(i);
        let build = |indices: Vec<usize>| {
            let index = index.clone();
            let images = self.images.clone();
            let entries = entries.clone();
            move |row: usize, need: &mut dyn FnMut(&str)| {
                let qi = indices[row];
                let Some(t) = index.track(entries[qi].track) else { return QueueItem::default() };
                let (cover, has_cover) = thumb(&images, t.artwork.as_deref(), need);
                QueueItem {
                    index: qi as i32,
                    title: t.title.clone().into(),
                    artist: t.display_artist().into(),
                    duration: format_duration_ms(t.duration_ms).into(),
                    cover,
                    has_cover,
                }
            }
        };
        self.models.queue_manual.set(manual.len(), build(manual));
        self.models.queue_context.set(context.len(), build(context));
        if let Some(ui) = self.ui.upgrade() {
            let mut name = self.queue.context().to_string();
            if hidden > 0 {
                name = format!("{name} (+{} more)", thousands(hidden));
            }
            ui.global::<AppState>().set_queue_context_name(name.into());
        }
    }

    fn update_now_playing(&mut self) {
        let np = match &self.current {
            Some(t) => {
                let (cover, has_cover) = match t.artwork.as_deref().and_then(|k| self.images.get_now(k, Size::Large)) {
                    Some(img) => (img, true),
                    None => (Image::default(), false),
                };
                let handle = self.handles.borrow_mut().get(t.album_id);
                NowPlaying {
                    has_track: true,
                    id: t.id as i32,
                    title: t.title.clone().into(),
                    artist: t.display_artist().into(),
                    album: t.album.clone().into(),
                    album_id: handle,
                    cover,
                    has_cover,
                    seed: seed_color(&t.album),
                    format: short_format(&t.format).into(),
                    quality: long_format(&t.format).into(),
                    liked: t.liked,
                }
            }
            None => NowPlaying::default(),
        };
        self.for_players(|p| p.set_now(np.clone()));
        self.sync_mini_helper();
    }

    fn update_player_state(&mut self) {
        let playing = self.status == PlaybackStatus::Playing;
        let shuffle = self.queue.shuffle();
        let repeat = match self.queue.repeat() {
            RepeatMode::Off => 0,
            RepeatMode::All => 1,
            RepeatMode::One => 2,
        };
        let (volume, muted) = (self.settings.volume, self.settings.muted);
        self.for_players(|p| {
            p.set_playing(playing);
            p.set_shuffle(shuffle);
            p.set_repeat(repeat);
            p.set_volume(volume);
            p.set_muted(muted);
        });
        if let Some(m) = &self.media {
            m.set_playback(match self.status {
                PlaybackStatus::Playing => Playback::Playing,
                PlaybackStatus::Paused => Playback::Paused,
                PlaybackStatus::Stopped => Playback::Stopped,
            });
        }
        self.update_progress_ui();
    }

    fn update_progress_ui(&mut self) {
        let (pos, dur) = (self.position_ms, self.duration_ms);
        let progress = if dur > 0 { pos as f32 / dur as f32 } else { 0.0 };
        let pos_text: SharedString = format_duration_ms(pos).into();
        let dur_text: SharedString = format_duration_ms(dur).into();
        self.for_players(|p| {
            p.set_progress(progress);
            p.set_duration_ms(dur.min(i32::MAX as u64) as i32);
            if p.get_position_text() != pos_text {
                p.set_position_text(pos_text.clone());
            }
            if p.get_duration_text() != dur_text {
                p.set_duration_text(dur_text.clone());
            }
        });
        self.sync_mini_helper();
    }

    fn for_players(&self, f: impl Fn(&Player)) {
        if let Some(ui) = self.ui.upgrade() {
            f(&ui.global::<Player>());
        }
        if let Some(mini) = &self.mini {
            f(&mini.global::<Player>());
        }
    }

    fn load_lyrics(&mut self) {
        self.lyrics = self.current.as_ref().and_then(|t| lychee_library::lyrics::load(&t.path));
        self.lyric_index = -1;
        let Some(ui) = self.ui.upgrade() else { return };
        let state = ui.global::<AppState>();
        match &self.lyrics {
            Some(l) => {
                let lines: Vec<Lyric> =
                    l.lines.iter().map(|line| Lyric { text: line.text.clone().into(), time_ms: line.time_ms.unwrap_or(0) as i32 }).collect();
                state.set_lyrics(ModelRc::new(VecModel::from(lines)));
                state.set_lyrics_synced(l.synced);
                state.set_lyrics_source(if l.synced { format!("Synced lyrics · {}", l.source) } else { l.source.clone() }.into());
            }
            None => {
                state.set_lyrics(ModelRc::default());
                state.set_lyrics_synced(false);
                state.set_lyrics_source(SharedString::new());
            }
        }
        state.set_lyric_index(-1);
    }

    fn update_lyric_index(&mut self) {
        let idx = self.lyrics.as_ref().and_then(|l| l.line_at(self.position_ms)).map_or(-1, |i| i as i32);
        if idx != self.lyric_index {
            self.lyric_index = idx;
            if let Some(ui) = self.ui.upgrade() {
                ui.global::<AppState>().set_lyric_index(idx);
            }
        }
    }

    fn update_details(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut rows: Vec<InfoRow> = Vec::new();
        let mut add = |label: &str, value: String| {
            if !value.is_empty() {
                rows.push(InfoRow { label: label.into(), value: value.into() });
            }
        };
        if let Some(t) = &self.current {
            add("Title", t.title.clone());
            add("Artist", t.display_artist().to_string());
            add("Album", t.album.clone());
            if t.album_artist != t.artist {
                add("Album artist", t.album_artist.clone());
            }
            let number = match (t.track_number, t.disc_number) {
                (Some(n), Some(d)) if d > 1 => format!("Track {n}, disc {d}"),
                (Some(n), _) => format!("Track {n}"),
                _ => String::new(),
            };
            add("Position", number);
            add("Year", t.year.map(|y| y.to_string()).unwrap_or_default());
            add("Genre", t.genre.clone().unwrap_or_default());
            add("Composer", t.composer.clone().unwrap_or_default());
            add("Format", long_format(&t.format));
            if let Some(info) = &self.stream_info {
                add("Decoder", format!("{} · {} Hz · {} ch", info.codec, info.sample_rate, info.channels));
            }
            add("Output", self.output.clone());
            add("Duration", format_duration_ms(t.duration_ms));
            add("File size", human_bytes(t.file_size));
            add("Plays", if t.play_count > 0 { t.play_count.to_string() } else { "Never".into() });
            add("Added", relative_day(t.added_at));
            add("File", t.path.display().to_string());
        }
        ui.global::<AppState>().set_details(ModelRc::new(VecModel::from(rows)));
    }

    fn update_settings_ui(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let state = ui.global::<AppState>();
        let folders: Vec<SharedString> = self.library.folders().iter().map(|p| p.display().to_string().into()).collect();
        state.set_folders(ModelRc::new(VecModel::from(folders)));
        let selected = self.settings.output_device.clone().unwrap_or_else(|| "Default Audio Device".into());
        let mut names = self.devices.clone();
        if !names.iter().any(|n| *n == selected) {
            names.insert(0, selected.clone());
        }
        let choices: Vec<Choice> = names.iter().map(|n| Choice { label: n.into(), selected: *n == selected }).collect();
        state.set_devices(ModelRc::new(VecModel::from(choices)));
        let index = self.library.index();
        let s = index.stats();
        state.set_library_summary(
            format!(
                "{} songs · {} albums · {} · {}% lossless",
                thousands(s.tracks),
                thousands(s.albums),
                human_bytes(s.bytes),
                if s.tracks > 0 { s.lossless_tracks * 100 / s.tracks } else { 0 }
            )
            .into(),
        );
        state.set_library_empty(self.library.folders().is_empty() && s.tracks == 0);
        drop(index);
        self.update_lastfm_ui();
    }

    fn is_dark(&self) -> bool {
        match self.settings.theme {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => self.system_dark,
        }
    }

    fn apply_theme(&mut self) {
        let dark = self.is_dark();
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Theme>().set_dark(dark);
        }
        if let Some(mini) = &self.mini {
            mini.global::<Theme>().set_dark(dark);
        }
        self.sync_mini_helper();
    }

    fn apply_accent(&mut self) {
        let index = self.library.index();
        let album = match &self.page {
            Page::Album(id) => index.album(*id),
            _ => None,
        }
        .or_else(|| self.current.as_ref().and_then(|t| index.album(t.album_id)));
        let rgb = if self.settings.dynamic_accent { album.and_then(|a| a.accent) } else { None };
        let tint_rgb = if self.settings.dynamic_accent { album.and_then(|a| a.tint.or(a.accent)) } else { None };
        let accent = rgb_color(rgb.unwrap_or(DEFAULT_ACCENT));
        let tint = tint_color(tint_rgb.unwrap_or(0x5A6B9A));
        if let Some(ui) = self.ui.upgrade() {
            let theme = ui.global::<Theme>();
            theme.set_accent(accent);
            theme.set_tint(tint);
        }
        // The mini player always follows what is playing.
        let playing_rgb = if self.settings.dynamic_accent {
            self.current.as_ref().and_then(|t| index.album(t.album_id)).and_then(|a| a.accent)
        } else {
            None
        };
        if let Some(mini) = &self.mini {
            mini.global::<Theme>().set_accent(rgb_color(playing_rgb.unwrap_or(DEFAULT_ACCENT)));
        }
        drop(index);
        self.sync_mini_helper();
    }

    fn toast(&mut self, text: &str) {
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<AppState>().set_toast(text.into());
        }
        self.toast_until = Some(Instant::now() + Duration::from_millis(2800));
    }

    fn open_mini(&mut self) {
        if self.mini_helper.is_some() || self.mini.as_ref().is_some_and(|m| m.window().is_visible()) {
            return;
        }
        if self.wants_mini_helper() {
            match mini::External::spawn() {
                Ok(helper) => {
                    // The main window steps aside once the helper reports that it is on screen.
                    self.mini_helper = Some(helper);
                    self.sync_mini_helper();
                    return;
                }
                Err(e) => {
                    tracing::warn!("could not start the mini player helper: {e}");
                    self.mini_helper_failed = true;
                }
            }
        }
        if self.mini.is_none() {
            match MiniWindow::new() {
                Ok(mini) => {
                    if let Some(d) = self.dispatcher.upgrade() {
                        wire_mini(&mini, &d);
                    }
                    let d = self.dispatcher.clone();
                    mini.window().on_close_requested(move || {
                        if let Some(d) = d.upgrade() {
                            d.dispatch(Action::CloseMini);
                        }
                        slint::CloseRequestResponse::HideWindow
                    });
                    self.mini = Some(mini);
                }
                Err(e) => {
                    self.toast(&format!("Could not open the mini player: {e}"));
                    return;
                }
            }
        }
        self.apply_theme();
        self.apply_accent();
        self.update_now_playing();
        self.update_player_state();
        if let Some(mini) = &self.mini {
            let _ = mini.show();
            mini::keep_on_top(mini);
        }
        self.main_steps_aside();
    }

    /// Wayland has no way to keep a window on top, so there the mini player runs as an XWayland
    /// helper process (unless the user turned "keep on top" off, or XWayland isn't there).
    fn wants_mini_helper(&mut self) -> bool {
        match std::env::var("LYCHEE_MINI").as_deref() {
            Ok("external") => return true,
            Ok("inprocess") => return false,
            _ => {}
        }
        cfg!(target_os = "linux")
            && self.settings.mini_on_top
            && !self.mini_helper_failed
            && std::env::var_os("DISPLAY").is_some()
            && self.main_is_wayland()
    }

    fn main_is_wayland(&mut self) -> bool {
        if self.wayland.is_none() {
            self.wayland = self.ui.upgrade().filter(|ui| ui.window().is_visible()).map(|ui| mini::is_wayland(ui.window()));
        }
        self.wayland.unwrap_or(false)
    }

    /// The mini player is up: get the main window out of the way.
    fn main_steps_aside(&mut self) {
        let wayland = self.main_is_wayland();
        let Some(ui) = self.ui.upgrade() else { return };
        ui.global::<AppState>().set_mini_open(true);
        if wayland {
            // A minimised Wayland window can only be brought back by the user, so hide it
            // completely; `RaiseMain` maps it again.
            let _ = ui.hide();
        } else {
            use slint::winit_030::WinitWindowAccessor;
            ui.window().with_winit_window(|w| w.set_minimized(true));
        }
    }

    fn on_mini_event(&mut self, event: MiniEvent) {
        match event {
            MiniEvent::Ready => {
                if let Some(helper) = &mut self.mini_helper {
                    helper.ready = true;
                }
                self.main_steps_aside();
            }
            MiniEvent::Command(MiniCommand::TogglePlay) => self.toggle_play(),
            MiniEvent::Command(MiniCommand::Next) => self.next(),
            MiniEvent::Command(MiniCommand::Previous) => self.previous(),
            MiniEvent::Command(MiniCommand::Close) => self.handle(Action::CloseMini),
            MiniEvent::Exited => {
                let was_ready = self.mini_helper.take().is_some_and(|h| h.ready);
                if was_ready {
                    self.handle(Action::CloseMini);
                } else {
                    // It never got a window up (no XWayland?): use the in-process mini player.
                    tracing::warn!("the mini player helper exited early; using an in-process window");
                    self.mini_helper_failed = true;
                    self.open_mini();
                }
            }
        }
    }

    fn sync_mini_helper(&mut self) {
        if self.mini_helper.is_none() {
            return;
        }
        let state = self.mini_state();
        if let Some(helper) = &mut self.mini_helper {
            helper.send(&state);
        }
    }

    fn mini_state(&self) -> MiniState {
        let index = self.library.index();
        let accent = if self.settings.dynamic_accent {
            self.current.as_ref().and_then(|t| index.album(t.album_id)).and_then(|a| a.accent)
        } else {
            None
        };
        let mut state = MiniState { dark: self.is_dark(), accent: accent.unwrap_or(DEFAULT_ACCENT), ..MiniState::default() };
        if let Some(t) = &self.current {
            let (pos, dur) = (self.position_ms, self.duration_ms);
            state.has_track = true;
            state.title = t.title.clone();
            state.artist = t.display_artist().to_string();
            state.format = short_format(&t.format);
            state.position = format_duration_ms(pos);
            state.duration = format_duration_ms(dur);
            state.progress = if dur > 0 { pos as f32 / dur as f32 } else { 0.0 };
            state.playing = self.status == PlaybackStatus::Playing;
            state.cover = t.artwork.as_deref().map(|k| self.library.artwork_large(k));
            state.seed = seed_color(&t.album).as_argb_encoded();
        }
        state
    }

    // ---------------------------------------------------------------------------------------
    // Session

    fn restore_session(&mut self) {
        let Some(json) = self.library.get_kv("session") else { return };
        let Ok(session) = serde_json::from_str::<SessionState>(&json) else { return };
        let index = self.library.index();
        let entries: Vec<(TrackId, bool)> = session.entries.into_iter().filter(|(id, _)| index.contains(*id)).collect();
        if entries.is_empty() {
            return;
        }
        let current = session.current.map(|c| c.min(entries.len() - 1));
        self.queue.restore(&entries, current, self.settings.shuffle, self.settings.repeat, session.context);
        if let Some(track) = self.queue.current().and_then(|id| index.track(id)) {
            drop(index);
            let pos = session.position_ms.min(track.duration_ms.saturating_sub(1000));
            self.set_current(track, pos, false);
            self.engine_loaded = false;
        }
    }

    fn save_session(&mut self) {
        self.last_session_save = Instant::now();
        let (entries, current) = self.queue.snapshot();
        let state = SessionState { entries, current, position_ms: self.position_ms, context: self.queue.context().to_string() };
        if let Ok(json) = serde_json::to_string(&state) {
            self.library.set_kv("session", &json);
        }
    }

    #[allow(dead_code)]
    pub fn attach_media_window(&self, hwnd: isize) {
        if let Some(m) = &self.media {
            m.attach_window(hwnd);
        }
    }

    /// Drive the UI from a script; used by headless screenshots (`screenshot.rs`).
    pub fn run_scene(&mut self, step: &str) {
        let (verb, arg) = step.split_once(':').unwrap_or((step, ""));
        let find_album = |app: &App, title: &str| {
            app.library.index().albums(AlbumSort::Title).iter().find(|a| a.title.eq_ignore_ascii_case(title)).map(|a| a.id)
        };
        match verb {
            "page" => self.handle(Action::Navigate(arg.into())),
            "album" => {
                if let Some(id) = find_album(self, arg) {
                    self.show_page(Page::Album(id), true);
                }
            }
            "artist" => self.show_page(Page::Artist(arg.to_string()), true),
            "play-album" => {
                if let Some(id) = find_album(self, arg) {
                    self.play_album(id, false);
                }
            }
            "track" => {
                if let Ok(n) = arg.parse::<usize>() {
                    if self.queue.jump(n).is_some() {
                        self.start_current(true, 0);
                    }
                }
            }
            "seek" => {
                if let Ok(ms) = arg.parse::<u64>() {
                    self.seek_to(ms);
                }
            }
            "pause" => {
                if self.status == PlaybackStatus::Playing {
                    self.toggle_play();
                }
            }
            "palette" => {
                self.open_palette();
                if let Some(ui) = self.ui.upgrade() {
                    ui.global::<AppState>().set_palette_query(arg.into());
                }
                self.palette_search(arg);
            }
            "tab" => {
                if let Some(ui) = self.ui.upgrade() {
                    ui.global::<AppState>().set_right_tab(arg.into());
                    ui.global::<AppState>().set_right_panel(true);
                }
            }
            "theme" => self.handle(Action::SetThemeMode(arg.parse().unwrap_or(1))),
            "like" => self.handle(Action::ToggleLikeCurrent),
            "queue" => {
                if let Some(id) = find_album(self, arg) {
                    let tracks = self.library.index().album(id).map(|a| a.tracks.clone()).unwrap_or_default();
                    self.queue.add(&tracks[..tracks.len().min(3)]);
                    self.queue_changed();
                }
            }
            "toast" => self.toast(arg),
            "add-folder" => {
                let _ = self.library.add_folder(std::path::Path::new(arg));
                if let Err(e) = self.library.scan(&|_| {}) {
                    tracing::warn!("scan failed: {e}");
                }
                self.refresh_library_views();
                self.refresh_page();
                self.update_settings_ui();
            }
            "like-album" => {
                if let Some(id) = find_album(self, arg) {
                    let tracks = self.library.index().album(id).map(|a| a.tracks.clone()).unwrap_or_default();
                    for t in tracks.iter().step_by(2) {
                        let _ = self.library.set_liked(*t, true);
                        let _ = self.library.record_play(*t);
                    }
                    self.refresh_library_views();
                }
            }
            "edit-album" => {
                if let Some(id) = find_album(self, arg) {
                    let handle = self.handles.borrow_mut().get(id);
                    self.handle(Action::AlbumAction("edit-tags".into(), handle));
                }
            }
            "editor-select" => {
                for (n, i) in arg.split(',').filter_map(|s| s.trim().parse::<i32>().ok()).enumerate() {
                    self.handle(Action::Editor(EditorAction::Select(i, n > 0, false)));
                }
            }
            "editor-tab" => {
                if let Some(ui) = self.ui.upgrade() {
                    ui.global::<crate::Editor>().set_tab(arg.into());
                }
            }
            "editor-field" => {
                if let Some((id, value)) = arg.split_once('=') {
                    self.handle(Action::Editor(EditorAction::SetField(id.into(), value.into())));
                }
            }
            "editor-more" => {
                if let Some(ui) = self.ui.upgrade() {
                    ui.global::<crate::Editor>().set_more_fields(true);
                }
            }
            "editor-scanner" => self.handle(Action::Editor(EditorAction::OpenScanner(arg.parse().unwrap_or(0)))),
            "editor-mask" => {
                if let Some(ui) = self.ui.upgrade() {
                    let e = ui.global::<crate::Editor>();
                    if e.get_scanner_mode() == 1 { e.set_rename_mask(arg.into()) } else { e.set_fill_mask(arg.into()) }
                }
                self.handle(Action::Editor(EditorAction::ScannerChanged));
            }
            "editor-lookup" => self.handle(Action::Editor(EditorAction::LookupStart)),
            "editor-lookup-apply" => self.handle(Action::Editor(EditorAction::LookupApply)),
            "editor-open" => {
                self.editor.open(vec![PathBuf::from(arg)], false, Vec::new(), None);
                self.show_page(Page::Editor, true);
            }
            "editor-save" => self.handle(Action::Editor(EditorAction::Save)),
            "editor-quit" => {
                self.request_close();
            }
            _ => tracing::warn!("unknown scene step {step}"),
        }
    }

    pub fn shutdown(&mut self) {
        self.save_session();
        self.settings.right_panel = self.ui.upgrade().map(|ui| ui.global::<AppState>().get_right_panel()).unwrap_or(true);
        self.settings.right_tab =
            self.ui.upgrade().map(|ui| ui.global::<AppState>().get_right_tab().to_string()).unwrap_or_else(|| "queue".into());
        self.settings.save(&self.paths.config_file);
        self.mini_helper = None;
        self.engine.stop();
    }
}

// -------------------------------------------------------------------------------------------
// Helpers

fn thumb(images: &ImageCache, key: Option<&str>, need: &mut dyn FnMut(&str)) -> (Image, bool) {
    match key {
        Some(k) => match images.get(k, Size::Thumb) {
            Some(img) => (img, true),
            None => {
                need(k);
                (Image::default(), false)
            }
        },
        None => (Image::default(), false),
    }
}

fn icon_by_name(icons: &Icons, name: &str) -> Image {
    match name {
        "play" => icons.get_play(),
        "next" => icons.get_next(),
        "prev" => icons.get_prev(),
        "shuffle" => icons.get_shuffle(),
        "repeat" => icons.get_repeat(),
        "trash" => icons.get_trash(),
        "mini" => icons.get_mini(),
        "lyrics" => icons.get_lyrics(),
        "sidebar" => icons.get_sidebar(),
        "moon" => icons.get_moon(),
        "disc" => icons.get_disc(),
        "user" => icons.get_user(),
        "music" => icons.get_music(),
        "heart" => icons.get_heart(),
        "settings" => icons.get_settings(),
        "folder-plus" => icons.get_folder_plus(),
        "refresh" => icons.get_refresh(),
        "scrobble" => icons.get_scrobble(),
        "edit" => icons.get_edit(),
        _ => icons.get_info(),
    }
}

fn rgb_color(rgb: u32) -> Color {
    Color::from_rgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

fn hsl(h: f32, s: f32, l: f32) -> Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h % 360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    Color::from_rgb_f32(r + m, g + m, b + m)
}

fn to_hsl(rgb: u32) -> (f32, f32, f32) {
    let (r, g, b) = (((rgb >> 16) & 255) as f32 / 255.0, ((rgb >> 8) & 255) as f32 / 255.0, (rgb & 255) as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

/// A deep, desaturated version of the accent for header backgrounds.
fn tint_color(rgb: u32) -> Color {
    let (h, s, _) = to_hsl(rgb);
    hsl(h, (s * 0.55).min(0.5), 0.17)
}

/// Placeholder colour derived from a name, so the same album always gets the same tile.
fn seed_color(text: &str) -> Color {
    let mut h: u32 = 0x811c_9dc5;
    for b in text.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    hsl((h % 360) as f32, 0.32, 0.36)
}

fn genre_color(text: &str) -> Color {
    let mut h: u32 = 0x811c_9dc5;
    for b in text.to_lowercase().bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    hsl((h % 360) as f32, 0.45, 0.42)
}

fn short_format(f: &AudioFormat) -> String {
    let q = f.short_quality();
    if q.is_empty() { f.codec.clone() } else { format!("{} {}", f.codec, q) }
}

fn long_format(f: &AudioFormat) -> String {
    let mut parts = vec![f.codec.clone()];
    let q = f.long_quality();
    if !q.is_empty() {
        parts.push(q);
    }
    if f.is_lossless() {
        if let Some(kbps) = f.bitrate_kbps.filter(|k| *k >= 100) {
            parts.push(format!("{} kbps", thousands(kbps as usize)));
        }
    }
    parts.join(" · ")
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn human_duration(ms: u64) -> String {
    let mins = ms / 60_000;
    let (days, hours, m) = (mins / 1440, (mins / 60) % 24, mins % 60);
    if days > 0 {
        format!("{days} d {hours} h")
    } else if hours > 0 {
        format!("{hours} h {m} min")
    } else if mins > 0 {
        format!("{mins} min")
    } else {
        format_duration(ms / 1000)
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[unit]) }
}

fn relative_day(unix: i64) -> String {
    if unix <= 0 {
        return String::new();
    }
    let days = (lychee_core::now_unix() - unix) / 86_400;
    match days {
        i64::MIN..=0 => "today".into(),
        1 => "yesterday".into(),
        2..=13 => format!("{days} days ago"),
        14..=59 => format!("{} weeks ago", days / 7),
        60..=729 => format!("{} months ago", days / 30),
        _ => format!("{} years ago", days / 365),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formatting() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(2931), "2,931");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(412 * 1024 * 1024 * 1024), "412.0 GB");
        assert_eq!(human_duration(43 * 60_000), "43 min");
        assert_eq!(human_duration((8 * 24 * 60 + 4 * 60) * 60_000), "8 d 4 h");
    }

    #[test]
    fn handles_round_trip() {
        let mut h = Handles::default();
        let a = h.get(i64::MAX - 5);
        let b = h.get(42);
        assert_ne!(a, b);
        assert_eq!(h.get(42), b);
        assert_eq!(h.album(a), Some(i64::MAX - 5));
        assert_eq!(h.album(99), None);
    }
}
