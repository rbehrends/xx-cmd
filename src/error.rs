use std::fmt::Display;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

pub(crate) const EX_USAGE: i32 = 2;
pub(crate) const EX_INPUT: i32 = 3;
pub(crate) const EX_ARCHIVE: i32 = 4;
pub(crate) const EX_SAFETY: i32 = 5;
pub(crate) const EX_OUTPUT: i32 = 6;

#[derive(Debug, Error)]
pub enum XxError {
    #[error("usage error: {0}")]
    Usage(String),
    #[error("could not open input {path}: {source}")]
    InputIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("format not recognized for input {0}")]
    FormatNotRecognized(PathBuf),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("archive error{entry}: {message}")]
    Archive { entry: String, message: String },
    #[error("unsafe archive path {path:?}: {reason}")]
    UnsafePath { path: Vec<u8>, reason: String },
    #[error("output error at {path}: {source}")]
    OutputIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("output conflict at {0}: {1}")]
    OutputConflict(PathBuf, String),
    #[error("output already exists: {0}")]
    OutputExists(PathBuf),
    #[error("listing output cannot represent {path:?}: {reason}")]
    UnrepresentableOutput { path: Vec<u8>, reason: String },
}

pub(crate) type Result<T> = std::result::Result<T, XxError>;

pub(crate) fn exit_code(error: &XxError) -> i32 {
    match error {
        XxError::Usage(_) | XxError::UnsupportedFormat(_) => EX_USAGE,
        XxError::FormatNotRecognized(_) | XxError::InputIo { .. } => EX_INPUT,
        XxError::Archive { .. } => EX_ARCHIVE,
        XxError::UnsafePath { .. } => EX_SAFETY,
        XxError::OutputIo { .. }
        | XxError::OutputConflict(..)
        | XxError::OutputExists(_)
        | XxError::UnrepresentableOutput { .. } => EX_OUTPUT,
    }
}

pub(crate) fn archive_error<E: Display>(entry: Option<&[u8]>, error: E) -> XxError {
    XxError::Archive {
        entry: entry.map(display_bytes).unwrap_or_default(),
        message: error.to_string(),
    }
}

pub(crate) fn output_error(source: io::Error) -> XxError {
    XxError::OutputIo {
        path: PathBuf::from("standard output"),
        source,
    }
}

pub(crate) fn output_error_at(path: &std::path::Path, source: io::Error) -> XxError {
    XxError::OutputIo {
        path: path.to_path_buf(),
        source,
    }
}

pub(crate) fn display_bytes(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
