use crate::error::{Result, XxError, output_error_at};
use crate::model::OutputMode;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use tempfile::{Builder as TempBuilder, TempDir};

use crate::safety::apply_metadata;

#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};

pub(crate) struct Staging {
    temp: TempDir,
}

impl Staging {
    pub(crate) fn create(parent: &Path) -> Result<Self> {
        let temp = TempBuilder::new()
            .prefix(".xx-")
            .tempdir_in(parent)
            .map_err(|source| XxError::OutputIo {
                path: parent.to_path_buf(),
                source,
            })?;
        Ok(Self { temp })
    }

    pub(crate) fn path(&self) -> &Path {
        self.temp.path()
    }
}
pub(crate) fn select_logical_root(staging: &Path) -> Result<PathBuf> {
    let mut children = Vec::new();
    let entries = fs::read_dir(staging).map_err(|source| output_error_at(staging, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| output_error_at(staging, source))?;
        children.push(entry.path());
    }
    if children.len() == 1 {
        let metadata = fs::symlink_metadata(&children[0])
            .map_err(|source| output_error_at(&children[0], source))?;
        if metadata.is_dir() {
            return Ok(children.remove(0));
        }
    }
    Ok(staging.to_path_buf())
}

pub(crate) fn commit(
    staging: Staging,
    logical_root: PathBuf,
    output: &Path,
    mode: OutputMode,
) -> Result<()> {
    let exists = fs::symlink_metadata(output).is_ok();
    if exists {
        let metadata =
            fs::symlink_metadata(output).map_err(|source| output_error_at(output, source))?;
        if metadata.file_type().is_symlink() {
            return Err(XxError::OutputConflict(
                output.to_path_buf(),
                "output is a symbolic link".into(),
            ));
        }
        if !metadata.is_dir() {
            return Err(XxError::OutputConflict(
                output.to_path_buf(),
                "output is not a directory".into(),
            ));
        }
    }

    match mode {
        OutputMode::CreateOnly => {
            if exists {
                return Err(XxError::OutputExists(output.to_path_buf()));
            }
            fs::rename(&logical_root, output).map_err(|source| output_error_at(output, source))?;
        }
        OutputMode::Replace => {
            if exists {
                fs::remove_dir_all(output).map_err(|source| output_error_at(output, source))?;
            }
            fs::rename(&logical_root, output).map_err(|source| output_error_at(output, source))?;
        }
        OutputMode::Merge => {
            if !exists {
                fs::rename(&logical_root, output)
                    .map_err(|source| output_error_at(output, source))?;
            } else {
                copy_tree(&logical_root, output)?;
            }
        }
    }
    drop(staging);
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let entries =
        fs::read_dir(source).map_err(|source_error| output_error_at(source, source_error))?;
    for entry in entries {
        let entry = entry.map_err(|source_error| output_error_at(source, source_error))?;
        copy_entry(&entry.path(), &destination.join(entry.file_name()))?;
    }
    Ok(())
}

fn copy_entry(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|source_error| output_error_at(source, source_error))?;
    ensure_destination_parent(destination)?;
    if metadata.file_type().is_symlink() {
        let target =
            fs::read_link(source).map_err(|source_error| output_error_at(source, source_error))?;
        remove_destination(destination)?;
        #[cfg(unix)]
        symlink(&target, destination)
            .map_err(|source_error| output_error_at(destination, source_error))?;
        #[cfg(not(unix))]
        return Err(XxError::OutputConflict(
            destination.to_path_buf(),
            "symbolic links are unsupported on this platform".into(),
        ));
        return Ok(());
    }
    if metadata.is_dir() {
        if let Ok(existing) = fs::symlink_metadata(destination)
            && (existing.file_type().is_symlink() || !existing.is_dir())
        {
            remove_destination(destination)?;
        }
        if !destination.exists() {
            fs::create_dir(destination)
                .map_err(|source_error| output_error_at(destination, source_error))?;
        }
        ensure_not_symlink(destination)?;
        copy_tree(source, destination)?;
        apply_metadata(
            destination,
            metadata_mode(&metadata),
            metadata_mtime(&metadata),
        )
    } else {
        remove_destination(destination)?;
        fs::copy(source, destination)
            .map_err(|source_error| output_error_at(destination, source_error))?;
        apply_metadata(
            destination,
            metadata_mode(&metadata),
            metadata_mtime(&metadata),
        )
    }
}

fn metadata_mode(metadata: &fs::Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        Some(metadata.permissions().mode())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

fn metadata_mtime(metadata: &fs::Metadata) -> Option<u64> {
    u64::try_from(filetime::FileTime::from_last_modification_time(metadata).unix_seconds()).ok()
}

fn remove_destination(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path).map_err(|source| output_error_at(path, source))?;
        }
        Ok(_) => fs::remove_file(path).map_err(|source| output_error_at(path, source))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(output_error_at(path, source)),
    }
    Ok(())
}

fn ensure_destination_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        let mut current = PathBuf::new();
        for component in parent.components() {
            match component {
                Component::Prefix(prefix) => current.push(prefix.as_os_str()),
                Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err(XxError::OutputConflict(
                        path.to_path_buf(),
                        "destination contains ..".into(),
                    ));
                }
                Component::Normal(name) => {
                    current.push(name);
                    match fs::symlink_metadata(&current) {
                        Ok(metadata) if metadata.file_type().is_symlink() => {
                            return Err(XxError::OutputConflict(
                                current,
                                "destination traversal follows a symbolic link".into(),
                            ));
                        }
                        Ok(metadata) if !metadata.is_dir() => {
                            return Err(XxError::OutputConflict(
                                current,
                                "destination parent is not a directory".into(),
                            ));
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {
                            fs::create_dir(&current)
                                .map_err(|source| output_error_at(&current, source))?
                        }
                        Err(source) => return Err(output_error_at(&current, source)),
                    }
                }
            }
        }
    }
    Ok(())
}

fn ensure_not_symlink(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|source| output_error_at(path, source))?;
    if metadata.file_type().is_symlink() {
        return Err(XxError::OutputConflict(
            path.to_path_buf(),
            "path is a symbolic link".into(),
        ));
    }
    Ok(())
}
