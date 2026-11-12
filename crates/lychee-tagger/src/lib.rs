//! The engine behind Lychee's tag editor, modelled on EasyTAG:
//!
//! * [`file`]: read and write tags and pictures with lofty, without losing frames the editor
//!   doesn't know about;
//! * [`mask`]: EasyTAG's scanner masks (`%n - %t`), to fill tags from file names and to rename
//!   files from tags;
//! * [`process`]: clean-up of field text (case, underscores, spaces);
//! * [`credits`]: artist credits written one way ("A feat. B", "A, B & C", "A; B");
//! * [`session`]: a set of files being edited, with pending changes, multi-selection and undo.
//!
//! Nothing here touches the UI or the library database.

pub mod credits;
pub mod fields;
pub mod file;
pub mod genres;
pub mod mask;
pub mod process;
pub mod session;

pub use credits::{CreditStyle, Credits};
pub use fields::Field;
pub use file::{FileInfo, Picture, PictureKind, ReadFile, TagData};
pub use genres::Genres;
pub use mask::Mask;
pub use process::{Case, Cleanup};
pub use session::{EditFile, SaveJob, Session, Value};

/// Errors carry a sentence for the user. lofty's own messages are generic ("failed to write to
/// file"); the real reason (permissions, a read-only drive...) sits in their source chain.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}", io_message(.0))]
    Io(#[from] std::io::Error),
    #[error("{}", explain(.0))]
    Read(#[from] lofty::error::FileParseError),
    #[error("{}", explain(.0))]
    Write(#[from] lofty::error::FileEncodingError),
    #[error("this file type can't be tagged")]
    Unsupported,
    #[error("“{0}” isn't a year or a date (use something like 2006 or 2006-04-17)")]
    BadDate(String),
}

/// A plain sentence for the I/O errors people actually run into.
pub fn io_message(e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::PermissionDenied => "Lychee isn't allowed to change this file (check its permissions)".into(),
        ErrorKind::ReadOnlyFilesystem => "the file is on a read-only drive".into(),
        ErrorKind::NotFound => "the file isn't there any more (moved or deleted?)".into(),
        ErrorKind::StorageFull => "the disk is full".into(),
        _ => e.to_string(),
    }
}

fn explain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut source = e.source();
    while let Some(s) = source {
        if let Some(io) = s.downcast_ref::<std::io::Error>() {
            return io_message(io);
        }
        source = s.source();
    }
    let mut message = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        message.push_str(": ");
        message.push_str(&s.to_string());
        source = s.source();
    }
    message
}
