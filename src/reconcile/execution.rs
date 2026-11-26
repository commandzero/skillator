//! Guarded staging, publication, and rollback of planned operations.
use super::*;

pub(super) fn apply_operation_with(
    item: &PlanItem,
    faults: &dyn FaultInjector,
    target_root: &Path,
) -> Result<(), ApplyFailure> {
    if !matches!(item.operation, Operation::None) {
        ensure_contained_physical_parent(&item.path, target_root)?;
    }
    match &item.operation {
        Operation::CreateDirectory => create_directory(item),
        Operation::WriteFile { bytes, expected } => write_file(item, faults, bytes, expected),
        Operation::Materialize {
            source,
            metadata_name,
            source_expected,
            kind,
            expected,
        } => MaterializationStage {
            source,
            metadata_name,
            source_expected,
            kind,
            expected,
        }
        .publish(item, faults),
        Operation::Remove { expected } => remove_materialization(item, expected),
        Operation::Recover {
            artifact,
            artifact_expected,
            container,
            destination,
            destination_expected,
        } => RecoveryOperation {
            artifact,
            artifact_expected,
            container,
            destination,
            destination_expected,
        }
        .apply(target_root),
        Operation::DiscardArtifact { expected } => discard_artifact(&item.path, expected),
        Operation::RemoveEmptyArtifact { identity } => identity
            .remove_empty(&item.path)
            .map_err(|_| ApplyFailure::Changed),
        Operation::None => Ok(()),
    }
}

pub(super) fn ensure_contained_physical_parent(
    path: &Path,
    target_root: &Path,
) -> Result<(), ApplyFailure> {
    let parent = path
        .parent()
        .ok_or_else(|| ApplyFailure::Failed("destination has no parent".to_owned()))?;
    let relative = parent.strip_prefix(target_root).map_err(|_| {
        ApplyFailure::Failed(format!(
            "The destination is outside the selected directory: {}",
            path.display()
        ))
    })?;
    let mut current = target_root.to_owned();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ApplyFailure::Failed(format!(
                    "parent folder is a symlink: {}",
                    current.display()
                )));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(ApplyFailure::Failed(format!(
                    "parent folder is not a directory: {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(failed(error)),
        }
    }
    Ok(())
}

pub(super) fn publish_with_faults(
    stage: &Path,
    destination: &Path,
    expected: &EntryFingerprint,
    faults: &dyn FaultInjector,
) -> Result<(), ApplyFailure> {
    Publication {
        stage,
        destination,
        expected,
        faults,
    }
    .publish()
}

struct Publication<'a> {
    stage: &'a Path,
    destination: &'a Path,
    expected: &'a EntryFingerprint,
    faults: &'a dyn FaultInjector,
}
impl Publication<'_> {
    fn publish(&self) -> Result<(), ApplyFailure> {
        let Self {
            stage,
            destination,
            expected,
            faults,
        } = *self;

        if &fingerprint(destination) != expected {
            let _ = remove_any(stage);
            return Err(ApplyFailure::Changed);
        }
        faults.before_publication(destination);
        if *expected == EntryFingerprint::Missing {
            return rename_noreplace(stage, destination).map_err(|error| {
                let _ = remove_any(stage);
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    ApplyFailure::Changed
                } else {
                    failed(error)
                }
            });
        }
        let parent = destination
            .parent()
            .ok_or_else(|| ApplyFailure::Failed("destination has no parent".to_owned()))?;
        let backup = BackupArtifact::create(parent, destination).inspect_err(|_| {
            let _ = remove_any(stage);
        })?;
        backup.capture(destination).inspect_err(|_| {
            let _ = remove_any(stage);
        })?;
        if !backup.matches(expected) {
            return self.restore_changed_destination(&backup);
        }
        let installation = if faults.fail_publication(destination) {
            Err(std::io::Error::other("injected publication failure"))
        } else {
            rename_noreplace(stage, destination)
        };
        if let Err(error) = installation {
            return self.rollback(&backup, error);
        }
        if faults.fail_backup_deletion(destination) {
            return Err(ApplyFailure::RecoveryRequired(format!(
                "new content is installed but backup remains at {}: injected backup deletion failure",
                backup.payload.display()
            )));
        }
        backup.remove_payload().map_err(|error| {
            ApplyFailure::RecoveryRequired(format!(
                "new content is installed but backup remains at {}: {error}",
                backup.payload.display()
            ))
        })?;
        backup.cleanup_empty()
    }
    fn restore_changed_destination(&self, backup: &BackupArtifact) -> Result<(), ApplyFailure> {
        let Self {
            stage, destination, ..
        } = *self;
        if backup.restore(destination).is_ok() {
            let _ = remove_any(stage);
            backup.cleanup_empty()?;
            Err(ApplyFailure::Changed)
        } else {
            Err(ApplyFailure::RecoveryRequired(format!(
                "destination changed after planning; preserved {}",
                backup.payload.display()
            )))
        }
    }

    fn rollback(&self, backup: &BackupArtifact, error: std::io::Error) -> Result<(), ApplyFailure> {
        let Self {
            stage,
            destination,
            faults,
            ..
        } = *self;

        let restored = !faults.fail_rollback(destination) && backup.restore(destination).is_ok();
        if restored {
            let _ = remove_any(stage);
            backup.cleanup_empty()?;
            Err(ApplyFailure::RolledBack(format!(
                "Saving failed; the original content was restored: {error}"
            )))
        } else {
            Err(ApplyFailure::RecoveryRequired(format!(
                "Saving failed and the original content could not be restored; recover it from {}: {error}",
                backup.payload.display()
            )))
        }
    }
}

fn create_directory(item: &PlanItem) -> Result<(), ApplyFailure> {
    fs::create_dir_all(&item.path).map_err(failed)?;
    match fs::symlink_metadata(&item.path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        _ => Err(ApplyFailure::Changed),
    }
}

fn write_file(
    item: &PlanItem,
    faults: &dyn FaultInjector,
    bytes: &[u8],
    expected: &EntryFingerprint,
) -> Result<(), ApplyFailure> {
    if &fingerprint(&item.path) != expected {
        return Err(ApplyFailure::Changed);
    }
    let parent = item
        .path
        .parent()
        .ok_or_else(|| ApplyFailure::Failed("destination has no parent".to_owned()))?;
    fs::create_dir_all(parent).map_err(failed)?;
    if faults.fail_staging(&item.path) {
        return Err(ApplyFailure::Failed("injected staging failure".to_owned()));
    }
    let stage = unique_artifact(parent, "stage");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stage)
        .map_err(failed)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = remove_any(&stage);
        return Err(failed(error));
    }
    publish_with_faults(&stage, &item.path, expected, faults)
}

fn remove_materialization(
    item: &PlanItem,
    expected: &EntryFingerprint,
) -> Result<(), ApplyFailure> {
    if &fingerprint(&item.path) != expected {
        return Err(ApplyFailure::Changed);
    }
    let parent = item
        .path
        .parent()
        .ok_or_else(|| ApplyFailure::Failed("destination has no parent".to_owned()))?;
    let backup = BackupArtifact::create(parent, &item.path)?;
    backup.capture(&item.path)?;
    if !backup.matches(expected) {
        return restore_changed_removal(&backup, &item.path);
    }
    if let Err(error) = backup.remove_payload() {
        return restore_failed_removal(&backup, &item.path, error);
    }
    backup.cleanup_empty()
}

struct MaterializationStage<'a> {
    source: &'a PathBuf,
    metadata_name: &'a str,
    source_expected: &'a EntryFingerprint,
    kind: &'a MaterializationKind,
    expected: &'a EntryFingerprint,
}
impl MaterializationStage<'_> {
    fn publish(&self, item: &PlanItem, faults: &dyn FaultInjector) -> Result<(), ApplyFailure> {
        let Self {
            source_expected,
            kind,
            expected,
            ..
        } = *self;

        if &fingerprint(&item.path) != expected {
            return Err(ApplyFailure::Changed);
        }
        let source = self.validate_source()?;
        let parent = item
            .path
            .parent()
            .ok_or_else(|| ApplyFailure::Failed("destination has no parent".to_owned()))?;
        fs::create_dir_all(parent).map_err(failed)?;
        if faults.fail_staging(&item.path) {
            return Err(ApplyFailure::Failed("injected staging failure".to_owned()));
        }
        let stage = unique_artifact(parent, "stage");
        match kind {
            MaterializationKind::Linked => stage_link(&source, &stage)?,
            MaterializationKind::Copied => stage_copy(&source, &stage, source_expected)?,
        }
        publish_with_faults(&stage, &item.path, expected, faults)
    }
    fn validate_source(&self) -> Result<PathBuf, ApplyFailure> {
        let Self {
            source,
            metadata_name,
            source_expected,
            kind,
            ..
        } = *self;
        let canonical_source = source.canonicalize().map_err(failed)?;
        if &canonical_source != source {
            return Err(ApplyFailure::Changed);
        }
        let source = canonical_source;
        if crate::library::validated_skill_name_at(&source).as_deref() != Some(metadata_name) {
            return Err(ApplyFailure::Changed);
        }
        if *kind == MaterializationKind::Copied
            && crate::materialization::skill_fingerprint(&source) != *source_expected
        {
            return Err(ApplyFailure::Changed);
        }
        Ok(source)
    }
}

fn stage_link(source: &Path, stage: &Path) -> Result<(), ApplyFailure> {
    #[cfg(unix)]
    std::os::unix::fs::symlink(source, stage).map_err(failed)?;
    #[cfg(not(unix))]
    return Err(ApplyFailure::Failed(
        "symbolic links require a Unix-compatible platform".to_owned(),
    ));
    if fs::read_link(stage).map_err(|error| {
        let _ = remove_any(stage);
        failed(error)
    })? != source
    {
        let _ = remove_any(stage);
        return Err(ApplyFailure::Failed(
            "The new link could not be verified".to_owned(),
        ));
    }
    Ok(())
}

fn stage_copy(
    source: &Path,
    stage: &Path,
    source_expected: &EntryFingerprint,
) -> Result<(), ApplyFailure> {
    copy_tree(source, stage).map_err(|error| {
        if copy_error_is_owned_stage_entry(&error, source, stage) {
            let _ = remove_any(stage);
        }
        ApplyFailure::Failed(error.to_string())
    })?;
    let source_tree = read_copy_tree(source, true, stage)?;
    let staged_tree = read_copy_tree(stage, false, stage)?;
    if source_tree != staged_tree {
        let _ = remove_any(stage);
        return Err(ApplyFailure::Changed);
    }
    if crate::materialization::skill_fingerprint(source) != *source_expected {
        let _ = remove_any(stage);
        return Err(ApplyFailure::Changed);
    }
    Ok(())
}

fn restore_changed_removal(backup: &BackupArtifact, path: &Path) -> Result<(), ApplyFailure> {
    if backup.restore(path).is_ok() {
        backup.cleanup_empty()?;
        Err(ApplyFailure::Changed)
    } else {
        Err(ApplyFailure::RecoveryRequired(format!(
            "The file changed before removal; a backup was kept at {}",
            backup.payload.display()
        )))
    }
}

fn restore_failed_removal(
    backup: &BackupArtifact,
    path: &Path,
    error: std::io::Error,
) -> Result<(), ApplyFailure> {
    if backup.restore(path).is_ok() {
        backup.cleanup_empty()?;
        Err(ApplyFailure::RolledBack(format!(
            "Removal failed; the original content was restored: {error}"
        )))
    } else {
        Err(ApplyFailure::RecoveryRequired(format!(
            "Removal failed and the original content could not be restored; recover it from {}: {error}",
            backup.payload.display()
        )))
    }
}

struct RecoveryOperation<'a> {
    artifact: &'a Path,
    artifact_expected: &'a EntryFingerprint,
    container: &'a Option<BackupArtifact>,
    destination: &'a Path,
    destination_expected: &'a EntryFingerprint,
}
impl RecoveryOperation<'_> {
    fn apply(&self, target_root: &Path) -> Result<(), ApplyFailure> {
        let Self {
            artifact,
            artifact_expected,
            container,
            destination,
            destination_expected,
        } = *self;

        ensure_contained_physical_parent(destination, target_root)?;
        let artifact_matches = container.as_ref().map_or_else(
            || &fingerprint(artifact) == artifact_expected,
            |container| container.matches(artifact_expected),
        );
        if !artifact_matches || &fingerprint(destination) != destination_expected {
            return Err(ApplyFailure::Changed);
        }
        rename_noreplace(artifact, destination).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ApplyFailure::Changed
            } else {
                failed(error)
            }
        })?;
        if let Some(container) = container {
            container.cleanup_empty()?;
        }
        Ok(())
    }
}

fn discard_artifact(path: &Path, expected: &EntryFingerprint) -> Result<(), ApplyFailure> {
    if &fingerprint(path) != expected {
        return Err(ApplyFailure::Changed);
    }
    remove_any(path).map_err(failed)
}

fn copy_error_is_owned_stage_entry(
    error: &crate::materialization::CopyError,
    source: &Path,
    stage: &Path,
) -> bool {
    matches!(error, crate::materialization::CopyError::Io { path, .. } if path != stage && path.starts_with(stage) && !path.starts_with(source))
}

fn read_copy_tree(path: &Path, source: bool, stage: &Path) -> Result<TreeSnapshot, ApplyFailure> {
    TreeSnapshot::read(path, source).map_err(|error| {
        let _ = remove_any(stage);
        ApplyFailure::Failed(error.to_string())
    })
}
