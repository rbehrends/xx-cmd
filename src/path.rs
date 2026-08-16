use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::{Result, XxError, display_bytes};
use crate::model::EntryKind;

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct RelPath {
    pub(crate) components: Vec<Vec<u8>>,
}

impl RelPath {
    pub(crate) fn parent(&self) -> Self {
        let mut components = self.components.clone();
        components.pop();
        Self { components }
    }

    pub(crate) fn to_path(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        for component in &self.components {
            path.push(os_from_bytes(component));
        }
        path
    }

    pub(crate) fn key(&self) -> Vec<u8> {
        let mut result = Vec::new();
        for (index, component) in self.components.iter().enumerate() {
            if index > 0 {
                result.push(b'/');
            }
            result.extend_from_slice(component);
        }
        result
    }

    pub(crate) fn ancestors(&self) -> impl Iterator<Item = RelPath> + '_ {
        (1..self.components.len()).map(|len| RelPath {
            components: self.components[..len].to_vec(),
        })
    }
}

pub(crate) fn normalize_path(raw: &[u8]) -> Result<RelPath> {
    if raw.is_empty() || raw.contains(&0) {
        return Err(XxError::UnsafePath {
            path: raw.to_vec(),
            reason: "empty or NUL-containing path".into(),
        });
    }
    let components = split_components(raw);
    if raw[0] == b'/'
        || raw[0] == b'\\'
        || components
            .first()
            .is_some_and(|component| component.get(1) == Some(&b':'))
    {
        return Err(XxError::UnsafePath {
            path: raw.to_vec(),
            reason: "absolute or drive-prefixed path".into(),
        });
    }
    let mut clean = Vec::new();
    for component in components {
        if component.is_empty() || component == b"." {
            continue;
        }
        if component == b".." {
            return Err(XxError::UnsafePath {
                path: raw.to_vec(),
                reason: "parent traversal".into(),
            });
        }
        clean.push(component);
    }
    if clean.is_empty() {
        return Err(XxError::UnsafePath {
            path: raw.to_vec(),
            reason: "empty path".into(),
        });
    }
    Ok(RelPath { components: clean })
}

pub(crate) fn split_components(raw: &[u8]) -> Vec<Vec<u8>> {
    // Archive paths use the same separator policy on every host so their
    // logical directory structure is portable across platforms.
    raw.split(|byte| *byte == b'/' || *byte == b'\\')
        .map(|part| part.to_vec())
        .collect()
}

pub(crate) fn link_target_path(path: &RelPath, target: &[u8]) -> Result<RelPath> {
    let mut components = path.parent().components;
    for component in split_components(target) {
        if component.is_empty() || component == b"." {
            continue;
        }
        if component == b".." {
            if components.pop().is_none() {
                return Err(XxError::UnsafePath {
                    path: target.to_vec(),
                    reason: "link target escapes extraction root".into(),
                });
            }
        } else {
            components.push(component);
        }
    }
    Ok(RelPath { components })
}

pub(crate) fn validate_link_target(path: &RelPath, target: &[u8]) -> Result<()> {
    if target.is_empty() || target[0] == b'/' || target[0] == b'\\' || target.contains(&0) {
        return Err(XxError::UnsafePath {
            path: target.to_vec(),
            reason: "link target is absolute or invalid".into(),
        });
    }
    let resolved = link_target_path(path, target)?;
    if resolved.components.is_empty() {
        return Err(XxError::UnsafePath {
            path: target.to_vec(),
            reason: "link target resolves to extraction root".into(),
        });
    }
    Ok(())
}

pub(crate) fn validate_entry_conflict(
    seen: &mut HashMap<RelPath, EntryKind>,
    path: &RelPath,
    kind: EntryKind,
    raw: &[u8],
) -> Result<()> {
    if path.components.is_empty() {
        return Err(XxError::UnsafePath {
            path: raw.to_vec(),
            reason: "empty path".into(),
        });
    }
    if let Some(previous) = seen.get(path)
        && *previous != kind
    {
        return Err(XxError::Archive {
            entry: display_bytes(raw),
            message: "duplicate path changes entry type".into(),
        });
    }
    for ancestor in path.ancestors() {
        if let Some(previous) = seen.get(&ancestor)
            && *previous != EntryKind::Directory
        {
            return Err(XxError::Archive {
                entry: display_bytes(raw),
                message: "a non-directory entry is an ancestor".into(),
            });
        }
    }
    seen.insert(path.clone(), kind);
    Ok(())
}

pub(crate) fn os_from_bytes(bytes: &[u8]) -> OsString {
    #[cfg(unix)]
    {
        OsString::from_vec(bytes.to_vec())
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8_lossy(bytes).into_owned())
    }
}
