//! A lazily-built Slint model: rows are produced only when the view asks for them, and rows
//! waiting for a cover image are refreshed when it arrives.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use slint::{Model, ModelNotify, ModelTracker};

type Builder<T> = Box<dyn Fn(usize, &mut dyn FnMut(&str)) -> T>;

pub struct LazyModel<T> {
    len: Cell<usize>,
    build: RefCell<Builder<T>>,
    waiting: RefCell<HashMap<String, Vec<usize>>>,
    notify: ModelNotify,
}

impl<T: Clone + Default + 'static> LazyModel<T> {
    pub fn new() -> Self {
        LazyModel {
            len: Cell::new(0),
            build: RefCell::new(Box::new(|_, _| T::default())),
            waiting: RefCell::default(),
            notify: ModelNotify::default(),
        }
    }

    /// Replace the contents. `build(row, need_image)` creates a row and reports missing images.
    pub fn set(&self, len: usize, build: impl Fn(usize, &mut dyn FnMut(&str)) -> T + 'static) {
        *self.build.borrow_mut() = Box::new(build);
        self.len.set(len);
        self.waiting.borrow_mut().clear();
        self.notify.reset();
    }

    /// Rebuild every visible row (e.g. the playing track changed).
    pub fn refresh(&self) {
        self.waiting.borrow_mut().clear();
        self.notify.reset();
    }

    pub fn images_ready(&self, keys: &[String]) {
        let mut rows = Vec::new();
        {
            let mut waiting = self.waiting.borrow_mut();
            for key in keys {
                if let Some(r) = waiting.remove(key) {
                    rows.extend(r);
                }
            }
        }
        rows.sort_unstable();
        rows.dedup();
        for row in rows {
            if row < self.len.get() {
                self.notify.row_changed(row);
            }
        }
    }
}

impl<T: Clone + Default + 'static> Model for LazyModel<T> {
    type Data = T;

    fn row_count(&self) -> usize {
        self.len.get()
    }

    fn row_data(&self, row: usize) -> Option<T> {
        if row >= self.len.get() {
            return None;
        }
        let mut missing: Vec<String> = Vec::new();
        let value = (self.build.borrow())(row, &mut |key| missing.push(key.to_string()));
        if !missing.is_empty() {
            let mut waiting = self.waiting.borrow_mut();
            for key in missing {
                let rows = waiting.entry(key).or_default();
                if !rows.contains(&row) {
                    rows.push(row);
                }
            }
        }
        Some(value)
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}
