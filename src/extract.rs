use crate::error::{Result, XxError, archive_error, display_bytes, output_error, output_error_at};
#[cfg(feature = "seven-z")]
use crate::format::seven_z_seconds;
use crate::format::{
    cpio_kind, format_zip_time, open_archive_reader, tar_kind, zip_kind, zip_time_seconds,
};

use crate::model::{EntryKind, EntryRecord, FormatId, PendingLink, Request, ZipPlanEntry};
use crate::path::{RelPath, normalize_path, validate_entry_conflict, validate_link_target};
use crate::safety::{
    apply_link, apply_metadata, ensure_directory, ensure_parent, mode_string,
    remove_leaf_if_present,
};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use tar::EntryType;
use zip::read::ZipArchive;

struct ProgressState {
    format: &'static str,
    completed: u64,
    bytes: u64,
}

#[derive(Clone)]
pub(crate) struct ExtractionReporter {
    output: Option<Arc<Mutex<io::Stdout>>>,
    progress: Option<Arc<Mutex<ProgressState>>>,
}

impl ExtractionReporter {
    pub(crate) fn new(verbose: bool, progress: bool, format: FormatId) -> Self {
        Self {
            output: verbose.then(|| Arc::new(Mutex::new(io::stdout()))),
            progress: progress.then(|| {
                Arc::new(Mutex::new(ProgressState {
                    format: format.label(),
                    completed: 0,
                    bytes: 0,
                }))
            }),
        }
    }

    pub(crate) fn start(&self) {
        self.emit_progress("extract", 0, false);
    }

    pub(crate) fn file(&self, path: &RelPath, size: u64) -> Result<()> {
        self.entry(path, Some(size))?;
        self.emit_progress("extract", size, true);
        Ok(())
    }

    pub(crate) fn directory(&self, path: &RelPath) -> Result<()> {
        self.entry(path, None)?;
        self.emit_progress("extract", 0, true);
        Ok(())
    }

    pub(crate) fn link(&self) {
        self.emit_progress("extract", 0, true);
    }

    fn entry(&self, path: &RelPath, size: Option<u64>) -> Result<()> {
        let Some(output) = &self.output else {
            return Ok(());
        };
        let mut output = output
            .lock()
            .map_err(|_| output_error(io::Error::other("standard output lock is poisoned")))?;
        let displayed_path = display_extraction_path(path, size.is_none());
        match size {
            Some(size) => writeln!(output, "{} ({})", displayed_path, human_extract_size(size)),
            None => writeln!(output, "{displayed_path}"),
        }
        .map_err(output_error)?;
        output.flush().map_err(output_error)
    }

    pub(crate) fn move_to(&self, source: &Path, destination: &Path) -> Result<()> {
        if let Some(output) = &self.output {
            let mut output = output
                .lock()
                .map_err(|_| output_error(io::Error::other("standard output lock is poisoned")))?;
            let source = normalize_display_path(source);
            writeln!(output, "{} -> {}", source.display(), destination.display())
                .map_err(output_error)?;
            output.flush().map_err(output_error)?;
        }
        self.emit_progress("publish", 0, false);
        Ok(())
    }

    fn emit_progress(&self, phase: &str, bytes: u64, completed: bool) {
        let Some(progress) = &self.progress else {
            return;
        };
        let Ok(mut progress) = progress.lock() else {
            return;
        };
        if bytes > 0 {
            progress.bytes = progress.bytes.saturating_add(bytes);
        }
        if completed {
            progress.completed = progress.completed.saturating_add(1);
        }
        let mut stderr = io::stderr().lock();
        let _ = writeln!(
            stderr,
            "xx: phase={phase} format={} completed={} entries bytes={}",
            progress.format, progress.completed, progress.bytes
        );
    }
}

pub(crate) fn display_extraction_path(path: &RelPath, directory: bool) -> String {
    let mut displayed = path.to_path(Path::new("")).display().to_string();
    if directory && !displayed.ends_with(std::path::MAIN_SEPARATOR) {
        displayed.push(std::path::MAIN_SEPARATOR);
    }
    displayed
}

pub(crate) fn normalize_display_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub(crate) fn human_extract_size(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["", "k", "M", "G", "T", "P", "E"];
    let mut unit = 0;
    let mut divisor = 1_u64;
    while bytes / divisor >= 1024 && unit < UNITS.len() - 1 {
        divisor *= 1024;
        unit += 1;
    }
    if unit == 0 {
        return bytes.to_string();
    }

    let value = bytes as f64 / divisor as f64;
    let value = if value < 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    };
    format!("{}{}", value.trim_end_matches(".0"), UNITS[unit])
}

pub(crate) fn extract_archive(
    request: &Request,
    destination: &Path,
    reporter: &ExtractionReporter,
) -> Result<()> {
    match request.format {
        FormatId::Zip | FormatId::Jar => extract_zip(request, destination, reporter),
        FormatId::Ar => extract_ar(request, destination, reporter),
        #[cfg(feature = "seven-z")]
        FormatId::SevenZ => extract_seven_z(request, destination, reporter),
        FormatId::Cpio
        | FormatId::CpioCompress
        | FormatId::CpioGzip
        | FormatId::CpioBzip2
        | FormatId::CpioZstd
        | FormatId::CpioXz => {
            let reader = open_archive_reader(request.format, &request.input)?;
            extract_cpio(reader, destination, reporter)
        }
        _ => {
            let reader = open_archive_reader(request.format, &request.input)?;
            extract_tar(reader, destination, reporter)
        }
    }
}

fn extract_tar<R: Read>(
    reader: R,
    destination: &Path,
    reporter: &ExtractionReporter,
) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|error| archive_error(None, error))?;
    let mut links = Vec::new();
    let mut directories = Vec::new();
    let mut seen: HashMap<RelPath, EntryKind> = HashMap::new();

    for entry in entries {
        let mut entry = entry.map_err(|error| archive_error(None, error))?;
        let header = entry.header();
        let entry_type = header.entry_type();
        if matches!(
            entry_type,
            EntryType::XHeader
                | EntryType::XGlobalHeader
                | EntryType::GNULongName
                | EntryType::GNULongLink
        ) {
            continue;
        }
        let raw_name = entry.path_bytes().to_vec();
        let path = normalize_path(&raw_name)?;
        let kind = tar_kind(entry_type);
        if kind == EntryKind::Special {
            return Err(XxError::Archive {
                entry: display_bytes(&raw_name),
                message: "special files are not supported".into(),
            });
        }
        validate_entry_conflict(&mut seen, &path, kind, &raw_name)?;
        let mode = header.mode().ok();
        let mtime = header.mtime().ok();
        let size = header
            .size()
            .map_err(|error| archive_error(Some(&raw_name), error))?;
        match kind {
            EntryKind::Directory => {
                ensure_directory(destination, &path)?;
                reporter.directory(&path)?;
                directories.push((path, mode, mtime));
            }
            EntryKind::File => {
                let target = path.to_path(destination);
                ensure_parent(destination, &path)?;
                remove_leaf_if_present(&target)?;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .map_err(|source| output_error_at(&target, source))?;
                io::copy(&mut entry, &mut output)
                    .map_err(|source| output_error_at(&target, source))?;
                output
                    .flush()
                    .map_err(|source| output_error_at(&target, source))?;
                apply_metadata(&target, mode, mtime)?;
                reporter.file(&path, size)?;
            }
            EntryKind::Symlink | EntryKind::Hardlink => {
                let target = entry
                    .link_name_bytes()
                    .map(|value| value.to_vec())
                    .ok_or_else(|| XxError::Archive {
                        entry: display_bytes(&raw_name),
                        message: "link has no target".into(),
                    })?;
                validate_link_target(&path, &target)?;
                links.push(PendingLink { path, kind, target });
            }
            EntryKind::Special => unreachable!(),
        }
    }

    for link in links {
        apply_link(destination, &link)?;
        reporter.link();
    }
    for (path, mode, mtime) in directories {
        apply_metadata(&path.to_path(destination), mode, mtime)?;
    }
    Ok(())
}

#[cfg(feature = "seven-z")]
fn extract_seven_z(
    request: &Request,
    destination: &Path,
    reporter: &ExtractionReporter,
) -> Result<()> {
    let mut archive =
        sevenz_rust2::ArchiveReader::open(&request.input, sevenz_rust2::Password::empty())
            .map_err(|error| archive_error(None, error))?;
    let mut seen: HashMap<RelPath, EntryKind> = HashMap::new();
    for entry in &archive.archive().files {
        let raw_name = entry.name.as_bytes().to_vec();
        let kind = if entry.is_anti_item {
            EntryKind::Special
        } else if entry.is_directory {
            EntryKind::Directory
        } else {
            EntryKind::File
        };
        if kind == EntryKind::Special {
            return Err(XxError::Archive {
                entry: display_bytes(&raw_name),
                message: "anti-items are not supported".into(),
            });
        }
        let path = normalize_path(&raw_name)?;
        validate_entry_conflict(&mut seen, &path, kind, &raw_name)?;
    }

    let thread_count = u32::try_from(request.threads.get()).unwrap_or(u32::MAX);
    archive.set_thread_count(thread_count);
    let mut callback_error = None;
    let decode_result = archive.for_each_entries(|entry, source| {
        let raw_name = entry.name.as_bytes().to_vec();
        let path = match normalize_path(&raw_name) {
            Ok(path) => path,
            Err(error) => {
                callback_error = Some(error);
                return Ok(false);
            }
        };
        let result = if entry.is_directory {
            ensure_directory(destination, &path)
                .and_then(|()| {
                    apply_metadata(
                        &path.to_path(destination),
                        None,
                        if entry.has_last_modified_date {
                            seven_z_seconds(entry.last_modified_date)
                        } else {
                            None
                        },
                    )
                })
                .and_then(|()| reporter.directory(&path))
        } else {
            let target = path.to_path(destination);
            ensure_parent(destination, &path)
                .and_then(|()| remove_leaf_if_present(&target))
                .and_then(|()| {
                    let mut output = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)
                        .map_err(|source| output_error_at(&target, source))?;
                    io::copy(source, &mut output)
                        .map_err(|source| output_error_at(&target, source))?;
                    output
                        .flush()
                        .map_err(|source| output_error_at(&target, source))?;
                    Ok(())
                })
                .and_then(|()| {
                    apply_metadata(
                        &target,
                        None,
                        if entry.has_last_modified_date {
                            seven_z_seconds(entry.last_modified_date)
                        } else {
                            None
                        },
                    )
                })
                .and_then(|()| reporter.file(&path, entry.size))
        };
        if let Err(error) = result {
            callback_error = Some(error);
            Ok(false)
        } else {
            Ok(true)
        }
    });
    if let Some(error) = callback_error {
        return Err(error);
    }
    decode_result.map_err(|error| archive_error(None, error))?;
    Ok(())
}

fn extract_cpio<R: Read>(
    reader: R,
    destination: &Path,
    reporter: &ExtractionReporter,
) -> Result<()> {
    let mut reader = cpio::NewcReader::new(reader).map_err(|error| archive_error(None, error))?;
    let mut links = Vec::new();
    let mut metadata = Vec::new();
    let mut seen: HashMap<RelPath, EntryKind> = HashMap::new();

    loop {
        let entry = reader.entry().clone();
        if entry.is_trailer() {
            break;
        }
        let raw_name = entry.name().as_bytes().to_vec();
        let path = normalize_path(&raw_name)?;
        let kind = cpio_kind(entry.mode());
        if kind == EntryKind::Special {
            return Err(XxError::Archive {
                entry: display_bytes(&raw_name),
                message: "special files are not supported".into(),
            });
        }
        validate_entry_conflict(&mut seen, &path, kind, &raw_name)?;
        let mode = Some(entry.mode());
        let mtime = Some(u64::from(entry.mtime()));
        let inner = match kind {
            EntryKind::Directory => {
                ensure_directory(destination, &path)?;
                reporter.directory(&path)?;
                metadata.push((path, mode, mtime));
                reader
                    .finish()
                    .map_err(|error| archive_error(Some(&raw_name), error))?
            }
            EntryKind::File => {
                let target = path.to_path(destination);
                ensure_parent(destination, &path)?;
                remove_leaf_if_present(&target)?;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .map_err(|source| output_error_at(&target, source))?;
                let inner = reader
                    .to_writer(&mut output)
                    .map_err(|error| output_error_at(&target, error))?;
                output
                    .flush()
                    .map_err(|source| output_error_at(&target, source))?;
                metadata.push((path.clone(), mode, mtime));
                reporter.file(&path, u64::from(entry.file_size()))?;
                inner
            }
            EntryKind::Symlink => {
                let mut target = Vec::new();
                reader
                    .read_to_end(&mut target)
                    .map_err(|error| archive_error(Some(&raw_name), error))?;
                validate_link_target(&path, &target)?;
                links.push(PendingLink { path, kind, target });
                reader
                    .finish()
                    .map_err(|error| archive_error(Some(&raw_name), error))?
            }
            EntryKind::Hardlink | EntryKind::Special => unreachable!(),
        };
        reader = cpio::NewcReader::new(inner).map_err(|error| archive_error(None, error))?;
    }

    for link in links {
        apply_link(destination, &link)?;
        reporter.link();
    }
    for (path, mode, mtime) in metadata {
        apply_metadata(&path.to_path(destination), mode, mtime)?;
    }
    Ok(())
}

fn extract_ar(request: &Request, destination: &Path, reporter: &ExtractionReporter) -> Result<()> {
    let file = File::open(&request.input).map_err(|source| XxError::InputIo {
        path: request.input.clone(),
        source,
    })?;
    let mut archive = ar::Archive::new(file);
    let mut seen: HashMap<RelPath, EntryKind> = HashMap::new();
    while let Some(entry) = archive.next_entry() {
        let mut entry = entry.map_err(|error| archive_error(None, error))?;
        let header = entry.header();
        let raw_name = header.identifier().to_vec();
        let mode = header.mode();
        let mtime = header.mtime();
        let size = header.size();
        let path = normalize_path(&raw_name)?;
        validate_entry_conflict(&mut seen, &path, EntryKind::File, &raw_name)?;
        let target = path.to_path(destination);
        ensure_parent(destination, &path)?;
        remove_leaf_if_present(&target)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|source| output_error_at(&target, source))?;
        io::copy(&mut entry, &mut output).map_err(|source| output_error_at(&target, source))?;
        output
            .flush()
            .map_err(|source| output_error_at(&target, source))?;
        apply_metadata(&target, Some(mode), Some(mtime))?;
        reporter.file(&path, size)?;
    }
    Ok(())
}

fn extract_zip(request: &Request, destination: &Path, reporter: &ExtractionReporter) -> Result<()> {
    let file = File::open(&request.input).map_err(|source| XxError::InputIo {
        path: request.input.clone(),
        source,
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| archive_error(None, error))?;
    let mut plan = Vec::with_capacity(archive.len());
    let mut seen: HashMap<RelPath, EntryKind> = HashMap::new();
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| archive_error(None, error))?;
        if file.encrypted() {
            return Err(XxError::Archive {
                entry: file.name().into(),
                message: "encrypted ZIP entries are not supported".into(),
            });
        }
        let raw_name = file.name_raw().to_vec();
        let path = normalize_path(&raw_name)?;
        let kind = zip_kind(&file);
        if kind == EntryKind::Special {
            return Err(XxError::Archive {
                entry: display_bytes(&raw_name),
                message: "special files are not supported".into(),
            });
        }
        validate_entry_conflict(&mut seen, &path, kind, &raw_name)?;
        let target = if kind == EntryKind::Symlink {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|error| archive_error(Some(&raw_name), error))?;
            validate_link_target(&path, &bytes)?;
            Some(bytes)
        } else {
            None
        };
        let record = EntryRecord {
            name: raw_name,
            kind,
            size: if kind == EntryKind::File {
                Some(file.size())
            } else {
                None
            },
            perm: file.unix_mode().map(mode_string),
            mtime: file.last_modified().and_then(format_zip_time),
            link_target: target,
        };
        let mode = file.unix_mode();
        let mtime = file.last_modified().and_then(zip_time_seconds);
        plan.push(ZipPlanEntry {
            index,
            path,
            record,
            mode,
            mtime,
        });
    }

    let mut winners = HashSet::new();
    let mut selected = Vec::new();
    for entry in plan.into_iter().rev() {
        if winners.insert(entry.path.clone()) {
            selected.push(entry);
        }
    }
    selected.reverse();
    let mut files = Vec::new();
    let mut links = Vec::new();
    let mut metadata = Vec::new();
    for entry in selected {
        match entry.record.kind {
            EntryKind::Directory => {
                ensure_directory(destination, &entry.path)?;
                reporter.directory(&entry.path)?;
                metadata.push((entry.path, entry.mode, entry.mtime));
            }
            EntryKind::File => files.push(entry),
            EntryKind::Symlink => links.push(entry),
            EntryKind::Hardlink => unreachable!("ZIP classification cannot produce hard links"),
            EntryKind::Special => unreachable!(),
        }
    }

    if !files.is_empty() {
        let count = request.threads.get().min(files.len());
        let next = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let files = Arc::new(files);
        let errors: Arc<Mutex<Option<XxError>>> = Arc::new(Mutex::new(None));
        let mut handles = Vec::new();
        for _ in 0..count {
            let next = Arc::clone(&next);
            let files = Arc::clone(&files);
            let errors = Arc::clone(&errors);
            let input = request.input.clone();
            let destination = destination.to_path_buf();
            let reporter = reporter.clone();
            handles.push(thread::spawn(move || {
                let file = match File::open(&input) {
                    Ok(file) => file,
                    Err(source) => {
                        *errors.lock().expect("error mutex poisoned") = Some(XxError::InputIo {
                            path: input,
                            source,
                        });
                        return;
                    }
                };
                let mut archive = match ZipArchive::new(file) {
                    Ok(archive) => archive,
                    Err(error) => {
                        *errors.lock().expect("error mutex poisoned") =
                            Some(archive_error(None, error));
                        return;
                    }
                };
                loop {
                    if errors.lock().expect("error mutex poisoned").is_some() {
                        return;
                    }
                    let position = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if position >= files.len() {
                        return;
                    }
                    let plan = &files[position];
                    let mut source = match archive.by_index(plan.index) {
                        Ok(source) => source,
                        Err(error) => {
                            *errors.lock().expect("error mutex poisoned") =
                                Some(archive_error(Some(&plan.record.name), error));
                            return;
                        }
                    };
                    let target = plan.path.to_path(&destination);
                    if let Err(error) = ensure_parent(&destination, &plan.path)
                        .and_then(|_| write_zip_file(&mut source, &target))
                        .and_then(|_| apply_metadata(&target, plan.mode, plan.mtime))
                        .and_then(|_| reporter.file(&plan.path, plan.record.size.unwrap_or(0)))
                    {
                        *errors.lock().expect("error mutex poisoned") = Some(error);
                        return;
                    }
                }
            }));
        }
        for handle in handles {
            if handle.join().is_err() {
                let mut worker_error = errors.lock().expect("error mutex poisoned");
                if worker_error.is_none() {
                    *worker_error = Some(XxError::Archive {
                        entry: String::new(),
                        message: "ZIP worker thread panicked".into(),
                    });
                }
            }
        }
        if let Some(error) = errors.lock().expect("error mutex poisoned").take() {
            return Err(error);
        }
    }

    for entry in links {
        let target = entry.record.link_target.ok_or_else(|| XxError::Archive {
            entry: display_bytes(&entry.record.name),
            message: "link target missing from ZIP plan".into(),
        })?;
        let pending = PendingLink {
            path: entry.path,
            kind: entry.record.kind,
            target,
        };
        apply_link(destination, &pending)?;
        reporter.link();
    }
    for (path, mode, mtime) in metadata {
        apply_metadata(&path.to_path(destination), mode, mtime)?;
    }
    Ok(())
}

fn write_zip_file<R: Read>(source: &mut R, target: &Path) -> Result<()> {
    remove_leaf_if_present(target)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|source| output_error_at(target, source))?;
    io::copy(source, &mut output).map_err(|source| output_error_at(target, source))?;
    output
        .flush()
        .map_err(|source| output_error_at(target, source))?;
    Ok(())
}
