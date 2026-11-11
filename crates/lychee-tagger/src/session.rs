//! Files open in the editor: pending changes, multi-selection helpers, undo and saving.
//!
//! Like EasyTAG, edits stay in memory until saved, every file shows whether it has unsaved
//! changes, and any step can be undone. Selections are lists of file indices in list order.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::file::{self, FileInfo, Picture, PictureKind, TagData};
use crate::mask::{self, Mask};
use crate::{Cleanup, Field};

/// One file in the editor.
#[derive(Clone, Debug)]
pub struct EditFile {
    pub path: PathBuf,
    pub info: FileInfo,
    pub supported: Vec<Field>,
    pub supports_pictures: bool,
    /// Tags as they are on disk.
    pub original: TagData,
    /// Tags with pending changes.
    pub tags: TagData,
    /// Pending new location from "rename files"; applied on save.
    pub new_path: Option<PathBuf>,
    /// Why the file couldn't be read. Such files can't be edited.
    pub error: Option<String>,
    /// Why the last save failed; the pending changes are kept.
    pub save_error: Option<String>,
}

impl EditFile {
    /// Read a file. Never fails: problems end up in `error` and the file can't be edited.
    pub fn open(path: &Path) -> EditFile {
        match file::read(path) {
            Ok(read) => EditFile {
                path: path.to_path_buf(),
                info: read.info,
                supported: read.supported,
                supports_pictures: read.supports_pictures,
                original: read.tags.clone(),
                tags: read.tags,
                new_path: None,
                error: None,
                save_error: None,
            },
            Err(e) => EditFile {
                path: path.to_path_buf(),
                info: FileInfo::default(),
                supported: Vec::new(),
                supports_pictures: false,
                original: TagData::default(),
                tags: TagData::default(),
                new_path: None,
                error: Some(e.to_string()),
                save_error: None,
            },
        }
    }

    pub fn is_modified(&self) -> bool {
        self.tags != self.original || self.new_path.is_some()
    }

    pub fn supports(&self, field: Field) -> bool {
        self.supported.contains(&field)
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// The file name it will have after saving.
    pub fn target_name(&self) -> Option<String> {
        self.new_path.as_ref().map(|p| {
            let dir = self.path.parent();
            match (dir, p.strip_prefix(dir.unwrap_or(Path::new("")))) {
                (Some(_), Ok(rel)) => rel.to_string_lossy().into_owned(),
                _ => p.display().to_string(),
            }
        })
    }
}

/// A field's value across the selected files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Same(String),
    Mixed,
}

#[derive(Clone, Debug)]
struct Snapshot {
    index: usize,
    tags: TagData,
    new_path: Option<PathBuf>,
}

#[derive(Debug)]
struct Step {
    label: String,
    files: Vec<Snapshot>,
    /// Typing into one field becomes a single undo step.
    merge_key: Option<(Field, Vec<usize>)>,
    at: Instant,
}

const UNDO_LIMIT: usize = 200;
const MERGE_WINDOW: Duration = Duration::from_millis(1500);

/// Everything one save needs, so it can run on a worker thread.
#[derive(Clone, Debug)]
pub struct SaveJob {
    pub index: usize,
    pub path: PathBuf,
    pub before: TagData,
    pub after: TagData,
    pub new_path: Option<PathBuf>,
}

impl SaveJob {
    /// Write the tags, then move the file (and its `.lrc` lyrics). Returns the final path.
    pub fn run(&self) -> Result<PathBuf, String> {
        file::write(&self.path, &self.before, &self.after).map_err(|e| e.to_string())?;
        let Some(target) = self.new_path.as_ref().filter(|t| **t != self.path) else { return Ok(self.path.clone()) };
        let case_only = target.to_string_lossy().to_lowercase() == self.path.to_string_lossy().to_lowercase();
        if target.exists() && !case_only {
            return Err(format!("{} already exists", target.display()));
        }
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {}", dir.display(), crate::io_message(&e)))?;
        }
        std::fs::rename(&self.path, target).map_err(|e| format!("can't rename: {}", crate::io_message(&e)))?;
        let lyrics = self.path.with_extension("lrc");
        if lyrics.exists() {
            let _ = std::fs::rename(&lyrics, target.with_extension("lrc"));
        }
        Ok(target.clone())
    }
}

#[derive(Debug, Default)]
pub struct Session {
    files: Vec<EditFile>,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl Session {
    pub fn new(mut files: Vec<EditFile>) -> Session {
        files.sort_by(|a, b| natural_cmp(&a.path, &b.path));
        Session { files, undo: Vec::new(), redo: Vec::new() }
    }

    pub fn files(&self) -> &[EditFile] {
        &self.files
    }

    pub fn file(&self, index: usize) -> Option<&EditFile> {
        self.files.get(index)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn modified_count(&self) -> usize {
        self.files.iter().filter(|f| f.is_modified()).count()
    }

    /// Add files that aren't open yet. Returns how many were added.
    pub fn add(&mut self, files: Vec<EditFile>) -> usize {
        let before = self.files.len();
        for f in files {
            if !self.files.iter().any(|e| e.path == f.path) {
                self.files.push(f);
            }
        }
        let added = self.files.len() - before;
        if added > 0 {
            // Indices change: old undo steps would point at the wrong files.
            self.files.sort_by(|a, b| natural_cmp(&a.path, &b.path));
            self.undo.clear();
            self.redo.clear();
        }
        added
    }

    /// Close files without saving.
    pub fn remove(&mut self, indices: &[usize]) {
        let mut sorted: Vec<usize> = indices.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        for i in sorted.into_iter().rev() {
            if i < self.files.len() {
                self.files.remove(i);
            }
        }
        self.undo.clear();
        self.redo.clear();
    }

    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.files.iter().position(|f| f.path == path)
    }

    pub fn value(&self, sel: &[usize], field: Field) -> Value {
        let mut values = sel.iter().filter_map(|i| self.files.get(*i)).map(|f| f.tags.get(field));
        let Some(first) = values.next() else { return Value::Same(String::new()) };
        if values.all(|v| v == first) { Value::Same(first.to_string()) } else { Value::Mixed }
    }

    /// Has the field been edited in any selected file?
    pub fn field_changed(&self, sel: &[usize], field: Field) -> bool {
        sel.iter().filter_map(|i| self.files.get(*i)).any(|f| f.tags.get(field) != f.original.get(field))
    }

    /// Can every selected file store the field?
    pub fn field_supported(&self, sel: &[usize], field: Field) -> bool {
        let mut files = sel.iter().filter_map(|i| self.files.get(*i)).peekable();
        files.peek().is_some() && files.all(|f| f.supports(field))
    }

    /// The pictures, if all selected files have the same ones.
    pub fn pictures(&self, sel: &[usize]) -> Option<Vec<Picture>> {
        let mut files = sel.iter().filter_map(|i| self.files.get(*i));
        let first = files.next()?;
        files.all(|f| f.tags.pictures == first.tags.pictures).then(|| first.tags.pictures.clone())
    }

    /// Apply `edit` to the selected files as one undo step. Returns how many files changed.
    fn edit(&mut self, sel: &[usize], label: &str, merge: Option<Field>, mut edit: impl FnMut(usize, &mut EditFile)) -> usize {
        let mut snapshots = Vec::new();
        for &i in sel {
            let Some(f) = self.files.get_mut(i) else { continue };
            if f.error.is_some() {
                continue;
            }
            let before = Snapshot { index: i, tags: f.tags.clone(), new_path: f.new_path.clone() };
            edit(i, f);
            if f.tags != before.tags || f.new_path != before.new_path {
                snapshots.push(before);
            }
        }
        let changed = snapshots.len();
        if changed == 0 {
            return 0;
        }
        self.redo.clear();
        let merge_key = merge.map(|field| (field, sel.to_vec()));
        if let Some(top) = self.undo.last_mut() {
            if merge_key.is_some() && top.merge_key == merge_key && top.at.elapsed() < MERGE_WINDOW {
                // Keep the state from before the first keystroke; add files it didn't cover.
                for s in snapshots {
                    if !top.files.iter().any(|t| t.index == s.index) {
                        top.files.push(s);
                    }
                }
                top.at = Instant::now();
                return changed;
            }
        }
        self.undo.push(Step { label: label.to_string(), files: snapshots, merge_key, at: Instant::now() });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        changed
    }

    pub fn set_field(&mut self, sel: &[usize], field: Field, value: &str) -> usize {
        let value = if field.is_number() { value.trim() } else { value };
        self.edit(sel, field.label(), Some(field), |_, f| {
            if f.supports(field) {
                f.tags.set(field, value);
            }
        })
    }

    /// Set several fields at once, per file (online lookup results, tag fill).
    pub fn apply(&mut self, label: &str, changes: &BTreeMap<usize, Vec<(Field, String)>>) -> usize {
        let sel: Vec<usize> = changes.keys().copied().collect();
        self.edit(&sel, label, None, |i, f| {
            for (field, value) in &changes[&i] {
                if f.supports(*field) {
                    f.tags.set(*field, value.as_str());
                }
            }
        })
    }

    /// EasyTAG's "#" button: number the selection in list order, per disc, and set the totals.
    pub fn number_tracks(&mut self, sel: &[usize]) -> usize {
        let mut per_disc: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut order: Vec<String> = Vec::new();
        for &i in sel {
            let Some(f) = self.files.get(i) else { continue };
            let disc = f.tags.get(Field::Disc).to_string();
            if !per_disc.contains_key(&disc) {
                order.push(disc.clone());
            }
            per_disc.entry(disc).or_default().push(i);
        }
        let mut numbers: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
        for disc in order {
            let files = &per_disc[&disc];
            for (n, i) in files.iter().enumerate() {
                numbers.insert(*i, (n + 1, files.len()));
            }
        }
        self.edit(sel, "Number tracks", None, |i, f| {
            if let Some((n, total)) = numbers.get(&i) {
                f.tags.set(Field::Track, n.to_string());
                f.tags.set(Field::TrackTotal, total.to_string());
            }
        })
    }

    /// Fill fields from file names. Returns (files changed, files that didn't fit the mask).
    pub fn fill_from_names(&mut self, sel: &[usize], mask: &Mask, overwrite: bool) -> (usize, usize) {
        let mut unmatched = 0;
        let mut found: BTreeMap<usize, Vec<(Field, String)>> = BTreeMap::new();
        for &i in sel {
            let Some(f) = self.files.get(i) else { continue };
            match mask.extract(&f.path) {
                Some(values) => {
                    let values = values.into_iter().filter(|(field, _)| overwrite || f.tags.get(*field).is_empty()).collect();
                    found.insert(i, values);
                }
                None => unmatched += 1,
            }
        }
        let changed = self.apply("Fill tags from file names", &found);
        (changed, unmatched)
    }

    /// What "rename files" would do: the new path, or why the file can't be renamed.
    pub fn rename_preview(&self, sel: &[usize], mask: &Mask, track_digits: usize) -> Vec<(usize, Result<PathBuf, String>)> {
        let mut out: Vec<(usize, Result<PathBuf, String>)> = sel
            .iter()
            .filter_map(|&i| self.files.get(i).map(|f| (i, f)))
            .map(|(i, f)| {
                let result = mask.format(&f.tags, track_digits).map(|rel| mask::renamed_path(&f.path, &rel)).map_err(|missing| {
                    let names: Vec<&str> = missing.iter().map(|m| m.label()).collect();
                    format!("No {}", names.join(", ").to_lowercase())
                });
                (i, result)
            })
            .collect();
        // Two files can't end up with the same name.
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for (_, r) in &out {
            if let Ok(p) = r {
                *seen.entry(p.to_string_lossy().to_lowercase()).or_default() += 1;
            }
        }
        for (i, r) in &mut out {
            if let Ok(p) = r {
                let key = p.to_string_lossy().to_lowercase();
                if seen[&key] > 1 {
                    *r = Err("Another file would get the same name".into());
                } else if p.exists() && self.files.get(*i).is_some_and(|f| f.path.to_string_lossy().to_lowercase() != key) {
                    *r = Err("A file with this name already exists".into());
                }
            }
        }
        out
    }

    /// Queue renames from tags. Returns (files queued, files skipped).
    pub fn rename_from_tags(&mut self, sel: &[usize], mask: &Mask, track_digits: usize) -> (usize, usize) {
        let preview = self.rename_preview(sel, mask, track_digits);
        let skipped = preview.iter().filter(|(_, r)| r.is_err()).count();
        let targets: BTreeMap<usize, PathBuf> = preview.into_iter().filter_map(|(i, r)| r.ok().map(|p| (i, p))).collect();
        let changed = self.edit(sel, "Rename files", None, |i, f| {
            if let Some(p) = targets.get(&i) {
                f.new_path = (*p != f.path).then(|| p.clone());
            }
        });
        (changed, skipped)
    }

    pub fn cleanup(&mut self, sel: &[usize], fields: &[Field], cleanup: &Cleanup) -> usize {
        self.edit(sel, "Clean up", None, |_, f| {
            for &field in fields {
                let old = f.tags.get(field);
                if !old.is_empty() {
                    let new = cleanup.apply(old);
                    f.tags.set(field, new);
                }
            }
        })
    }

    pub fn set_picture(&mut self, sel: &[usize], picture: &Picture) -> usize {
        self.edit(sel, "Set picture", None, |_, f| {
            if f.supports_pictures {
                f.tags.set_picture(picture.clone());
            }
        })
    }

    /// Give each file its own picture (cover art found per folder or per album).
    pub fn set_pictures(&mut self, label: &str, pictures: &BTreeMap<usize, Picture>) -> usize {
        let sel: Vec<usize> = pictures.keys().copied().collect();
        self.edit(&sel, label, None, |i, f| {
            if f.supports_pictures {
                f.tags.set_picture(pictures[&i].clone());
            }
        })
    }

    pub fn remove_picture(&mut self, sel: &[usize], kind: PictureKind) -> usize {
        self.edit(sel, "Remove picture", None, |_, f| f.tags.pictures.retain(|p| p.kind != kind))
    }

    pub fn set_picture_kind(&mut self, sel: &[usize], from: PictureKind, to: PictureKind) -> usize {
        self.edit(sel, "Change picture type", None, |_, f| {
            if let Some(p) = f.tags.pictures.iter().find(|p| p.kind == from).cloned() {
                f.tags.pictures.retain(|p| p.kind != from && p.kind != to);
                f.tags.pictures.push(Picture { kind: to, ..p });
            }
        })
    }

    /// Drop pending changes.
    pub fn revert(&mut self, sel: &[usize]) -> usize {
        self.edit(sel, "Revert", None, |_, f| {
            f.tags = f.original.clone();
            f.new_path = None;
        })
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo the last step. Returns its label and the files it touched.
    pub fn undo(&mut self) -> Option<(String, Vec<usize>)> {
        let step = self.undo.pop()?;
        let inverse = self.swap(step);
        let result = (inverse.label.clone(), inverse.files.iter().map(|s| s.index).collect());
        self.redo.push(inverse);
        Some(result)
    }

    pub fn redo(&mut self) -> Option<(String, Vec<usize>)> {
        let step = self.redo.pop()?;
        let inverse = self.swap(step);
        let result = (inverse.label.clone(), inverse.files.iter().map(|s| s.index).collect());
        self.undo.push(inverse);
        Some(result)
    }

    /// Restore a step's snapshots and return the step that undoes that.
    fn swap(&mut self, step: Step) -> Step {
        let mut inverse = Vec::with_capacity(step.files.len());
        for s in step.files {
            if let Some(f) = self.files.get_mut(s.index) {
                inverse.push(Snapshot { index: s.index, tags: std::mem::replace(&mut f.tags, s.tags), new_path: std::mem::replace(&mut f.new_path, s.new_path) });
            }
        }
        Step { label: step.label, files: inverse, merge_key: None, at: Instant::now() }
    }

    pub fn save_jobs(&self) -> Vec<SaveJob> {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_modified() && f.error.is_none())
            .map(|(index, f)| SaveJob {
                index,
                path: f.path.clone(),
                before: f.original.clone(),
                after: f.tags.clone(),
                new_path: f.new_path.clone(),
            })
            .collect()
    }

    /// Record the outcome of a save job: on success the file is read back from its new path.
    pub fn finish_save(&mut self, job: &SaveJob, result: Result<EditFile, String>) {
        let Some(f) = self.files.get_mut(job.index).filter(|f| f.path == job.path) else { return };
        match result {
            Ok(fresh) => *f = fresh,
            Err(e) => f.save_error = Some(e),
        }
    }
}

/// Order paths like a person would: "2 - x" before "10 - x".
pub fn natural_cmp(a: &Path, b: &Path) -> std::cmp::Ordering {
    let (a, b) = (a.to_string_lossy().to_lowercase(), b.to_string_lossy().to_lowercase());
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (m2, n2) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let ord = m2.len().cmp(&n2.len()).then_with(|| m2.cmp(n2));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(name: &str, title: &str) -> EditFile {
        let mut tags = TagData::default();
        tags.set(Field::Title, title);
        EditFile {
            path: PathBuf::from(format!("/music/album/{name}")),
            info: FileInfo::default(),
            supported: Field::ALL.to_vec(),
            supports_pictures: true,
            original: tags.clone(),
            tags,
            new_path: None,
            error: None,
            save_error: None,
        }
    }

    fn session() -> Session {
        Session::new(vec![fake("10 - c.flac", "C"), fake("2 - b.flac", "B"), fake("1 - a.flac", "A")])
    }

    #[test]
    fn natural_order() {
        let s = session();
        let names: Vec<String> = s.files().iter().map(EditFile::file_name).collect();
        assert_eq!(names, ["1 - a.flac", "2 - b.flac", "10 - c.flac"]);
    }

    #[test]
    fn multi_values_and_editing() {
        let mut s = session();
        assert_eq!(s.value(&[0, 1], Field::Title), Value::Mixed);
        assert_eq!(s.value(&[0, 1], Field::Album), Value::Same(String::new()));
        assert_eq!(s.set_field(&[0, 1, 2], Field::Album, "Record"), 3);
        assert_eq!(s.value(&[0, 1, 2], Field::Album), Value::Same("Record".into()));
        assert!(s.field_changed(&[1], Field::Album));
        assert_eq!(s.modified_count(), 3);
        s.revert(&[1]);
        assert_eq!(s.modified_count(), 2);
    }

    #[test]
    fn typing_is_one_undo_step() {
        let mut s = session();
        for text in ["R", "Re", "Rec"] {
            s.set_field(&[0], Field::Album, text);
        }
        s.set_field(&[0], Field::Genre, "Jazz");
        assert_eq!(s.undo().unwrap().0, "Genre");
        assert_eq!(s.files()[0].tags.get(Field::Album), "Rec");
        assert_eq!(s.undo().unwrap().0, "Album");
        assert_eq!(s.files()[0].tags.get(Field::Album), "");
        assert!(s.undo().is_none());
        s.redo();
        assert_eq!(s.files()[0].tags.get(Field::Album), "Rec");
    }

    #[test]
    fn numbering_per_disc() {
        let mut s = Session::new(vec![fake("a1.flac", "a"), fake("a2.flac", "b"), fake("b1.flac", "c")]);
        s.set_field(&[0, 1], Field::Disc, "1");
        s.set_field(&[2], Field::Disc, "2");
        s.number_tracks(&[0, 1, 2]);
        let got: Vec<(&str, &str)> = s.files().iter().map(|f| (f.tags.get(Field::Track), f.tags.get(Field::TrackTotal))).collect();
        assert_eq!(got, [("1", "2"), ("2", "2"), ("1", "1")]);
    }

    #[test]
    fn fill_and_rename() {
        let mut s = session();
        let mask = Mask::parse("%n - %t").unwrap();
        let (changed, unmatched) = s.fill_from_names(&[0, 1, 2], &mask, true);
        assert_eq!((changed, unmatched), (3, 0));
        assert_eq!(s.files()[2].tags.get(Field::Track), "10");
        assert_eq!(s.files()[2].tags.get(Field::Title), "c");

        let rename = Mask::parse("%n %t").unwrap();
        let (queued, skipped) = s.rename_from_tags(&[0, 1, 2], &rename, 2);
        assert_eq!((queued, skipped), (3, 0));
        assert_eq!(s.files()[0].target_name().as_deref(), Some("01 a.flac"));
        s.undo();
        assert!(s.files()[0].new_path.is_none());
    }

    #[test]
    fn rename_conflicts_are_reported() {
        let s = session();
        let mask = Mask::parse("same").unwrap();
        let preview = s.rename_preview(&[0, 1], &mask, 2);
        assert!(preview.iter().all(|(_, r)| r.is_err()));
    }
}
