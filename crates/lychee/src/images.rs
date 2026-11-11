//! Cover images for the UI: decoded on a background thread, kept in a small LRU cache.
//!
//! `slint::Image` must be created on the UI thread, so the worker sends back raw pixel buffers
//! and the UI thread wraps them. Grids only ask for the covers of rows actually on screen.

use std::cell::RefCell;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, Sender};
use lru::LruCache;
use lychee_library::Library;
use slint::{Image, Rgb8Pixel, SharedPixelBuffer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Size {
    Thumb,
    Large,
}

type Decoded = (String, Size, Option<SharedPixelBuffer<Rgb8Pixel>>);

pub struct ImageCache {
    library: Arc<Library>,
    thumbs: RefCell<LruCache<String, Image>>,
    large: RefCell<LruCache<String, Image>>,
    pending: RefCell<HashSet<(String, Size)>>,
    missing: RefCell<HashSet<String>>,
    requests: Sender<(String, Size, PathBuf)>,
    results: Receiver<Decoded>,
    /// Decode on the calling thread instead (used for headless screenshots).
    synchronous: bool,
}

impl ImageCache {
    pub fn new(library: Arc<Library>, synchronous: bool) -> ImageCache {
        let (req_tx, req_rx) = crossbeam_channel::unbounded::<(String, Size, PathBuf)>();
        let (res_tx, res_rx) = crossbeam_channel::unbounded::<Decoded>();
        for i in 0..2 {
            let rx = req_rx.clone();
            let tx = res_tx.clone();
            let _ = thread::Builder::new().name(format!("lychee-images-{i}")).spawn(move || {
                while let Ok((key, size, path)) = rx.recv() {
                    let _ = tx.send((key, size, decode(&path)));
                }
            });
        }
        ImageCache {
            library,
            thumbs: RefCell::new(LruCache::new(NonZeroUsize::new(360).unwrap())),
            large: RefCell::new(LruCache::new(NonZeroUsize::new(12).unwrap())),
            pending: RefCell::default(),
            missing: RefCell::default(),
            requests: req_tx,
            results: res_rx,
            synchronous,
        }
    }

    fn path(&self, key: &str, size: Size) -> PathBuf {
        match size {
            Size::Thumb => self.library.artwork_thumb(key),
            Size::Large => self.library.artwork_large(key),
        }
    }

    fn cache(&self, size: Size) -> &RefCell<LruCache<String, Image>> {
        match size {
            Size::Thumb => &self.thumbs,
            Size::Large => &self.large,
        }
    }

    /// The image if it is ready; otherwise schedule it and return `None`.
    pub fn get(&self, key: &str, size: Size) -> Option<Image> {
        if let Some(img) = self.cache(size).borrow_mut().get(key) {
            return Some(img.clone());
        }
        if self.missing.borrow().contains(key) {
            return None;
        }
        if self.synchronous {
            return self.get_now(key, size);
        }
        if self.pending.borrow_mut().insert((key.to_string(), size)) {
            let _ = self.requests.send((key.to_string(), size, self.path(key, size)));
        }
        None
    }

    /// Decode right away on this thread (one-off large images such as the now-playing cover).
    pub fn get_now(&self, key: &str, size: Size) -> Option<Image> {
        if let Some(img) = self.cache(size).borrow_mut().get(key) {
            return Some(img.clone());
        }
        match decode(&self.path(key, size)) {
            Some(buf) => {
                let img = Image::from_rgb8(buf);
                self.cache(size).borrow_mut().put(key.to_string(), img.clone());
                Some(img)
            }
            None => {
                self.missing.borrow_mut().insert(key.to_string());
                None
            }
        }
    }

    /// Collect finished decodes. Returns the keys that just became available.
    pub fn poll(&self) -> Vec<String> {
        let mut ready = Vec::new();
        while let Ok((key, size, buf)) = self.results.try_recv() {
            self.pending.borrow_mut().remove(&(key.clone(), size));
            match buf {
                Some(buf) => {
                    self.cache(size).borrow_mut().put(key.clone(), Image::from_rgb8(buf));
                    ready.push(key);
                }
                None => {
                    self.missing.borrow_mut().insert(key);
                }
            }
        }
        ready
    }

    /// Forget negative lookups after a rescan (new art may exist now).
    pub fn clear_missing(&self) {
        self.missing.borrow_mut().clear();
    }
}

fn decode(path: &std::path::Path) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
    let img = image::open(path).ok()?.to_rgb8();
    Some(SharedPixelBuffer::clone_from_slice(img.as_raw(), img.width(), img.height()))
}
