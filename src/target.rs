//! Target selection and immutable observation.

use crate::config::{RepositoryConfig, SkillDirectoryConfig};
use crate::domain::{Enablement, MaterializationKind};
use crate::git::{GitError, GitRepository, PathFacts};
use crate::library::LibrarySnapshot;
use crate::materialization::{EntryFingerprint, TreeSnapshot, fingerprint};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum TargetError {
    #[error("Target does not exist: {0}")]
    Missing(PathBuf),
    #[error("Target is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("Target is not in a Git worktree: {0}")]
    NotGit(PathBuf),
    #[error("Target is a bare Git repository: {0}")]
    Bare(PathBuf),
    #[error("cannot inspect Target: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Git(#[from] GitError),
}

#[derive(Debug, Clone)]
pub struct Target {
    supplied_path: PathBuf,
    root: PathBuf,
    repository: Option<GitRepository>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetScope {
    Repository,
    User,
}

pub const CONTROL_FILE_CONTENT: &str = "# Managed by skillator.\n*\n!.gitignore\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootState {
    Absent,
    Directory,
    Inaccessible,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    InSync,
    Drifted,
    Unverifiable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterializationState {
    Missing,
    CanonicalLink,
    NoncanonicalLink,
    BrokenLink,
    MisdirectedLink,
    EquivalentCopy,
    DivergedCopy,
    CopyIneligible,
    WrongKind,
    Uninspectable,
    ExpectedEntryCollision,
    UnknownExpectedEntry,
}

#[derive(Debug, Clone)]
pub struct EnablementObservation {
    enablement: Enablement,
    expected_entry: Option<String>,
    comparison: Comparison,
    state: MaterializationState,
    unresolved: bool,
    path: Option<PathBuf>,
    tracked: bool,
    fingerprint: EntryFingerprint,
    overlap_advisory: bool,
}

impl EnablementObservation {
    pub fn enablement(&self) -> &Enablement {
        &self.enablement
    }

    pub fn expected_entry(&self) -> Option<&str> {
        self.expected_entry.as_deref()
    }

    pub fn comparison(&self) -> Comparison {
        self.comparison
    }

    pub fn state(&self) -> &MaterializationState {
        &self.state
    }

    pub fn unresolved(&self) -> bool {
        self.unresolved
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn tracked(&self) -> bool {
        self.tracked
    }

    pub fn overlap_advisory(&self) -> bool {
        self.overlap_advisory
    }

    pub(crate) fn fingerprint(&self) -> &EntryFingerprint {
        &self.fingerprint
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UnmanagedObservation {
    pub path: PathBuf,
    pub tracked: bool,
    pub fingerprint: EntryFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlFileState {
    NotRequired,
    Missing,
    Canonical,
    Modified,
    WrongKind,
    Uninspectable,
}

#[derive(Debug, Clone)]
pub struct DirectoryObservation {
    key: String,
    path: PathBuf,
    root_state: RootState,
    comparison: Comparison,
    control_file: ControlFileState,
    control_tracked: bool,
    control_protected: bool,
    control_ignored: bool,
    generated_ignored: bool,
    unmanaged_entries: Vec<PathBuf>,
    unmanaged: Vec<UnmanagedObservation>,
    duplicate_entries: Vec<PathBuf>,
    compatible_agents: Vec<&'static str>,
    recovery_artifacts: Vec<PathBuf>,
    diagnostics: Vec<String>,
}

impl DirectoryObservation {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn root_state(&self) -> RootState {
        self.root_state
    }

    pub fn comparison(&self) -> Comparison {
        self.comparison
    }

    pub fn control_file(&self) -> &ControlFileState {
        &self.control_file
    }

    pub fn control_tracked(&self) -> bool {
        self.control_tracked
    }

    pub(crate) fn control_protected(&self) -> bool {
        self.control_protected
    }

    pub fn control_ignored(&self) -> bool {
        self.control_ignored
    }

    pub fn generated_ignored(&self) -> bool {
        self.generated_ignored
    }

    pub fn unmanaged_entries(&self) -> &[PathBuf] {
        &self.unmanaged_entries
    }

    pub(crate) fn unmanaged(&self) -> &[UnmanagedObservation] {
        &self.unmanaged
    }

    pub fn recovery_artifacts(&self) -> &[PathBuf] {
        &self.recovery_artifacts
    }

    pub fn duplicate_entries(&self) -> &[PathBuf] {
        &self.duplicate_entries
    }

    pub fn compatible_agents(&self) -> &[&'static str] {
        &self.compatible_agents
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}

#[derive(Debug, Clone)]
pub struct ObservedState {
    target_root: PathBuf,
    directories: Vec<DirectoryObservation>,
    enablements: Vec<EnablementObservation>,
}

impl ObservedState {
    pub fn target_root(&self) -> &Path {
        &self.target_root
    }

    pub fn directories(&self) -> &[DirectoryObservation] {
        &self.directories
    }

    pub fn enablements(&self) -> impl ExactSizeIterator<Item = &EnablementObservation> {
        self.enablements.iter()
    }
}

impl Target {
    pub fn select(path: impl AsRef<Path>) -> Result<Self, TargetError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(TargetError::Missing(path.to_owned()));
        }
        if !path.is_dir() {
            return Err(TargetError::NotDirectory(path.to_owned()));
        }
        let supplied_path = path.canonicalize()?;
        let repository = match GitRepository::discover(&supplied_path) {
            Ok(repository) => repository,
            Err(GitError::Bare) => return Err(TargetError::Bare(supplied_path)),
            Err(GitError::NotWorktree { .. }) => return Err(TargetError::NotGit(supplied_path)),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            supplied_path,
            root: repository.root().to_owned(),
            repository: Some(repository),
        })
    }

    pub fn user(home: impl AsRef<Path>) -> Result<Self, TargetError> {
        let home = home.as_ref();
        if !home.exists() {
            return Err(TargetError::Missing(home.to_owned()));
        }
        if !home.is_dir() {
            return Err(TargetError::NotDirectory(home.to_owned()));
        }
        let root = home.canonicalize()?;
        Ok(Self {
            supplied_path: root.clone(),
            root,
            repository: None,
        })
    }

    pub fn supplied_path(&self) -> &Path {
        &self.supplied_path
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn repository(&self) -> &GitRepository {
        self.repository
            .as_ref()
            .expect("Repository Target has Git facts")
    }

    pub fn git_repository(&self) -> Option<&GitRepository> {
        self.repository.as_ref()
    }

    pub fn scope(&self) -> TargetScope {
        if self.repository.is_some() {
            TargetScope::Repository
        } else {
            TargetScope::User
        }
    }

    pub fn lock_path(&self) -> &Path {
        self.repository
            .as_ref()
            .map_or(self.root(), |repository| repository.git_dir())
    }
}

pub fn observe(
    target: &Target,
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
) -> ObservedState {
    let mut enablements = Vec::new();
    let mut directories = Vec::new();
    let mut observed_git_facts = BTreeMap::<PathBuf, PathFacts>::new();
    let mut expected_by_directory: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut expected_names = Vec::new();
    for enablement in config.enablements() {
        let resolved = library.resolve(enablement.skill());
        let expected = if enablement.skill().path().as_str() == "." {
            resolved.and_then(|skill| skill.name()).map(str::to_owned)
        } else {
            enablement
                .skill()
                .path()
                .as_str()
                .rsplit('/')
                .next()
                .map(str::to_owned)
        };
        if let Some(name) = &expected {
            *expected_by_directory
                .entry(enablement.directory().as_str().to_owned())
                .or_default()
                .entry(name.clone())
                .or_default() += 1;
        }
        expected_names.push(expected);
    }

    for directory in config.skill_directories() {
        let root = target.root().join(directory.path().as_str());
        let root_state = if has_symlink_ancestor(target.root(), &root) {
            RootState::Symlink
        } else {
            inspect_root(&root)
        };
        let children = if root_state == RootState::Directory {
            read_children_stable(&root)
        } else {
            Ok(Vec::new())
        };
        let scan_unstable = children.is_err();
        let mut diagnostics = Vec::new();
        let children = match children {
            Ok(children) => children,
            Err(message) => {
                diagnostics.push(message);
                Vec::new()
            }
        };
        let expected: BTreeSet<_> = expected_by_directory
            .get(directory.key().as_str())
            .into_iter()
            .flat_map(|names| names.keys().cloned())
            .collect();
        let control_relative = PathBuf::from(directory.path().as_str()).join(".gitignore");
        let probe_relative = PathBuf::from(directory.path().as_str()).join(".skillator-probe");
        let mut fact_paths = children
            .iter()
            .filter_map(|child| child.strip_prefix(target.root()).ok().map(Path::to_owned))
            .collect::<Vec<_>>();
        fact_paths.extend(
            expected
                .iter()
                .map(|name| PathBuf::from(directory.path().as_str()).join(name)),
        );
        fact_paths.push(control_relative.clone());
        fact_paths.push(probe_relative.clone());
        fact_paths.sort();
        fact_paths.dedup();
        let git_facts = target
            .git_repository()
            .map(|repository| repository.facts_for_many(&fact_paths));
        if let Some(Ok(facts)) = &git_facts {
            observed_git_facts.extend(facts.clone());
        }
        let mut unmanaged_entries = Vec::new();
        let mut unmanaged = Vec::new();
        let mut duplicate_entries = Vec::new();
        let mut recovery_artifacts = Vec::new();
        for child in &children {
            let name = child
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if target.scope() == TargetScope::Repository && name == ".gitignore" {
                continue;
            }
            if name.starts_with(".skillator-") {
                recovery_artifacts.push(child.clone());
            } else if !expected.contains(name) {
                unmanaged_entries.push(child.clone());
                let relative = child.strip_prefix(target.root()).unwrap_or(child);
                let protected = git_facts
                    .as_ref()
                    .and_then(|facts| facts.as_ref().ok())
                    .and_then(|facts| facts.get(relative))
                    .map_or(target.git_repository().is_some(), |facts| {
                        facts.tracked || facts.staged || facts.unmerged
                    });
                unmanaged.push(UnmanagedObservation {
                    path: child.clone(),
                    tracked: protected,
                    fingerprint: fingerprint(child),
                });
                if expected
                    .iter()
                    .any(|expected| expected.eq_ignore_ascii_case(name))
                {
                    diagnostics.push(format!(
                        "entry `{name}` differs from an Expected Entry only by case"
                    ));
                }
                if fs::symlink_metadata(child)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                    && fs::canonicalize(child).is_ok_and(|resolved| {
                        library.sources().any(|source| {
                            source.skills().any(|skill| {
                                skill.absolute_path().is_some_and(|path| path == resolved)
                            })
                        })
                    })
                {
                    duplicate_entries.push(child.clone());
                    diagnostics.push(format!(
                        "possible Duplicate Materialization at {}",
                        child.display()
                    ));
                }
            }
        }
        let control_facts = git_facts
            .as_ref()
            .and_then(|facts| facts.as_ref().ok())
            .and_then(|facts| facts.get(&control_relative));
        let (control_file, control_tracked, control_protected, control_ignored) =
            inspect_control_file(target, directory, root_state, control_facts);
        let generated_ignored = target.git_repository().is_none_or(|_| {
            git_facts
                .as_ref()
                .and_then(|facts| facts.as_ref().ok())
                .and_then(|facts| facts.get(&probe_relative))
                .is_some_and(|facts| facts.ignored)
        });
        match control_file {
            ControlFileState::Missing if root_state == RootState::Absent => {}
            ControlFileState::Missing => diagnostics.push(format!(
                "Skill Directory Control File is missing at {}; save to create it",
                control_relative.display()
            )),
            ControlFileState::Modified => diagnostics.push(format!(
                "Skill Directory Control File differs from Skillator's required content at {}; save to review replacement",
                control_relative.display()
            )),
            ControlFileState::WrongKind => diagnostics.push(format!(
                "Skill Directory Control File path is not a regular file: {}",
                control_relative.display()
            )),
            ControlFileState::Uninspectable => diagnostics.push(format!(
                "Skill Directory Control File cannot be read: {}",
                control_relative.display()
            )),
            ControlFileState::Canonical | ControlFileState::NotRequired => {}
        }
        if control_file == ControlFileState::Canonical && !control_tracked {
            diagnostics.push(format!(
                "track the control file with `git add {}-- {}/.gitignore`",
                if control_ignored { "-f " } else { "" },
                directory.path()
            ));
        }
        if control_file == ControlFileState::Canonical && !generated_ignored {
            diagnostics.push("generated entries are not effectively Git-ignored".to_owned());
        }
        let compatible_agents = compatibility(directory.path().as_str());
        for other in config.skill_directories() {
            if other.key() == directory.key() {
                continue;
            }
            let overlap = compatibility(other.path().as_str())
                .iter()
                .any(|agent| compatible_agents.contains(agent));
            let shared_skill = config.enablements().iter().any(|left| {
                left.directory() == directory.key()
                    && config.enablements().iter().any(|right| {
                        right.directory() == other.key() && right.skill() == left.skill()
                    })
            });
            if overlap && shared_skill {
                diagnostics.push(format!(
                    "agent compatibility overlaps with Skill Directory `{}`",
                    other.key()
                ));
            }
        }
        let comparison = aggregate_directory_comparison(
            root_state,
            scan_unstable,
            root_state != RootState::Directory
                || !unmanaged_entries.is_empty()
                || !recovery_artifacts.is_empty()
                || (target.scope() == TargetScope::Repository
                    && (control_file != ControlFileState::Canonical
                        || !control_tracked
                        || control_ignored
                        || !generated_ignored)),
        );
        directories.push(DirectoryObservation {
            key: directory.key().as_str().to_owned(),
            path: root,
            root_state,
            comparison,
            control_file,
            control_tracked,
            control_protected,
            control_ignored,
            generated_ignored,
            unmanaged_entries,
            unmanaged,
            duplicate_entries,
            compatible_agents,
            recovery_artifacts,
            diagnostics,
        });
    }

    for (index, enablement) in config.enablements().iter().enumerate() {
        let directory = config
            .skill_directories()
            .iter()
            .find(|directory| directory.key() == enablement.directory())
            .expect("validated relationship");
        let root = target.root().join(directory.path().as_str());
        let resolved = library.resolve(enablement.skill());
        let expected = expected_names[index].clone();
        let collision = expected.as_ref().is_some_and(|name| {
            expected_by_directory
                .get(enablement.directory().as_str())
                .and_then(|names| names.get(name))
                .is_some_and(|count| *count > 1)
        });
        let (comparison, state, path) = if collision {
            (
                Comparison::Drifted,
                MaterializationState::ExpectedEntryCollision,
                expected.as_ref().map(|name| root.join(name)),
            )
        } else if let Some(expected) = &expected {
            let path = root.join(expected);
            let (comparison, state) =
                inspect_materialization(&path, enablement, resolved, target.scope());
            (comparison, state, Some(path))
        } else {
            (
                Comparison::Unverifiable,
                MaterializationState::UnknownExpectedEntry,
                None,
            )
        };
        let fingerprint = path
            .as_deref()
            .map(fingerprint)
            .unwrap_or(EntryFingerprint::Missing);
        let tracked = path
            .as_deref()
            .and_then(|path| path.strip_prefix(target.root()).ok())
            .map(|relative| {
                observed_git_facts
                    .get(relative)
                    .map_or(target.git_repository().is_some(), |facts| {
                        facts.tracked || facts.staged || facts.unmerged
                    })
            })
            .unwrap_or(true);
        enablements.push(EnablementObservation {
            enablement: enablement.clone(),
            expected_entry: expected,
            comparison,
            state,
            unresolved: resolved.is_none(),
            path,
            tracked,
            fingerprint,
            overlap_advisory: library.has_overlap_advisory(enablement.skill()),
        });
    }

    for directory in &mut directories {
        let comparisons: Vec<_> = enablements
            .iter()
            .filter(|observation| observation.enablement.directory().as_str() == directory.key)
            .map(|observation| observation.comparison)
            .collect();
        if comparisons.contains(&Comparison::Drifted) {
            directory.comparison = Comparison::Drifted;
        } else if directory.comparison == Comparison::InSync
            && comparisons.contains(&Comparison::Unverifiable)
        {
            directory.comparison = Comparison::Unverifiable;
        }
    }

    ObservedState {
        target_root: target.root().to_owned(),
        directories,
        enablements,
    }
}

fn compatibility(path: &str) -> Vec<&'static str> {
    match path.trim_end_matches('/') {
        ".agents/skills" => vec!["Codex", "GitHub Copilot", "Cursor", "Gemini CLI"],
        ".claude/skills" => vec!["Claude Code", "GitHub Copilot", "Cursor"],
        ".github/skills" => vec!["GitHub Copilot"],
        ".cursor/skills" => vec!["Cursor"],
        ".gemini/skills" => vec!["Gemini CLI"],
        _ => Vec::new(),
    }
}

fn aggregate_directory_comparison(
    root_state: RootState,
    scan_unstable: bool,
    drifted: bool,
) -> Comparison {
    if root_state == RootState::Inaccessible || scan_unstable {
        Comparison::Unverifiable
    } else if drifted {
        Comparison::Drifted
    } else {
        Comparison::InSync
    }
}

fn inspect_root(path: &Path) -> RootState {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => RootState::Symlink,
        Ok(metadata) if metadata.is_dir() => match fs::read_dir(path) {
            Ok(_) => RootState::Directory,
            Err(_) => RootState::Inaccessible,
        },
        Ok(_) => RootState::Other,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => RootState::Absent,
        Err(_) => RootState::Inaccessible,
    }
}

fn has_symlink_ancestor(target_root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(target_root) else {
        return true;
    };
    let mut current = target_root.to_owned();
    for component in relative.components() {
        current.push(component.as_os_str());
        if current == path {
            break;
        }
        if fs::symlink_metadata(&current).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return true;
        }
    }
    false
}

fn read_children_stable(root: &Path) -> Result<Vec<PathBuf>, String> {
    fn names(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
        let mut entries = fs::read_dir(root)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort();
        Ok(entries)
    }
    read_children_stable_with(|| names(root))
}

fn read_children_stable_with(
    mut scan: impl FnMut() -> Result<Vec<PathBuf>, std::io::Error>,
) -> Result<Vec<PathBuf>, String> {
    let first = scan().map_err(|error| format!("cannot inspect directory: {error}"))?;
    let second = scan().map_err(|error| format!("cannot verify directory scan: {error}"))?;
    if first == second {
        Ok(first)
    } else {
        Err("Skill Directory changed while it was inspected".to_owned())
    }
}

fn inspect_control_file(
    target: &Target,
    directory: &SkillDirectoryConfig,
    root_state: RootState,
    facts: Option<&PathFacts>,
) -> (ControlFileState, bool, bool, bool) {
    if target.git_repository().is_none() {
        return (ControlFileState::NotRequired, true, false, false);
    }
    let relative = PathBuf::from(directory.path().as_str()).join(".gitignore");
    if root_state != RootState::Directory {
        return (
            ControlFileState::Missing,
            facts.is_none_or(|facts| facts.tracked),
            facts.is_none_or(|facts| facts.tracked || facts.staged || facts.unmerged),
            facts.is_none_or(|facts| facts.ignored),
        );
    }
    let path = target.root().join(&relative);
    let state = match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_file() => ControlFileState::WrongKind,
        Ok(_) => match fs::read(&path) {
            Ok(bytes) if bytes == CONTROL_FILE_CONTENT.as_bytes() => ControlFileState::Canonical,
            Ok(_) => ControlFileState::Modified,
            Err(_) => ControlFileState::Uninspectable,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ControlFileState::Missing,
        Err(_) => ControlFileState::Uninspectable,
    };
    (
        state,
        facts.is_none_or(|facts| facts.tracked),
        facts.is_none_or(|facts| facts.tracked || facts.staged || facts.unmerged),
        facts.is_none_or(|facts| facts.ignored),
    )
}

fn inspect_materialization(
    path: &Path,
    enablement: &Enablement,
    resolved: Option<&crate::library::LibrarySkill>,
    scope: TargetScope,
) -> (Comparison, MaterializationState) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if enablement.materialization() == MaterializationKind::Copied
                && let Some(source) = resolved.and_then(|skill| skill.absolute_path())
                && TreeSnapshot::read(source, true).is_err()
            {
                return (Comparison::Drifted, MaterializationState::CopyIneligible);
            }
            return (Comparison::Drifted, MaterializationState::Missing);
        }
        Err(_) => {
            return (
                Comparison::Unverifiable,
                MaterializationState::Uninspectable,
            );
        }
    };
    match enablement.materialization() {
        MaterializationKind::Linked => {
            if !metadata.file_type().is_symlink() {
                if metadata.is_dir()
                    && let Some(source) = resolved.and_then(|skill| skill.absolute_path())
                    && let (Ok(source_tree), Ok(destination_tree)) = (
                        TreeSnapshot::read(source, true),
                        TreeSnapshot::read(path, false),
                    )
                    && source_tree == destination_tree
                {
                    return (Comparison::Drifted, MaterializationState::EquivalentCopy);
                }
                return (Comparison::Drifted, MaterializationState::WrongKind);
            }
            let Ok(stored) = fs::read_link(path) else {
                return (
                    Comparison::Unverifiable,
                    MaterializationState::Uninspectable,
                );
            };
            let Some(source) = resolved.and_then(|skill| skill.absolute_path()) else {
                return (
                    Comparison::Unverifiable,
                    MaterializationState::Uninspectable,
                );
            };
            if stored == source {
                return (Comparison::InSync, MaterializationState::CanonicalLink);
            }
            let resolved_link = if stored.is_absolute() {
                stored.canonicalize()
            } else {
                path.parent()
                    .unwrap_or(Path::new("."))
                    .join(stored)
                    .canonicalize()
            };
            match resolved_link {
                Ok(candidate) if candidate == source && scope == TargetScope::User => {
                    (Comparison::InSync, MaterializationState::NoncanonicalLink)
                }
                Ok(candidate) if candidate == source => {
                    (Comparison::Drifted, MaterializationState::NoncanonicalLink)
                }
                Ok(_) => (Comparison::Drifted, MaterializationState::MisdirectedLink),
                Err(_) => (Comparison::Drifted, MaterializationState::BrokenLink),
            }
        }
        MaterializationKind::Copied => {
            if metadata.file_type().is_symlink()
                && let Some(source) = resolved.and_then(|skill| skill.absolute_path())
                && fs::read_link(path).is_ok_and(|target| target == source)
            {
                return (Comparison::Drifted, MaterializationState::CanonicalLink);
            }
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return (Comparison::Drifted, MaterializationState::WrongKind);
            }
            let Some(source) = resolved.and_then(|skill| skill.absolute_path()) else {
                return (
                    Comparison::Unverifiable,
                    MaterializationState::Uninspectable,
                );
            };
            let source_tree = match TreeSnapshot::read(source, true) {
                Ok(tree) => tree,
                Err(_) => return (Comparison::Drifted, MaterializationState::CopyIneligible),
            };
            let destination_tree = match TreeSnapshot::read(path, false) {
                Ok(tree) => tree,
                Err(_) => {
                    return (
                        Comparison::Unverifiable,
                        MaterializationState::Uninspectable,
                    );
                }
            };
            if source_tree == destination_tree {
                (Comparison::InSync, MaterializationState::EquivalentCopy)
            } else {
                (Comparison::Drifted, MaterializationState::DivergedCopy)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_immediate_child_scan_is_unstable() {
        let mut scans = vec![vec![PathBuf::from("one")], vec![PathBuf::from("two")]].into_iter();
        let result = read_children_stable_with(|| Ok(scans.next().expect("two scans")));
        assert_eq!(
            result.unwrap_err(),
            "Skill Directory changed while it was inspected"
        );
    }

    #[test]
    fn stable_immediate_child_scan_returns_snapshot() {
        let expected = vec![PathBuf::from("one"), PathBuf::from("two")];
        let result = read_children_stable_with(|| Ok(expected.clone())).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn unstable_scan_makes_directory_unverifiable_even_without_other_drift() {
        assert_eq!(
            aggregate_directory_comparison(RootState::Directory, true, false),
            Comparison::Unverifiable
        );
    }
}
