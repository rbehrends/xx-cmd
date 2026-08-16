use std::num::NonZeroUsize;
use std::path::PathBuf;

use crate::path::RelPath;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormatId {
    Tar,
    TarCompress,
    TarGzip,
    TarBzip2,
    TarZstd,
    TarXz,
    Pax,
    PaxCompress,
    PaxGzip,
    PaxBzip2,
    PaxZstd,
    PaxXz,
    Cpio,
    CpioCompress,
    CpioGzip,
    CpioBzip2,
    CpioZstd,
    CpioXz,
    Ar,
    #[cfg(feature = "seven-z")]
    SevenZ,
    Zip,
    Jar,
}

impl FormatId {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Tar => "tar",
            Self::TarCompress => "tar.Z",
            Self::TarGzip => "tar.gz",
            Self::TarBzip2 => "tar.bz2",
            Self::TarZstd => "tar.zst",
            Self::TarXz => "tar.xz",
            Self::Pax => "pax",
            Self::PaxCompress => "pax.Z",
            Self::PaxGzip => "pax.gz",
            Self::PaxBzip2 => "pax.bz2",
            Self::PaxZstd => "pax.zst",
            Self::PaxXz => "pax.xz",
            Self::Cpio => "cpio",
            Self::CpioCompress => "cpio.Z",
            Self::CpioGzip => "cpio.gz",
            Self::CpioBzip2 => "cpio.bz2",
            Self::CpioZstd => "cpio.zst",
            Self::CpioXz => "cpio.xz",
            Self::Ar => "ar",
            #[cfg(feature = "seven-z")]
            Self::SevenZ => "7z",
            Self::Zip => "zip",
            Self::Jar => "jar",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputMode {
    CreateOnly,
    Replace,
    Merge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListKind {
    Names,
    Verbose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListFormat {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListDelimiter {
    Line,
    Nul,
}

#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub(crate) input: PathBuf,
    pub(crate) format: FormatId,
    pub(crate) output: Option<PathBuf>,
    pub(crate) mode: OutputMode,
    pub(crate) threads: NonZeroUsize,
    pub(crate) list_kind: Option<ListKind>,
    pub(crate) list_format: ListFormat,
    pub(crate) delimiter: ListDelimiter,
    pub(crate) progress: bool,
    pub(crate) verbose: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    File,
    Directory,
    Symlink,
    Hardlink,
    Special,
}

impl EntryKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
            Self::Hardlink => "hardlink",
            Self::Special => "special",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EntryRecord {
    pub(crate) name: Vec<u8>,
    pub(crate) kind: EntryKind,
    pub(crate) size: Option<u64>,
    pub(crate) perm: Option<String>,
    pub(crate) mtime: Option<String>,
    pub(crate) link_target: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingLink {
    pub(crate) path: RelPath,
    pub(crate) kind: EntryKind,
    pub(crate) target: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct ZipPlanEntry {
    pub(crate) index: usize,
    pub(crate) path: RelPath,
    pub(crate) record: EntryRecord,
    pub(crate) mode: Option<u32>,
    pub(crate) mtime: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct JsonRecord {
    pub(crate) name: Vec<u8>,
    pub(crate) kind: String,
    pub(crate) size: Option<u64>,
    pub(crate) perm: Option<String>,
    pub(crate) mtime: Option<String>,
    pub(crate) link_target: Option<Vec<u8>>,
}
