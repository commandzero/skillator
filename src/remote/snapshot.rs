use super::{
    Error, Result, process,
    state::{self, Entry, History},
};
use crate::app::AppPaths;
use crate::config::{
    LibraryConfig, LibraryConfigCodec, LoadResult, RepositoryConfig, RepositoryConfigCodec,
};
use crate::library::{SkillValidity, SourceKind, scan_library};
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
    #[serde(default)]
    pub invalid_skills: BTreeSet<String>,
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
    #[serde(default)]
    pub acquisition_aliases: BTreeMap<String, String>,
    pub user_directories: BTreeSet<String>,
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
    library_from_bytes(state::read_contained(paths.home(), ".skillator/library.yaml")?.as_deref())
}

pub(super) fn library_from_bytes(bytes: Option<&[u8]>) -> Result<LibraryConfig> {
    parse_load(
        bytes
            .map(LibraryConfigCodec::parse)
            .unwrap_or(LoadResult::Missing),
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
        state::read_contained(paths.home(), ".agents/skillator.yaml")?
            .as_deref()
            .map(RepositoryConfigCodec::parse)
            .unwrap_or(LoadResult::Missing),
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
    let home = paths.home().canonicalize().map_err(Error::input_display)?;
    for location in config.locations() {
        let expanded = crate::library::expand_location(
            location.path(),
            paths.library_config().parent().unwrap(),
            paths.home(),
            paths.environment(),
        )
        .map_err(Error::input_display)?;
        if expanded.canonicalize().is_ok_and(|path| path == home) {
            return Err(Error::input(
                "library rsync does not support a home-rooted location; register directories below the user home instead of ~",
            ));
        }
    }
    let user_config = user(paths)?;
    let mut user_directories = user_directories(paths.home(), &user_config)?;
    let history = History::load(paths.home())?;
    let library_snapshot = scan_library(
        &config,
        &paths.library_config(),
        paths.home(),
        paths.environment(),
    );
    if let Some(diagnostic) = library_snapshot
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code == "discovery_failed")
    {
        return Err(Error::input(&diagnostic.message));
    }
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
            .is_some_and(crate::library::reserved_temporary)
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
            invalid_skills: source
                .skills()
                .filter(|skill| skill.validity() == SkillValidity::Invalid)
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
        for path in [&incoming.root, &incoming.location] {
            state::home_relative(paths.home(), &state::contained(paths.home(), path)?)?;
        }
        if let Some(found) = sources
            .iter_mut()
            .find(|s| s.root == incoming.root && s.key == incoming.key)
        {
            found.skills.extend(incoming.skills.iter().cloned());
            found
                .invalid_skills
                .extend(incoming.invalid_skills.iter().cloned());
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
    let source_roots: Vec<_> = sources.iter().map(|source| source.root.clone()).collect();
    for source in &mut sources {
        let nested_roots: BTreeSet<String> = source_roots
            .iter()
            .filter(|root| root.starts_with(&format!("{}/", source.root)))
            .cloned()
            .collect();
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
            !within_nested_source(
                &Path::new(&source.root).join(skill).to_string_lossy(),
                &nested_roots,
            ) && !exclusions
                .matched_path_or_any_parents(root.join(skill), true)
                .is_ignore()
        });
        source
            .invalid_skills
            .retain(|skill| source.skills.contains(skill));
        source.committed.retain(|path, _| {
            !state::administrative(Path::new(path))
                && !within_nested_source(path, &nested_roots)
                && !exclusions
                    .matched_path_or_any_parents(paths.home().join(path), false)
                    .is_ignore()
        });
        for path in source.committed.keys().cloned().collect::<Vec<_>>() {
            if !outside_user_directories(paths.home(), &path, &user_directories)? {
                source.committed.remove(&path);
            }
        }
        for skill in &source.skills {
            if !skill.is_empty() {
                state::relative(skill)?;
            }
            let logical = root.join(skill);
            let skill_path = state::home_relative(paths.home(), &logical)?;
            if !outside_user_directories(paths.home(), &skill_path, &user_directories)? {
                user_directories.insert(skill_path);
                continue;
            }
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
            if real == home || !real.starts_with(&home) {
                return Err(Error::input("skill resolves outside user home"));
            }
            if exclusions
                .matched_path_or_any_parents(&logical, true)
                .is_ignore()
            {
                continue;
            }
            collect(
                &CollectionScope {
                    home: paths.home(),
                    boundary: &real,
                    exclusions: &exclusions,
                    user_directories: &user_directories,
                    nested_roots: &nested_roots,
                },
                &logical,
                &mut source.files,
                true,
            )?;
        }
        for peer in history.peers.values() {
            for path in peer.files.keys() {
                if !state::transferable(paths.home(), path)?
                    || !outside_user_directories(paths.home(), path, &user_directories)?
                {
                    continue;
                }
                if path.starts_with(&format!("{}/", source.root)) {
                    if within_nested_source(path, &nested_roots) {
                        continue;
                    }
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
    let user_bytes = state::read_contained(paths.home(), ".agents/skillator.yaml")?;
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
    let mut acquisition_aliases = BTreeMap::new();
    if let Some(local) = locations.first() {
        for source in &sources {
            if source.root != local.path {
                continue;
            }
            for (path, entry) in &source.files {
                if !matches!(entry, Entry::Directory)
                    || Path::new(path).parent() != Some(Path::new(&local.path))
                    || !fs::symlink_metadata(paths.home().join(path))
                        .is_ok_and(|meta| meta.file_type().is_symlink())
                {
                    continue;
                }
                let target = Path::new(&physical_paths[path]);
                acquisition_aliases
                    .insert(path.clone(), state::home_relative(paths.home(), target)?);
            }
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
        protocol: 4,
        version: env!("CARGO_PKG_VERSION").into(),
        home: paths.home().canonicalize().map_err(Error::input_display)?,
        history,
        locations,
        sources,
        physical_paths,
        acquisition_aliases,
        user_directories,
        user: user_entries(&user_config)?,
        user_present: user_bytes.is_some(),
        library_hash: state::read_contained(paths.home(), ".skillator/library.yaml")?
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
    if !super::session::valid_origin(&origin) {
        return Err(Error::input(
            "Git origin contains credentials or an unsupported URL; configure a credential-free origin and use host authentication",
        ));
    }
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

fn within_nested_source(path: &str, nested_roots: &BTreeSet<String>) -> bool {
    nested_roots
        .iter()
        .any(|root| path == root || path.starts_with(&format!("{root}/")))
}

struct CollectionScope<'a> {
    home: &'a Path,
    boundary: &'a Path,
    exclusions: &'a ignore::gitignore::Gitignore,
    user_directories: &'a BTreeSet<String>,
    nested_roots: &'a BTreeSet<String>,
}

fn collect(
    scope: &CollectionScope<'_>,
    logical: &Path,
    files: &mut BTreeMap<String, Entry>,
    root: bool,
) -> Result<()> {
    if logical.file_name().is_some_and(|name| {
        name == ".git"
            || name
                .to_str()
                .is_some_and(crate::library::reserved_temporary)
    }) {
        return Ok(());
    }
    let path = state::home_relative(scope.home, logical)?;
    if within_nested_source(&path, scope.nested_roots) {
        return Ok(());
    }
    if !state::transferable(scope.home, &path)?
        || !outside_user_directories(scope.home, &path, scope.user_directories)?
    {
        return Ok(());
    }
    let actual = state::contained(scope.home, &path)?;
    let entry = if root {
        Some(Entry::Directory)
    } else {
        state::observe(&actual)?
    };
    let Some(entry) = entry else {
        return Ok(());
    };
    if scope
        .exclusions
        .matched_path_or_any_parents(logical, matches!(entry, Entry::Directory))
        .is_ignore()
    {
        return Ok(());
    }
    match &entry {
        Entry::Directory => {
            let resolved = logical.canonicalize().map_err(Error::input_display)?;
            if !resolved.starts_with(scope.boundary) {
                return Err(Error::input("internal skill directory escapes the skill"));
            }
            files.insert(path, entry.clone());
            for child in fs::read_dir(logical).map_err(Error::input_display)? {
                collect(
                    scope,
                    &child.map_err(Error::input_display)?.path(),
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
            if !resolved.starts_with(scope.boundary) {
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

pub(super) fn user_directories(home: &Path, config: &RepositoryConfig) -> Result<BTreeSet<String>> {
    let mut directories = BTreeSet::from([".agents/skills".into()]);
    directories.extend(
        config
            .skill_directories()
            .iter()
            .map(|directory| directory.path().as_str().to_owned()),
    );
    for directory in directories.clone() {
        if let Some(path) = state::observation_path(home, &directory)? {
            let physical = if path.exists() {
                path.canonicalize().map_err(Error::input_display)?
            } else {
                path
            };
            directories.insert(state::home_relative(home, &physical)?);
        }
    }
    Ok(directories)
}

pub(super) fn outside_user_directories(
    home: &Path,
    path: &str,
    directories: &BTreeSet<String>,
) -> Result<bool> {
    let protected = |path: &Path| {
        directories
            .iter()
            .any(|directory| path.starts_with(directory))
    };
    if protected(state::relative(path)?) {
        return Ok(false);
    }
    let Some(actual) = state::observation_path(home, path)? else {
        return Ok(true);
    };
    let physical = if actual.exists() {
        actual.canonicalize().map_err(Error::input_display)?
    } else {
        actual
    };
    Ok(!protected(
        physical
            .strip_prefix(home.canonicalize().map_err(Error::input_display)?)
            .map_err(Error::input_display)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symlinked_library_and_user_configuration_are_rejected_before_snapshot() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(home.path().into());
        fs::create_dir_all(home.path().join(".skillator")).unwrap();
        fs::create_dir_all(home.path().join(".agents")).unwrap();
        fs::write(
            outside.path().join("library.yaml"),
            "version: 1\nlocations: []\n",
        )
        .unwrap();
        fs::write(
            outside.path().join("skillator.yaml"),
            "version: 1\nskill_directories: []\nenablements: []\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path().join("library.yaml"), paths.library_config())
            .unwrap();
        std::os::unix::fs::symlink(outside.path().join("skillator.yaml"), paths.user_config())
            .unwrap();
        assert!(library(&paths).is_err());
        assert!(user(&paths).is_err());
        assert!(inspect(&paths, &[]).is_err());
        fs::remove_file(paths.library_config()).unwrap();
        assert!(inspect(&paths, &[]).is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("library.yaml")).unwrap(),
            "version: 1\nlocations: []\n"
        );
    }

    #[test]
    fn interrupted_publication_entries_are_not_discovered_as_skills() {
        let home = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(home.path().into());
        let library_path = home.path().join(".skillator/library");
        fs::create_dir_all(&library_path).unwrap();
        fs::write(
            paths.library_config(),
            "version: 1\nlocations: [{path: '~/.skillator/library'}]\n",
        )
        .unwrap();
        for prefix in [
            ".skillator-alias-",
            ".skillator-rsync-",
            ".skillator-clone-",
        ] {
            let staged = library_path.join(format!("{prefix}0123456789abcdef0123456789abcdef"));
            fs::create_dir(&staged).unwrap();
            fs::write(staged.join("SKILL.md"), "---\nname: staged\n---\n").unwrap();
        }
        let observed = scan_library(
            &library(&paths).unwrap(),
            &paths.library_config(),
            home.path(),
            paths.environment(),
        );
        assert!(
            observed
                .sources()
                .all(|source| source.skills().next().is_none())
        );
    }

    #[test]
    fn directory_collection_stays_inside_the_physical_skill_boundary() {
        let home = tempfile::tempdir().unwrap();
        let skill = home.path().join("library/demo");
        let unrelated = home.path().join("unrelated");
        fs::create_dir_all(skill.join("assets")).unwrap();
        fs::create_dir_all(&unrelated).unwrap();
        fs::write(skill.join("assets/inside.txt"), "inside").unwrap();
        fs::write(unrelated.join("private.txt"), "private").unwrap();
        let exclusions = ignore::gitignore::GitignoreBuilder::new(home.path())
            .build()
            .unwrap();
        let boundary = skill.canonicalize().unwrap();
        let mut files = BTreeMap::new();

        std::os::unix::fs::symlink("assets", skill.join("shortcut")).unwrap();
        let user_directories = BTreeSet::new();
        let nested_roots = BTreeSet::new();
        let scope = CollectionScope {
            home: home.path(),
            boundary: &boundary,
            exclusions: &exclusions,
            user_directories: &user_directories,
            nested_roots: &nested_roots,
        };
        collect(&scope, &skill.join("shortcut"), &mut files, true).unwrap();
        assert!(files.contains_key("library/demo/shortcut/inside.txt"));

        std::os::unix::fs::symlink(&unrelated, skill.join("escape")).unwrap();
        let error = collect(&scope, &skill.join("escape"), &mut files, true).unwrap_err();
        assert!(error.message.contains("directory escapes the skill"));
        assert!(!files.contains_key("library/demo/escape"));
        assert!(!files.contains_key("library/demo/escape/private.txt"));
    }

    #[test]
    fn credential_bearing_origin_never_enters_a_git_reference() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("repo");
        repository(&root);
        process::git(
            &root,
            &[
                "remote",
                "set-url",
                "origin",
                "https://user:secret@example.invalid/repo.git",
            ],
        )
        .unwrap();
        let error = git_ref(&root).unwrap_err();
        assert!(error.message.contains("credential-free origin"));
        assert!(!error.message.contains("secret"));
        process::git(
            &root,
            &[
                "remote",
                "set-url",
                "origin",
                "ssh://git@example.invalid/repo.git",
            ],
        )
        .unwrap();
        assert!(git_ref(&root).is_ok());
    }

    #[test]
    fn inaccessible_location_parent_is_not_treated_as_absent() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(home.path().into());
        let parent = home.path().join("restricted");
        let root = parent.join("skills");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(home.path().join(".skillator")).unwrap();
        fs::write(
            paths.library_config(),
            "version: 1\nlocations: [{path: '~/restricted/skills'}]\n",
        )
        .unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o000)).unwrap();
        let inaccessible = root.canonicalize().is_err();
        let observed = inspect(&paths, &[]);
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        if inaccessible {
            assert!(
                observed
                    .unwrap_err()
                    .message
                    .contains("library folder is unavailable")
            );
        }
        fs::remove_dir(&root).unwrap();
        assert!(inspect(&paths, &[]).is_ok());
        fs::write(&root, "not a directory").unwrap();
        assert!(inspect(&paths, &[]).is_err());
        assert!(!home.path().join(".skillator/rsync").exists());
    }

    #[test]
    fn unreadable_discovery_fails_preflight_unless_excluded() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(home.path().into());
        let directory = home.path().join(".skillator/library/hidden");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("SKILL.md"),
            "---\nname: hidden\ndescription: test\n---\n",
        )
        .unwrap();
        fs::write(
            paths.library_config(),
            "version: 1\nlocations: [{path: '~/.skillator/library'}]\n",
        )
        .unwrap();
        assert!(inspect(&paths, &[]).is_ok());
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable = fs::read_dir(&directory).is_err();
        let observed = inspect(&paths, &[]);
        fs::write(
            paths.library_config(),
            "version: 1\nlocations: [{path: '~/.skillator/library', exclusions: [hidden]}]\n",
        )
        .unwrap();
        let excluded = inspect(&paths, &[]);
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        if unreadable {
            assert!(
                observed
                    .unwrap_err()
                    .message
                    .contains("cannot discover skills")
            );
        }
        assert!(excluded.is_ok(), "{excluded:?}");
        assert!(!home.path().join(".skillator/rsync").exists());
    }

    #[test]
    fn home_rooted_locations_fail_with_specific_guidance_before_observation() {
        let home = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(home.path().into());
        fs::create_dir(home.path().join(".skillator")).unwrap();
        std::os::unix::fs::symlink(home.path(), home.path().join("home-alias")).unwrap();
        for expression in ["~", "~/home-alias"] {
            fs::write(
                paths.library_config(),
                format!("version: 1\nlocations: [{{path: '{expression}'}}]\n"),
            )
            .unwrap();
            let error = inspect(&paths, &[]).unwrap_err();
            assert!(
                error
                    .message
                    .contains("register directories below the user home"),
                "{error}"
            );
            assert!(!home.path().join(".skillator/rsync").exists());
        }
    }

    #[test]
    fn incoming_source_and_location_aliases_cannot_resolve_to_or_outside_home() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(home.path(), home.path().join("home-alias")).unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("outside-alias")).unwrap();
        fs::create_dir(home.path().join("library")).unwrap();
        let source = Source {
            key: "local/library".into(),
            root: "library".into(),
            location: "library".into(),
            exclusions: vec![],
            git: None,
            branch: None,
            skills: BTreeSet::new(),
            invalid_skills: BTreeSet::new(),
            files: BTreeMap::new(),
            committed: BTreeMap::new(),
            problems: vec![],
        };
        let paths = AppPaths::new(home.path().into());
        for alias in ["home-alias", "outside-alias"] {
            for location in [false, true] {
                let mut incoming = source.clone();
                if location {
                    incoming.location = alias.into();
                } else {
                    incoming.root = alias.into();
                }
                assert!(
                    inspect(&paths, &[incoming]).is_err(),
                    "{alias} location={location}"
                );
            }
        }
        assert!(!home.path().join(".skillator").exists());
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }

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
    fn parent_skill_does_not_collect_nested_git_source_content() {
        let home = tempfile::tempdir().unwrap();
        let parent = home.path().join("parent");
        fs::create_dir_all(&parent).unwrap();
        fs::write(
            parent.join("SKILL.md"),
            "---\nname: parent\ndescription: Parent skill\n---\n",
        )
        .unwrap();
        fs::write(parent.join("parent.txt"), "parent content").unwrap();
        let nested = parent.join("nested");
        repository(&nested);
        fs::write(nested.join("unrelated.txt"), "outside nested skill").unwrap();
        let paths = AppPaths::new(home.path().into());
        fs::create_dir(home.path().join(".skillator")).unwrap();
        fs::write(
            paths.library_config(),
            "version: 1\nlocations: [{path: '~/parent'}]\n",
        )
        .unwrap();
        let observed = inspect(&paths, &[]).unwrap();
        let parent_source = observed
            .sources
            .iter()
            .find(|source| source.root == "parent")
            .unwrap();
        let nested_source = observed
            .sources
            .iter()
            .find(|source| source.root == "parent/nested")
            .unwrap();
        assert!(parent_source.files.contains_key("parent/parent.txt"));
        assert!(
            parent_source
                .files
                .keys()
                .all(|path| !path.starts_with("parent/nested"))
        );
        assert!(
            parent_source
                .committed
                .keys()
                .all(|path| !path.starts_with("parent/nested"))
        );
        assert!(
            nested_source
                .files
                .contains_key("parent/nested/demo/SKILL.md")
        );
        assert!(
            !nested_source
                .files
                .contains_key("parent/nested/unrelated.txt")
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
