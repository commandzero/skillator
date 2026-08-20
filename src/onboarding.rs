//! First-run Library and User Scope onboarding.

use crate::acquisition::LibraryAcquisitionMode;
use crate::app::{AppPaths, WorkflowError};
use crate::config::{
    Fingerprint, LibraryConfig, LibraryConfigCodec, LibraryLocationConfig, LoadResult,
    RegisteredSkillConfig, RegisteredSourceConfig, RepositoryConfig, RepositoryConfigCodec,
    SkillDirectoryConfig, load_library, load_repository, save_library, save_repository,
};
use crate::domain::{Enablement, MaterializationKind, SkillKey, SkillPath, SourceKey};
use crate::fs_safety::rename_noreplace;
use crate::git::GitRepository;
use crate::library::{
    expand_location, suggested_source_key, validated_skill_metadata_at, validated_skill_name_at,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingEntryKind {
    Physical,
    Symlink,
    Invalid,
}

#[derive(Debug, Clone)]
pub struct OnboardingEntry {
    name: String,
    path: PathBuf,
    kind: OnboardingEntryKind,
    selectable: bool,
    selected_by_default: bool,
    detail: String,
}

impl OnboardingEntry {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn kind(&self) -> OnboardingEntryKind {
        self.kind
    }

    pub fn selectable(&self) -> bool {
        self.selectable
    }

    pub fn selected_by_default(&self) -> bool {
        self.selected_by_default
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

#[derive(Debug, Clone)]
pub struct OnboardingSession {
    entries: Vec<OnboardingEntry>,
    library_expected: Fingerprint,
    user_expected: Fingerprint,
    prior_user_bytes: Option<Vec<u8>>,
    original_user: RepositoryConfig,
}

impl OnboardingSession {
    pub fn entries(&self) -> &[OnboardingEntry] {
        &self.entries
    }

    pub fn default_location_expression(&self) -> &str {
        "./library"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingReviewItem {
    pub action: String,
    pub source: Option<PathBuf>,
    pub destination: PathBuf,
}

#[derive(Debug)]
pub struct PreparedOnboarding {
    home: PathBuf,
    library_config_path: PathBuf,
    user_config_path: PathBuf,
    library_root: PathBuf,
    library_config: LibraryConfig,
    user_config: RepositoryConfig,
    library_expected: Fingerprint,
    user_expected: Fingerprint,
    prior_user_bytes: Option<Vec<u8>>,
    imports: Vec<Import>,
    review: Vec<OnboardingReviewItem>,
}

impl PreparedOnboarding {
    pub fn review(&self) -> &[OnboardingReviewItem] {
        &self.review
    }

    pub fn library_config(&self) -> &LibraryConfig {
        &self.library_config
    }

    pub fn user_config(&self) -> &RepositoryConfig {
        &self.user_config
    }
}

#[derive(Debug, Clone)]
struct Import {
    source: PathBuf,
    destination: PathBuf,
    name: String,
    mode: LibraryAcquisitionMode,
}

#[derive(Debug, thiserror::Error)]
pub enum OnboardingError {
    #[error("{0}")]
    Invalid(String),
    #[error("onboarding failed: {0}")]
    Failed(String),
    #[error("onboarding rollback requires manual recovery: {0}")]
    RecoveryRequired(String),
}

impl From<OnboardingError> for WorkflowError {
    fn from(error: OnboardingError) -> Self {
        match error {
            OnboardingError::Invalid(message) => WorkflowError::InvalidInput { message },
            OnboardingError::Failed(message) | OnboardingError::RecoveryRequired(message) => {
                WorkflowError::Fatal { message }
            }
        }
    }
}

pub struct OnboardingWorkflow;

impl OnboardingWorkflow {
    pub fn load(paths: &AppPaths) -> Result<OnboardingSession, OnboardingError> {
        let library_expected = match load_library(&paths.library_config()).map_err(failed)? {
            LoadResult::Missing => Fingerprint::Absent,
            _ => {
                return Err(OnboardingError::Invalid(
                    "Library Configuration already exists; open the normal Library workspace"
                        .to_owned(),
                ));
            }
        };
        let (original_user, user_expected, prior_user_bytes) =
            match load_repository(&paths.user_config()).map_err(failed)? {
                LoadResult::Missing => (
                    RepositoryConfig::user_first_run(),
                    Fingerprint::Absent,
                    None,
                ),
                LoadResult::Valid(loaded) => (
                    loaded.value().clone(),
                    loaded.fingerprint().clone(),
                    fs::read(paths.user_config()).ok(),
                ),
                LoadResult::Unsupported { version, .. } => {
                    return Err(OnboardingError::Invalid(format!(
                        "unsupported User Scope Configuration version {version}"
                    )));
                }
                LoadResult::Invalid { issues } => {
                    return Err(OnboardingError::Invalid(format!(
                        "invalid User Scope Configuration: {}",
                        issues
                            .into_iter()
                            .map(|issue| format!("{}: {}", issue.path, issue.message))
                            .collect::<Vec<_>>()
                            .join("; ")
                    )));
                }
            };
        let entries = inventory(&paths.home().join(".agents/skills"));
        Ok(OnboardingSession {
            entries,
            library_expected,
            user_expected,
            prior_user_bytes,
            original_user,
        })
    }

    pub fn prepare(
        paths: &AppPaths,
        session: &OnboardingSession,
        location_expression: &str,
        selected: &BTreeSet<String>,
    ) -> Result<PreparedOnboarding, OnboardingError> {
        let modes = selected
            .iter()
            .map(|name| (name.clone(), LibraryAcquisitionMode::Move))
            .collect::<BTreeMap<_, _>>();
        Self::prepare_with_modes(paths, session, location_expression, &modes)
    }

    pub fn prepare_with_modes(
        paths: &AppPaths,
        session: &OnboardingSession,
        location_expression: &str,
        selected: &BTreeMap<String, LibraryAcquisitionMode>,
    ) -> Result<PreparedOnboarding, OnboardingError> {
        let library_root = expand_location(
            location_expression,
            paths
                .library_config()
                .parent()
                .unwrap_or_else(|| Path::new(".")),
            paths.home(),
            paths.environment(),
        )
        .map_err(OnboardingError::Invalid)?;
        if library_root == paths.home().join(".agents/skills")
            || library_root.starts_with(paths.home().join(".agents/skills"))
        {
            return Err(OnboardingError::Invalid(
                "Library Location must not be inside ~/.agents/skills".to_owned(),
            ));
        }

        let chosen = session
            .entries
            .iter()
            .filter(|entry| selected.contains_key(entry.name()))
            .collect::<Vec<_>>();
        if chosen.iter().any(|entry| !entry.selectable) {
            return Err(OnboardingError::Invalid(
                "an invalid onboarding entry was selected".to_owned(),
            ));
        }

        let mut imports = Vec::new();
        let mut local_skills = Vec::new();
        let mut link_sources: BTreeMap<PathBuf, LinkSource> = BTreeMap::new();
        let mut enablements = session.original_user.enablements().to_vec();
        let mut directories = session.original_user.skill_directories().to_vec();
        let user_directory = if let Some(directory) = directories
            .iter()
            .find(|directory| directory.path().as_str() == ".agents/skills")
        {
            directory.key().clone()
        } else {
            let mut candidate = if directories
                .iter()
                .any(|directory| directory.key().as_str() == "agents")
            {
                "user-agents".to_owned()
            } else {
                "agents".to_owned()
            };
            let mut suffix = 2;
            while directories
                .iter()
                .any(|directory| directory.key().as_str() == candidate)
            {
                candidate = format!("user-agents-{suffix}");
                suffix += 1;
            }
            let key = crate::domain::SkillDirectoryKey::parse(&candidate).map_err(invalid)?;
            directories.insert(
                0,
                SkillDirectoryConfig::new(
                    key.clone(),
                    crate::domain::RepositoryRelativePath::parse(".agents/skills")
                        .expect("built-in path"),
                    Some("User".to_owned()),
                ),
            );
            key
        };
        let mut review = Vec::new();
        for entry in chosen {
            match entry.kind {
                OnboardingEntryKind::Physical => {
                    let mode = selected
                        .get(entry.name())
                        .copied()
                        .unwrap_or(LibraryAcquisitionMode::Move);
                    let destination = library_root.join(entry.name());
                    if fs::symlink_metadata(&destination).is_ok() {
                        return Err(OnboardingError::Invalid(format!(
                            "Library destination already exists: {}",
                            destination.display()
                        )));
                    }
                    let skill_path = SkillPath::parse(entry.name()).map_err(invalid)?;
                    local_skills.push(RegisteredSkillConfig::new(skill_path.clone()));
                    push_enablement(
                        &mut enablements,
                        &user_directory,
                        SkillKey::new(
                            SourceKey::parse("local/library").expect("built-in key"),
                            skill_path,
                        ),
                        if mode == LibraryAcquisitionMode::Move {
                            MaterializationKind::Linked
                        } else {
                            MaterializationKind::Copied
                        },
                    );
                    imports.push(Import {
                        source: entry.path.clone(),
                        destination: destination.clone(),
                        name: entry.name.clone(),
                        mode,
                    });
                    review.push(OnboardingReviewItem {
                        action: match mode {
                            LibraryAcquisitionMode::Move => "Move to Library",
                            LibraryAcquisitionMode::Copy => "Copy to Library",
                            LibraryAcquisitionMode::Link => "Link to Library",
                        }
                        .to_owned(),
                        source: Some(entry.path.clone()),
                        destination,
                    });
                }
                OnboardingEntryKind::Symlink => {
                    let canonical = entry.path.canonicalize().map_err(failed)?;
                    let (root, origin, skill_path) = infer_source(&canonical)?;
                    if root == library_root || root.starts_with(&library_root) {
                        let relative = canonical.strip_prefix(&library_root).map_err(failed)?;
                        let path = skill_path_from(relative)?;
                        local_skills.push(RegisteredSkillConfig::new(path.clone()));
                        push_enablement(
                            &mut enablements,
                            &user_directory,
                            SkillKey::new(
                                SourceKey::parse("local/library").expect("built-in key"),
                                path,
                            ),
                            MaterializationKind::Linked,
                        );
                    } else {
                        let key = suggested_source_key(&root, origin.as_deref());
                        let source =
                            link_sources
                                .entry(root.clone())
                                .or_insert_with(|| LinkSource {
                                    key: key.clone(),
                                    root: root.clone(),
                                    skills: Vec::new(),
                                });
                        if source.key != key {
                            return Err(OnboardingError::Invalid(format!(
                                "Source Key collision for linked Skill `{}`",
                                entry.name
                            )));
                        }
                        source
                            .skills
                            .push(RegisteredSkillConfig::new(skill_path.clone()));
                        push_enablement(
                            &mut enablements,
                            &user_directory,
                            SkillKey::new(source.key.clone(), skill_path),
                            MaterializationKind::Linked,
                        );
                    }
                    review.push(OnboardingReviewItem {
                        action: "Register Source".to_owned(),
                        source: Some(canonical),
                        destination: entry.path.clone(),
                    });
                }
                OnboardingEntryKind::Invalid => unreachable!("selection validated"),
            }
        }

        local_skills.sort_by(|left, right| left.path().cmp(right.path()));
        local_skills.dedup_by(|left, right| left.path() == right.path());
        let mut locations = vec![LibraryLocationConfig::new(
            location_expression.to_owned(),
            Vec::new(),
            false,
            vec![RegisteredSourceConfig::new(
                SourceKey::parse("local/library").expect("built-in key"),
                SkillPath::parse(".").expect("root path"),
                local_skills,
            )],
        )];
        let mut keys = BTreeSet::from(["local/library".to_owned()]);
        for (_, mut source) in link_sources {
            if !keys.insert(source.key.as_str().to_owned()) {
                return Err(OnboardingError::Invalid(format!(
                    "Source Key collision: {}",
                    source.key
                )));
            }
            source
                .skills
                .sort_by(|left, right| left.path().cmp(right.path()));
            source
                .skills
                .dedup_by(|left, right| left.path() == right.path());
            locations.push(LibraryLocationConfig::new(
                source.root.to_string_lossy().into_owned(),
                Vec::new(),
                false,
                vec![RegisteredSourceConfig::new(
                    source.key,
                    SkillPath::parse(".").expect("root path"),
                    source.skills,
                )],
            ));
        }
        let library_config = LibraryConfig::new(locations).map_err(config_invalid)?;
        let user_config =
            RepositoryConfig::new(directories, enablements).map_err(config_invalid)?;
        review.push(OnboardingReviewItem {
            action: "Write Library config".to_owned(),
            source: None,
            destination: paths.library_config(),
        });
        review.push(OnboardingReviewItem {
            action: "Write User config".to_owned(),
            source: None,
            destination: paths.user_config(),
        });
        Ok(PreparedOnboarding {
            home: paths.home().to_owned(),
            library_config_path: paths.library_config(),
            user_config_path: paths.user_config(),
            library_root,
            library_config,
            user_config,
            library_expected: session.library_expected.clone(),
            user_expected: session.user_expected.clone(),
            prior_user_bytes: session.prior_user_bytes.clone(),
            imports,
            review,
        })
    }

    pub fn commit(prepared: PreparedOnboarding) -> Result<(), OnboardingError> {
        commit_with(prepared, &NoFaults)
    }
}

#[derive(Debug)]
struct LinkSource {
    key: SourceKey,
    root: PathBuf,
    skills: Vec<RegisteredSkillConfig>,
}

fn inventory(root: &Path) -> Vec<OnboardingEntry> {
    let Ok(read) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut entries = read
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            let display = entry.file_name().to_string_lossy().into_owned();
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => match path
                    .canonicalize()
                    .ok()
                    .and_then(|target| validated_skill_metadata_at(&target))
                {
                    Some((name, description)) => OnboardingEntry {
                        name,
                        path,
                        kind: OnboardingEntryKind::Symlink,
                        selectable: true,
                        selected_by_default: false,
                        detail: description,
                    },
                    None => {
                        invalid_entry(display, path, "Symlink does not resolve to a valid Skill")
                    }
                },
                Ok(metadata) if metadata.is_dir() => match validated_skill_metadata_at(&path) {
                    Some((name, description)) => OnboardingEntry {
                        name,
                        path,
                        kind: OnboardingEntryKind::Physical,
                        selectable: true,
                        selected_by_default: true,
                        detail: description,
                    },
                    None => invalid_entry(display, path, "Directory is not a valid Skill"),
                },
                Ok(_) => invalid_entry(display, path, "Entry is not a Skill directory or symlink"),
                Err(error) => {
                    invalid_entry(display, path, &format!("Cannot inspect entry: {error}"))
                }
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}

fn invalid_entry(name: String, path: PathBuf, detail: &str) -> OnboardingEntry {
    OnboardingEntry {
        name,
        path,
        kind: OnboardingEntryKind::Invalid,
        selectable: false,
        selected_by_default: false,
        detail: detail.to_owned(),
    }
}

fn infer_source(skill: &Path) -> Result<(PathBuf, Option<String>, SkillPath), OnboardingError> {
    if let Ok(repository) = GitRepository::discover(skill) {
        let relative = skill.strip_prefix(repository.root()).map_err(failed)?;
        return Ok((
            repository.root().to_owned(),
            repository.origin().map_err(failed)?,
            skill_path_from(relative)?,
        ));
    }
    let root = skill.parent().ok_or_else(|| {
        OnboardingError::Invalid(format!("linked Skill has no parent: {}", skill.display()))
    })?;
    Ok((
        root.to_owned(),
        None,
        skill_path_from(skill.file_name().map(Path::new).unwrap_or(Path::new(".")))?,
    ))
}

fn skill_path_from(path: &Path) -> Result<SkillPath, OnboardingError> {
    let text = if path.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        path.components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    };
    SkillPath::parse(text).map_err(invalid)
}

fn push_enablement(
    enablements: &mut Vec<Enablement>,
    directory: &crate::domain::SkillDirectoryKey,
    skill: SkillKey,
    materialization: MaterializationKind,
) {
    if !enablements
        .iter()
        .any(|enablement| enablement.directory() == directory && enablement.skill() == &skill)
    {
        enablements.push(Enablement::new(directory.clone(), skill, materialization));
    }
}

trait FaultInjector {
    fn fail_after_publish(&self, _index: usize) -> bool {
        false
    }
}

struct NoFaults;
impl FaultInjector for NoFaults {}

#[derive(Debug)]
struct PublishedImport {
    source: PathBuf,
    destination: PathBuf,
    backup: Option<PathBuf>,
}

fn commit_with(
    prepared: PreparedOnboarding,
    faults: &impl FaultInjector,
) -> Result<(), OnboardingError> {
    revalidate(&prepared)?;
    LibraryConfigCodec::render(&prepared.library_config).map_err(failed)?;
    RepositoryConfigCodec::render(&prepared.user_config).map_err(failed)?;
    let library_existed = prepared.library_root.exists();
    fs::create_dir_all(&prepared.library_root).map_err(failed)?;
    let mut stages = Vec::new();
    for import in &prepared.imports {
        let stage = artifact_path(&prepared.library_root, "stage", &import.name);
        let staged = match import.mode {
            LibraryAcquisitionMode::Move | LibraryAcquisitionMode::Copy => {
                copy_tree_all(&import.source, &stage)
                    .and_then(|()| trees_equal(&import.source, &stage))
            }
            LibraryAcquisitionMode::Link => {
                #[cfg(unix)]
                let result = std::os::unix::fs::symlink(
                    import.source.canonicalize().map_err(failed)?,
                    &stage,
                )
                .map(|()| true);
                #[cfg(not(unix))]
                let result = Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "symbolic links require Unix",
                ));
                result
            }
        };
        if !matches!(staged, Ok(true)) {
            let mut cleanup = stages.clone();
            cleanup.push(stage);
            if let Err(message) = cleanup_paths(&cleanup) {
                return Err(OnboardingError::RecoveryRequired(message));
            }
            cleanup_created_library(&prepared.library_root, library_existed);
            return Err(match staged {
                Ok(false) => OnboardingError::Failed(format!(
                    "staged import differs from {}",
                    import.source.display()
                )),
                Err(error) => failed(error),
                Ok(true) => unreachable!(),
            });
        }
        stages.push(stage);
    }

    let mut published = Vec::new();
    let mut library_saved = false;
    let mut user_saved = false;
    let result = (|| {
        for (index, (import, stage)) in prepared.imports.iter().zip(&stages).enumerate() {
            let backup = if import.mode == LibraryAcquisitionMode::Move {
                let backup = artifact_path(
                    import.source.parent().unwrap_or(&prepared.home),
                    "backup",
                    &import.name,
                );
                rename_noreplace(&import.source, &backup).map_err(failed)?;
                match trees_equal(&backup, stage) {
                    Ok(true) => {}
                    Ok(false) => {
                        restore_unpublished(import, &backup, false)?;
                        return Err(OnboardingError::Failed(format!(
                            "Skill changed while onboarding: {}",
                            import.source.display()
                        )));
                    }
                    Err(error) => {
                        restore_unpublished(import, &backup, false)?;
                        return Err(failed(error));
                    }
                }
                Some(backup)
            } else {
                None
            };
            if let Err(error) = rename_noreplace(stage, &import.destination) {
                if let Some(backup) = &backup {
                    restore_unpublished(import, backup, false)?;
                }
                return Err(failed(error));
            }
            if import.mode == LibraryAcquisitionMode::Move {
                #[cfg(unix)]
                if let Err(error) = std::os::unix::fs::symlink(
                    import.destination.canonicalize().map_err(failed)?,
                    &import.source,
                ) {
                    restore_unpublished(import, backup.as_ref().expect("Move has a backup"), true)?;
                    return Err(failed(error));
                }
            }
            published.push(PublishedImport {
                source: import.source.clone(),
                destination: import.destination.clone(),
                backup,
            });
            if faults.fail_after_publish(index) {
                return Err(OnboardingError::Failed(
                    "injected onboarding failure".to_owned(),
                ));
            }
        }
        save_library(
            &prepared.library_config_path,
            &prepared.library_config,
            &prepared.library_expected,
        )
        .map_err(failed)?;
        library_saved = true;
        save_repository(
            &prepared.user_config_path,
            &prepared.user_config,
            &prepared.user_expected,
        )
        .map_err(failed)?;
        user_saved = true;
        Ok(())
    })();

    if let Err(error) = result {
        let rollback = rollback(
            &prepared,
            &published,
            &stages,
            library_saved,
            user_saved,
            library_existed,
        );
        return match rollback {
            Ok(()) => Err(error),
            Err(message) => Err(OnboardingError::RecoveryRequired(message)),
        };
    }

    for published in published {
        if let Some(backup) = published.backup {
            fs::remove_dir_all(&backup).map_err(|error| {
                OnboardingError::RecoveryRequired(format!(
                    "onboarding succeeded but backup remains at {}: {error}",
                    backup.display()
                ))
            })?;
        }
    }
    Ok(())
}

fn cleanup_paths(paths: &[PathBuf]) -> Result<(), String> {
    let failures = paths
        .iter()
        .filter(|path| fs::symlink_metadata(path).is_ok())
        .filter_map(|path| {
            remove_any(path)
                .err()
                .map(|error| format!("remove {}: {error}", path.display()))
        })
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn restore_unpublished(
    import: &Import,
    backup: &Path,
    remove_destination: bool,
) -> Result<(), OnboardingError> {
    let mut failures = Vec::new();
    if remove_destination
        && fs::symlink_metadata(&import.destination).is_ok()
        && let Err(error) = remove_any(&import.destination)
    {
        failures.push(format!("remove {}: {error}", import.destination.display()));
    }
    if let Err(error) = rename_noreplace(backup, &import.source) {
        failures.push(format!(
            "restore {} from {}: {error}",
            import.source.display(),
            backup.display()
        ));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(OnboardingError::RecoveryRequired(failures.join("; ")))
    }
}

fn revalidate(prepared: &PreparedOnboarding) -> Result<(), OnboardingError> {
    if !matches!(
        load_library(&prepared.library_config_path).map_err(failed)?,
        LoadResult::Missing
    ) {
        return Err(OnboardingError::Invalid(
            "Library Configuration changed during onboarding".to_owned(),
        ));
    }
    for import in &prepared.imports {
        if validated_skill_name_at(&import.source).as_deref() != Some(import.name.as_str()) {
            return Err(OnboardingError::Invalid(format!(
                "Skill changed during onboarding: {}",
                import.source.display()
            )));
        }
        if fs::symlink_metadata(&import.destination).is_ok() {
            return Err(OnboardingError::Invalid(format!(
                "Library destination appeared during onboarding: {}",
                import.destination.display()
            )));
        }
    }
    Ok(())
}

fn rollback(
    prepared: &PreparedOnboarding,
    published: &[PublishedImport],
    stages: &[PathBuf],
    library_saved: bool,
    user_saved: bool,
    library_existed: bool,
) -> Result<(), String> {
    let mut failures = Vec::new();
    if user_saved {
        let result = match &prepared.prior_user_bytes {
            Some(bytes) => fs::write(&prepared.user_config_path, bytes),
            None => fs::remove_file(&prepared.user_config_path),
        };
        if let Err(error) = result {
            failures.push(format!(
                "restore {}: {error}",
                prepared.user_config_path.display()
            ));
        }
    }
    if library_saved && let Err(error) = fs::remove_file(&prepared.library_config_path) {
        failures.push(format!(
            "remove {}: {error}",
            prepared.library_config_path.display()
        ));
    }
    for item in published.iter().rev() {
        if item.backup.is_some()
            && fs::symlink_metadata(&item.source).is_ok()
            && let Err(error) = remove_any(&item.source)
        {
            failures.push(format!("remove {}: {error}", item.source.display()));
            continue;
        }
        if fs::symlink_metadata(&item.destination).is_ok()
            && let Err(error) = remove_any(&item.destination)
        {
            failures.push(format!("remove {}: {error}", item.destination.display()));
        }
        if let Some(backup) = &item.backup
            && let Err(error) = rename_noreplace(backup, &item.source)
        {
            failures.push(format!(
                "restore {} from {}: {error}",
                item.source.display(),
                backup.display()
            ));
        }
    }
    for stage in stages {
        if fs::symlink_metadata(stage).is_ok()
            && let Err(error) = remove_any(stage)
        {
            failures.push(format!("remove {}: {error}", stage.display()));
        }
    }
    cleanup_created_library(&prepared.library_root, library_existed);
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn cleanup_created_library(path: &Path, existed: bool) {
    if !existed {
        let _ = fs::remove_dir(path);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

fn artifact_path(parent: &Path, kind: &str, name: &str) -> PathBuf {
    let sequence = ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".skillator-onboarding-{kind}-{}-{sequence}-{name}",
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

fn failed(error: impl std::fmt::Display) -> OnboardingError {
    OnboardingError::Failed(error.to_string())
}

fn invalid(error: impl std::fmt::Display) -> OnboardingError {
    OnboardingError::Invalid(error.to_string())
}

fn config_invalid(issues: Vec<crate::config::ConfigIssue>) -> OnboardingError {
    OnboardingError::Invalid(
        issues
            .into_iter()
            .map(|issue| format!("{}: {}", issue.path, issue.message))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailAfterFirst;
    impl FaultInjector for FailAfterFirst {
        fn fail_after_publish(&self, index: usize) -> bool {
            index == 0
        }
    }

    #[test]
    fn publication_failure_restores_original_skill() {
        let home = tempfile::tempdir().unwrap();
        let global = home.path().join(".agents/skills/demo");
        fs::create_dir_all(&global).unwrap();
        fs::write(
            global.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        )
        .unwrap();
        let paths = AppPaths::new(home.path().to_owned());
        let session = OnboardingWorkflow::load(&paths).unwrap();
        let prepared = OnboardingWorkflow::prepare(
            &paths,
            &session,
            "./library",
            &BTreeSet::from(["demo".to_owned()]),
        )
        .unwrap();

        let error = commit_with(prepared, &FailAfterFirst).unwrap_err();

        std::assert_matches!(error, OnboardingError::Failed(_));
        assert!(global.is_dir());
        assert!(!global.symlink_metadata().unwrap().file_type().is_symlink());
        assert!(!paths.library_config().exists());
        assert!(!paths.user_config().exists());
    }

    #[cfg(unix)]
    #[test]
    fn staging_failure_cleans_every_prior_stage_without_moving_sources() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let home = tempfile::tempdir().unwrap();
        for name in ["first", "second"] {
            let skill = home.path().join(".agents/skills").join(name);
            fs::create_dir_all(&skill).unwrap();
            fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name}\n---\n"),
            )
            .unwrap();
        }
        let unsupported = home.path().join(".agents/skills/second/unsupported");
        let unsupported = CString::new(unsupported.as_os_str().as_bytes()).unwrap();
        // SAFETY: `unsupported` is a valid, owned C string and the mode contains only permission bits.
        assert_eq!(unsafe { libc::mkfifo(unsupported.as_ptr(), 0o600) }, 0);
        let paths = AppPaths::new(home.path().to_owned());
        let session = OnboardingWorkflow::load(&paths).unwrap();
        let prepared = OnboardingWorkflow::prepare(
            &paths,
            &session,
            "./library",
            &BTreeSet::from(["first".to_owned(), "second".to_owned()]),
        )
        .unwrap();

        let error = OnboardingWorkflow::commit(prepared).unwrap_err();

        std::assert_matches!(error, OnboardingError::Failed(_));
        assert!(home.path().join(".agents/skills/first").is_dir());
        assert!(home.path().join(".agents/skills/second").is_dir());
        assert!(!home.path().join(".skillator/library").exists());
        assert!(!paths.library_config().exists());
        assert!(!paths.user_config().exists());
    }
}
