//! Conservative recognition and recovery of interrupted-save artifacts.
use super::*;

pub(super) fn plan_recovery(plan: &mut Plan, target: &Target, config: &RepositoryConfig) {
    let mut planner = RecoveryPlanner { plan, target };
    for directory in config.skill_directories() {
        planner.plan_directory(&target.root().join(directory.path().as_str()));
    }
}
type StageEntries = std::collections::BTreeMap<PathBuf, Vec<PathBuf>>;
type BackupEntries = std::collections::BTreeMap<PathBuf, Vec<(PathBuf, Option<BackupArtifact>)>>;
struct RecoveryPlanner<'a> {
    plan: &'a mut Plan,
    target: &'a Target,
}
impl RecoveryPlanner<'_> {
    fn plan_directory(&mut self, root: &Path) {
        if !recovery_root_available(root, self.target.root()) {
            return;
        }
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        let mut artifacts = RecoveryArtifacts::default();
        for entry in entries.flatten() {
            self.inspect_entry(root, entry, &mut artifacts);
        }
        let destinations: std::collections::BTreeSet<_> = artifacts
            .backups
            .keys()
            .chain(artifacts.stages.keys())
            .cloned()
            .collect();
        for destination in destinations {
            self.plan_destination(&destination, &artifacts.backups, &artifacts.stages);
        }
    }
    fn inspect_entry(
        &mut self,
        root: &Path,
        entry: fs::DirEntry,
        artifacts: &mut RecoveryArtifacts,
    ) {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            return;
        };
        // New stages are anonymous siblings. Only backups retain a destination
        // basename; anonymous stages stay read-only for manual recovery.
        if bounded_artifact_name(&name, "backup") {
            self.inspect_bounded_backup(root, &path, &mut artifacts.backups);
        } else if let Some(destination) = artifact_destination(root, &name, "backup") {
            artifacts
                .backups
                .entry(destination)
                .or_default()
                .push((path, None));
        } else if let Some(destination) = artifact_destination(root, &name, "stage") {
            artifacts.stages.entry(destination).or_default().push(path);
        }
    }
    fn inspect_bounded_backup(&mut self, root: &Path, path: &Path, backups: &mut BackupEntries) {
        let Ok(identity) = ContainerIdentity::read(path) else {
            return;
        };
        let Ok(payload) = identity.contents(path) else {
            return;
        };
        let Some(payload) = payload else {
            self.plan_empty_backup(path, identity);
            return;
        };
        let Some(basename) = payload.file_name() else {
            return;
        };
        let destination = root.join(basename);
        let container = BackupArtifact {
            container: path.to_owned(),
            payload,
            identity,
        };
        backups
            .entry(destination)
            .or_default()
            .push((path.to_owned(), Some(container)));
    }
    fn plan_empty_backup(&mut self, path: &Path, identity: ContainerIdentity) {
        if !git_path_unprotected(self.target, path) {
            return;
        }
        if let Some(item) = self
            .plan
            .items
            .iter_mut()
            .find(|item| item.action == Action::Recover && item.path == path)
        {
            item.safety = Safety::Safe;
            item.reason = "Remove an empty backup folder left by an interrupted save".to_owned();
            item.operation = Operation::RemoveEmptyArtifact { identity };
        }
    }
    fn plan_destination(
        &mut self,
        destination: &PathBuf,
        backups: &BackupEntries,
        stages: &StageEntries,
    ) {
        let backup = backups.get(destination).map(Vec::as_slice).unwrap_or(&[]);
        let staged = stages.get(destination).map(Vec::as_slice).unwrap_or(&[]);
        let safe = if self.backup_can_be_restored(destination, backup) {
            let Some(safe) = self.plan_backup_restore(destination, &backup[0]) else {
                return;
            };
            safe
        } else {
            backup.is_empty()
        };
        if safe {
            self.plan_stale_stages(staged);
        }
    }
    fn plan_backup_restore(
        &mut self,
        destination: &Path,
        backup: &(PathBuf, Option<BackupArtifact>),
    ) -> Option<bool> {
        let (path, container) = backup;
        let payload = container
            .as_ref()
            .map_or(path, |container| &container.payload);
        if container.as_ref().is_some_and(|container| {
            container
                .identity
                .contents(&container.container)
                .ok()
                .flatten()
                .as_ref()
                != Some(payload)
        }) {
            return None;
        }
        let expected = fingerprint(payload);
        if !recoverable_fingerprint(&expected)
            || container
                .as_ref()
                .is_some_and(|container| !container.matches(&expected))
        {
            Some(false)
        } else if let Some(item) = self
            .plan
            .items
            .iter_mut()
            .find(|item| item.action == Action::Recover && item.path == *path)
        {
            item.safety = Safety::Safe;
            item.reason = "Restore the backup from the interrupted save".to_owned();
            item.operation = Operation::Recover {
                artifact: payload.clone(),
                artifact_expected: expected,
                container: container.clone(),
                destination: destination.to_owned(),
                destination_expected: EntryFingerprint::Missing,
            };
            Some(true)
        } else {
            Some(false)
        }
    }

    fn plan_stale_stages(&mut self, staged: &[PathBuf]) {
        for stage in staged {
            if !git_path_unprotected(self.target, stage) {
                continue;
            }
            let expected = fingerprint(stage);
            if !recoverable_fingerprint(&expected) {
                continue;
            }
            if let Some(item) = self
                .plan
                .items
                .iter_mut()
                .find(|item| item.action == Action::Recover && item.path == *stage)
            {
                item.safety = Safety::Safe;
                item.reason = "Remove temporary files left by an interrupted save".to_owned();
                item.operation = Operation::DiscardArtifact { expected };
            }
        }
    }
    fn backup_can_be_restored(
        &self,
        destination: &Path,
        backup: &[(PathBuf, Option<BackupArtifact>)],
    ) -> bool {
        let destination_absent = fs::symlink_metadata(destination)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
        backup.len() == 1
            && destination_absent
            && git_path_unprotected(self.target, &backup[0].0)
            && git_path_unprotected(self.target, destination)
    }
}

fn git_path_unprotected(target: &Target, path: &Path) -> bool {
    target_path_facts(target, path)
        .ok()
        .is_some_and(|facts| !facts.tracked && !facts.staged && !facts.unmerged)
}

pub(super) fn target_path_facts(target: &Target, path: &Path) -> Result<PathFacts, String> {
    let relative = path
        .strip_prefix(target.root())
        .map_err(|_| "The destination is outside the selected directory".to_owned())?;
    if let Some(repository) = target.git_repository() {
        repository
            .facts_for(relative)
            .map_err(|error| error.to_string())
    } else {
        Ok(PathFacts {
            tracked: false,
            staged: false,
            unmerged: false,
            ignored: false,
            ignore_rule: None,
        })
    }
}

// Persisted pre-container artifacts remain recovery data; new artifacts never
// encode a destination basename in their filesystem component.
fn artifact_destination(root: &Path, name: &str, kind: &str) -> Option<PathBuf> {
    let suffix = name.strip_prefix(&format!(".skillator-{kind}-"))?;
    let mut parts = suffix.splitn(3, '-');
    parts.next()?.parse::<u32>().ok()?;
    parts.next()?.parse::<u64>().ok()?;
    let bytes = decode_artifact_basename(parts.next()?.as_bytes())?;
    #[cfg(unix)]
    let name = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(bytes)
    };
    #[cfg(not(unix))]
    let name = std::ffi::OsString::from(String::from_utf8(bytes).ok()?);
    let destination = root.join(&name);
    (destination.parent() == Some(root) && destination.file_name() == Some(name.as_os_str()))
        .then_some(destination)
}

fn bounded_artifact_name(name: &str, kind: &str) -> bool {
    let Some(suffix) = name.strip_prefix(&format!(".skillator-{kind}-")) else {
        return false;
    };
    let mut parts = suffix.split('-');
    parts.next().is_some_and(|part| {
        part.bytes().all(|byte| byte.is_ascii_digit()) && part.parse::<u32>().is_ok()
    }) && parts.next().is_some_and(|part| {
        part.bytes().all(|byte| byte.is_ascii_digit()) && part.parse::<u64>().is_ok()
    }) && parts.next().is_none()
}

#[derive(Default)]
struct RecoveryArtifacts {
    stages: StageEntries,
    backups: BackupEntries,
}

fn recovery_root_available(root: &Path, target_root: &Path) -> bool {
    ensure_contained_physical_parent(root, target_root).is_ok()
        && fs::symlink_metadata(root)
            .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn decode_artifact_basename(encoded: &[u8]) -> Option<Vec<u8>> {
    if encoded.is_empty() || !encoded.len().is_multiple_of(2) {
        return None;
    }
    let bytes = encoded
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let pair = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(pair, 16).ok()
        })
        .collect::<Option<Vec<_>>>()?;
    Some(bytes)
}

fn recoverable_fingerprint(expected: &EntryFingerprint) -> bool {
    !matches!(
        expected,
        EntryFingerprint::Missing | EntryFingerprint::Other | EntryFingerprint::Uninspectable
    )
}
