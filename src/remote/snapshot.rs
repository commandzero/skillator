use super::{
    Error, Result, process,
    state::{self, Entry, History},
};
use crate::app::AppPaths;
use crate::config::{LibraryConfig, LoadResult, RepositoryConfig};
use crate::library::{SourceKind, scan_library};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Location {
    pub path: String,
    pub exclusions: Vec<String>,
    pub allow_overlap: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GitRef {
    pub origin: String,
    pub commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Source {
    pub key: String,
    pub root: String,
    pub location: String,
    pub exclusions: Vec<String>,
    pub git: Option<GitRef>,
    #[serde(default)]
    pub branch: Option<String>,
    pub skills: BTreeSet<String>,
    pub files: BTreeMap<String, Entry>,
    pub committed: BTreeMap<String, Entry>,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    pub protocol: u64,
    pub version: String,
    pub home: PathBuf,
    pub history: History,
    pub locations: Vec<Location>,
    pub available_locations: BTreeSet<String>,
    pub sources: Vec<Source>,
    pub physical_paths: BTreeMap<String, String>,
    pub user: BTreeMap<String, String>,
    pub user_present: bool,
    pub library_hash: Option<String>,
    pub user_hash: Option<String>,
    pub problems: Vec<String>,
}

pub(super) fn file_context(snapshot: &Snapshot, path: &str) -> Option<String> {
    let source = snapshot
        .sources
        .iter()
        .filter(|source| path == source.root || path.starts_with(&format!("{}/", source.root)))
        .max_by_key(|source| source.root.len())?;
    Some(state::digest(
        &serde_json::to_vec(&(&source.key, &source.root, &source.git)).ok()?,
    ))
}

pub(super) fn library(paths: &AppPaths) -> Result<LibraryConfig> {
    parse_load(
        crate::config::load_library(&state::contained(paths.home(), ".skillator/library.yaml")?)
            .map_err(Error::input_display)?,
        LibraryConfig::empty(),
    )
}

fn parse_load<T: Clone>(loaded: LoadResult<T>, empty: T) -> Result<T> {
    match loaded {
        LoadResult::Missing => Ok(empty),
        LoadResult::Valid(value) => Ok(value.value().clone()),
        LoadResult::Unsupported { version, .. } => Err(Error::input(format!(
            "unsupported configuration version {version}"
        ))),
        LoadResult::Invalid { issues } => {
            Err(Error::input(format!("invalid configuration: {issues:?}")))
        }
    }
}

pub(super) fn user(paths: &AppPaths) -> Result<RepositoryConfig> {
    parse_load(
        crate::config::load_repository(&state::contained(paths.home(), ".agents/skillator.yaml")?)
            .map_err(Error::input_display)?,
        RepositoryConfig::empty(),
    )
}

pub(super) fn user_entries(config: &RepositoryConfig) -> Result<BTreeMap<String, String>> {
    let mut entries = BTreeMap::new();
    for directory in config.skill_directories() {
        entries.insert(
            format!("directory/{}", directory.key()),
            serde_json::to_string(&(directory.path().as_str(), directory.label()))
                .map_err(Error::input_display)?,
        );
    }
    for enablement in config.enablements() {
        let key = serde_json::to_string(&(
            enablement.directory().as_str(),
            enablement.skill().source().as_str(),
            enablement.skill().path().as_str(),
        ))
        .map_err(Error::input_display)?;
        entries.insert(
            format!("enablement/{key}"),
            serde_json::to_string(&enablement.materialization()).map_err(Error::input_display)?,
        );
    }
    Ok(entries)
}

pub(super) fn user_from_entries(entries: &BTreeMap<String, String>) -> Result<RepositoryConfig> {
    use crate::config::SkillDirectoryConfig;
    use crate::domain::{
        Enablement, MaterializationKind, RepositoryRelativePath, SkillDirectoryKey, SkillKey,
        SkillPath, SourceKey,
    };
    let mut directories = Vec::new();
    let mut enablements = Vec::new();
    for (key, value) in entries {
        if let Some(key) = key.strip_prefix("directory/") {
            let (path, label): (String, Option<String>) =
                serde_json::from_str(value).map_err(Error::input_display)?;
            directories.push(SkillDirectoryConfig::new(
                SkillDirectoryKey::parse(key).map_err(Error::input_display)?,
                RepositoryRelativePath::parse(path).map_err(Error::input_display)?,
                label,
            ));
        } else if let Some(key) = key.strip_prefix("enablement/") {
            let (directory, source, skill): (String, String, String) =
                serde_json::from_str(key).map_err(Error::input_display)?;
            let mode: MaterializationKind =
                serde_json::from_str(value).map_err(Error::input_display)?;
            enablements.push(Enablement::new(
                SkillDirectoryKey::parse(directory).map_err(Error::input_display)?,
                SkillKey::new(
                    SourceKey::parse(source).map_err(Error::input_display)?,
                    SkillPath::parse(skill).map_err(Error::input_display)?,
                ),
                mode,
            ));
        } else {
            return Err(Error::input("unknown user state entry"));
        }
    }
    RepositoryConfig::new(directories, enablements)
        .map_err(|issues| Error::input(format!("invalid merged user state: {issues:?}")))
}

pub(super) fn inspect(paths: &AppPaths, extra: &[Source]) -> Result<Snapshot> {
    let config = library(paths)?;
    let user_config = user(paths)?;
    let history = History::load(paths.home())?;
    let library_snapshot = scan_library(
        &config,
        &paths.library_config(),
        paths.home(),
        paths.environment(),
    );
    let mut locations = Vec::new();
    for (index, location) in library_snapshot.locations().iter().enumerate() {
        let resolved = location.resolved().ok_or_else(|| {
            Error::input(format!(
                "unresolvable library location {}",
                location.expression()
            ))
        })?;
        locations.push(Location {
            path: state::home_relative(paths.home(), resolved)?,
            exclusions: config.locations()[index].exclusions().to_vec(),
            allow_overlap: config.locations()[index].allow_overlap(),
        });
    }
    let mut sources = Vec::new();
    for source in library_snapshot.sources() {
        let Some(root) = source.root() else {
            continue;
        };
        if root
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(super::session::reserved_temporary)
        {
            continue;
        }
        let mut observed = Source {
            key: source.key().to_string(),
            root: state::home_relative(paths.home(), root)?,
            location: locations[source.location_index()].path.clone(),
            exclusions: locations[source.location_index()].exclusions.clone(),
            git: None,
            branch: None,
            skills: source
                .skills()
                .map(|skill| {
                    if skill.path() == "." {
                        String::new()
                    } else {
                        skill.path().to_owned()
                    }
                })
                .collect(),
            files: BTreeMap::new(),
            committed: BTreeMap::new(),
            problems: Vec::new(),
        };
        if source.key_collision() {
            observed.problems.push("source identity collision".into());
        }
        if source.kind() == SourceKind::Git {
            match git_ref(root) {
                Ok(git) => {
                    observed.git = Some(git);
                    observed.branch = git_branch(root)?;
                }
                Err(error) => observed.problems.push(error.to_string()),
            }
        }
        sources.push(observed);
    }
    let known_boundaries: BTreeSet<_> = sources
        .iter()
        .flat_map(|source| {
            source
                .skills
                .iter()
                .map(move |skill| Path::new(&source.root).join(skill))
        })
        .collect();
    for incoming in extra {
        state::relative(&incoming.root)?;
        if let Some(found) = sources
            .iter_mut()
            .find(|s| s.root == incoming.root && s.key == incoming.key)
        {
            found.skills.extend(incoming.skills.iter().cloned());
        } else {
            let root = state::contained(paths.home(), &incoming.root)?;
            let mut source = incoming.clone();
            source.files.clear();
            source.committed.clear();
            source.problems.clear();
            source.git = if incoming.git.is_some() && root.exists() {
                match git_ref(&root) {
                    Ok(git) => Some(git),
                    Err(error) => {
                        source.problems.push(error.to_string());
                        None
                    }
                }
            } else {
                None
            };
            source.branch = if source.git.is_some() {
                git_branch(&root)?
            } else {
                None
            };
            sources.push(source);
        }
    }
    for source in &mut sources {
        let root = paths.home().join(&source.root);
        let location = paths.home().join(&source.location);
        let physical_location = location.canonicalize().unwrap_or(location);
        if library_snapshot.diagnostics().iter().any(|diagnostic| {
            diagnostic.code == "overlapping_locations"
                && diagnostic.path.as_ref().is_some_and(|path| {
                    path.starts_with(&physical_location) || physical_location.starts_with(path)
                })
        }) {
            source
                .problems
                .push("library locations overlap without mutual permission".into());
        }
        if let Some(git) = &source.git {
            let unmerged = process::git(&root, &["ls-files", "-u"])?;
            if !unmerged.is_empty() {
                source.problems.push("unmerged Git index".into());
            }
            // Committed manifests retain the boundary when a skill manifest was deleted.
            let tree = process::git(&root, &["ls-tree", "-r", "-z", &git.commit])?;
            let mut tracked = Vec::new();
            for record in tree.split(|b| *b == 0).filter(|r| !r.is_empty()) {
                let record = std::str::from_utf8(record).map_err(Error::input_display)?;
                let (metadata, path) = record
                    .split_once('\t')
                    .ok_or_else(|| Error::input("invalid Git tree record"))?;
                let fields: Vec<_> = metadata.split(' ').collect();
                if fields.len() != 3 {
                    return Err(Error::input("invalid Git tree metadata"));
                }
                if path == "SKILL.md" {
                    source.skills.insert(String::new());
                } else if let Some(parent) = path.strip_suffix("/SKILL.md") {
                    source.skills.insert(parent.to_owned());
                }
                tracked.push((
                    fields[0].to_owned(),
                    fields[1].to_owned(),
                    fields[2].to_owned(),
                    path.to_owned(),
                ));
            }
            for (mode, kind, oid, path) in tracked {
                if kind != "blob" || !in_skills(&path, &source.skills) {
                    continue;
                }
                let bytes = process::git(&root, &["cat-file", "blob", &oid])?;
                let entry = if mode == "120000" {
                    Entry::Link {
                        target: String::from_utf8(bytes).map_err(Error::input_display)?,
                    }
                } else {
                    Entry::File {
                        hash: state::digest(&bytes),
                        executable: mode == "100755",
                    }
                };
                source
                    .committed
                    .insert(format!("{}/{path}", source.root), entry);
            }
        }
        let mut exclusions =
            ignore::gitignore::GitignoreBuilder::new(paths.home().join(&source.location));
        for pattern in &source.exclusions {
            exclusions
                .add_line(None, pattern)
                .map_err(Error::input_display)?;
        }
        let exclusions = exclusions.build().map_err(Error::input_display)?;
        source.skills.retain(|skill| {
            !exclusions
                .matched_path_or_any_parents(root.join(skill), true)
                .is_ignore()
        });
        source.committed.retain(|path, _| {
            !state::administrative(Path::new(path))
                && !exclusions
                    .matched_path_or_any_parents(paths.home().join(path), false)
                    .is_ignore()
        });
        for skill in &source.skills {
            if !skill.is_empty() {
                state::relative(skill)?;
            }
            let logical = root.join(skill);
            if !logical.exists() {
                continue;
            }
            let manifest = state::home_relative(paths.home(), &logical.join("SKILL.md"))?;
            if !known_boundaries.contains(&Path::new(&source.root).join(skill))
                && !logical.join("SKILL.md").is_file()
                && !source.committed.contains_key(&manifest)
                && !history.peers.values().any(|peer| {
                    peer.files.contains_key(&manifest)
                        || peer
                            .files
                            .get(format!("{}/{}", source.root, skill).trim_end_matches('/'))
                            == Some(&Some(Entry::Directory))
                })
            {
                source.problems.push(format!(
                    "incoming skill destination is occupied by an unmanaged directory: {}",
                    logical.display()
                ));
                continue;
            }
            let real = logical.canonicalize().map_err(Error::input_display)?;
            if !real.starts_with(paths.home().canonicalize().map_err(Error::input_display)?) {
                return Err(Error::input("skill resolves outside user home"));
            }
            if exclusions
                .matched_path_or_any_parents(&logical, true)
                .is_ignore()
            {
                continue;
            }
            collect(
                paths.home(),
                &logical,
                &real,
                &exclusions,
                &mut source.files,
                true,
            )?;
        }
        for peer in history.peers.values() {
            for path in peer.files.keys() {
                if !state::transferable(paths.home(), path)? {
                    continue;
                }
                if path.starts_with(&format!("{}/", source.root)) {
                    let logical = paths.home().join(path);
                    if exclusions
                        .matched_path_or_any_parents(&logical, false)
                        .is_ignore()
                    {
                        continue;
                    }
                    if let Some(actual) = state::observation_path(paths.home(), path)?
                        && let Some(entry) = state::observe(&actual)?
                    {
                        source.files.entry(path.clone()).or_insert(entry);
                    }
                }
            }
        }
    }
    sources.sort_by(|a, b| (&a.root, &a.key).cmp(&(&b.root, &b.key)));
    let user_bytes = state::read_optional(&paths.user_config())?;
    let mut physical_paths = BTreeMap::new();
    for source in &sources {
        for (path, entry) in &source.files {
            let physical = state::contained(paths.home(), path)?;
            let physical = if matches!(entry, Entry::Directory) {
                physical.canonicalize().map_err(Error::input_display)?
            } else {
                physical
            };
            physical_paths.insert(path.clone(), physical.to_string_lossy().into_owned());
        }
    }
    let mut problems: Vec<_> = library_snapshot
        .diagnostics()
        .iter()
        .map(|d| d.message.clone())
        .chain(library_snapshot.sources().flat_map(|source| {
            source
                .skills()
                .flat_map(|skill| skill.diagnostics().iter().cloned())
        }))
        .collect();
    if !super::session::pending_recovery(paths.home())?.is_empty() {
        problems.push("an interrupted publication needs recovery before new work".into());
    }
    let mut available_locations = BTreeSet::new();
    for location in locations
        .iter()
        .map(|location| &location.path)
        .chain(sources.iter().map(|source| &source.location))
    {
        if state::contained(paths.home(), location)?.is_dir() {
            available_locations.insert(location.clone());
        }
    }
    Ok(Snapshot {
        available_locations,
        protocol: 1,
        version: env!("CARGO_PKG_VERSION").into(),
        home: paths.home().to_path_buf(),
        history,
        locations,
        sources,
        physical_paths,
        user: user_entries(&user_config)?,
        user_present: user_bytes.is_some(),
        library_hash: state::read_optional(&paths.library_config())?
            .map(|bytes| state::digest(&bytes)),
        user_hash: user_bytes.map(|bytes| state::digest(&bytes)),
        problems,
    })
}

fn git_branch(root: &Path) -> Result<Option<String>> {
    let branch = String::from_utf8(process::git(root, &["rev-parse", "--abbrev-ref", "HEAD"])?)
        .map_err(Error::input_display)?;
    let branch = branch.trim();
    Ok((branch != "HEAD").then(|| branch.to_owned()))
}

pub(super) fn git_ref(root: &Path) -> Result<GitRef> {
    let root_text = String::from_utf8(process::git(root, &["rev-parse", "--show-toplevel"])?)
        .map_err(Error::input_display)?;
    if Path::new(root_text.trim())
        .canonicalize()
        .map_err(Error::input_display)?
        != root.canonicalize().map_err(Error::input_display)?
    {
        return Err(Error::input(
            "destination is not the expected Git repository root",
        ));
    }
    let origin = String::from_utf8(process::git(
        root,
        &["config", "--get", "remote.origin.url"],
    )?)
    .map_err(Error::input_display)?
    .trim()
    .to_owned();
    let commit = String::from_utf8(process::git(
        root,
        &["rev-parse", "--verify", "HEAD^{commit}"],
    )?)
    .map_err(Error::input_display)?
    .trim()
    .to_owned();
    Ok(GitRef { origin, commit })
}

fn in_skills(path: &str, skills: &BTreeSet<String>) -> bool {
    skills
        .iter()
        .any(|skill| skill.is_empty() || path == skill || path.starts_with(&format!("{skill}/")))
}

fn collect(
    home: &Path,
    logical: &Path,
    boundary: &Path,
    exclusions: &ignore::gitignore::Gitignore,
    files: &mut BTreeMap<String, Entry>,
    root: bool,
) -> Result<()> {
    if logical.file_name().is_some_and(|name| {
        name == ".git"
            || name
                .to_str()
                .is_some_and(super::session::reserved_temporary)
    }) {
        return Ok(());
    }
    let path = state::home_relative(home, logical)?;
    if !state::transferable(home, &path)? {
        return Ok(());
    }
    let actual = state::contained(home, &path)?;
    let entry = if root {
        Some(Entry::Directory)
    } else {
        state::observe(&actual)?
    };
    let Some(entry) = entry else {
        return Ok(());
    };
    if exclusions
        .matched_path_or_any_parents(logical, matches!(entry, Entry::Directory))
        .is_ignore()
    {
        return Ok(());
    }
    match &entry {
        Entry::Directory => {
            files.insert(path, entry.clone());
            for child in fs::read_dir(logical).map_err(Error::input_display)? {
                collect(
                    home,
                    &child.map_err(Error::input_display)?.path(),
                    boundary,
                    exclusions,
                    files,
                    false,
                )?;
            }
        }
        Entry::Link { target } => {
            if Path::new(target).is_absolute() {
                return Err(Error::input("absolute internal skill link"));
            }
            let resolved = logical
                .parent()
                .unwrap()
                .join(target)
                .canonicalize()
                .map_err(Error::input_display)?;
            if !resolved.starts_with(boundary) {
                return Err(Error::input("internal skill link escapes the skill"));
            }
            files.insert(path, entry);
        }
        Entry::File { .. } => {
            files.insert(path, entry);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository(root: &Path) {
        fs::create_dir_all(root.join("demo")).unwrap();
        fs::write(
            root.join("demo/SKILL.md"),
            "---\nname: demo\ndescription: Git observation\n---\n",
        )
        .unwrap();
        process::git(root, &["init", "-q"]).unwrap();
        process::git(root, &["add", "."]).unwrap();
        process::git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "initial",
            ],
        )
        .unwrap();
        process::git(
            root,
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/acme/skills.git",
            ],
        )
        .unwrap();
    }

    #[test]
    fn worktrees_submodules_and_detached_head_have_independent_source_roots() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("source");
        repository(&root);
        let reference = git_ref(&root).unwrap();
        let worktree = home.path().join("worktree");
        process::git(
            &root,
            &["worktree", "add", "--detach", worktree.to_str().unwrap()],
        )
        .unwrap();
        assert_eq!(git_ref(&worktree).unwrap(), reference);
        assert!(worktree.join(".git").is_file());
        let parent = home.path().join("parent");
        repository(&parent);
        process::git(
            &parent,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                root.to_str().unwrap(),
                "nested",
            ],
        )
        .unwrap();
        let nested = git_ref(&parent.join("nested")).unwrap();
        assert_eq!(nested.commit, reference.commit);
        assert_eq!(nested.origin, root.to_string_lossy());
        assert!(parent.join("nested/.git").is_file());
        let paths = AppPaths::new(home.path().into());
        fs::create_dir(home.path().join(".skillator")).unwrap();
        fs::write(
            paths.library_config(),
            "version: 1\nlocations: [{path: '~/worktree'}, {path: '~/parent/nested'}]\n",
        )
        .unwrap();
        let observed = inspect(&paths, &[]).unwrap();
        assert_eq!(observed.sources.len(), 2);
        assert!(
            observed
                .sources
                .iter()
                .all(|source| source.git.as_ref().unwrap().commit == reference.commit)
        );
        assert!(
            observed
                .sources
                .iter()
                .all(|source| source.files.keys().all(|path| !path.contains(".git")))
        );
    }

    #[test]
    fn invalid_hidden_skills_and_mode_changes_are_observed() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(home.path().into());
        let skill = home.path().join(".skillator/library/.hidden");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            paths.library_config(),
            "version: 1\nlocations: [{path: '~/.skillator/library'}]\n",
        )
        .unwrap();
        fs::write(skill.join("SKILL.md"), "invalid manifest").unwrap();
        fs::write(skill.join("run.sh"), "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(skill.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        let observed = inspect(&paths, &[]).unwrap();
        assert!(!observed.problems.is_empty());
        assert!(observed.sources.iter().any(|source| matches!(
            source.files.get(".skillator/library/.hidden/run.sh"),
            Some(Entry::File {
                executable: true,
                ..
            })
        )));
        assert!(observed.user.is_empty());
    }
    #[test]
    fn environment_paths_are_portable_and_unmerged_indexes_block_the_source() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("Development/skills");
        repository(&root);
        let paths = AppPaths::with_environment(
            home.path().into(),
            BTreeMap::from([("SKILLS_ROOT".into(), root.to_string_lossy().into_owned())]),
        );
        fs::create_dir(home.path().join(".skillator")).unwrap();
        fs::write(
            paths.library_config(),
            "version: 1\nlocations: [{path: '${SKILLS_ROOT}'}]\n",
        )
        .unwrap();
        let first = inspect(&paths, &[]).unwrap();
        assert_eq!(first.locations[0].path, "Development/skills");
        let blob =
            String::from_utf8(process::git(&root, &["rev-parse", "HEAD:demo/SKILL.md"]).unwrap())
                .unwrap();
        let mut child = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["update-index", "--index-info"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        writeln!(
            child.stdin.take().unwrap(),
            "0 {}\tdemo/SKILL.md\n100644 {} 1\tdemo/SKILL.md\n100644 {} 2\tdemo/SKILL.md",
            "0".repeat(40),
            blob.trim(),
            blob.trim()
        )
        .unwrap();
        assert!(child.wait().unwrap().success());
        let before = fs::read(root.join(".git/index")).unwrap();
        let observed = inspect(&paths, &[]).unwrap();
        assert!(observed.sources.iter().any(|source| {
            source
                .problems
                .iter()
                .any(|message| message.contains("unmerged"))
        }));
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), before);
    }
}
