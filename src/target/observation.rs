//! Assemble directory and enablement observations from one consistent scan.
use super::*;
type ExpectedEntryIndex = BTreeMap<String, BTreeMap<String, usize>>;

pub fn observe_with_repository_skills(
    target: &Target,
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
    repository_skills: &RepositorySkillExceptions,
) -> ObservedState {
    Observer::new(target, config, library, repository_skills).observe()
}

struct Observer<'a> {
    target: &'a Target,
    config: &'a RepositoryConfig,
    library: &'a LibrarySnapshot,
    expected_by_directory: ExpectedEntryIndex,
    expected_names: Vec<Option<String>>,
    control_entries_by_parent: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    repository_rules_by_parent: BTreeMap<PathBuf, BTreeSet<String>>,
    observed_git_facts: BTreeMap<PathBuf, PathFacts>,
}
impl<'a> Observer<'a> {
    fn new(
        target: &'a Target,
        config: &'a RepositoryConfig,
        library: &'a LibrarySnapshot,
        repository_skills: &RepositorySkillExceptions,
    ) -> Self {
        let (expected_by_directory, expected_names) = expected_entry_index(config, library);
        let (control_entries_by_parent, repository_rules_by_parent) =
            control_rule_index(config, repository_skills);
        Self {
            target,
            config,
            library,
            expected_by_directory,
            expected_names,
            control_entries_by_parent,
            repository_rules_by_parent,
            observed_git_facts: BTreeMap::new(),
        }
    }
    fn observe(mut self) -> ObservedState {
        let mut directories = Vec::new();
        for directory in self.config.skill_directories() {
            directories.push(self.observe_directory(directory));
        }
        let enablements = self
            .config
            .enablements()
            .iter()
            .enumerate()
            .map(|(index, enablement)| self.observe_enablement(index, enablement))
            .collect::<Vec<_>>();
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
            target_root: self.target.root().to_owned(),
            directories,
            enablements,
        }
    }
    fn observe_directory(&mut self, directory: &SkillDirectoryConfig) -> DirectoryObservation {
        let DirectoryListing {
            root,
            root_state,
            children,
            scan_unstable,
            mut diagnostics,
        } = self.list_directory(directory);
        let expected: BTreeSet<_> = self
            .expected_by_directory
            .get(directory.key().as_str())
            .into_iter()
            .flat_map(|names| names.keys().cloned())
            .collect();
        let control_relative = control_file_path(directory);
        let git_facts = self.directory_git_facts(directory, &children, &expected);
        let (unmanaged_entries, duplicate_entries, recovery_artifacts) =
            self.classify_children(&children, &expected, &mut diagnostics);
        let control_facts = git_facts
            .as_ref()
            .and_then(|facts| facts.get(&control_relative));
        let control_content = self.directory_control_content(&control_relative);
        let (control_file, control_tracked, control_protected, control_ignored) =
            inspect_control_file(
                self.target,
                &control_relative,
                control_facts,
                &control_content,
            );
        let generated_ignored =
            self.generated_entries_ignored(directory, &expected, git_facts.as_ref());
        diagnostics.extend(control_diagnostics(
            &control_relative,
            root_state,
            &control_file,
            (control_ignored, generated_ignored),
        ));
        let compatible_agents = self.directory_compatibility(directory, &mut diagnostics);
        let comparison = aggregate_directory_comparison(
            root_state,
            scan_unstable,
            root_state != RootState::Directory
                || !recovery_artifacts.is_empty()
                || (self.target.scope() == TargetScope::Repository
                    && (control_file != ControlFileState::Canonical
                        || control_protected
                        || !control_ignored
                        || !generated_ignored)),
        );
        DirectoryObservation {
            key: directory.key().as_str().to_owned(),
            path: root,
            control_path: self.target.root().join(&control_relative),
            control_content,
            root_state,
            comparison,
            control_file,
            control_tracked,
            control_protected,
            control_ignored,
            generated_ignored,
            unmanaged_entries,
            duplicate_entries,
            compatible_agents,
            recovery_artifacts,
            diagnostics,
        }
    }
    fn generated_entries_ignored(
        &self,
        directory: &SkillDirectoryConfig,
        expected: &BTreeSet<String>,
        git_facts: Option<&BTreeMap<PathBuf, PathFacts>>,
    ) -> bool {
        self.target.git_repository().is_none_or(|_| {
            expected.iter().all(|name| {
                let path = PathBuf::from(directory.path().as_str()).join(name);
                git_facts
                    .and_then(|facts| facts.get(&path))
                    .is_some_and(|facts| facts.ignored)
            })
        })
    }
    fn list_directory(&self, directory: &SkillDirectoryConfig) -> DirectoryListing {
        let root = self.target.root().join(directory.path().as_str());
        let root_state = if has_symlink_ancestor(self.target.root(), &root) {
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
        DirectoryListing {
            root,
            root_state,
            children,
            scan_unstable,
            diagnostics,
        }
    }
    fn classify_children(
        &self,
        children: &[PathBuf],
        expected: &BTreeSet<String>,
        diagnostics: &mut Vec<String>,
    ) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
        let mut unmanaged_entries = Vec::new();
        let mut duplicate_entries = Vec::new();
        let mut recovery_artifacts = Vec::new();
        for child in children {
            let name = child
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if self.target.scope() == TargetScope::Repository && name == ".gitignore" {
                continue;
            }
            if name.starts_with(".skillator-") {
                recovery_artifacts.push(child.clone());
                continue;
            }
            if expected.contains(name) {
                continue;
            }
            unmanaged_entries.push(child.clone());
            if self.inspect_unmanaged_entry(child, expected, diagnostics) {
                duplicate_entries.push(child.clone());
            }
        }
        (unmanaged_entries, duplicate_entries, recovery_artifacts)
    }
    fn inspect_unmanaged_entry(
        &self,
        child: &Path,
        expected: &BTreeSet<String>,
        diagnostics: &mut Vec<String>,
    ) -> bool {
        let name = child
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let mut duplicate = false;
        if expected
            .iter()
            .any(|expected| expected.eq_ignore_ascii_case(name))
        {
            diagnostics.push(format!(
                "entry `{name}` differs from a configured skill name only by letter case"
            ));
        }
        if self.is_duplicate_skill_link(child) {
            duplicate = true;
            diagnostics.push(format!(
                "The same skill may be installed twice: {}",
                child.display()
            ));
        }
        duplicate
    }
    fn is_duplicate_skill_link(&self, child: &Path) -> bool {
        fs::symlink_metadata(child).is_ok_and(|metadata| metadata.file_type().is_symlink())
            && fs::canonicalize(child).is_ok_and(|resolved| {
                self.library.sources().any(|source| {
                    source
                        .skills()
                        .any(|skill| skill.absolute_path().is_some_and(|path| path == resolved))
                })
            })
    }
    fn directory_compatibility(
        &self,
        directory: &SkillDirectoryConfig,
        diagnostics: &mut Vec<String>,
    ) -> Vec<&'static str> {
        let compatible_agents = compatibility(directory.path().as_str());
        for other in self.config.skill_directories() {
            if other.key() == directory.key() {
                continue;
            }
            let overlap = compatibility(other.path().as_str())
                .iter()
                .any(|agent| compatible_agents.contains(agent));
            let shared_skill = self.config.enablements().iter().any(|left| {
                left.directory() == directory.key()
                    && self.config.enablements().iter().any(|right| {
                        right.directory() == other.key() && right.skill() == left.skill()
                    })
            });
            if overlap && shared_skill {
                diagnostics.push(format!(
                    "The same agents also read skills from `{}`",
                    other.key()
                ));
            }
        }
        compatible_agents
    }
    fn directory_git_facts(
        &mut self,
        directory: &SkillDirectoryConfig,
        children: &[PathBuf],
        expected: &BTreeSet<String>,
    ) -> Option<BTreeMap<PathBuf, PathFacts>> {
        let mut fact_paths = children
            .iter()
            .filter_map(|child| {
                child
                    .strip_prefix(self.target.root())
                    .ok()
                    .map(Path::to_owned)
            })
            .collect::<Vec<_>>();
        fact_paths.extend(
            expected
                .iter()
                .map(|name| PathBuf::from(directory.path().as_str()).join(name)),
        );
        fact_paths.push(control_file_path(directory));
        fact_paths.sort();
        fact_paths.dedup();
        let git_facts = self
            .target
            .git_repository()
            .and_then(|repository| repository.facts_for_many(&fact_paths).ok());
        if let Some(facts) = &git_facts {
            self.observed_git_facts.extend(facts.clone());
        }
        git_facts
    }
    fn directory_control_content(&self, control_relative: &Path) -> Vec<u8> {
        let control_parent = control_relative.parent().unwrap_or_else(|| Path::new("."));
        let existing_control = fs::read(self.target.root().join(control_relative)).ok();
        control_file_content(
            self.control_entries_by_parent
                .get(control_parent)
                .expect("all configured directories have a control group"),
            control_parent,
            existing_control.as_deref(),
            self.repository_rules_by_parent.get(control_parent),
        )
        .into_bytes()
    }
    fn observe_enablement(&self, index: usize, enablement: &Enablement) -> EnablementObservation {
        let directory = self
            .config
            .skill_directories()
            .iter()
            .find(|directory| directory.key() == enablement.directory())
            .expect("validated relationship");
        let root = self.target.root().join(directory.path().as_str());
        let resolved = self.library.resolve(enablement.skill());
        let expected = self.expected_names[index].clone();
        let collision = expected.as_ref().is_some_and(|name| {
            self.expected_by_directory
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
                inspect_materialization(&path, enablement, resolved, self.target.scope());
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
            .and_then(|path| path.strip_prefix(self.target.root()).ok())
            .map(|relative| {
                self.observed_git_facts
                    .get(relative)
                    .map_or(self.target.git_repository().is_some(), |facts| {
                        facts.tracked || facts.staged || facts.unmerged
                    })
            })
            .unwrap_or(true);
        EnablementObservation {
            enablement: enablement.clone(),
            expected_entry: expected,
            comparison,
            state,
            unresolved: resolved.is_none(),
            path,
            tracked,
            fingerprint,
            overlap_advisory: self.library.has_overlap_advisory(enablement.skill()),
        }
    }
}

fn expected_entry_index(
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
) -> (ExpectedEntryIndex, Vec<Option<String>>) {
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

    (expected_by_directory, expected_names)
}

fn control_rule_index(
    config: &RepositoryConfig,
    repository_skills: &RepositorySkillExceptions,
) -> (
    BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    BTreeMap<PathBuf, BTreeSet<String>>,
) {
    let mut control_entries_by_parent = BTreeMap::<PathBuf, BTreeSet<PathBuf>>::new();
    let mut repository_rules_by_parent = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    for directory in config.skill_directories() {
        let control = control_file_path(directory);
        let parent = control
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_owned();
        let Ok(skill_root) = Path::new(directory.path().as_str()).strip_prefix(&parent) else {
            continue;
        };
        let entries = control_entries_by_parent.entry(parent).or_default();
        entries.insert(skill_root.join("*"));
        if let Some(names) = repository_skills.get(directory.key().as_str()) {
            let rules = repository_rules_by_parent
                .entry(
                    control
                        .parent()
                        .unwrap_or_else(|| Path::new("."))
                        .to_owned(),
                )
                .or_default();
            for name in names {
                if let Some(rule) = repository_tracking_rule(directory, name) {
                    rules.insert(rule);
                }
            }
        }
    }

    (control_entries_by_parent, repository_rules_by_parent)
}

fn control_diagnostics(
    control_relative: &Path,
    root_state: RootState,
    control_file: &ControlFileState,
    ignored: (bool, bool),
) -> Vec<String> {
    let (control_ignored, generated_ignored) = ignored;
    let mut diagnostics = Vec::new();
    match control_file {
        ControlFileState::Missing if root_state == RootState::Absent => {}
        ControlFileState::Missing => diagnostics.push(format!(
            ".gitignore is missing at {}; save to create it",
            control_relative.display()
        )),
        ControlFileState::PrefixRequired => diagnostics.push(format!(
            ".gitignore needs Skillator's rules at {}; save to add them and keep existing rules",
            control_relative.display()
        )),
        ControlFileState::Modified => diagnostics.push(format!(
            ".gitignore has different contents at {}; save to review replacing the entire file",
            control_relative.display()
        )),
        ControlFileState::WrongKind => diagnostics.push(format!(
            ".gitignore must be a regular file, not a directory or link: {}",
            control_relative.display()
        )),
        ControlFileState::Uninspectable => diagnostics.push(format!(
            "Cannot read .gitignore: {}",
            control_relative.display()
        )),
        ControlFileState::Canonical | ControlFileState::NotRequired => {}
    }
    if *control_file == ControlFileState::Canonical && !control_ignored {
        diagnostics.push(format!(
            "The repository's root .gitignore does not ignore this file: {}",
            control_relative.display(),
        ));
    }
    if *control_file == ControlFileState::Canonical && !generated_ignored {
        diagnostics.push("Generated skill files are not ignored by Git".to_owned());
    }
    diagnostics
}

struct DirectoryListing {
    root: PathBuf,
    root_state: RootState,
    children: Vec<PathBuf>,
    scan_unstable: bool,
    diagnostics: Vec<String>,
}
