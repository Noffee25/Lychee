//! The engine behind Lychee's tag editor, modelled on EasyTAG:
//!
//! * [`file`]: read and write tags and pictures with lofty, without losing frames the editor
//!   doesn't know about;
//! * [`mask`]: EasyTAG's scanner masks (`%n - %t`), to fill tags from file names and to rename
//!   files from tags;
//! * [`process`]: clean-up of field text (case, underscores, spaces);
//! * [`session`]: a set of files being edited, with pending changes, multi-selection and undo.
//!
//! Nothing here touches the UI or the library database.

pub mod fields;
pub mod file;
pub mod mask;
pub mod process;
pub mod session;

pub use fields::Field;
pub use file::{FileInfo, Picture, PictureKind, ReadFile, TagData};
pub use mask::Mask;
pub use process::{Case, Cleanup};
pub use session::{EditFile, SaveJob, Session, Value};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Read(#[from] lofty::error::FileParseError),
    #[error("{0}")]
    Write(#[from] lofty::error::FileEncodingError),
    #[error("this file type can't be tagged")]
    Unsupported,
}
