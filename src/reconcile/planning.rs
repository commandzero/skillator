//! Pure generation of materialization and control-file plan items.
use super::*;
use crate::target::{DirectoryObservation, EnablementObservation};

pub fn plan(
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
    observed: &ObservedState,
) -> Plan {
    let _ = config;
    Planner {
        library,
        observed,
        items: Vec::new(),
        blocked_directories: Default::default(),
        planned_control_paths: Default::default(),
    }
    .plan()
}
struct Planner<'a> {
    library: &'a LibrarySnapshot,
    observed: &'a ObservedState,
    items: Vec<PlanItem>,
    blocked_directories: std::collections::BTreeSet<String>,
    planned_control_paths: std::collections::BTreeSet<PathBuf>,
}
impl Planner<'_> {
    fn plan(mut self) -> Plan {
        for directory in self.observed.directories() {
            self.plan_directory(directory);
        }
        for observation in self.observed.enablements() {
            self.plan_enablement(observation);
        }
        self.items.sort_by_key(|item| {
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
        Plan { items: self.items }
    }
    fn plan_root(&mut self, directory: &DirectoryObservation) -> bool {
        match directory.root_state() {
            RootState::Absent => self.items.push(PlanItem {
                path: directory.path().to_owned(),
                action: Action::CreateDirectory,
                safety: Safety::Safe,
                reason: "skill folder is missing".to_owned(),
                operation: Operation::CreateDirectory,
            }),
            RootState::Directory => {}
            RootState::Inaccessible => {
                self.blocked_directories.insert(directory.key().to_owned());
                self.items.push(blocked(
                    directory.path(),
                    Action::CreateDirectory,
                    "Cannot access the skill folder; check its permissions",
                ));
                return false;
            }
            RootState::Symlink | RootState::Other => {
                self.blocked_directories.insert(directory.key().to_owned());
                self.items.push(blocked(
                    directory.path(),
                    Action::CreateDirectory,
                    "The skill folder must be a directory, not a link or file",
                ));
                return false;
            }
        }
        true
    }
    fn plan_directory(&mut self, directory: &DirectoryObservation) {
        if !self.plan_root(directory) {
            return;
        }
        let control = directory.control_path();
        if self.planned_control_paths.insert(control.to_owned()) {
            self.plan_control(directory);
        }
        for recovery in directory.recovery_artifacts() {
            self.items.push(blocked(
                recovery,
                Action::Recover,
                "Files remain from an interrupted save; review them before continuing",
            ));
        }
    }
    fn plan_control(&mut self, directory: &DirectoryObservation) {
        let control = directory.control_path();
        match directory.control_file() {
            ControlFileState::NotRequired => {}
            ControlFileState::Missing => self.plan_missing_control(directory),
            ControlFileState::Canonical => self.check_canonical_control(directory),
            ControlFileState::PrefixRequired => self.plan_control_prefix(directory),
            ControlFileState::Modified | ControlFileState::WrongKind => {
                self.plan_control_replacement(directory)
            }
            ControlFileState::Uninspectable => self.items.push(blocked(
                control,
                Action::WriteControlFile,
                "Cannot read .gitignore; check its type and permissions",
            )),
        }
    }
    fn plan_missing_control(&mut self, directory: &DirectoryObservation) {
        let control = directory.control_path();

        if directory.control_protected() {
            self.items.push(blocked(
                            control,
                            Action::WriteControlFile,
                            ".gitignore is tracked, staged, or has merge conflicts; resolve its Git status before replacing it",
                        ));
        } else {
            self.items.push(PlanItem {
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
    fn check_canonical_control(&mut self, directory: &DirectoryObservation) {
        let control = directory.control_path();

        if directory.control_protected() {
            self.items.push(blocked(
                            control,
                            Action::WriteControlFile,
                            ".gitignore is tracked, staged, or has merge conflicts; resolve its Git status before replacing it",
                        ));
        } else if !directory.control_ignored() {
            self.items.push(blocked(
                control,
                Action::WriteControlFile,
                ".gitignore is not ignored by Git; check the repository's ignore rules",
            ));
        } else if !directory.generated_ignored() {
            self.items.push(blocked(
                control,
                Action::WriteControlFile,
                "Generated skill files are not ignored by Git; check the repository's ignore rules",
            ));
        }
    }
    fn plan_control_prefix(&mut self, directory: &DirectoryObservation) {
        let control = directory.control_path();

        let expected = fingerprint(control);
        if expected == EntryFingerprint::Uninspectable || directory.control_protected() {
            self.items.push(blocked(
                control,
                Action::WriteControlFile,
                "Cannot update .gitignore; check its permissions and whether Git tracks it",
            ));
        } else {
            self.items.push(PlanItem {
                path: control.to_owned(),
                action: Action::WriteControlFile,
                safety: Safety::Safe,
                reason: "Add Skillator's ignore rules and keep existing rules".to_owned(),
                operation: Operation::WriteFile {
                    bytes: directory.control_content().to_vec(),
                    expected,
                },
            });
        }
    }
    fn plan_control_replacement(&mut self, directory: &DirectoryObservation) {
        let control = directory.control_path();

        let expected = fingerprint(control);
        let safety = if expected == EntryFingerprint::Uninspectable || directory.control_protected()
        {
            Safety::Blocked
        } else {
            Safety::Guarded
        };
        self.items.push(PlanItem {
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
    fn plan_enablement(&mut self, observation: &EnablementObservation) {
        if self
            .blocked_directories
            .contains(observation.enablement().directory().as_str())
        {
            self.items.push(blocked(
                observation.path().unwrap_or(self.observed.target_root()),
                materialize_action(observation.enablement().materialization()),
                "The skill folder is inaccessible or points outside the selected directory",
            ));
            return;
        }
        if observation.comparison() == Comparison::InSync {
            return;
        }
        let Some(path) = observation.path() else {
            self.items.push(blocked(
                self.observed.target_root(),
                materialize_action(observation.enablement().materialization()),
                "Cannot determine the skill's destination name",
            ));
            return;
        };
        let resolved = self.library.resolve(observation.enablement().skill());
        let source = resolved.and_then(|skill| skill.absolute_path());
        let action = match observation.state() {
            MaterializationState::Missing => {
                materialize_action(observation.enablement().materialization())
            }
            _ => Action::Replace,
        };
        let safety =
            classify_materialization(observation.state(), observation.tracked(), source.is_some());
        self.items.push(PlanItem {
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
                    metadata_name: resolved
                        .expect("checked above")
                        .name()
                        .expect("valid skill has a name")
                        .to_owned(),
                    source_expected: resolved.expect("checked above").fingerprint().clone(),
                    kind: observation.enablement().materialization(),
                    expected: observation.fingerprint().clone(),
                }
            },
        });
    }
}
pub(super) fn classify_materialization(
    state: &MaterializationState,
    tracked: bool,
    resolved: bool,
) -> Safety {
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

pub(super) fn blocked(path: &Path, action: Action, reason: &str) -> PlanItem {
    PlanItem {
        path: path.to_owned(),
        action,
        safety: Safety::Blocked,
        reason: reason.to_owned(),
        operation: Operation::None,
    }
}

pub(super) fn materialize_action(kind: MaterializationKind) -> Action {
    match kind {
        MaterializationKind::Linked => Action::Link,
        MaterializationKind::Copied => Action::Copy,
    }
}
