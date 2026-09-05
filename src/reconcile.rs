//! Pure reconciliation planning and guarded execution.

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
        destination: PathBuf,
        destination_expected: EntryFingerprint,
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

pub fn plan(
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
    observed: &ObservedState,
) -> Plan {
    let mut items = Vec::new();
    let mut blocked_directories = std::collections::BTreeSet::new();
    let mut planned_control_paths = std::collections::BTreeSet::new();
    for directory in observed.directories() {
        match directory.root_state() {
            RootState::Absent => items.push(PlanItem {
                path: directory.path().to_owned(),
                action: Action::CreateDirectory,
                safety: Safety::Safe,
                reason: "skill folder is missing".to_owned(),
                operation: Operation::CreateDirectory,
            }),
            RootState::Directory => {}
            RootState::Inaccessible => {
                blocked_directories.insert(directory.key().to_owned());
                items.push(blocked(
                    directory.path(),
                    Action::CreateDirectory,
                    "Cannot access the skill folder; check its permissions",
                ));
                continue;
            }
            RootState::Symlink | RootState::Other => {
                blocked_directories.insert(directory.key().to_owned());
                items.push(blocked(
                    directory.path(),
                    Action::CreateDirectory,
                    "The skill folder must be a directory, not a link or file",
                ));
                continue;
            }
        }
        let control = directory.control_path();
        if planned_control_paths.insert(control.to_owned()) {
            match directory.control_file() {
                ControlFileState::NotRequired => {}
                ControlFileState::Missing => {
                    if directory.control_protected() {
                        items.push(blocked(
                            control,
                            Action::WriteControlFile,
                            ".gitignore is tracked, staged, or has merge conflicts; resolve its Git status before replacing it",
                        ));
                    } else {
                        items.push(PlanItem {
                            path: control.to_owned(),
                            action: Action::WriteControlFile,
                            safety: Safety::Safe,
                            reason: ".gitignore is missing".to_owned(),
                            operation: Operation::WriteFile {
                                bytes: directory.control_content().to_vec(),
                                expected: EntryFingerprint::Missing,
                            },
                        });
                    }
                }
                ControlFileState::Canonical => {
                    if directory.control_protected() {
                        items.push(blocked(
                            control,
                            Action::WriteControlFile,
                            ".gitignore is tracked, staged, or has merge conflicts; resolve its Git status before replacing it",
                        ));
                    } else if !directory.control_ignored() {
                        items.push(blocked(
                            control,
                            Action::WriteControlFile,
                            ".gitignore is not ignored by Git; check the repository's ignore rules",
                        ));
                    } else if !directory.generated_ignored() {
                        items.push(blocked(
                            control,
                            Action::WriteControlFile,
                            "Generated skill files are not ignored by Git; check the repository's ignore rules",
                        ));
                    }
                }
                ControlFileState::PrefixRequired => {
                    let expected = fingerprint(control);
                    if expected == EntryFingerprint::Uninspectable || directory.control_protected()
                    {
                        items.push(blocked(
                            control,
                            Action::WriteControlFile,
                            "Cannot update .gitignore; check its permissions and whether Git tracks it",
                        ));
                    } else {
                        items.push(PlanItem {
                            path: control.to_owned(),
                            action: Action::WriteControlFile,
                            safety: Safety::Safe,
                            reason: "Add Skillator's ignore rules and keep existing rules"
                                .to_owned(),
                            operation: Operation::WriteFile {
                                bytes: directory.control_content().to_vec(),
                                expected,
                            },
                        });
                    }
                }
                ControlFileState::Modified | ControlFileState::WrongKind => {
                    let expected = fingerprint(control);
                    let safety = if expected == EntryFingerprint::Uninspectable
                        || directory.control_protected()
                    {
                        Safety::Blocked
                    } else {
                        Safety::Guarded
                    };
                    items.push(PlanItem {
                        path: control.to_owned(),
                        action: Action::WriteControlFile,
                        safety,
                        reason: if directory.control_protected() {
                            ".gitignore is tracked, staged, or has merge conflicts; resolve its Git status before replacing it"
                        } else if expected == EntryFingerprint::Uninspectable {
                            "Cannot read .gitignore; check its type and permissions"
                        } else if *directory.control_file() == ControlFileState::WrongKind {
                            "A folder or link occupies the .gitignore path. Saving will replace it with Skillator's ignore file"
                        } else {
                            "This .gitignore has different contents. Saving will replace the entire file with Skillator's ignore rules"
                        }.to_owned(),
                        operation: if safety == Safety::Blocked {
                            Operation::None
                        } else {
                            Operation::WriteFile {
                                bytes: directory.control_content().to_vec(),
                                expected,
                            }
                        },
                    });
                }
                ControlFileState::Uninspectable => items.push(blocked(
                    control,
                    Action::WriteControlFile,
                    "Cannot read .gitignore; check its type and permissions",
                )),
            }
        }
        for recovery in directory.recovery_artifacts() {
            items.push(blocked(
                recovery,
                Action::Recover,
                "Files remain from an interrupted save; review them before continuing",
            ));
        }
    }

    for observation in observed.enablements() {
        if blocked_directories.contains(observation.enablement().directory().as_str()) {
            items.push(blocked(
                observation.path().unwrap_or(observed.target_root()),
                materialize_action(observation.enablement().materialization()),
                "The skill folder is inaccessible or points outside the selected directory",
            ));
            continue;
        }
        if observation.comparison() == Comparison::InSync {
            continue;
        }
        let Some(path) = observation.path() else {
            items.push(blocked(
                observed.target_root(),
                materialize_action(observation.enablement().materialization()),
                "Cannot determine the skill's destination name",
            ));
            continue;
        };
        let resolved = library.resolve(observation.enablement().skill());
        let source = resolved.and_then(|skill| skill.absolute_path());
        let action = match observation.state() {
            MaterializationState::Missing => {
                materialize_action(observation.enablement().materialization())
            }
            _ => Action::Replace,
        };
        let safety =
            classify_materialization(observation.state(), observation.tracked(), source.is_some());
        items.push(PlanItem {
            path: path.to_owned(),
            action,
            safety,
            reason: if observation.tracked() {
                "Git tracks this path; Skillator cannot replace it".to_owned()
            } else if source.is_none() {
                "Cannot find or read the source skill".to_owned()
            } else {
                observation.state().description().to_owned()
            },
            operation: if safety == Safety::Blocked {
                Operation::None
            } else {
                Operation::Materialize {
                    source: source.expect("checked above").to_owned(),
                    source_expected: resolved.expect("checked above").fingerprint().clone(),
                    kind: observation.enablement().materialization(),
                    expected: observation.fingerprint().clone(),
                }
            },
        });
    }

    items.sort_by_key(|item| {
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
    let _ = config;
    Plan { items }
}

fn classify_materialization(state: &MaterializationState, tracked: bool, resolved: bool) -> Safety {
    if tracked || !resolved {
        return Safety::Blocked;
    }
    match state {
        MaterializationState::Missing
        | MaterializationState::NoncanonicalLink
        | MaterializationState::BrokenLink => Safety::Safe,
        MaterializationState::MisdirectedLink
        | MaterializationState::DivergedCopy
        | MaterializationState::WrongKind => Safety::Guarded,
        MaterializationState::CopyIneligible
        | MaterializationState::Uninspectable
        | MaterializationState::ExpectedEntryCollision
        | MaterializationState::UnknownExpectedEntry => Safety::Blocked,
        MaterializationState::CanonicalLink | MaterializationState::EquivalentCopy => Safety::Safe,
    }
}

fn blocked(path: &Path, action: Action, reason: &str) -> PlanItem {
    PlanItem {
        path: path.to_owned(),
        action,
        safety: Safety::Blocked,
        reason: reason.to_owned(),
        operation: Operation::None,
    }
}

fn materialize_action(kind: MaterializationKind) -> Action {
    match kind {
        MaterializationKind::Linked => Action::Link,
        MaterializationKind::Copied => Action::Copy,
    }
}

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
    prepare_transition_with_locks(
        target,
        original,
        staged,
        library,
        TargetLocks::acquire(&[target])?,
    )
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
    let observed = observe(target, config, library);
    let mut plan = plan(config, library, &observed);
    plan_recovery(&mut plan, target, config);
    Ok(PreparedPlan {
        git_facts: capture_git_facts(target, &plan),
        plan,
        _locks: locks,
    })
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

fn plan_recovery(plan: &mut Plan, target: &Target, config: &RepositoryConfig) {
    for directory in config.skill_directories() {
        let root = target.root().join(directory.path().as_str());
        if ensure_contained_physical_parent(&root, target.root()).is_err()
            || !fs::symlink_metadata(&root)
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        {
            continue;
        }
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        let mut stages: std::collections::BTreeMap<PathBuf, Vec<PathBuf>> =
            std::collections::BTreeMap::new();
        let mut backups: std::collections::BTreeMap<PathBuf, Vec<PathBuf>> =
            std::collections::BTreeMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Some(destination) = artifact_destination(&root, &name, "backup") {
                backups.entry(destination).or_default().push(path);
            } else if let Some(destination) = artifact_destination(&root, &name, "stage") {
                stages.entry(destination).or_default().push(path);
            }
        }
        let destinations: std::collections::BTreeSet<_> =
            backups.keys().chain(stages.keys()).cloned().collect();
        for destination in destinations {
            let backup = backups.get(&destination).map(Vec::as_slice).unwrap_or(&[]);
            let staged = stages.get(&destination).map(Vec::as_slice).unwrap_or(&[]);
            let destination_absent = fs::symlink_metadata(&destination)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
            let safe = if backup.len() == 1
                && destination_absent
                && git_path_unprotected(target, &backup[0])
                && git_path_unprotected(target, &destination)
            {
                if let Some(item) = plan
                    .items
                    .iter_mut()
                    .find(|item| item.action == Action::Recover && item.path == backup[0])
                {
                    item.safety = Safety::Safe;
                    item.reason = "Restore the backup from the interrupted save".to_owned();
                    item.operation = Operation::Recover {
                        artifact: backup[0].clone(),
                        artifact_expected: fingerprint(&backup[0]),
                        destination: destination.clone(),
                        destination_expected: EntryFingerprint::Missing,
                    };
                }
                true
            } else {
                backup.is_empty()
            };
            if safe {
                for stage in staged {
                    if !git_path_unprotected(target, stage) {
                        continue;
                    }
                    if let Some(item) = plan
                        .items
                        .iter_mut()
                        .find(|item| item.action == Action::Recover && item.path == *stage)
                    {
                        item.safety = Safety::Safe;
                        item.reason =
                            "Remove temporary files left by an interrupted save".to_owned();
                        item.operation = Operation::Remove {
                            expected: fingerprint(stage),
                        };
                    }
                }
            }
        }
    }
}

fn git_path_unprotected(target: &Target, path: &Path) -> bool {
    target_path_facts(target, path)
        .ok()
        .is_some_and(|facts| !facts.tracked && !facts.staged && !facts.unmerged)
}

fn target_path_facts(target: &Target, path: &Path) -> Result<PathFacts, String> {
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

fn artifact_destination(root: &Path, name: &str, kind: &str) -> Option<PathBuf> {
    let suffix = name.strip_prefix(&format!(".skillator-{kind}-"))?;
    let mut parts = suffix.splitn(3, '-');
    parts.next()?.parse::<u32>().ok()?;
    parts.next()?.parse::<u64>().ok()?;
    let encoded = parts.next()?;
    if encoded.is_empty() || encoded.len() % 2 != 0 {
        return None;
    }
    let bytes = (0..encoded.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&encoded[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    #[cfg(unix)]
    let name = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(bytes)
    };
    #[cfg(not(unix))]
    let name = std::ffi::OsString::from(String::from_utf8(bytes).ok()?);
    let destination = root.join(name);
    (destination.parent() == Some(root)).then_some(destination)
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

fn apply_operation_with(
    item: &PlanItem,
    faults: &dyn FaultInjector,
    target_root: &Path,
) -> Result<(), ApplyFailure> {
    if !matches!(item.operation, Operation::None) {
        ensure_contained_physical_parent(&item.path, target_root)?;
    }
    match &item.operation {
        Operation::CreateDirectory => {
            fs::create_dir_all(&item.path).map_err(failed)?;
            match fs::symlink_metadata(&item.path) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
                _ => Err(ApplyFailure::Changed),
            }
        }
        Operation::WriteFile { bytes, expected } => {
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
            let stage = unique_artifact(parent, "stage", &item.path);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&stage)
                .map_err(failed)?;
            file.write_all(bytes).map_err(failed)?;
            file.sync_all().map_err(failed)?;
            publish_with_faults(&stage, &item.path, expected, faults)
        }
        Operation::Materialize {
            source,
            source_expected,
            kind,
            expected,
        } => {
            if &fingerprint(&item.path) != expected {
                return Err(ApplyFailure::Changed);
            }
            let canonical_source = source.canonicalize().map_err(failed)?;
            if &canonical_source != source {
                return Err(ApplyFailure::Changed);
            }
            let source = canonical_source;
            let expected_name = item.path.file_name().and_then(|name| name.to_str());
            if crate::library::validated_skill_name_at(&source).as_deref() != expected_name {
                return Err(ApplyFailure::Changed);
            }
            if *kind == MaterializationKind::Copied
                && crate::materialization::skill_fingerprint(&source) != *source_expected
            {
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
            let stage = unique_artifact(parent, "stage", &item.path);
            match kind {
                MaterializationKind::Linked => {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&source, &stage).map_err(failed)?;
                    #[cfg(not(unix))]
                    return Err(ApplyFailure::Failed(
                        "symbolic links require a Unix-compatible platform".to_owned(),
                    ));
                    if fs::read_link(&stage).map_err(failed)? != source {
                        let _ = remove_any(&stage);
                        return Err(ApplyFailure::Failed(
                            "The new link could not be verified".to_owned(),
                        ));
                    }
                }
                MaterializationKind::Copied => {
                    copy_tree(&source, &stage).map_err(|error| {
                        let _ = remove_any(&stage);
                        ApplyFailure::Failed(error.to_string())
                    })?;
                    let source_tree = TreeSnapshot::read(&source, true).map_err(|error| {
                        let _ = remove_any(&stage);
                        ApplyFailure::Failed(error.to_string())
                    })?;
                    let staged_tree = TreeSnapshot::read(&stage, false).map_err(|error| {
                        let _ = remove_any(&stage);
                        ApplyFailure::Failed(error.to_string())
                    })?;
                    if source_tree != staged_tree {
                        let _ = remove_any(&stage);
                        return Err(ApplyFailure::Changed);
                    }
                    if crate::materialization::skill_fingerprint(&source) != *source_expected {
                        let _ = remove_any(&stage);
                        return Err(ApplyFailure::Changed);
                    }
                }
            }
            publish_with_faults(&stage, &item.path, expected, faults)
        }
        Operation::Remove { expected } => {
            if &fingerprint(&item.path) != expected {
                return Err(ApplyFailure::Changed);
            }
            let parent = item
                .path
                .parent()
                .ok_or_else(|| ApplyFailure::Failed("destination has no parent".to_owned()))?;
            let backup = unique_artifact(parent, "backup", &item.path);
            rename_noreplace(&item.path, &backup).map_err(failed)?;
            if &fingerprint(&backup) != expected {
                return if rename_noreplace(&backup, &item.path).is_ok() {
                    Err(ApplyFailure::Changed)
                } else {
                    Err(ApplyFailure::RecoveryRequired(format!(
                        "The file changed before removal; a backup was kept at {}",
                        backup.display()
                    )))
                };
            }
            remove_any(&backup).map_err(|error| {
                if rename_noreplace(&backup, &item.path).is_ok() {
                    ApplyFailure::RolledBack(format!("Removal failed; the original content was restored: {error}"))
                } else {
                    ApplyFailure::RecoveryRequired(format!(
                        "Removal failed and the original content could not be restored; recover it from {}: {error}",
                        backup.display()
                    ))
                }
            })
        }
        Operation::Recover {
            artifact,
            artifact_expected,
            destination,
            destination_expected,
        } => {
            if &fingerprint(artifact) != artifact_expected
                || &fingerprint(destination) != destination_expected
            {
                return Err(ApplyFailure::Changed);
            }
            rename_noreplace(artifact, destination).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    ApplyFailure::Changed
                } else {
                    failed(error)
                }
            })
        }
        Operation::None => Ok(()),
    }
}

fn ensure_contained_physical_parent(path: &Path, target_root: &Path) -> Result<(), ApplyFailure> {
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

fn publish_with_faults(
    stage: &Path,
    destination: &Path,
    expected: &EntryFingerprint,
    faults: &dyn FaultInjector,
) -> Result<(), ApplyFailure> {
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
    let backup = unique_artifact(parent, "backup", destination);
    rename_noreplace(destination, &backup).map_err(failed)?;
    if &fingerprint(&backup) != expected {
        return if rename_noreplace(&backup, destination).is_ok() {
            let _ = remove_any(stage);
            Err(ApplyFailure::Changed)
        } else {
            Err(ApplyFailure::RecoveryRequired(format!(
                "destination changed after planning; preserved {}",
                backup.display()
            )))
        };
    }
    let installation = if faults.fail_publication(destination) {
        Err(std::io::Error::other("injected publication failure"))
    } else {
        rename_noreplace(stage, destination)
    };
    if let Err(error) = installation {
        let restored =
            !faults.fail_rollback(destination) && rename_noreplace(&backup, destination).is_ok();
        return if restored {
            let _ = remove_any(stage);
            Err(ApplyFailure::RolledBack(format!(
                "Saving failed; the original content was restored: {error}"
            )))
        } else {
            Err(ApplyFailure::RecoveryRequired(format!(
                "Saving failed and the original content could not be restored; recover it from {}: {error}",
                backup.display()
            )))
        };
    }
    if faults.fail_backup_deletion(destination) {
        return Err(ApplyFailure::RecoveryRequired(format!(
            "new content is installed but backup remains at {}: injected backup deletion failure",
            backup.display()
        )));
    }
    remove_any(&backup).map_err(|error| {
        ApplyFailure::RecoveryRequired(format!(
            "new content is installed but backup remains at {}: {error}",
            backup.display()
        ))
    })
}

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
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("skill");
        let stage = root.path().join("stage");
        fs::write(&stage, "new").unwrap();

        let result = publish_with_faults(
            &stage,
            &destination,
            &EntryFingerprint::Missing,
            &ConcurrentWrite { bytes: b"external" },
        );

        std::assert_matches!(result, Err(ApplyFailure::Changed));
        assert_eq!(fs::read(&destination).unwrap(), b"external");
    }

    #[test]
    fn publication_restores_a_destination_changed_during_replacement() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("skill");
        let stage = root.path().join("stage");
        fs::write(&destination, "old").unwrap();
        fs::write(&stage, "new").unwrap();

        let result = publish_with_faults(
            &stage,
            &destination,
            &EntryFingerprint::File(b"old".to_vec()),
            &ConcurrentWrite { bytes: b"external" },
        );

        std::assert_matches!(result, Err(ApplyFailure::Changed));
        assert_eq!(fs::read(&destination).unwrap(), b"external");
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
    }

    #[test]
    fn publication_failure_rolls_back_or_retains_recovery_artifact() {
        let root = tempfile::tempdir().unwrap();
        let rolled_back = replacement(root.path(), "rolled-back");
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

        let recovery = replacement(root.path(), "recovery");
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
        assert!(fs::read_dir(root.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".skillator-backup-")
        }));
    }

    #[test]
    fn backup_deletion_failure_keeps_new_content_and_backup() {
        let root = tempfile::tempdir().unwrap();
        let item = replacement(root.path(), "destination");
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
        assert!(fs::read_dir(root.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".skillator-backup-")
        }));
    }
}

fn unique_artifact(parent: &Path, kind: &str, destination: &Path) -> PathBuf {
    let sequence = OPERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("entry");
    let encoded: String = name
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    parent.join(format!(
        ".skillator-{kind}-{}-{sequence}-{encoded}",
        std::process::id()
    ))
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
