use crate::error::display_bytes;
use crate::error::{Result, XxError, output_error_at};
use crate::model::{EntryKind, PendingLink};
use crate::path::{RelPath, link_target_path, os_from_bytes};
use std::fs;
use std::io;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};

pub(crate) fn ensure_parent(root: &Path, path: &RelPath) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in path
        .components
        .iter()
        .take(path.components.len().saturating_sub(1))
    {
        current.push(os_from_bytes(component));
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(XxError::UnsafePath {
                    path: path.key(),
                    reason: "parent is a symbolic link".into(),
                });
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(XxError::Archive {
                    entry: display_bytes(&path.key()),
                    message: "a non-directory entry is an ancestor".into(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        let metadata = fs::symlink_metadata(&current)
                            .map_err(|source| output_error_at(&current, source))?;
                        if metadata.file_type().is_symlink() {
                            return Err(XxError::UnsafePath {
                                path: path.key(),
                                reason: "parent is a symbolic link".into(),
                            });
                        }
                        if !metadata.is_dir() {
                            return Err(XxError::Archive {
                                entry: display_bytes(&path.key()),
                                message: "a non-directory entry is an ancestor".into(),
                            });
                        }
                    }
                    Err(source) => return Err(output_error_at(&current, source)),
                }
            }
            Err(source) => return Err(output_error_at(&current, source)),
        }
    }
    Ok(())
}

pub(crate) fn ensure_directory(root: &Path, path: &RelPath) -> Result<()> {
    ensure_parent(root, path)?;
    let target = path.to_path(root);
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(XxError::UnsafePath {
            path: path.key(),
            reason: "directory path is a symbolic link".into(),
        }),
        Ok(metadata) if !metadata.is_dir() => Err(XxError::Archive {
            entry: display_bytes(&path.key()),
            message: "entry conflicts with an existing file".into(),
        }),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&target).map_err(|source| output_error_at(&target, source))
        }
        Err(source) => Err(output_error_at(&target, source)),
    }
}

pub(crate) fn remove_leaf_if_present(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            Err(XxError::Archive {
                entry: path.display().to_string(),
                message: "entry conflicts with an existing directory".into(),
            })
        }
        Ok(_) => fs::remove_file(path).map_err(|source| output_error_at(path, source)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(output_error_at(path, source)),
    }
}

pub(crate) fn apply_link(root: &Path, link: &PendingLink) -> Result<()> {
    ensure_parent(root, &link.path)?;
    let destination = link.path.to_path(root);
    remove_leaf_if_present(&destination)?;
    match link.kind {
        EntryKind::Symlink => {
            #[cfg(unix)]
            symlink(os_from_bytes(&link.target), &destination)
                .map_err(|source| output_error_at(&destination, source))?;
            #[cfg(not(unix))]
            return Err(XxError::Archive {
                entry: display_bytes(&link.path.key()),
                message: "symbolic links are unsupported on this platform".into(),
            });
        }
        EntryKind::Hardlink => {
            let target = link_target_path(&link.path, &link.target)?;
            let target_path = target.to_path(root);
            fs::hard_link(&target_path, &destination)
                .map_err(|source| output_error_at(&destination, source))?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

pub(crate) fn apply_metadata(path: &Path, mode: Option<u32>, mtime: Option<u64>) -> Result<()> {
    if let Some(mode) = mode {
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o7777 & !0o6000))
            .map_err(|source| output_error_at(path, source))?;
    }
    if let Some(mtime) = mtime {
        let file_time = filetime::FileTime::from_unix_time(mtime as i64, 0);
        filetime::set_file_mtime(path, file_time)
            .map_err(|source| output_error_at(path, source))?;
    }
    Ok(())
}

pub(crate) fn mode_string(mode: u32) -> String {
    format!("{:04o}", mode & 0o7777)
}
