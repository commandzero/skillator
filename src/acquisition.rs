//! Transactional acquisition of Skills into the writable local library.

use crate::fs_safety::rename_noreplace;
use crate::library::validated_skill_name_at;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryAcquisitionMode {
    Move,
    Copy,
    Link,
}

impl LibraryAcquisitionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Copy => "copy",
            Self::Link => "link",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryAcquisition {
    source: PathBuf,
    name: String,
    mode: LibraryAcquisitionMode,
    source_root_git_skill: bool,
}

impl LibraryAcquisition {
    pub fn new(
        source: PathBuf,
        name: String,
        mode: LibraryAcquisitionMode,
        source_root_git_skill: bool,
    ) -> Self {
        Self {
            source,
            name,
            mode,
            source_root_git_skill,
        }
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn mode(&self) -> LibraryAcquisitionMode {
        self.mode
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AcquisitionError {
    #[error("{0}")]
    Invalid(String),
    #[error("Could not add skills to the library: {0}")]
    Failed(String),
    #[error("Adding skills failed and could not be undone; restore these files manually: {0}")]
    RecoveryRequired(String),
}

#[derive(Debug)]
struct PreparedItem {
    request: LibraryAcquisition,
    destination: PathBuf,
    stage: PathBuf,
    backup: Option<PathBuf>,
    published: bool,
}

#[derive(Debug)]
pub(crate) struct PreparedAcquisitions {
    items: Vec<PreparedItem>,
    finished: bool,
}

impl PreparedAcquisitions {
    pub(crate) fn prepare(
        local_root: &Path,
        requests: &[LibraryAcquisition],
    ) -> Result<Self, AcquisitionError> {
        let metadata = fs::symlink_metadata(local_root).map_err(failed)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(AcquisitionError::Invalid(format!(
                "local library must be a directory, not a link: {}",
                local_root.display()
            )));
        }
        let local_root = local_root.canonicalize().map_err(failed)?;
        let mut names = BTreeSet::new();
        let mut items = Vec::new();
        for request in requests {
            if !names.insert(request.name.clone()) {
                cleanup_items(&items)?;
                return Err(AcquisitionError::Invalid(format!(
                    "More than one skill would use the library name `{}`",
                    request.name
                )));
            }
            if request.source_root_git_skill {
                cleanup_items(&items)?;
                return Err(AcquisitionError::Invalid(format!(
                    "Skill `{}` is at a Git repository root and cannot be imported as a single folder",
                    request.name
                )));
            }
            let source = request.source.canonicalize().map_err(failed)?;
            if source.starts_with(&local_root) {
                cleanup_items(&items)?;
                return Err(AcquisitionError::Invalid(format!(
                    "Skill is already inside the local library: {}",
                    source.display()
                )));
            }
            if validated_skill_name_at(&source).as_deref() != Some(request.name.as_str()) {
                cleanup_items(&items)?;
                return Err(AcquisitionError::Invalid(format!(
                    "Skill is unavailable or changed: {}",
                    source.display()
                )));
            }
            let destination = local_root.join(&request.name);
            if fs::symlink_metadata(&destination).is_ok() {
                cleanup_items(&items)?;
                return Err(AcquisitionError::Invalid(format!(
                    "local library destination already exists: {}",
                    destination.display()
                )));
            }
            let stage = artifact_path(&local_root, "stage", &request.name);
            let result = match request.mode {
                LibraryAcquisitionMode::Move | LibraryAcquisitionMode::Copy => {
                    copy_tree_all(&source, &stage)
                        .and_then(|()| trees_equal(&source, &stage))
                        .and_then(|equal| {
                            if equal {
                                Ok(())
                            } else {
                                Err(io::Error::other(
                                    "The temporary copy does not match the source",
                                ))
                            }
                        })
                }
                LibraryAcquisitionMode::Link => {
                    #[cfg(unix)]
                    {
                        std::os::unix::fs::symlink(&source, &stage)
                    }
                    #[cfg(not(unix))]
                    {
                        Err(io::Error::new(
                            io::ErrorKind::Unsupported,
                            "symbolic links require Unix",
                        ))
                    }
                }
            };
            if let Err(error) = result {
                let _ = remove_any(&stage);
                cleanup_items(&items)?;
                return Err(failed(error));
            }
            let mut prepared_request = request.clone();
            prepared_request.source = source;
            items.push(PreparedItem {
                request: prepared_request,
                destination,
                stage,
                backup: None,
                published: false,
            });
        }
        Ok(Self {
            items,
            finished: false,
        })
    }

    pub(crate) fn publish(&mut self) -> Result<(), AcquisitionError> {
        for index in 0..self.items.len() {
            let result = publish_item(&mut self.items[index]);
            if let Err(error) = result {
                return match self.rollback() {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(rollback),
                };
            }
        }
        Ok(())
    }

    pub(crate) fn rollback(&mut self) -> Result<(), AcquisitionError> {
        let mut failures = Vec::new();
        for item in self.items.iter_mut().rev() {
            if fs::symlink_metadata(&item.destination).is_ok()
                && let Err(error) = remove_any(&item.destination)
            {
                failures.push(format!("remove {}: {error}", item.destination.display()));
            }
            if let Some(backup) = &item.backup
                && fs::symlink_metadata(backup).is_ok()
                && let Err(error) = rename_noreplace(backup, &item.request.source)
            {
                failures.push(format!(
                    "restore {} from {}: {error}",
                    item.request.source.display(),
                    backup.display()
                ));
            }
            if fs::symlink_metadata(&item.stage).is_ok()
                && let Err(error) = remove_any(&item.stage)
            {
                failures.push(format!("remove {}: {error}", item.stage.display()));
            }
            item.published = false;
        }
        self.finished = true;
        if failures.is_empty() {
            Ok(())
        } else {
            Err(AcquisitionError::RecoveryRequired(failures.join("; ")))
        }
    }

    pub(crate) fn finish(mut self) -> Result<(), AcquisitionError> {
        self.finished = true;
        let mut failures = Vec::new();
        for item in &self.items {
            if let Some(backup) = &item.backup
                && fs::symlink_metadata(backup).is_ok()
                && let Err(error) = remove_any(backup)
            {
                failures.push(format!("remove backup {}: {error}", backup.display()));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(AcquisitionError::RecoveryRequired(failures.join("; ")))
        }
    }
}

impl Drop for PreparedAcquisitions {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.rollback();
        }
    }
}

fn publish_item(item: &mut PreparedItem) -> Result<(), AcquisitionError> {
    if validated_skill_name_at(&item.request.source).as_deref() != Some(item.request.name.as_str())
    {
        return Err(AcquisitionError::Invalid(format!(
            "The skill changed before it could be added; refresh and retry: {}",
            item.request.source.display()
        )));
    }
    if fs::symlink_metadata(&item.destination).is_ok() {
        return Err(AcquisitionError::Invalid(format!(
            "The destination was created by another process: {}",
            item.destination.display()
        )));
    }
    if item.request.mode == LibraryAcquisitionMode::Move {
        let parent = item.request.source.parent().ok_or_else(|| {
            AcquisitionError::Invalid("The source skill has no parent folder".to_owned())
        })?;
        let backup = artifact_path(parent, "backup", &item.request.name);
        rename_noreplace(&item.request.source, &backup).map_err(failed)?;
        item.backup = Some(backup.clone());
        if !trees_equal(&backup, &item.stage).map_err(failed)? {
            return Err(AcquisitionError::Failed(format!(
                "The skill changed while it was being added; refresh and retry: {}",
                item.request.source.display()
            )));
        }
    }
    rename_noreplace(&item.stage, &item.destination).map_err(failed)?;
    item.published = true;
    Ok(())
}

fn cleanup_items(items: &[PreparedItem]) -> Result<(), AcquisitionError> {
    let failures = items
        .iter()
        .filter(|item| fs::symlink_metadata(&item.stage).is_ok())
        .filter_map(|item| {
            remove_any(&item.stage)
                .err()
                .map(|error| format!("remove {}: {error}", item.stage.display()))
        })
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(AcquisitionError::RecoveryRequired(failures.join("; ")))
    }
}

fn artifact_path(parent: &Path, kind: &str, name: &str) -> PathBuf {
    let sequence = ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".skillator-acquisition-{kind}-{}-{sequence}-{name}",
        std::process::id()
    ))
}

fn copy_tree_all(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree_all(&source_path, &destination_path)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(&source_path)?, &destination_path)?;
        } else if kind.is_file() {
            fs::copy(&source_path, &destination_path)?;
            fs::set_permissions(&destination_path, fs::metadata(&source_path)?.permissions())?;
        } else {
            return Err(io::Error::other(format!(
                "unsupported entry: {}",
                source_path.display()
            )));
        }
    }
    fs::set_permissions(destination, fs::metadata(source)?.permissions())?;
    Ok(())
}

fn trees_equal(left: &Path, right: &Path) -> io::Result<bool> {
    let mut left_entries = fs::read_dir(left)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut right_entries = fs::read_dir(right)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    left_entries.sort();
    right_entries.sort();
    if left_entries != right_entries {
        return Ok(false);
    }
    for name in left_entries {
        let left_path = left.join(&name);
        let right_path = right.join(name);
        let left_meta = fs::symlink_metadata(&left_path)?;
        let right_meta = fs::symlink_metadata(&right_path)?;
        if left_meta.file_type().is_symlink() != right_meta.file_type().is_symlink()
            || left_meta.is_dir() != right_meta.is_dir()
            || left_meta.is_file() != right_meta.is_file()
        {
            return Ok(false);
        }
        #[cfg(unix)]
        if !left_meta.file_type().is_symlink() {
            use std::os::unix::fs::PermissionsExt;
            if left_meta.permissions().mode() & 0o7777 != right_meta.permissions().mode() & 0o7777 {
                return Ok(false);
            }
        }
        if left_meta.file_type().is_symlink() {
            if fs::read_link(left_path)? != fs::read_link(right_path)? {
                return Ok(false);
            }
        } else if left_meta.is_dir() {
            if !trees_equal(&left_path, &right_path)? {
                return Ok(false);
            }
        } else if fs::read(left_path)? != fs::read(right_path)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn remove_any(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn failed(error: impl std::fmt::Display) -> AcquisitionError {
    AcquisitionError::Failed(error.to_string())
}
