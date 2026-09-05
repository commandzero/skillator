//! Physical Materialization inspection and staging.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TreeEntry {
    Directory,
    File { bytes: Vec<u8>, executable: bool },
    Symlink { target: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeSnapshot {
    pub entries: BTreeMap<PathBuf, TreeEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EntryFingerprint {
    Missing,
    Symlink(PathBuf),
    File(Vec<u8>),
    Directory(TreeSnapshot),
    Other,
    Uninspectable,
}

pub(crate) fn fingerprint(path: &Path) -> EntryFingerprint {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::read_link(path)
            .map(EntryFingerprint::Symlink)
            .unwrap_or(EntryFingerprint::Uninspectable),
        Ok(metadata) if metadata.is_file() => fs::read(path)
            .map(EntryFingerprint::File)
            .unwrap_or(EntryFingerprint::Uninspectable),
        Ok(metadata) if metadata.is_dir() => TreeSnapshot::read(path, false)
            .map(EntryFingerprint::Directory)
            .unwrap_or(EntryFingerprint::Uninspectable),
        Ok(_) => EntryFingerprint::Other,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => EntryFingerprint::Missing,
        Err(_) => EntryFingerprint::Uninspectable,
    }
}

pub(crate) fn skill_fingerprint(path: &Path) -> EntryFingerprint {
    match TreeSnapshot::read(path, true) {
        Ok(tree) => EntryFingerprint::Directory(tree),
        Err(_) => EntryFingerprint::Uninspectable,
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CopyError {
    #[error("cannot inspect `{path}`: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot copy link `{path}`: use a relative path within the skill folder")]
    AbsoluteSymlink { path: PathBuf },
    #[error("cannot copy link `{path}`: it points outside the skill folder")]
    EscapingSymlink { path: PathBuf },
    #[error("cannot copy link `{path}`: its destination is missing, unreadable, or forms a loop")]
    UnresolvedSymlink { path: PathBuf },
    #[error("cannot copy `{path}`: only regular files, folders, and links are supported")]
    Unsupported { path: PathBuf },
}

impl TreeSnapshot {
    pub(crate) fn read(root: &Path, validate_internal_links: bool) -> Result<Self, CopyError> {
        let canonical_root = root.canonicalize().map_err(|source| CopyError::Io {
            path: root.to_owned(),
            source,
        })?;
        let mut entries = BTreeMap::new();
        walk(
            root,
            root,
            &canonical_root,
            validate_internal_links,
            &mut entries,
        )?;
        Ok(Self { entries })
    }
}

fn walk(
    root: &Path,
    directory: &Path,
    canonical_root: &Path,
    validate_internal_links: bool,
    entries: &mut BTreeMap<PathBuf, TreeEntry>,
) -> Result<(), CopyError> {
    let iterator = fs::read_dir(directory).map_err(|source| CopyError::Io {
        path: directory.to_owned(),
        source,
    })?;
    let mut children: Vec<_> =
        iterator
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| CopyError::Io {
                path: directory.to_owned(),
                source,
            })?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        if child.file_name() == OsStr::new(".git") {
            continue;
        }
        let path = child.path();
        let relative = path.strip_prefix(root).unwrap_or(&path).to_owned();
        let metadata = fs::symlink_metadata(&path).map_err(|source| CopyError::Io {
            path: path.clone(),
            source,
        })?;
        let kind = metadata.file_type();
        if kind.is_symlink() {
            let target = fs::read_link(&path).map_err(|source| CopyError::Io {
                path: path.clone(),
                source,
            })?;
            if validate_internal_links {
                validate_internal_symlink(root, canonical_root, &path, &target)?;
            }
            entries.insert(relative, TreeEntry::Symlink { target });
        } else if kind.is_dir() {
            entries.insert(relative, TreeEntry::Directory);
            walk(
                root,
                &path,
                canonical_root,
                validate_internal_links,
                entries,
            )?;
        } else if kind.is_file() {
            let bytes = fs::read(&path).map_err(|source| CopyError::Io {
                path: path.clone(),
                source,
            })?;
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            entries.insert(relative, TreeEntry::File { bytes, executable });
        } else {
            return Err(CopyError::Unsupported { path });
        }
    }
    Ok(())
}

fn validate_internal_symlink(
    root: &Path,
    canonical_root: &Path,
    link: &Path,
    target: &Path,
) -> Result<(), CopyError> {
    if target.is_absolute() {
        return Err(CopyError::AbsoluteSymlink {
            path: link.to_owned(),
        });
    }
    let parent = link.parent().unwrap_or(root);
    let base = parent.strip_prefix(root).unwrap_or(Path::new("."));
    let Some(relative) = normalize_inside(base, target) else {
        return Err(CopyError::EscapingSymlink {
            path: link.to_owned(),
        });
    };
    let candidate = root.join(relative);
    validate_resolution_steps(root, canonical_root, &candidate, link)?;
    let resolved = candidate
        .canonicalize()
        .map_err(|_| CopyError::UnresolvedSymlink {
            path: link.to_owned(),
        })?;
    if !resolved.starts_with(canonical_root) {
        return Err(CopyError::EscapingSymlink {
            path: link.to_owned(),
        });
    }
    Ok(())
}

fn normalize_inside(base: &Path, target: &Path) -> Option<PathBuf> {
    let mut components = Vec::new();
    for component in base.components().chain(target.components()) {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => components.push(value.to_owned()),
            Component::ParentDir => {
                components.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(components.into_iter().collect())
}

fn validate_resolution_steps(
    root: &Path,
    canonical_root: &Path,
    candidate: &Path,
    link: &Path,
) -> Result<(), CopyError> {
    let relative = candidate
        .strip_prefix(root)
        .map_err(|_| CopyError::EscapingSymlink {
            path: link.to_owned(),
        })?;
    let mut current = root.to_owned();
    for component in relative.components() {
        current.push(component.as_os_str());
        let metadata =
            fs::symlink_metadata(&current).map_err(|_| CopyError::UnresolvedSymlink {
                path: link.to_owned(),
            })?;
        if metadata.file_type().is_symlink() {
            let resolved = current
                .canonicalize()
                .map_err(|_| CopyError::UnresolvedSymlink {
                    path: link.to_owned(),
                })?;
            if !resolved.starts_with(canonical_root) {
                return Err(CopyError::EscapingSymlink {
                    path: link.to_owned(),
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn copy_tree(source: &Path, destination: &Path) -> Result<(), CopyError> {
    let snapshot = TreeSnapshot::read(source, true)?;
    fs::create_dir(destination).map_err(|source| CopyError::Io {
        path: destination.to_owned(),
        source,
    })?;
    for (relative, entry) in snapshot.entries {
        let path = destination.join(relative);
        match entry {
            TreeEntry::Directory => fs::create_dir(&path).map_err(|source| CopyError::Io {
                path: path.clone(),
                source,
            })?,
            TreeEntry::File { bytes, executable } => {
                fs::write(&path, bytes).map_err(|source| CopyError::Io {
                    path: path.clone(),
                    source,
                })?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = if executable { 0o755 } else { 0o644 };
                    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).map_err(
                        |source| CopyError::Io {
                            path: path.clone(),
                            source,
                        },
                    )?;
                }
            }
            TreeEntry::Symlink { target } => {
                #[cfg(unix)]
                std::os::unix::fs::symlink(target, &path).map_err(|source| CopyError::Io {
                    path: path.clone(),
                    source,
                })?;
                #[cfg(not(unix))]
                return Err(CopyError::Unsupported { path });
            }
        }
    }
    Ok(())
}
