//! Pure reconciliation planning and guarded execution.

mod recovery;
use recovery::{plan_recovery, target_path_facts};
mod artifacts;
use artifacts::{BackupArtifact, ContainerIdentity, unique_artifact};
mod execution;
#[cfg(test)]
use execution::publish_with_faults;
use execution::{apply_operation_with, ensure_contained_physical_parent};
mod planning;
#[cfg(test)]
use planning::classify_materialization;

use crate::config::RepositoryConfig;
use crate::domain::MaterializationKind;
use crate::fs_safety::rename_noreplace;
use crate::git::PathFacts;
use crate::library::LibrarySnapshot;
use crate::materialization::{EntryFingerprint, TreeSnapshot, copy_tree, fingerprint};
use crate::target::{
    Comparison, ControlFileState, MaterializationState, ObservedState, RepositorySkillExceptions,
    RootState, Target, observe, observe_with_repository_skills,
};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static OPERATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Safety {
    Safe,
    Guarded,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    CreateDirectory,
    WriteControlFile,
    Link,
    Copy,
    Replace,
    RemoveUnmanaged,
    TrackControlFile,
    Recover,
}

#[derive(Debug, Clone)]
enum Operation {
    CreateDirectory,
    WriteFile {
        bytes: Vec<u8>,
        expected: EntryFingerprint,
    },
    Materialize {
        source: PathBuf,
        metadata_name: String,
        source_expected: EntryFingerprint,
        kind: MaterializationKind,
        expected: EntryFingerprint,
    },
    Remove {
        expected: EntryFingerprint,
    },
    Recover {
        artifact: PathBuf,
        artifact_expected: EntryFingerprint,
        container: Option<BackupArtifact>,
        destination: PathBuf,
        destination_expected: EntryFingerprint,
    },
    DiscardArtifact {
        expected: EntryFingerprint,
    },
    RemoveEmptyArtifact {
        identity: ContainerIdentity,
    },
    None,
}

#[derive(Debug, Clone)]
pub struct PlanItem {
    path: PathBuf,
    action: Action,
    safety: Safety,
    reason: String,
    operation: Operation,
}

impl PlanItem {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn action(&self) -> Action {
        self.action
    }

    pub fn safety(&self) -> Safety {
        self.safety
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    items: Vec<PlanItem>,
}

impl Plan {
    pub fn items(&self) -> &[PlanItem] {
        &self.items
    }

    pub fn is_converged(&self) -> bool {
        self.items.is_empty()
    }

    pub fn has_guarded(&self) -> bool {
        self.items.iter().any(|item| item.safety == Safety::Guarded)
    }
}

pub use planning::plan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authorization {
    SafeOnly,
    AllGuarded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Another Skillator process is saving changes")]
pub struct TargetBusy;

/// Locks one or more Targets in canonical root-path order.
pub struct TargetLocks {
    locks: Vec<File>,
}

impl TargetLocks {
    pub fn acquire(targets: &[&Target]) -> Result<Self, TargetBusy> {
        let mut paths = targets
            .iter()
            .map(|target| (target.root().to_owned(), target.lock_path().to_owned()))
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup_by(|left, right| left.1 == right.1);

        let mut locks = Vec::with_capacity(paths.len());
        for (_, path) in paths {
            let file = File::open(path).map_err(|_| TargetBusy)?;
            file.try_lock().map_err(|_| TargetBusy)?;
            locks.push(file);
        }
        Ok(Self { locks })
    }
}

impl Drop for TargetLocks {
    fn drop(&mut self) {
        for lock in &self.locks {
            let _ = lock.unlock();
        }
    }
}

pub struct PreparedPlan {
    plan: Plan,
    git_facts: std::collections::BTreeMap<PathBuf, Result<PathFacts, String>>,
    _locks: TargetLocks,
}

impl std::fmt::Debug for PreparedPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedPlan")
            .field("plan", &self.plan)
            .finish_non_exhaustive()
    }
}

impl PreparedPlan {
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
}

impl Drop for PreparedPlan {
    fn drop(&mut self) {}
}

pub fn prepare_check(
    target: &Target,
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
) -> Result<PreparedPlan, TargetBusy> {
    prepare_check_with_locks(target, config, library, TargetLocks::acquire(&[target])?)
}

pub fn prepare_check_with_locks(
    target: &Target,
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
    locks: TargetLocks,
) -> Result<PreparedPlan, TargetBusy> {
    let observed = observe(target, config, library);
    let mut plan = plan(config, library, &observed);
    plan_recovery(&mut plan, target, config);
    Ok(PreparedPlan {
        git_facts: capture_git_facts(target, &plan),
        plan,
        _locks: locks,
    })
}

pub fn prepare_transition(
    target: &Target,
    original: &RepositoryConfig,
    staged: &RepositoryConfig,
    library: &LibrarySnapshot,
) -> Result<PreparedPlan, TargetBusy> {
    let locks = TargetLocks::acquire(&[target])?;
    prepare_transition_with_locks(target, original, staged, library, locks)
}

pub fn prepare_transition_with_locks(
    target: &Target,
    original: &RepositoryConfig,
    staged: &RepositoryConfig,
    library: &LibrarySnapshot,
    locks: TargetLocks,
) -> Result<PreparedPlan, TargetBusy> {
    prepare_transition_with_locks_and_repository_skills(
        target,
        original,
        staged,
        library,
        &RepositorySkillExceptions::new(),
        locks,
    )
}

pub fn prepare_transition_with_locks_and_repository_skills(
    target: &Target,
    original: &RepositoryConfig,
    staged: &RepositoryConfig,
    library: &LibrarySnapshot,
    repository_skills: &RepositorySkillExceptions,
    locks: TargetLocks,
) -> Result<PreparedPlan, TargetBusy> {
    let original_observed = observe(target, original, library);
    let staged_observed =
        observe_with_repository_skills(target, staged, library, repository_skills);
    let mut plan = plan(staged, library, &staged_observed);
    let disabled = original_observed
        .enablements()
        .filter(|entry| {
            !staged.enablements().iter().any(|enablement| {
                enablement.directory() == entry.enablement().directory()
                    && enablement.skill() == entry.enablement().skill()
            })
        })
        .filter_map(|entry| entry.path().map(|path| (entry, path.to_owned())))
        .collect::<Vec<_>>();
    for (entry, path) in disabled {
        let safety = if entry.tracked() || entry.fingerprint() == &EntryFingerprint::Uninspectable {
            Safety::Blocked
        } else if entry.comparison() == Comparison::InSync {
            Safety::Safe
        } else {
            Safety::Guarded
        };
        plan.items.push(PlanItem {
            path,
            action: Action::RemoveUnmanaged,
            safety,
            reason: if safety == Safety::Safe {
                "Remove the disabled skill's unchanged link or copy".to_owned()
            } else if entry.tracked() {
                "Git tracks this disabled skill; Skillator cannot remove it".to_owned()
            } else {
                "This disabled skill has changed; saving will remove its existing link or copy"
                    .to_owned()
            },
            operation: if safety == Safety::Blocked {
                Operation::None
            } else {
                Operation::Remove {
                    expected: entry.fingerprint().clone(),
                }
            },
        });
    }
    plan.items.sort_by_key(|item| {
        let order = match item.action {
            Action::Recover => 0,
            Action::CreateDirectory => 1,
            Action::WriteControlFile => 2,
            Action::Link | Action::Copy | Action::Replace => 3,
            Action::TrackControlFile => 4,
            Action::RemoveUnmanaged => 5,
        };
        (order, item.path.clone())
    });
    plan_recovery(&mut plan, target, staged);
    Ok(PreparedPlan {
        git_facts: capture_git_facts(target, &plan),
        plan,
        _locks: locks,
    })
}

pub fn prepare_apply(
    target: &Target,
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
) -> Result<PreparedPlan, TargetBusy> {
    prepare_apply_with_locks(target, config, library, TargetLocks::acquire(&[target])?)
}

pub fn prepare_apply_with_locks(
    target: &Target,
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
    locks: TargetLocks,
) -> Result<PreparedPlan, TargetBusy> {
    prepare_check_with_locks(target, config, library, locks)
}

fn capture_git_facts(
    target: &Target,
    plan: &Plan,
) -> std::collections::BTreeMap<PathBuf, Result<PathFacts, String>> {
    plan.items
        .iter()
        .flat_map(|item| mutation_paths(item).into_iter().flatten())
        .map(|path| {
            let facts = target_path_facts(target, path);
            (path.to_owned(), facts)
        })
        .collect()
}

fn mutation_paths(item: &PlanItem) -> [Option<&Path>; 2] {
    [
        Some(item.path.as_path()),
        match &item.operation {
            Operation::Recover { destination, .. } => Some(destination.as_path()),
            _ => None,
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Applied,
    NotAuthorized,
    Blocked,
    Failed,
    RolledBack,
    RecoveryRequired,
}

#[derive(Debug, Clone)]
pub struct ApplyOutcome {
    pub path: PathBuf,
    pub action: Action,
    pub safety: Safety,
    pub outcome: Outcome,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ApplyResult {
    outcomes: Vec<ApplyOutcome>,
    final_observed: ObservedState,
}

impl ApplyResult {
    pub fn outcomes(&self) -> &[ApplyOutcome] {
        &self.outcomes
    }

    pub fn final_observed(&self) -> &ObservedState {
        &self.final_observed
    }

    pub fn converged(&self) -> bool {
        self.final_observed
            .enablements()
            .all(|entry| entry.comparison() == Comparison::InSync)
            && self
                .final_observed
                .directories()
                .iter()
                .all(|directory| directory.comparison() == Comparison::InSync)
    }
}

pub fn execute(
    mut prepared: PreparedPlan,
    authorization: Authorization,
    target: &Target,
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
) -> ApplyResult {
    let mut outcomes = Vec::new();
    let items = std::mem::take(&mut prepared.plan.items);
    let expected_git = std::mem::take(&mut prepared.git_facts);
    for item in items {
        if item.safety == Safety::Blocked {
            outcomes.push(outcome(&item, Outcome::Blocked, item.reason.clone()));
            continue;
        }
        if item.safety == Safety::Guarded && authorization == Authorization::SafeOnly {
            outcomes.push(outcome(
                &item,
                Outcome::NotAuthorized,
                "This change needs confirmation; review the path before using --force".to_owned(),
            ));
            continue;
        }
        let git_matches = mutation_paths(&item).into_iter().flatten().all(|path| {
            expected_git.get(path).is_some_and(|expected| {
                expected.as_ref().is_ok_and(|expected| {
                    target_path_facts(target, path)
                        .ok()
                        .is_some_and(|current| git_protection_matches(expected, &current))
                })
            })
        });
        if !git_matches {
            outcomes.push(outcome(
                &item,
                Outcome::Blocked,
                "Git status changed or could not be checked; review the changes and retry"
                    .to_owned(),
            ));
            continue;
        }
        let result = apply_operation(&item, target.root());
        outcomes.push(match result {
            Ok(()) => outcome(&item, Outcome::Applied, "applied".to_owned()),
            Err(ApplyFailure::Changed) => outcome(
                &item,
                Outcome::Blocked,
                "path changed after planning".to_owned(),
            ),
            Err(ApplyFailure::RolledBack(message)) => outcome(&item, Outcome::RolledBack, message),
            Err(ApplyFailure::RecoveryRequired(message)) => {
                outcome(&item, Outcome::RecoveryRequired, message)
            }
            Err(ApplyFailure::Failed(message)) => outcome(&item, Outcome::Failed, message),
        });
    }
    let final_observed = observe(target, config, library);
    ApplyResult {
        outcomes,
        final_observed,
    }
}

fn git_protection_matches(expected: &PathFacts, current: &PathFacts) -> bool {
    !expected.tracked
        && !expected.staged
        && !expected.unmerged
        && !current.tracked
        && !current.staged
        && !current.unmerged
}

fn outcome(item: &PlanItem, outcome: Outcome, message: String) -> ApplyOutcome {
    ApplyOutcome {
        path: item.path.clone(),
        action: item.action,
        safety: item.safety,
        outcome,
        message,
    }
}

#[derive(Debug)]
enum ApplyFailure {
    Changed,
    Failed(String),
    RolledBack(String),
    RecoveryRequired(String),
}

fn apply_operation(item: &PlanItem, target_root: &Path) -> Result<(), ApplyFailure> {
    apply_operation_with(item, &NoFaults, target_root)
}

trait FaultInjector {
    fn fail_staging(&self, _path: &Path) -> bool {
        false
    }
    fn fail_publication(&self, _path: &Path) -> bool {
        false
    }
    fn fail_rollback(&self, _path: &Path) -> bool {
        false
    }
    fn fail_backup_deletion(&self, _path: &Path) -> bool {
        false
    }
    fn before_publication(&self, _path: &Path) {}
}

struct NoFaults;
impl FaultInjector for NoFaults {}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Faults {
        staging: bool,
        publication: bool,
        rollback: bool,
        backup_deletion: bool,
    }

    impl FaultInjector for Faults {
        fn fail_staging(&self, _: &Path) -> bool {
            self.staging
        }
        fn fail_publication(&self, _: &Path) -> bool {
            self.publication
        }
        fn fail_rollback(&self, _: &Path) -> bool {
            self.rollback
        }
        fn fail_backup_deletion(&self, _: &Path) -> bool {
            self.backup_deletion
        }
    }

    struct ConcurrentWrite<'a> {
        bytes: &'a [u8],
    }

    impl FaultInjector for ConcurrentWrite<'_> {
        fn before_publication(&self, path: &Path) {
            fs::write(path, self.bytes).unwrap();
        }
    }

    #[test]
    fn publication_never_overwrites_a_concurrently_created_destination() {
        assert_concurrent_destination_preserved(None, EntryFingerprint::Missing);
    }

    #[test]
    fn publication_restores_a_destination_changed_during_replacement() {
        assert_concurrent_destination_preserved(
            Some(b"old"),
            EntryFingerprint::File(b"old".to_vec()),
        );
    }

    fn assert_concurrent_destination_preserved(initial: Option<&[u8]>, expected: EntryFingerprint) {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("skill");
        let stage = root.path().join("stage");
        if let Some(initial) = initial {
            fs::write(&destination, initial).unwrap();
        }
        fs::write(&stage, "new").unwrap();
        let result = publish_with_faults(
            &stage,
            &destination,
            &expected,
            &ConcurrentWrite { bytes: b"external" },
        );
        std::assert_matches!(result, Err(ApplyFailure::Changed));
        assert_eq!(fs::read(&destination).unwrap(), b"external");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn materialization_classification_table_covers_safety_boundaries() {
        let cases = [
            (MaterializationState::Missing, false, true, Safety::Safe),
            (
                MaterializationState::NoncanonicalLink,
                false,
                true,
                Safety::Safe,
            ),
            (
                MaterializationState::DivergedCopy,
                false,
                true,
                Safety::Guarded,
            ),
            (
                MaterializationState::MisdirectedLink,
                false,
                true,
                Safety::Guarded,
            ),
            (
                MaterializationState::Uninspectable,
                false,
                true,
                Safety::Blocked,
            ),
            (MaterializationState::Missing, true, true, Safety::Blocked),
            (MaterializationState::Missing, false, false, Safety::Blocked),
        ];
        for (state, tracked, resolved, expected) in cases {
            assert_eq!(
                classify_materialization(&state, tracked, resolved),
                expected,
                "state={state:?}, tracked={tracked}, resolved={resolved}"
            );
        }
    }

    fn replacement(root: &Path, name: &str) -> PlanItem {
        let path = root.join(name);
        fs::write(&path, "old").unwrap();
        PlanItem {
            path,
            action: Action::WriteControlFile,
            safety: Safety::Guarded,
            reason: "test".to_owned(),
            operation: Operation::WriteFile {
                bytes: b"new".to_vec(),
                expected: EntryFingerprint::File(b"old".to_vec()),
            },
        }
    }

    fn retained_original_content(root: &Path) -> bool {
        fs::read_dir(root).unwrap().any(|entry| {
            let path = entry.unwrap().path();
            if !fs::symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
            {
                return false;
            }
            fs::read_dir(path)
                .unwrap()
                .any(|payload| fs::read(payload.unwrap().path()).is_ok_and(|bytes| bytes == b"old"))
        })
    }

    #[test]
    fn fault_injection_preserves_content_and_isolates_operations() {
        let root = tempfile::tempdir().unwrap();
        let first = replacement(root.path(), "first");
        let second = replacement(root.path(), "second");
        let failure = apply_operation_with(
            &first,
            &Faults {
                staging: true,
                ..Faults::default()
            },
            root.path(),
        );
        std::assert_matches!(failure, Err(ApplyFailure::Failed(_)));
        assert_eq!(fs::read_to_string(&first.path).unwrap(), "old");
        apply_operation_with(&second, &Faults::default(), root.path()).unwrap();
        assert_eq!(fs::read_to_string(&second.path).unwrap(), "new");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn publication_failure_rolls_back_or_retains_recovery_artifact() {
        let root = tempfile::tempdir().unwrap();
        let rolled_back = replacement(root.path(), &"r".repeat(255));
        let result = apply_operation_with(
            &rolled_back,
            &Faults {
                publication: true,
                ..Faults::default()
            },
            root.path(),
        );
        std::assert_matches!(result, Err(ApplyFailure::RolledBack(_)));
        assert_eq!(fs::read_to_string(&rolled_back.path).unwrap(), "old");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);

        let recovery = replacement(root.path(), &"z".repeat(255));
        let result = apply_operation_with(
            &recovery,
            &Faults {
                publication: true,
                rollback: true,
                ..Faults::default()
            },
            root.path(),
        );
        std::assert_matches!(result, Err(ApplyFailure::RecoveryRequired(_)));
        assert!(!recovery.path.exists());
        assert!(retained_original_content(root.path()));
    }

    #[test]
    fn backup_deletion_failure_keeps_new_content_and_backup() {
        let root = tempfile::tempdir().unwrap();
        let item = replacement(root.path(), &"d".repeat(255));
        let result = apply_operation_with(
            &item,
            &Faults {
                backup_deletion: true,
                ..Faults::default()
            },
            root.path(),
        );
        std::assert_matches!(result, Err(ApplyFailure::RecoveryRequired(_)));
        assert_eq!(fs::read_to_string(&item.path).unwrap(), "new");
        assert!(retained_original_content(root.path()));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }
}

fn remove_any(path: &Path) -> Result<(), std::io::Error> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn failed(error: impl std::fmt::Display) -> ApplyFailure {
    ApplyFailure::Failed(error.to_string())
}
