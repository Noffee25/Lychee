//! The tag editor page: an EasyTAG-style editor over a [`lychee_tagger::Session`].
//!
//! Reading, saving and online lookups run on worker threads and report back through a channel
//! the app polls on its tick. This module keeps the `Editor` global in sync with the session.
//! Async results refer to files by path, since indices change when files are added or closed.

use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use lru::LruCache;
use lychee_library::Library;
use lychee_lookup::Client;
use lychee_lookup::matching::{self, Candidate, FileFacts, Guess, TrackTags};
use lychee_lookup::musicbrainz::Release;
use lychee_tagger::session::{EditFile, SaveJob, Session, Value};
use lychee_tagger::{Case, Cleanup, Field, Mask, Picture, PictureKind};
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

use crate::app::{Action, Dispatcher};
use crate::{AppWindow, Editor, EditorField, EditorFile, EditorPicture, InfoRow, LookupItem, LookupTrack, ScanRow};

#[derive(Debug)]
pub enum EditorAction {
    OpenFolder,
    AddFiles,
    CloseFiles,
    Select(i32, bool, bool),
    SelectAll,
    FileAction(String, i32),
    SetField(String, String),
    NumberTracks,
    Save,
    Revert,
    Undo,
    Redo,
    AddPicture,
    FolderPicture,
    RemovePicture(i32),
    ExportPicture(i32),
    SetPictureKind(i32, i32),
    SetLyrics(String),
    LyricsOnline,
    OpenScanner(i32),
    ScannerChanged,
    ScannerApply,
    LookupStart,
    LookupSearch(String, String),
    LookupPick(i32),
    LookupOptionsChanged,
    LookupApply,
}

pub fn wire(ui: &AppWindow, d: &Rc<Dispatcher>) {
    let e = ui.global::<Editor>();
    macro_rules! on {
        ($setter:ident, || $action:expr) => {{
            let d = d.clone();
            e.$setter(move || d.dispatch(Action::Editor($action)));
        }};
        ($setter:ident, |$($arg:ident),*| $action:expr) => {{
            let d = d.clone();
            e.$setter(move |$($arg),*| d.dispatch(Action::Editor($action)));
        }};
    }
    on!(on_open_folder, || EditorAction::OpenFolder);
    on!(on_add_files, || EditorAction::AddFiles);
    on!(on_close_files, || EditorAction::CloseFiles);
    on!(on_select, |i, ctrl, shift| EditorAction::Select(i, ctrl, shift));
    on!(on_select_all, || EditorAction::SelectAll);
    on!(on_file_action, |what, i| EditorAction::FileAction(what.into(), i));
    on!(on_set_field, |id, text| EditorAction::SetField(id.into(), text.into()));
    on!(on_number_tracks, || EditorAction::NumberTracks);
    on!(on_save, || EditorAction::Save);
    on!(on_revert, || EditorAction::Revert);
    on!(on_undo, || EditorAction::Undo);
    on!(on_redo, || EditorAction::Redo);
    on!(on_add_picture, || EditorAction::AddPicture);
    on!(on_folder_picture, || EditorAction::FolderPicture);
    on!(on_remove_picture, |i| EditorAction::RemovePicture(i));
    on!(on_export_picture, |i| EditorAction::ExportPicture(i));
    on!(on_set_picture_kind, |i, k| EditorAction::SetPictureKind(i, k));
    on!(on_set_lyrics, |text| EditorAction::SetLyrics(text.into()));
    on!(on_lyrics_online, || EditorAction::LyricsOnline);
    on!(on_open_scanner, |mode| EditorAction::OpenScanner(mode));
    on!(on_scanner_changed, || EditorAction::ScannerChanged);
    on!(on_scanner_apply, || EditorAction::ScannerApply);
    on!(on_lookup_start, || EditorAction::LookupStart);
    on!(on_lookup_search, |artist, album| EditorAction::LookupSearch(artist.into(), album.into()));
    on!(on_lookup_pick, |i| EditorAction::LookupPick(i));
    on!(on_lookup_options_changed, || EditorAction::LookupOptionsChanged);
    on!(on_lookup_apply, || EditorAction::LookupApply);
    let d2 = d.clone();
    e.on_quit_save(move || d2.dispatch(Action::QuitSave));
    let d3 = d.clone();
    e.on_quit_discard(move || d3.dispatch(Action::QuitDiscard));
}

enum Msg {
    Picked { paths: Vec<PathBuf>, add: bool },
    LoadProgress { seq: u64, done: usize, total: usize },
    Loaded { seq: u64, files: Vec<EditFile>, add: bool, select: Vec<PathBuf>, title: Option<String> },
    SaveProgress { done: usize, total: usize },
    Saved { results: Vec<(SaveJob, Result<EditFile, String>)> },
    PicturePicked(Vec<u8>),
    LookupProgress { seq: u64, text: String },
    LookupFound { seq: u64, result: Result<Vec<Candidate>, String> },
    LookupRelease { seq: u64, release_id: String, result: Result<Option<Release>, String> },
    LookupCover { seq: u64, release_id: String, bytes: Option<Vec<u8>> },
    Artwork { paths: Vec<PathBuf>, overwrite: bool, bytes: Option<Vec<u8>>, error: Option<String> },
    Lyrics { path: PathBuf, text: Option<String>, overwrite: bool, done: usize, total: usize },
}

/// A lookup in progress or on screen.
struct Lookup {
    seq: u64,
    paths: Vec<PathBuf>,
    facts: Vec<FileFacts>,
    guess: Guess,
    candidates: Vec<Candidate>,
    selected: Option<usize>,
    cover: Option<(String, Image)>,
    /// Releases whose release-group genre was already asked for.
    genre_checked: HashSet<String>,
}

/// What the app should do after an editor action.
#[derive(Default)]
pub struct Outcome {
    pub toast: Option<String>,
    /// Files in the library changed on disk; the old and new paths of every saved file.
    pub library_changed: bool,
    /// All pending changes were saved (or there were none) after a "save and quit".
    pub ready_to_quit: bool,
}

pub struct EditorCtl {
    ui: slint::Weak<AppWindow>,
    library: Arc<Library>,
    client: Arc<Client>,
    session: Session,
    selection: Vec<usize>,
    anchor: Option<usize>,
    files_model: Rc<VecModel<EditorFile>>,
    fields_model: Rc<VecModel<EditorField>>,
    pictures_model: Rc<VecModel<EditorPicture>>,
    generation: i32,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    load_seq: u64,
    lookup_seq: u64,
    lookup: Option<Lookup>,
    /// Blocking work: loading or saving. Edits are refused meanwhile.
    busy: Option<String>,
    /// Background downloads (covers, lyrics), shown but not blocking.
    downloads: usize,
    download_text: String,
    thumbs: LruCache<(usize, usize), (Image, u32, u32)>,
    quit_after_save: bool,
    outcome: Outcome,
}

const PREVIEW_ROWS: usize = 300;

impl EditorCtl {
    pub fn new(ui: &AppWindow, library: Arc<Library>) -> EditorCtl {
        let files_model = Rc::new(VecModel::default());
        let fields: Vec<EditorField> = Field::ALL
            .iter()
            .map(|f| EditorField { id: f.id().into(), label: f.label().into(), ..Default::default() })
            .collect();
        let fields_model = Rc::new(VecModel::from(fields));
        let pictures_model = Rc::new(VecModel::default());
        let e = ui.global::<Editor>();
        e.set_files(ModelRc::from(files_model.clone()));
        e.set_fields(ModelRc::from(fields_model.clone()));
        e.set_pictures(ModelRc::from(pictures_model.clone()));
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut ctl = EditorCtl {
            ui: ui.as_weak(),
            library,
            client: Arc::new(Client::new()),
            session: Session::default(),
            selection: Vec::new(),
            anchor: None,
            files_model,
            fields_model,
            pictures_model,
            generation: 0,
            tx,
            rx,
            load_seq: 0,
            lookup_seq: 0,
            lookup: None,
            busy: None,
            downloads: 0,
            download_text: String::new(),
            thumbs: LruCache::new(NonZeroUsize::new(48).unwrap()),
            quit_after_save: false,
            outcome: Outcome::default(),
        };
        ctl.refresh_all();
        ctl
    }

    pub fn has_unsaved(&self) -> bool {
        self.session.modified_count() > 0
    }

    pub fn take_outcome(&mut self) -> Outcome {
        std::mem::take(&mut self.outcome)
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.outcome.toast = Some(text.into());
    }

    // -----------------------------------------------------------------------------------------
    // Opening files

    /// Open files or folders. Unless `add`, files without unsaved changes are closed first.
    pub fn open(&mut self, paths: Vec<PathBuf>, add: bool, select: Vec<PathBuf>, title: Option<String>) {
        if self.busy.is_some() {
            self.toast("Wait for the editor to finish first");
            return;
        }
        self.load_seq += 1;
        let seq = self.load_seq;
        let tx = self.tx.clone();
        self.set_busy(Some("Reading files…".into()));
        std::thread::Builder::new()
            .name("lychee-editor-read".into())
            .spawn(move || {
                let files = expand(&paths);
                let total = files.len();
                let done = std::sync::atomic::AtomicUsize::new(0);
                let chunks: Vec<&[PathBuf]> = files.chunks(files.len().div_ceil(4).max(1)).collect();
                let read: Vec<Vec<EditFile>> = std::thread::scope(|s| {
                    let handles: Vec<_> = chunks
                        .into_iter()
                        .map(|chunk| {
                            let (tx, done) = (&tx, &done);
                            s.spawn(move || {
                                chunk
                                    .iter()
                                    .map(|p| {
                                        let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                                        if n % 20 == 0 {
                                            let _ = tx.send(Msg::LoadProgress { seq, done: n, total });
                                        }
                                        EditFile::open(p)
                                    })
                                    .collect::<Vec<_>>()
                            })
                        })
                        .collect();
                    handles.into_iter().filter_map(|h| h.join().ok()).collect()
                });
                let files = read.into_iter().flatten().collect();
                let _ = tx.send(Msg::Loaded { seq, files, add, select, title });
            })
            .ok();
    }

    fn pick_files(&mut self, folder: bool, add: bool) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let paths = if folder {
                rfd::FileDialog::new().set_title("Open a folder in the tag editor").pick_folder().map(|p| vec![p])
            } else {
                rfd::FileDialog::new()
                    .set_title("Add files to the tag editor")
                    .add_filter("Audio", lychee_audio::SUPPORTED_EXTENSIONS)
                    .pick_files()
            };
            if let Some(paths) = paths.filter(|p| !p.is_empty()) {
                let _ = tx.send(Msg::Picked { paths, add });
            }
        });
    }

    fn on_loaded(&mut self, files: Vec<EditFile>, add: bool, select: Vec<PathBuf>, title: Option<String>) {
        self.set_busy(None);
        if files.is_empty() {
            self.toast("No audio files there");
            return;
        }
        let unreadable = files.iter().filter(|f| f.error.is_some()).count();
        let mut kept = 0;
        if !add {
            // Never drop unsaved work: keep modified files around.
            let close: Vec<usize> = (0..self.session.len()).filter(|&i| !self.session.files()[i].is_modified()).collect();
            kept = self.session.len() - close.len();
            self.session.remove(&close);
        }
        let opened: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
        self.session.add(files);
        let wanted: HashSet<&PathBuf> = if select.is_empty() { opened.iter().collect() } else { select.iter().collect() };
        self.selection = (0..self.session.len()).filter(|&i| wanted.contains(&self.session.files()[i].path)).collect();
        self.anchor = self.selection.first().copied();
        if let Some(title) = title {
            if let Some(ui) = self.ui.upgrade() {
                ui.global::<Editor>().set_title(title.into());
            }
        } else if !add {
            let title = common_folder(&opened).map_or_else(|| "Tag editor".to_string(), |d| d);
            if let Some(ui) = self.ui.upgrade() {
                ui.global::<Editor>().set_title(title.into());
            }
        }
        self.refresh_all();
        let mut notes = Vec::new();
        if kept > 0 {
            notes.push(format!("kept {kept} file{} with unsaved changes", plural(kept)));
        }
        if unreadable > 0 {
            notes.push(format!("{unreadable} file{} couldn't be read", plural(unreadable)));
        }
        if !notes.is_empty() {
            self.toast(capitalize(&notes.join("; ")));
        }
    }

    // -----------------------------------------------------------------------------------------
    // Actions

    pub fn handle(&mut self, action: EditorAction) {
        let blocking_ok = matches!(
            action,
            EditorAction::Select(..)
                | EditorAction::SelectAll
                | EditorAction::FileAction(..)
                | EditorAction::LookupPick(_)
                | EditorAction::LookupOptionsChanged
                | EditorAction::ScannerChanged
                | EditorAction::OpenScanner(_)
        );
        if self.busy.is_some() && !blocking_ok {
            // Typing during a save would be lost when the files are read back.
            self.refresh_form();
            return;
        }
        match action {
            EditorAction::OpenFolder => self.pick_files(true, false),
            EditorAction::AddFiles => self.pick_files(false, true),
            EditorAction::CloseFiles => self.close_selected(),
            EditorAction::Select(i, ctrl, shift) => self.select(i.max(0) as usize, ctrl, shift),
            EditorAction::SelectAll => {
                self.selection = (0..self.session.len()).collect();
                self.anchor = Some(0);
                self.refresh_selection();
            }
            EditorAction::FileAction(what, i) => self.file_action(&what, i.max(0) as usize),
            EditorAction::SetField(id, text) => {
                let Some(field) = Field::from_id(&id) else { return };
                let sel = self.selection.clone();
                if self.session.set_field(&sel, field, &text) > 0 {
                    self.after_edit(&sel, Some(field));
                }
            }
            EditorAction::NumberTracks => {
                let sel = self.selection.clone();
                if self.session.number_tracks(&sel) > 0 {
                    self.after_edit(&sel, None);
                    self.refresh_form();
                }
            }
            EditorAction::Save => self.save(),
            EditorAction::Revert => {
                let targets = if self.selection.iter().any(|&i| self.session.files()[i].is_modified()) {
                    self.selection.clone()
                } else {
                    (0..self.session.len()).collect()
                };
                let n = self.session.revert(&targets);
                if n > 0 {
                    self.toast(format!("Reverted {n} file{}", plural(n)));
                    self.after_edit(&targets, None);
                    self.refresh_form();
                }
            }
            EditorAction::Undo => {
                if let Some((label, files)) = self.session.undo() {
                    self.toast(format!("Undid “{label}”"));
                    self.after_edit(&files, None);
                    self.refresh_form();
                }
            }
            EditorAction::Redo => {
                if let Some((label, files)) = self.session.redo() {
                    self.toast(format!("Redid “{label}”"));
                    self.after_edit(&files, None);
                    self.refresh_form();
                }
            }
            EditorAction::AddPicture => {
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let file = rfd::FileDialog::new()
                        .set_title("Choose an image")
                        .add_filter("Images", &["jpg", "jpeg", "png", "webp", "gif", "bmp"])
                        .pick_file();
                    if let Some(bytes) = file.and_then(|f| std::fs::read(f).ok()) {
                        let _ = tx.send(Msg::PicturePicked(bytes));
                    }
                });
            }
            EditorAction::FolderPicture => self.folder_picture(),
            EditorAction::RemovePicture(i) => {
                let Some(kind) = self.shown_picture(i).map(|p| p.kind) else { return };
                let sel = self.selection.clone();
                if self.session.remove_picture(&sel, kind) > 0 {
                    self.after_edit(&sel, None);
                    self.refresh_form();
                }
            }
            EditorAction::ExportPicture(i) => {
                let Some(picture) = self.shown_picture(i) else { return };
                let name = format!("{}.{}", picture.kind.label(), picture.extension());
                let dir = self.selection.first().and_then(|&i| self.session.files()[i].path.parent().map(Path::to_path_buf));
                std::thread::spawn(move || {
                    let mut dialog = rfd::FileDialog::new().set_title("Save image").set_file_name(&name);
                    if let Some(dir) = dir {
                        dialog = dialog.set_directory(dir);
                    }
                    if let Some(path) = dialog.save_file() {
                        if let Err(e) = std::fs::write(&path, &*picture.data) {
                            tracing::warn!("could not save {}: {e}", path.display());
                        }
                    }
                });
            }
            EditorAction::SetPictureKind(i, k) => {
                let (Some(from), Some(to)) = (self.shown_picture(i).map(|p| p.kind), PictureKind::ALL.get(k.max(0) as usize)) else {
                    return;
                };
                let sel = self.selection.clone();
                if from != *to && self.session.set_picture_kind(&sel, from, *to) > 0 {
                    self.after_edit(&sel, None);
                    self.refresh_form();
                }
            }
            EditorAction::SetLyrics(text) => {
                let sel = self.selection.clone();
                if sel.len() == 1 && self.session.set_field(&sel, Field::Lyrics, &text) > 0 {
                    self.after_edit(&sel, Some(Field::Lyrics));
                    if text.is_empty() {
                        self.refresh_form();
                    }
                }
            }
            EditorAction::LyricsOnline => {
                let sel = self.selection.clone();
                let overwrite = sel.len() == 1;
                self.fetch_lyrics(&sel, overwrite);
            }
            EditorAction::OpenScanner(mode) => {
                if let Some(ui) = self.ui.upgrade() {
                    let e = ui.global::<Editor>();
                    e.set_scanner_mode(mode.clamp(0, 2));
                    e.set_scanner_open(true);
                }
                self.scanner_preview();
            }
            EditorAction::ScannerChanged => self.scanner_preview(),
            EditorAction::ScannerApply => self.scanner_apply(),
            EditorAction::LookupStart => self.lookup_start(),
            EditorAction::LookupSearch(artist, album) => self.lookup_search(artist, album),
            EditorAction::LookupPick(i) => self.lookup_pick(i.max(0) as usize),
            EditorAction::LookupOptionsChanged => self.lookup_details(),
            EditorAction::LookupApply => self.lookup_apply(),
        }
    }

    fn select(&mut self, index: usize, ctrl: bool, shift: bool) {
        if index >= self.session.len() {
            return;
        }
        if shift {
            let anchor = self.anchor.unwrap_or(index);
            let (a, b) = (anchor.min(index), anchor.max(index));
            if !ctrl {
                self.selection.clear();
            }
            self.selection.extend(a..=b);
        } else if ctrl {
            match self.selection.iter().position(|&i| i == index) {
                Some(p) => {
                    self.selection.remove(p);
                }
                None => self.selection.push(index),
            }
            self.anchor = Some(index);
        } else {
            self.selection = vec![index];
            self.anchor = Some(index);
        }
        self.selection.sort_unstable();
        self.selection.dedup();
        self.refresh_selection();
    }

    fn close_selected(&mut self) {
        let sel = self.selection.clone();
        let unsaved = sel.iter().filter(|&&i| self.session.files()[i].is_modified()).count();
        self.session.remove(&sel);
        self.selection.clear();
        self.anchor = None;
        if unsaved > 0 {
            self.toast(format!("Closed {} file{}; {unsaved} had unsaved changes", sel.len(), plural(sel.len())));
        }
        self.refresh_all();
    }

    fn file_action(&mut self, what: &str, index: usize) {
        if index >= self.session.len() {
            return;
        }
        // Act on the whole selection when the clicked file is part of it.
        let targets = if self.selection.contains(&index) { self.selection.clone() } else { vec![index] };
        match what {
            "revert" => {
                if self.session.revert(&targets) > 0 {
                    self.after_edit(&targets, None);
                    self.refresh_form();
                }
            }
            "show-folder" => {
                if let Some(dir) = self.session.files()[index].path.parent() {
                    let _ = lychee_platform::open(dir);
                }
            }
            "close" => {
                self.selection = targets;
                self.close_selected();
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------------------------
    // Saving

    fn save(&mut self) {
        let jobs = self.session.save_jobs();
        if jobs.is_empty() {
            if self.quit_after_save {
                self.outcome.ready_to_quit = true;
            }
            return;
        }
        let total = jobs.len();
        self.set_busy(Some(format!("Saving {total} file{}…", plural(total))));
        let tx = self.tx.clone();
        let library = self.library.clone();
        std::thread::Builder::new()
            .name("lychee-editor-save".into())
            .spawn(move || {
                let mut results = Vec::with_capacity(total);
                let mut moved = Vec::new();
                for (n, job) in jobs.into_iter().enumerate() {
                    let result = job.run().map(|path| {
                        moved.push((job.path.clone(), path.clone()));
                        EditFile::open(&path)
                    });
                    if let Err(e) = &result {
                        tracing::warn!("saving {} failed: {e}", job.path.display());
                    }
                    results.push((job, result));
                    let _ = tx.send(Msg::SaveProgress { done: n + 1, total });
                }
                if let Err(e) = library.refresh_files(&moved) {
                    tracing::warn!("could not update the library after saving tags: {e}");
                }
                let _ = tx.send(Msg::Saved { results });
            })
            .ok();
    }

    fn on_saved(&mut self, results: Vec<(SaveJob, Result<EditFile, String>)>) {
        self.set_busy(None);
        let failed = results.iter().filter(|(_, r)| r.is_err()).count();
        let saved = results.len() - failed;
        for (job, result) in results {
            self.session.finish_save(&job, result);
        }
        self.outcome.library_changed = saved > 0;
        if failed == 0 {
            self.toast(format!("Saved {saved} file{}", plural(saved)));
            if self.quit_after_save {
                self.outcome.ready_to_quit = true;
            }
        } else {
            self.quit_after_save = false;
            self.toast(format!("Saved {saved}; {failed} file{} couldn't be saved (see the list)", plural(failed)));
        }
        self.refresh_all();
    }

    /// "Save and quit" from the close prompt.
    pub fn save_then_quit(&mut self) {
        self.quit_after_save = true;
        // A save already running finishes the job (and then quits).
        if self.busy.is_none() {
            self.save();
        }
    }

    // -----------------------------------------------------------------------------------------
    // Pictures

    /// The picture shown at `index` in the Artwork tab.
    fn shown_picture(&self, index: i32) -> Option<Picture> {
        let pictures = self.session.pictures(&self.selection)?;
        pictures.get(usize::try_from(index).ok()?).cloned()
    }

    fn folder_picture(&mut self) {
        let sel = self.selection.clone();
        let mut by_file = BTreeMap::new();
        for &i in &sel {
            let Some(dir) = self.session.files()[i].path.parent() else { continue };
            let Some(image) = lychee_library::folder_image(dir) else { continue };
            if let Ok(bytes) = std::fs::read(&image) {
                by_file.insert(i, Picture::new(PictureKind::Front, bytes));
            }
        }
        if by_file.is_empty() {
            self.toast("No cover image (cover.jpg, folder.png…) next to these files");
            return;
        }
        let n = self.session.set_pictures("Use folder image", &by_file);
        if n > 0 {
            self.after_edit(&sel, None);
            self.refresh_form();
            self.toast(format!("Added the folder image to {n} file{}", plural(n)));
        }
    }

    fn thumbnail(&mut self, picture: &Picture) -> Option<(Image, u32, u32)> {
        let key = (Arc::as_ptr(&picture.data) as *const u8 as usize, picture.data.len());
        if let Some(hit) = self.thumbs.get(&key) {
            return Some(hit.clone());
        }
        let decoded = image::load_from_memory(&picture.data).ok()?;
        let (w, h) = (decoded.width(), decoded.height());
        let small = decoded.thumbnail(360, 360).to_rgba8();
        let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(small.as_raw(), small.width(), small.height());
        let entry = (Image::from_rgba8(buffer), w, h);
        self.thumbs.put(key, entry.clone());
        Some(entry)
    }

    // -----------------------------------------------------------------------------------------
    // Scanner

    fn scanner_preview(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let e = ui.global::<Editor>();
        let sel = self.selection.clone();
        let mut rows = Vec::new();
        let (status, ok) = match e.get_scanner_mode() {
            0 => match Mask::parse(&e.get_fill_mask()) {
                Err(err) => (err.to_string(), false),
                Ok(mask) => {
                    let overwrite = e.get_fill_overwrite();
                    let mut matched = 0;
                    for &i in sel.iter().take(PREVIEW_ROWS) {
                        let f = &self.session.files()[i];
                        let before = f.file_name();
                        match mask.extract(&f.path) {
                            Some(values) => {
                                matched += 1;
                                let shown: Vec<String> = values
                                    .iter()
                                    .filter(|(field, _)| overwrite || f.tags.get(*field).is_empty())
                                    .map(|(field, v)| format!("{}: {v}", field.label()))
                                    .collect();
                                let after = if shown.is_empty() { "Nothing new".into() } else { shown.join(" · ") };
                                rows.push(ScanRow { before: before.into(), after: after.into(), ok: true });
                            }
                            None => rows.push(ScanRow { before: before.into(), after: "Doesn't fit the mask".into(), ok: false }),
                        }
                    }
                    let n = sel.len().min(PREVIEW_ROWS);
                    (format!("{matched} of {n} file{} fit the mask", plural(n)), matched > 0)
                }
            },
            1 => match Mask::parse(&e.get_rename_mask()) {
                Err(err) => (err.to_string(), false),
                Ok(mask) => {
                    let preview = self.session.rename_preview(&sel, &mask, 2);
                    let mut good = 0;
                    for (i, result) in preview.iter().take(PREVIEW_ROWS) {
                        let f = &self.session.files()[*i];
                        let dir = f.path.parent().unwrap_or(Path::new(""));
                        let (after, ok) = match result {
                            Ok(p) => {
                                good += 1;
                                (p.strip_prefix(dir).unwrap_or(p).display().to_string(), true)
                            }
                            Err(why) => (why.clone(), false),
                        };
                        rows.push(ScanRow { before: f.file_name().into(), after: after.into(), ok });
                    }
                    let bad = preview.len() - good;
                    let status = if bad > 0 {
                        format!("{good} file{} will be renamed when you save; {bad} can't be", plural(good))
                    } else {
                        format!("{good} file{} will be renamed when you save", plural(good))
                    };
                    (status, good > 0)
                }
            },
            _ => {
                let (fields, cleanup) = self.cleanup_settings(&e);
                let mut changed = 0;
                for &i in &sel {
                    let f = &self.session.files()[i];
                    let mut first = None;
                    let mut any = false;
                    for &field in &fields {
                        let old = f.tags.get(field);
                        let new = cleanup.apply(old);
                        if !old.is_empty() && new != old {
                            any = true;
                            first.get_or_insert((old.to_string(), new));
                        }
                    }
                    if any {
                        changed += 1;
                    }
                    if rows.len() < PREVIEW_ROWS {
                        let (before, after) = first.unwrap_or_else(|| (f.tags.get(fields[0]).to_string(), "No change".into()));
                        rows.push(ScanRow { before: before.into(), after: after.into(), ok: any });
                    }
                }
                (format!("Changes {changed} of {} file{}", sel.len(), plural(sel.len())), changed > 0)
            }
        };
        e.set_scan_rows(ModelRc::new(VecModel::from(rows)));
        e.set_scan_status(status.into());
        e.set_scan_ok(ok);
    }

    fn cleanup_settings(&self, e: &Editor<'_>) -> (Vec<Field>, Cleanup) {
        let fields = match e.get_cleanup_fields() {
            1 => vec![Field::Title],
            2 => vec![Field::Artist, Field::AlbumArtist, Field::Composer, Field::OriginalArtist],
            3 => vec![Field::Album],
            _ => Field::TEXT.to_vec(),
        };
        let find = e.get_cleanup_find().to_string();
        let cleanup = Cleanup {
            underscores_to_spaces: e.get_cleanup_underscores(),
            percent20_to_spaces: e.get_cleanup_percent(),
            tidy_spaces: e.get_cleanup_spaces(),
            case: match e.get_cleanup_case() {
                1 => Case::Title,
                2 => Case::Sentence,
                3 => Case::Upper,
                4 => Case::Lower,
                _ => Case::Keep,
            },
            replace: (!find.is_empty()).then(|| (find, e.get_cleanup_replace().to_string())),
        };
        (fields, cleanup)
    }

    fn scanner_apply(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let e = ui.global::<Editor>();
        let sel = self.selection.clone();
        let message = match e.get_scanner_mode() {
            0 => {
                let Ok(mask) = Mask::parse(&e.get_fill_mask()) else { return };
                let (changed, unmatched) = self.session.fill_from_names(&sel, &mask, e.get_fill_overwrite());
                if unmatched > 0 {
                    format!("Filled tags in {changed} file{}; {unmatched} didn't fit the mask", plural(changed))
                } else {
                    format!("Filled tags in {changed} file{}", plural(changed))
                }
            }
            1 => {
                let Ok(mask) = Mask::parse(&e.get_rename_mask()) else { return };
                let (queued, skipped) = self.session.rename_from_tags(&sel, &mask, 2);
                let base = format!("{queued} file{} will be renamed when you save", plural(queued));
                if skipped > 0 { format!("{base}; {skipped} skipped") } else { base }
            }
            _ => {
                let (fields, cleanup) = self.cleanup_settings(&e);
                let n = self.session.cleanup(&sel, &fields, &cleanup);
                format!("Cleaned up {n} file{}", plural(n))
            }
        };
        e.set_scanner_open(false);
        self.toast(message);
        self.after_edit(&sel, None);
        self.refresh_form();
    }

    // -----------------------------------------------------------------------------------------
    // Online lookup

    fn facts(&self, indices: &[usize]) -> Vec<FileFacts> {
        indices
            .iter()
            .map(|&i| {
                let f = &self.session.files()[i];
                let t = &f.tags;
                let name = |p: Option<&Path>| p.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let stem = f.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                // Untagged files: the file name is the best clue to the title.
                let title = match t.get(Field::Title) {
                    "" => matching::title_from_file_name(&stem),
                    title => title.to_string(),
                };
                FileFacts {
                    name: stem,
                    folder: name(f.path.parent()),
                    parent_folder: name(f.path.parent().and_then(Path::parent)),
                    title,
                    artist: t.get(Field::Artist).to_string(),
                    album: t.get(Field::Album).to_string(),
                    album_artist: t.get(Field::AlbumArtist).to_string(),
                    track: t.get(Field::Track).parse().ok(),
                    disc: t.get(Field::Disc).parse().ok(),
                    duration_ms: f.info.duration_ms,
                    release_id: t.get(Field::MbReleaseId).to_string(),
                    recording_id: t.get(Field::MbRecordingId).to_string(),
                }
            })
            .collect()
    }

    fn lookup_start(&mut self) {
        if self.session.is_empty() {
            return;
        }
        if self.selection.is_empty() {
            self.selection = (0..self.session.len()).collect();
            self.refresh_selection();
        }
        let indices: Vec<usize> = self.selection.iter().copied().filter(|&i| self.session.files()[i].error.is_none()).collect();
        let facts = self.facts(&indices);
        let guess = matching::guess(&facts);
        let paths = indices.iter().map(|&i| self.session.files()[i].path.clone()).collect();
        if let Some(ui) = self.ui.upgrade() {
            let e = ui.global::<Editor>();
            e.set_lookup_artist(guess.artist.as_str().into());
            e.set_lookup_album(guess.album.as_str().into());
            e.set_lookup_open(true);
        }
        self.lookup = Some(Lookup {
            seq: 0,
            paths,
            facts,
            guess: guess.clone(),
            candidates: Vec::new(),
            selected: None,
            cover: None,
            genre_checked: HashSet::new(),
        });
        self.run_search(guess);
    }

    fn lookup_search(&mut self, artist: String, album: String) {
        let Some(lookup) = &self.lookup else { return };
        let mut guess = lookup.guess.clone();
        if guess.artist != artist.trim() || guess.album != album.trim() {
            // New words: the release id in the tags no longer applies.
            guess.release_id.clear();
        }
        guess.artist = artist.trim().to_string();
        guess.album = album.trim().to_string();
        if guess.artist.is_empty() && guess.album.is_empty() && guess.release_id.is_empty() {
            self.set_lookup_state("empty", "Type an artist and an album to search for.");
            return;
        }
        self.run_search(guess);
    }

    fn run_search(&mut self, guess: Guess) {
        self.lookup_seq += 1;
        let seq = self.lookup_seq;
        let Some(lookup) = &mut self.lookup else { return };
        lookup.seq = seq;
        lookup.guess = guess.clone();
        lookup.candidates.clear();
        lookup.selected = None;
        lookup.cover = None;
        let facts = lookup.facts.clone();
        if guess.album.is_empty() && guess.release_id.is_empty() && facts.iter().all(|f| f.title.is_empty()) {
            self.set_lookup_state("empty", "There's nothing to search for yet: type the artist and album above.");
            self.lookup_details();
            return;
        }
        self.set_lookup_state("searching", "Searching MusicBrainz…");
        // Forget the last search's list and selection on screen too.
        self.update_lookup_results();
        self.lookup_details();
        let client = self.client.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let report = |text: String| {
                let _ = tx.send(Msg::LookupProgress { seq, text });
            };
            let result = matching::find_candidates(&client, &facts, &guess, 3, &report).map_err(|e| e.to_string());
            let _ = tx.send(Msg::LookupFound { seq, result });
        });
    }

    fn set_lookup_state(&self, state: &str, message: &str) {
        if let Some(ui) = self.ui.upgrade() {
            let e = ui.global::<Editor>();
            e.set_lookup_state(state.into());
            e.set_lookup_message(message.into());
        }
    }

    fn on_lookup_found(&mut self, result: Result<Vec<Candidate>, String>) {
        let Some(lookup) = &mut self.lookup else { return };
        match result {
            Err(e) => {
                self.set_lookup_state("error", &format!("MusicBrainz couldn't be reached: {e}"));
            }
            Ok(candidates) if candidates.is_empty() => {
                self.set_lookup_state("empty", "No releases found. Try fewer or different words, or the album's original title.");
            }
            Ok(candidates) => {
                lookup.candidates = candidates;
                self.set_lookup_state("results", "");
                self.update_lookup_results();
                self.lookup_pick(0);
                return;
            }
        }
        self.update_lookup_results();
        self.lookup_details();
    }

    fn update_lookup_results(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let e = ui.global::<Editor>();
        let items: Vec<LookupItem> = self
            .lookup
            .iter()
            .flat_map(|l| l.candidates.iter())
            .map(|c| {
                let s = &c.summary;
                let mut detail: Vec<String> = Vec::new();
                for part in [&s.date, &s.country, &s.format] {
                    if !part.is_empty() {
                        detail.push(part.clone());
                    }
                }
                if s.track_count > 0 {
                    detail.push(format!("{} tracks", s.track_count));
                }
                if !s.label.is_empty() {
                    detail.push(s.label.clone());
                }
                let title = if s.disambiguation.is_empty() { s.title.clone() } else { format!("{} ({})", s.title, s.disambiguation) };
                LookupItem {
                    title: title.into(),
                    artist: s.artist.as_str().into(),
                    detail: detail.join(" · ").into(),
                    score: if c.release.is_some() { format!("{}%", c.score).into() } else { SharedString::new() },
                    good: c.release.is_some() && c.score >= 80,
                }
            })
            .collect();
        e.set_lookup_results(ModelRc::new(VecModel::from(items)));
        e.set_lookup_selected(self.lookup.as_ref().and_then(|l| l.selected).map_or(-1, |i| i as i32));
    }

    fn lookup_pick(&mut self, index: usize) {
        let Some(lookup) = &mut self.lookup else { return };
        let Some(candidate) = lookup.candidates.get(index) else { return };
        lookup.selected = Some(index);
        let seq = lookup.seq;
        let (release_id, group_id) = (candidate.summary.id.clone(), candidate.summary.release_group_id.clone());
        let need_release = candidate.release.is_none();
        let have_cover = lookup.cover.as_ref().is_some_and(|(id, _)| *id == release_id);
        // Fetched releases without a genre: ask their release group, once.
        let needs_genre = candidate.release.as_ref().is_some_and(|r| r.genre.is_empty()) && lookup.genre_checked.insert(release_id.clone());
        if needs_genre {
            let mut release = candidate.release.clone().unwrap_or_default();
            let client = self.client.clone();
            let tx = self.tx.clone();
            let id = release_id.clone();
            std::thread::spawn(move || {
                client.complete_genre(&mut release);
                if !release.genre.is_empty() {
                    let _ = tx.send(Msg::LookupRelease { seq, release_id: id, result: Ok(Some(release)) });
                }
            });
        }
        if need_release {
            let client = self.client.clone();
            let tx = self.tx.clone();
            let id = release_id.clone();
            std::thread::spawn(move || {
                let result = client.release(&id).map_err(|e| e.to_string()).map(|r| {
                    r.map(|mut r| {
                        client.complete_genre(&mut r);
                        r
                    })
                });
                let _ = tx.send(Msg::LookupRelease { seq, release_id: id, result });
            });
        }
        if !have_cover {
            let client = self.client.clone();
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let bytes = client.front_cover(&release_id, &group_id, 250).ok().flatten();
                let _ = tx.send(Msg::LookupCover { seq, release_id, bytes });
            });
        }
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Editor>().set_lookup_selected(index as i32);
        }
        self.lookup_details();
    }

    /// Fill the right half of the lookup dialog for the selected candidate.
    fn lookup_details(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let e = ui.global::<Editor>();
        let Some(lookup) = &self.lookup else { return };
        let candidate = lookup.selected.and_then(|i| lookup.candidates.get(i));
        let mut rows = Vec::new();
        let mut summary = String::new();
        match candidate {
            None => {
                e.set_lookup_heading(SharedString::new());
                e.set_lookup_subheading(SharedString::new());
                e.set_lookup_loading_tracks(false);
                e.set_lookup_has_cover(false);
            }
            Some(c) => {
                let s = &c.summary;
                e.set_lookup_heading(format!("{} — {}", s.title, s.artist).into());
                let mut sub = Vec::new();
                if let Some(r) = &c.release {
                    if !r.year().is_empty() {
                        sub.push(if r.date.get(..4).is_some_and(|y| y != r.year()) { format!("{} (this release {})", r.year(), r.date) } else { r.year().to_string() });
                    }
                    for part in [&r.country, &r.genre, &r.label, &r.catalog_number] {
                        if !part.is_empty() {
                            sub.push(part.clone());
                        }
                    }
                    if !s.format.is_empty() {
                        sub.push(s.format.clone());
                    }
                } else {
                    sub.push(s.date.clone());
                }
                e.set_lookup_subheading(sub.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ").into());
                e.set_lookup_loading_tracks(c.release.is_none());
                match &lookup.cover {
                    Some((id, img)) if *id == s.id => {
                        e.set_lookup_cover(img.clone());
                        e.set_lookup_has_cover(true);
                    }
                    _ => e.set_lookup_has_cover(false),
                }
                if let Some(r) = &c.release {
                    let mut file_for: BTreeMap<(usize, usize), usize> = BTreeMap::new();
                    for (fi, m) in c.mapping.iter().enumerate() {
                        if let Some(pos) = m {
                            file_for.insert(*pos, fi);
                        }
                    }
                    let multi = r.media.len() > 1;
                    for (mi, m) in r.media.iter().enumerate() {
                        for (ti, t) in m.tracks.iter().enumerate() {
                            let number = if multi { format!("{}.{:02}", m.position, t.position) } else { format!("{:02}", t.position) };
                            let file = file_for.get(&(mi, ti)).map(|&fi| &lookup.facts[fi]);
                            let theirs = t.length_ms.map(lychee_core::format_duration_ms).unwrap_or_default();
                            let times = match file {
                                Some(f) if f.duration_ms > 0 => format!("{theirs} · {}", lychee_core::format_duration_ms(f.duration_ms)),
                                _ => theirs,
                            };
                            rows.push(LookupTrack {
                                file: file.map(|f| f.name.clone()).unwrap_or_else(|| "no file".into()).into(),
                                track: format!("{number}  {}", t.title).into(),
                                times: times.into(),
                                ok: file.is_some(),
                            });
                        }
                    }
                    for (fi, m) in c.mapping.iter().enumerate() {
                        if m.is_none() {
                            rows.push(LookupTrack {
                                file: lookup.facts[fi].name.as_str().into(),
                                track: "Not matched: left as it is".into(),
                                times: lychee_core::format_duration_ms(lookup.facts[fi].duration_ms).into(),
                                ok: false,
                            });
                        }
                    }
                    summary = self.lookup_summary(&e, c);
                }
            }
        }
        e.set_lookup_tracks(ModelRc::new(VecModel::from(rows)));
        e.set_lookup_summary(summary.into());
    }

    /// The tag values a candidate would set, per file path (fill-in or replace).
    fn proposed(&self, candidate: &Candidate, paths: &[PathBuf], overwrite: bool) -> BTreeMap<usize, Vec<(Field, String)>> {
        let mut out = BTreeMap::new();
        let Some(release) = &candidate.release else { return out };
        for (path, m) in paths.iter().zip(&candidate.mapping) {
            let (Some((mi, ti)), Some(index)) = (m, self.session.index_of(path)) else { continue };
            let Some(tags) = matching::track_tags(release, *mi, *ti) else { continue };
            let file = &self.session.files()[index];
            let values: Vec<(Field, String)> = tag_values(&tags)
                .into_iter()
                .filter(|(field, value)| {
                    let current = file.tags.get(*field);
                    current != value && (overwrite || current.is_empty() || field.is_musicbrainz())
                })
                .collect();
            if !values.is_empty() {
                out.insert(index, values);
            }
        }
        out
    }

    fn lookup_summary(&self, e: &Editor<'_>, candidate: &Candidate) -> String {
        let Some(lookup) = &self.lookup else { return String::new() };
        let mapped = candidate.mapping.iter().filter(|m| m.is_some()).count();
        let mut parts = Vec::new();
        if e.get_lookup_tags() {
            let proposed = self.proposed(candidate, &lookup.paths, e.get_lookup_overwrite());
            let fields: usize = proposed.values().map(|v| v.iter().filter(|(f, _)| !f.is_musicbrainz()).count()).sum();
            parts.push(if fields == 0 { "tags already match".to_string() } else { format!("{fields} tag value{}", plural(fields)) });
        }
        if e.get_lookup_artwork() {
            parts.push("cover".into());
        }
        if e.get_lookup_lyrics() {
            parts.push("lyrics".into());
        }
        if parts.is_empty() {
            return String::new();
        }
        format!("{} for {mapped} of {} file{}", capitalize(&parts.join(", ")), lookup.paths.len(), plural(lookup.paths.len()))
    }

    fn lookup_apply(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let e = ui.global::<Editor>();
        let Some(lookup) = &self.lookup else { return };
        let Some(candidate) = lookup.selected.and_then(|i| lookup.candidates.get(i)).cloned() else { return };
        let Some(release) = candidate.release.clone() else { return };
        let overwrite = e.get_lookup_overwrite();
        let paths = lookup.paths.clone();
        let mapped: Vec<PathBuf> =
            paths.iter().zip(&candidate.mapping).filter(|(_, m)| m.is_some()).map(|(p, _)| p.clone()).collect();
        let mut notes = Vec::new();
        if e.get_lookup_tags() {
            let changes = self.proposed(&candidate, &paths, overwrite);
            let n = self.session.apply(&format!("Look up “{}”", release.title), &changes);
            notes.push(format!("filled tags in {n} file{}", plural(n)));
            let touched: Vec<usize> = changes.keys().copied().collect();
            self.after_edit(&touched, None);
        }
        if e.get_lookup_artwork() {
            let client = self.client.clone();
            let tx = self.tx.clone();
            let (id, group) = (release.id.clone(), release.release_group_id.clone());
            let paths = mapped.clone();
            self.start_download("Downloading the cover…");
            std::thread::spawn(move || {
                let (bytes, error) = match client.front_cover(&id, &group, 1200) {
                    Ok(b) => (b, None),
                    Err(e) => (None, Some(e.to_string())),
                };
                let _ = tx.send(Msg::Artwork { paths, overwrite, bytes, error });
            });
            notes.push("downloading the cover".into());
        }
        if e.get_lookup_lyrics() {
            let indices: Vec<usize> = mapped.iter().filter_map(|p| self.session.index_of(p)).collect();
            self.fetch_lyrics(&indices, overwrite);
        }
        e.set_lookup_open(false);
        self.refresh_form();
        if !notes.is_empty() {
            self.toast(format!("{}. Review, then save.", capitalize(&notes.join(", "))));
        }
    }

    /// Look up lyrics on LRCLIB for files, using their current (possibly just filled) tags.
    fn fetch_lyrics(&mut self, indices: &[usize], overwrite: bool) {
        let jobs: Vec<(PathBuf, String, String, String, Option<u64>)> = indices
            .iter()
            .filter_map(|&i| self.session.file(i))
            .filter(|f| f.error.is_none() && (overwrite || f.tags.get(Field::Lyrics).is_empty()))
            .map(|f| {
                let t = &f.tags;
                let artist = if t.get(Field::Artist).is_empty() { t.get(Field::AlbumArtist) } else { t.get(Field::Artist) };
                let duration = (f.info.duration_ms > 0).then_some(f.info.duration_ms / 1000);
                (f.path.clone(), artist.to_string(), t.get(Field::Title).to_string(), t.get(Field::Album).to_string(), duration)
            })
            .filter(|(_, artist, title, ..)| !artist.is_empty() && !title.is_empty())
            .collect();
        if jobs.is_empty() {
            self.toast(if overwrite { "These files need an artist and a title to find lyrics" } else { "These files already have lyrics" });
            return;
        }
        let total = jobs.len();
        self.start_download(&format!("Looking for lyrics (0 of {total})…"));
        let client = self.client.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            for (done, (path, artist, title, album, duration)) in jobs.into_iter().enumerate() {
                let text = match client.lyrics(&artist, &title, &album, duration) {
                    Ok(Some(l)) if !l.text.is_empty() => Some(l.text),
                    Ok(_) => None,
                    Err(e) => {
                        tracing::info!("lyrics for {title}: {e}");
                        None
                    }
                };
                let _ = tx.send(Msg::Lyrics { path, text, overwrite, done: done + 1, total });
            }
        });
    }

    fn start_download(&mut self, text: &str) {
        self.downloads += 1;
        self.download_text = text.to_string();
        self.update_busy();
    }

    fn finish_download(&mut self) {
        self.downloads = self.downloads.saturating_sub(1);
        self.update_busy();
    }

    // -----------------------------------------------------------------------------------------
    // Background results

    /// Collect worker results. Called on every app tick.
    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Picked { paths, add } => self.open(paths, add, Vec::new(), None),
                Msg::LoadProgress { seq, done, total } => {
                    if seq == self.load_seq {
                        self.busy = Some(format!("Reading files ({done} of {total})…"));
                        self.update_busy();
                    }
                }
                Msg::Loaded { seq, files, add, select, title } => {
                    if seq == self.load_seq {
                        self.on_loaded(files, add, select, title);
                    }
                }
                Msg::SaveProgress { done, total } => {
                    self.busy = Some(format!("Saving ({done} of {total})…"));
                    self.update_busy();
                }
                Msg::Saved { results } => self.on_saved(results),
                Msg::PicturePicked(bytes) => {
                    if image::load_from_memory(&bytes).is_err() {
                        self.toast("That file isn't an image Lychee can read");
                        continue;
                    }
                    let sel = self.selection.clone();
                    let n = self.session.set_picture(&sel, &Picture::new(PictureKind::Front, bytes));
                    if n > 0 {
                        self.after_edit(&sel, None);
                        self.refresh_form();
                    }
                }
                Msg::LookupProgress { seq, text } => {
                    if self.lookup.as_ref().is_some_and(|l| l.seq == seq) {
                        if let Some(ui) = self.ui.upgrade() {
                            let e = ui.global::<Editor>();
                            if e.get_lookup_state() == "searching" {
                                e.set_lookup_message(text.into());
                            }
                        }
                    }
                }
                Msg::LookupFound { seq, result } => {
                    if self.lookup.as_ref().is_some_and(|l| l.seq == seq) {
                        self.on_lookup_found(result);
                    }
                }
                Msg::LookupRelease { seq, release_id, result } => {
                    let Some(lookup) = self.lookup.as_mut().filter(|l| l.seq == seq) else { continue };
                    let Some(index) = lookup.candidates.iter().position(|c| c.summary.id == release_id) else { continue };
                    match result {
                        Ok(Some(release)) => {
                            let old = &lookup.candidates[index].summary;
                            let (status, disambiguation) = (old.status.clone(), old.disambiguation.clone());
                            let mut c = Candidate::fetched(release, &lookup.facts, &lookup.guess);
                            c.summary.status = status;
                            c.summary.disambiguation = disambiguation;
                            lookup.candidates[index] = c;
                        }
                        Ok(None) => lookup.candidates[index].summary.title.push_str(" (gone)"),
                        Err(e) => {
                            tracing::warn!("release {release_id}: {e}");
                            self.set_lookup_state("error", &format!("Couldn't load that release: {e}"));
                        }
                    }
                    self.update_lookup_results();
                    self.lookup_details();
                }
                Msg::LookupCover { seq, release_id, bytes } => {
                    let Some(bytes) = bytes else { continue };
                    let Some(image) = decode_image(&bytes, 240) else { continue };
                    if let Some(lookup) = self.lookup.as_mut().filter(|l| l.seq == seq) {
                        lookup.cover = Some((release_id, image));
                        self.lookup_details();
                    }
                }
                Msg::Artwork { paths, overwrite, bytes, error } => {
                    self.finish_download();
                    let Some(bytes) = bytes else {
                        self.toast(match error {
                            Some(e) => format!("Couldn't download the cover: {e}"),
                            None => "The Cover Art Archive has no front cover for this album".into(),
                        });
                        continue;
                    };
                    let picture = Picture::new(PictureKind::Front, bytes);
                    let targets: BTreeMap<usize, Picture> = paths
                        .iter()
                        .filter_map(|p| self.session.index_of(p))
                        .filter(|&i| overwrite || !self.session.files()[i].tags.pictures.iter().any(|p| p.kind == PictureKind::Front))
                        .map(|i| (i, picture.clone()))
                        .collect();
                    let n = self.session.set_pictures("Cover from the Cover Art Archive", &targets);
                    let touched: Vec<usize> = targets.keys().copied().collect();
                    self.after_edit(&touched, None);
                    self.refresh_form();
                    self.toast(if n > 0 {
                        format!("Added the cover to {n} file{}", plural(n))
                    } else {
                        "These files already have a front cover".into()
                    });
                }
                Msg::Lyrics { path, text, overwrite, done, total } => {
                    if let (Some(text), Some(i)) = (text, self.session.index_of(&path)) {
                        let current = self.session.files()[i].tags.get(Field::Lyrics);
                        if overwrite || current.is_empty() {
                            let mut change = BTreeMap::new();
                            change.insert(i, vec![(Field::Lyrics, text)]);
                            if self.session.apply("Lyrics from LRCLIB", &change) > 0 {
                                self.after_edit(&[i], None);
                            }
                        }
                    }
                    if done == total {
                        self.finish_download();
                        self.refresh_form();
                        self.toast(format!("Looked for lyrics for {total} file{}", plural(total)));
                    } else {
                        self.download_text = format!("Looking for lyrics ({done} of {total})…");
                        self.update_busy();
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // UI sync

    fn set_busy(&mut self, busy: Option<String>) {
        self.busy = busy;
        self.update_busy();
    }

    fn update_busy(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let e = ui.global::<Editor>();
        let text = self.busy.clone().or_else(|| (self.downloads > 0).then(|| self.download_text.clone()));
        e.set_busy(text.is_some());
        e.set_busy_text(text.unwrap_or_default().into());
        e.set_busy_progress(-1.0);
    }

    /// Everything: file list, summary and form.
    fn refresh_all(&mut self) {
        let rows: Vec<EditorFile> = (0..self.session.len()).map(|i| self.file_row(i)).collect();
        if rows.len() == self.files_model.row_count() {
            for (i, row) in rows.into_iter().enumerate() {
                self.files_model.set_row_data(i, row);
            }
        } else {
            self.files_model.set_vec(rows);
        }
        self.refresh_form();
    }

    fn file_row(&self, i: usize) -> EditorFile {
        let f = &self.session.files()[i];
        let t = &f.tags;
        let title = t.get(Field::Title);
        EditorFile {
            name: f.file_name().into(),
            new_name: f.target_name().unwrap_or_default().into(),
            folder: f.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().into(),
            title: if title.is_empty() { "—".into() } else { title.into() },
            artist: t.get(Field::Artist).into(),
            album: t.get(Field::Album).into(),
            track: t.get(Field::Track).into(),
            modified: f.is_modified(),
            selected: self.selection.binary_search(&i).is_ok(),
            error: f.error.clone().or_else(|| f.save_error.clone()).unwrap_or_default().into(),
        }
    }

    fn refresh_selection(&mut self) {
        for i in 0..self.files_model.row_count() {
            if let Some(mut row) = self.files_model.row_data(i) {
                let selected = self.selection.binary_search(&i).is_ok();
                if row.selected != selected {
                    row.selected = selected;
                    self.files_model.set_row_data(i, row);
                }
            }
        }
        self.refresh_form();
    }

    /// After an edit: update the touched rows and the summary; refresh one field's markers.
    fn after_edit(&mut self, touched: &[usize], field: Option<Field>) {
        for &i in touched {
            if i < self.files_model.row_count() {
                self.files_model.set_row_data(i, self.file_row(i));
            }
        }
        if let Some(field) = field {
            let index = Field::ALL.iter().position(|f| *f == field).unwrap_or(0);
            if let Some(mut row) = self.fields_model.row_data(index) {
                row.changed = self.session.field_changed(&self.selection, field);
                if let Value::Same(v) = self.session.value(&self.selection, field) {
                    row.value = v.into();
                    row.mixed = false;
                }
                self.fields_model.set_row_data(index, row);
            }
        }
        self.refresh_summary();
    }

    fn refresh_summary(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let e = ui.global::<Editor>();
        let n = self.session.len();
        let modified = self.session.modified_count();
        e.set_modified_count(modified as i32);
        e.set_selected_count(self.selection.len() as i32);
        let mut summary = format!("{n} file{}", plural(n));
        if modified > 0 {
            summary.push_str(&format!(" · {modified} with unsaved changes"));
        }
        let formats: Vec<&str> = {
            let mut f: Vec<&str> = self.session.files().iter().map(|f| f.info.format.as_str()).filter(|s| !s.is_empty()).collect();
            f.sort_unstable();
            f.dedup();
            f
        };
        if !formats.is_empty() && formats.len() <= 3 {
            summary.push_str(&format!(" · {}", formats.join(", ")));
        }
        e.set_summary(summary.into());
        e.set_can_undo(self.session.can_undo());
        e.set_can_redo(self.session.can_redo());
        e.set_undo_label(crate::palette::shortcut("Undo (⌘Z)", "Undo (Ctrl Z)").into());
        e.set_redo_label(crate::palette::shortcut("Redo (⇧⌘Z)", "Redo (Ctrl Shift Z)").into());
    }

    /// Reload every input from the session (selection changed, undo, a tool ran).
    fn refresh_form(&mut self) {
        let len = self.session.len();
        self.selection.retain(|&i| i < len);
        if self.anchor.is_some_and(|a| a >= len) {
            self.anchor = None;
        }
        let sel = self.selection.clone();
        for (index, field) in Field::ALL.iter().enumerate() {
            let (value, mixed) = match self.session.value(&sel, *field) {
                Value::Same(v) => (v, false),
                Value::Mixed => (String::new(), true),
            };
            self.fields_model.set_row_data(
                index,
                EditorField {
                    id: field.id().into(),
                    label: field.label().into(),
                    value: value.into(),
                    mixed,
                    enabled: self.session.field_supported(&sel, *field),
                    changed: self.session.field_changed(&sel, *field),
                },
            );
        }
        // Artwork
        let pictures = self.session.pictures(&sel);
        let shown: Vec<EditorPicture> = pictures
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|p| {
                let (image, w, h) = self.thumbnail(p).unwrap_or_default();
                let kind_index = PictureKind::ALL.iter().position(|k| *k == p.kind).unwrap_or(5) as i32;
                let size = if w > 0 { format!("{w}×{h} · ") } else { String::new() };
                let kb = (p.data.len() as u64).div_ceil(1024);
                let bytes = if kb >= 1024 { format!("{:.1} MB", kb as f64 / 1024.0) } else { format!("{kb} KB") };
                EditorPicture { image, kind: p.kind.label().into(), kind_index, detail: format!("{size}{bytes}").into() }
            })
            .collect();
        self.pictures_model.set_vec(shown);

        let Some(ui) = self.ui.upgrade() else { return };
        let e = ui.global::<Editor>();
        e.set_pictures_mixed(pictures.is_none() && !sel.is_empty());
        e.set_pictures_supported(sel.iter().all(|&i| self.session.files()[i].supports_pictures));
        // Lyrics belong to one file at a time.
        e.set_lyrics_editable(sel.len() == 1 && self.session.field_supported(&sel, Field::Lyrics));
        e.set_lyrics(match (sel.len(), self.session.value(&sel, Field::Lyrics)) {
            (1, Value::Same(v)) => v.into(),
            _ => SharedString::new(),
        });
        e.set_info(ModelRc::new(VecModel::from(self.info_rows(&sel))));
        self.generation += 1;
        e.set_generation(self.generation);
        self.refresh_summary();
    }

    fn info_rows(&self, sel: &[usize]) -> Vec<InfoRow> {
        let mut rows = Vec::new();
        let mut add = |label: &str, value: String| {
            if !value.is_empty() {
                rows.push(InfoRow { label: label.into(), value: value.into() });
            }
        };
        match sel {
            [] => {}
            [one] => {
                let f = &self.session.files()[*one];
                let i = &f.info;
                add("File", f.path.display().to_string());
                if let Some(name) = f.target_name() {
                    add("After saving", name);
                }
                if let Some(e) = f.error.as_ref().or(f.save_error.as_ref()) {
                    add("Problem", e.clone());
                }
                let mut format = vec![i.format.clone()];
                if let Some(r) = i.sample_rate {
                    format.push(format!("{:.1} kHz", r as f32 / 1000.0).replace(".0 kHz", " kHz"));
                }
                if let Some(b) = i.bit_depth {
                    format.push(format!("{b}-bit"));
                }
                match i.channels {
                    Some(1) => format.push("mono".into()),
                    Some(2) => format.push("stereo".into()),
                    Some(n) => format.push(format!("{n} channels")),
                    None => {}
                }
                add("Format", format.join(" · "));
                // Lossless files of near-silence report silly rates like "1 kbps".
                let lossless = matches!(i.format.as_str(), "FLAC" | "WAV" | "AIFF" | "ALAC" | "WavPack" | "Monkey's Audio");
                add("Bitrate", i.bitrate_kbps.filter(|b| !lossless || *b >= 100).map(|b| format!("{b} kbps")).unwrap_or_default());
                add("Duration", if i.duration_ms > 0 { lychee_core::format_duration_ms(i.duration_ms) } else { String::new() });
                add("Size", human_bytes(i.size));
                add("Tags", i.tag_format.clone());
                for field in Field::ALL.iter().filter(|f| f.is_musicbrainz()) {
                    add(field.label(), f.tags.get(*field).to_string());
                }
            }
            many => {
                let files: Vec<&EditFile> = many.iter().map(|&i| &self.session.files()[i]).collect();
                add("Files", files.len().to_string());
                let duration: u64 = files.iter().map(|f| f.info.duration_ms).sum();
                add("Total duration", lychee_core::format_duration_ms(duration));
                add("Total size", human_bytes(files.iter().map(|f| f.info.size).sum()));
                let mut formats: BTreeMap<&str, usize> = BTreeMap::new();
                for f in &files {
                    *formats.entry(f.info.format.as_str()).or_default() += 1;
                }
                add("Formats", formats.iter().map(|(f, n)| format!("{f} ×{n}")).collect::<Vec<_>>().join(", "));
                let folders: HashSet<&Path> = files.iter().filter_map(|f| f.path.parent()).collect();
                if folders.len() == 1 {
                    add("Folder", folders.iter().next().map(|p| p.display().to_string()).unwrap_or_default());
                } else {
                    add("Folders", folders.len().to_string());
                }
            }
        }
        rows
    }
}

/// Values from a release track as editor fields.
fn tag_values(t: &TrackTags) -> Vec<(Field, String)> {
    let mut v = vec![
        (Field::Title, t.title.clone()),
        (Field::Artist, t.artist.clone()),
        (Field::Album, t.album.clone()),
        (Field::AlbumArtist, t.album_artist.clone()),
        (Field::Date, t.year.clone()),
        (Field::Genre, t.genre.clone()),
        (Field::Track, t.track.to_string()),
        (Field::TrackTotal, t.track_total.to_string()),
        (Field::Label, t.label.clone()),
        (Field::CatalogNumber, t.catalog_number.clone()),
        (Field::Isrc, t.isrc.clone()),
        (Field::MbReleaseId, t.release_id.clone()),
        (Field::MbReleaseGroupId, t.release_group_id.clone()),
        (Field::MbRecordingId, t.recording_id.clone()),
        (Field::MbTrackId, t.track_id.clone()),
        (Field::MbArtistId, t.artist_id.clone()),
        (Field::MbAlbumArtistId, t.album_artist_id.clone()),
    ];
    if t.disc > 0 {
        v.push((Field::Disc, t.disc.to_string()));
        v.push((Field::DiscTotal, t.disc_total.to_string()));
    }
    v.retain(|(_, value)| !value.is_empty() && value != "0");
    v
}

/// Files and folders to audio files, folders walked recursively in name order.
fn expand(paths: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        entries.sort_by(|a, b| lychee_tagger::session::natural_cmp(a, b));
        for p in entries {
            if p.is_dir() {
                if depth < 16 {
                    walk(&p, out, depth + 1);
                }
            } else if is_audio(&p) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            walk(p, &mut out, 0);
        } else if is_audio(p) {
            out.push(p.clone());
        }
    }
    out.dedup();
    out
}

fn is_audio(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(lychee_audio::is_supported_extension)
}

/// The folder all paths share, by name, for the page title.
fn common_folder(paths: &[PathBuf]) -> Option<String> {
    let first = paths.first()?.parent()?;
    let all_same = paths.iter().all(|p| p.parent() == Some(first));
    let dir = if all_same { first } else { first.parent()? };
    dir.file_name().map(|n| n.to_string_lossy().into_owned())
}

fn decode_image(bytes: &[u8], size: u32) -> Option<Image> {
    let img = image::load_from_memory(bytes).ok()?.thumbnail(size, size).to_rgba8();
    Some(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(img.as_raw(), img.width(), img.height())))
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_values_skip_empty_and_single_disc() {
        let t = TrackTags { title: "Angel".into(), track: 1, track_total: 11, album: "Mezzanine".into(), ..Default::default() };
        let v = tag_values(&t);
        assert!(v.contains(&(Field::Track, "1".into())));
        assert!(!v.iter().any(|(f, _)| *f == Field::Disc || *f == Field::Genre));
    }

    #[test]
    fn folder_titles() {
        let paths = [PathBuf::from("/m/Artist/Album/01.flac"), PathBuf::from("/m/Artist/Album/02.flac")];
        assert_eq!(common_folder(&paths).as_deref(), Some("Album"));
        let mixed = [PathBuf::from("/m/Artist/A/01.flac"), PathBuf::from("/m/Artist/B/01.flac")];
        assert_eq!(common_folder(&mixed).as_deref(), Some("Artist"));
    }
}
