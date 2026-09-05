//! Library discovery and immutable snapshots.

use crate::config::{LibraryConfig, LibraryLocationConfig};
use crate::domain::{SkillKey, SourceKey};
use crate::git::GitRepository;
use crate::materialization::{EntryFingerprint, skill_fingerprint};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillValidity {
    Valid,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Local,
    Git,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct LibrarySkill {
    path: String,
    name: Option<String>,
    description: Option<String>,
    validity: SkillValidity,
    available: bool,
    diagnostics: Vec<String>,
    absolute_path: Option<PathBuf>,
    fingerprint: EntryFingerprint,
}

impl LibrarySkill {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn validity(&self) -> SkillValidity {
        self.validity
    }

    pub fn available(&self) -> bool {
        self.available
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub fn absolute_path(&self) -> Option<&Path> {
        self.absolute_path.as_deref()
    }

    pub(crate) fn fingerprint(&self) -> &EntryFingerprint {
        &self.fingerprint
    }
}

#[derive(Debug, Clone)]
pub struct LibrarySource {
    key: SourceKey,
    suggested_key: SourceKey,
    kind: SourceKind,
    available: bool,
    root: Option<PathBuf>,
    origin: Option<String>,
    skills: BTreeMap<String, LibrarySkill>,
    location_index: usize,
    relative_path: String,
    key_collision: bool,
}

impl LibrarySource {
    pub fn key(&self) -> &SourceKey {
        &self.key
    }

    pub fn suggested_key(&self) -> &SourceKey {
        &self.suggested_key
    }

    pub fn kind(&self) -> SourceKind {
        self.kind
    }

    pub fn available(&self) -> bool {
        self.available
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn origin(&self) -> Option<&str> {
        self.origin.as_deref()
    }

    pub fn skills(&self) -> impl Iterator<Item = &LibrarySkill> {
        self.skills.values()
    }

    pub fn location_index(&self) -> usize {
        self.location_index
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn key_collision(&self) -> bool {
        self.key_collision
    }

    pub fn skill(&self, path: &str) -> Option<&LibrarySkill> {
        self.skills.get(path)
    }
}

#[derive(Debug, Clone)]
pub struct LibraryLocation {
    expression: String,
    resolved: Option<PathBuf>,
    available: bool,
}

impl LibraryLocation {
    pub fn expression(&self) -> &str {
        &self.expression
    }

    pub fn resolved(&self) -> Option<&Path> {
        self.resolved.as_deref()
    }

    pub fn available(&self) -> bool {
        self.available
    }
}

#[derive(Debug, Clone)]
pub struct LibrarySnapshot {
    locations: Vec<LibraryLocation>,
    sources: BTreeMap<String, LibrarySource>,
    diagnostics: Vec<LibraryDiagnostic>,
    overlap_advisory_locations: BTreeSet<usize>,
}

impl LibrarySnapshot {
    pub fn locations(&self) -> &[LibraryLocation] {
        &self.locations
    }

    pub fn sources(&self) -> impl ExactSizeIterator<Item = &LibrarySource> {
        self.sources.values()
    }

    pub fn source(&self, key: &str) -> Option<&LibrarySource> {
        self.sources.get(key)
    }

    pub fn diagnostics(&self) -> &[LibraryDiagnostic] {
        &self.diagnostics
    }

    pub fn resolve(&self, key: &SkillKey) -> Option<&LibrarySkill> {
        self.source(key.source().as_str())
            .and_then(|source| source.skill(key.path().as_str()))
            .filter(|skill| skill.available && skill.validity == SkillValidity::Valid)
    }

    pub fn has_overlap_advisory(&self, key: &SkillKey) -> bool {
        self.source(key.source().as_str()).is_some_and(|source| {
            self.overlap_advisory_locations
                .contains(&source.location_index)
        })
    }

    pub fn location_has_overlap_advisory(&self, location_index: usize) -> bool {
        self.overlap_advisory_locations.contains(&location_index)
    }
}

pub fn scan_library(
    config: &LibraryConfig,
    config_path: &Path,
    home: &Path,
    environment: &BTreeMap<String, String>,
) -> LibrarySnapshot {
    let mut snapshot = LibrarySnapshot {
        locations: Vec::new(),
        sources: BTreeMap::new(),
        diagnostics: Vec::new(),
        overlap_advisory_locations: BTreeSet::new(),
    };
    let mut resolved_locations = Vec::<(usize, PathBuf, bool)>::new();
    for (location_index, location_config) in config.locations().iter().enumerate() {
        let expanded = match expand_location(
            location_config.path(),
            config_path.parent().unwrap_or_else(|| Path::new(".")),
            home,
            environment,
        ) {
            Ok(path) => path,
            Err(message) => {
                snapshot.locations.push(LibraryLocation {
                    expression: location_config.path().to_owned(),
                    resolved: None,
                    available: false,
                });
                snapshot.diagnostics.push(LibraryDiagnostic {
                    code: "location_expansion_failed",
                    message,
                    path: None,
                });
                continue;
            }
        };
        let canonical = match expanded.canonicalize() {
            Ok(path) if path.is_dir() => path,
            _ => {
                snapshot.locations.push(LibraryLocation {
                    expression: location_config.path().to_owned(),
                    resolved: Some(expanded.clone()),
                    available: false,
                });
                snapshot.diagnostics.push(LibraryDiagnostic {
                    code: "location_unavailable",
                    message: format!("library folder is unavailable: {}", expanded.display()),
                    path: Some(expanded),
                });
                continue;
            }
        };
        for (other_index, other, other_allowed) in &resolved_locations {
            if path_overlap(&canonical, other) {
                let allowed = location_config.allow_overlap() && *other_allowed;
                snapshot.diagnostics.push(LibraryDiagnostic {
                    code: if allowed {
                        "overlapping_locations_allowed"
                    } else {
                        "overlapping_locations"
                    },
                    message: format!(
                        "library folders overlap{}: {} and {}",
                        if allowed {
                            " as allowed in the configuration"
                        } else {
                            ""
                        },
                        other.display(),
                        canonical.display()
                    ),
                    path: Some(canonical.clone()),
                });
                if allowed {
                    snapshot.overlap_advisory_locations.insert(location_index);
                    snapshot.overlap_advisory_locations.insert(*other_index);
                }
            }
        }
        resolved_locations.push((
            location_index,
            canonical.clone(),
            location_config.allow_overlap(),
        ));
        snapshot.locations.push(LibraryLocation {
            expression: location_config.path().to_owned(),
            resolved: Some(canonical.clone()),
            available: true,
        });
        scan_location(location_index, location_config, &canonical, &mut snapshot);
    }
    snapshot
}

pub(crate) fn expand_location(
    expression: &str,
    config_parent: &Path,
    home: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    let mut expanded = String::new();
    let mut remaining = expression;
    while let Some(start) = remaining.find("${") {
        expanded.push_str(&remaining[..start]);
        let rest = &remaining[start + 2..];
        let Some(end) = rest.find('}') else {
            return Err(format!("invalid variable expression `{expression}`"));
        };
        let name = &rest[..end];
        let Some(value) = environment.get(name) else {
            return Err(format!("environment variable `{name}` is not set"));
        };
        expanded.push_str(value);
        remaining = &rest[end + 1..];
    }
    expanded.push_str(remaining);
    let path = if expanded == "~" {
        home.to_owned()
    } else if let Some(relative) = expanded.strip_prefix("~/") {
        home.join(relative)
    } else {
        let path = PathBuf::from(expanded);
        if path.is_absolute() {
            path
        } else {
            config_parent.join(path)
        }
    };
    // Keep the configured expression intact, but present and use a clean resolved
    // path. `components` removes redundant `.` components without collapsing
    // `..`, whose meaning can depend on symlinks.
    Ok(path.components().collect())
}

pub(crate) fn suggested_source_key(root: &Path, origin: Option<&str>) -> SourceKey {
    if let Some(origin) = origin
        && let Some(candidate) = key_from_remote(origin)
        && let Ok(key) = SourceKey::parse(candidate)
    {
        return key;
    }
    let name = root
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("library")
        .to_ascii_lowercase()
        .replace('_', "-");
    SourceKey::parse(format!("local/{name}"))
        .unwrap_or_else(|_| SourceKey::parse("local/library").expect("fallback key is valid"))
}

fn path_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn scan_location(
    location_index: usize,
    config: &LibraryLocationConfig,
    root: &Path,
    snapshot: &mut LibrarySnapshot,
) {
    let exclusions = build_exclusions(root, config.exclusions(), &mut snapshot.diagnostics);
    let mut discovered = Vec::new();
    if is_git_root(root) {
        discover_source(root, root, SourceKind::Git, &exclusions, &mut discovered);
    } else {
        let mut local =
            DiscoveredSource::new(root.to_owned(), PathBuf::from("."), SourceKind::Local);
        let is_local_library = location_index == 0;
        discover_tree(
            root,
            root,
            &exclusions,
            &mut local,
            &mut discovered,
            is_local_library,
        );
        discovered.push(local);
    }

    for discovered_source in discovered {
        insert_discovered_source(snapshot, location_index, discovered_source);
    }
}

fn build_exclusions(
    root: &Path,
    patterns: &[String],
    diagnostics: &mut Vec<LibraryDiagnostic>,
) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    for pattern in patterns {
        if let Err(error) = builder.add_line(None, pattern) {
            diagnostics.push(LibraryDiagnostic {
                code: "invalid_exclusion",
                message: error.to_string(),
                path: Some(root.to_owned()),
            });
        }
    }
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

#[derive(Debug)]
struct DiscoveredSource {
    root: PathBuf,
    relative: PathBuf,
    kind: SourceKind,
    origin: Option<String>,
    skills: Vec<LibrarySkill>,
}

impl DiscoveredSource {
    fn new(root: PathBuf, relative: PathBuf, kind: SourceKind) -> Self {
        let origin = (kind == SourceKind::Git)
            .then(|| {
                GitRepository::discover(&root)
                    .ok()
                    .and_then(|git| git.origin().ok().flatten())
            })
            .flatten();
        Self {
            root,
            relative,
            kind,
            origin,
            skills: Vec::new(),
        }
    }
}

fn discover_source(
    location_root: &Path,
    source_root: &Path,
    kind: SourceKind,
    exclusions: &Gitignore,
    sources: &mut Vec<DiscoveredSource>,
) {
    let relative = source_root
        .strip_prefix(location_root)
        .unwrap_or(Path::new("."))
        .to_owned();
    let relative = if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative
    };
    let mut source = DiscoveredSource::new(source_root.to_owned(), relative, kind);
    discover_tree(
        source_root,
        location_root,
        exclusions,
        &mut source,
        sources,
        false,
    );
    sources.push(source);
}

fn discover_tree(
    directory: &Path,
    location_root: &Path,
    exclusions: &Gitignore,
    current: &mut DiscoveredSource,
    sources: &mut Vec<DiscoveredSource>,
    follow_root_skill_links: bool,
) {
    let relative_to_source = directory
        .strip_prefix(&current.root)
        .unwrap_or(Path::new("."));
    if directory.join("SKILL.md").is_file() {
        current
            .skills
            .push(read_skill(directory, relative_to_source));
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if entry.file_name() == OsStr::new(".git") {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink()
            && follow_root_skill_links
            && directory == current.root
            && path.join("SKILL.md").is_file()
        {
            let relative_to_location = path.strip_prefix(location_root).unwrap_or(&path);
            if !exclusions
                .matched_path_or_any_parents(relative_to_location, true)
                .is_ignore()
            {
                current.skills.push(read_skill(&path, relative_to_location));
            }
            continue;
        }
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let relative_to_location = path.strip_prefix(location_root).unwrap_or(&path);
        if exclusions
            .matched_path_or_any_parents(relative_to_location, true)
            .is_ignore()
        {
            continue;
        }
        if is_git_root(&path) {
            discover_source(location_root, &path, SourceKind::Git, exclusions, sources);
        } else {
            discover_tree(
                &path,
                location_root,
                exclusions,
                current,
                sources,
                follow_root_skill_links,
            );
        }
    }
}

fn is_git_root(path: &Path) -> bool {
    let marker = path.join(".git");
    marker.is_dir() || marker.is_file()
}

#[derive(Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
}

fn read_skill(directory: &Path, relative: &Path) -> LibrarySkill {
    let path = path_text(relative);
    let absolute_path = directory.canonicalize().ok();
    let fingerprint = absolute_path
        .as_deref()
        .map(skill_fingerprint)
        .unwrap_or(EntryFingerprint::Uninspectable);
    let bytes = fs::read(directory.join("SKILL.md"));
    let mut diagnostics = Vec::new();
    let metadata = match bytes {
        Ok(bytes) => parse_frontmatter(&bytes),
        Err(error) => Err(format!("cannot read SKILL.md: {error}")),
    };
    match metadata {
        Ok(metadata) => {
            let basename = directory.file_name().and_then(OsStr::to_str);
            if !valid_skill_name(&metadata.name) {
                diagnostics.push("SKILL.md name must be 1 to 64 lowercase letters, digits, or single hyphens, with no leading or trailing hyphen".to_owned());
            }
            if !relative.as_os_str().is_empty()
                && relative != Path::new(".")
                && basename != Some(metadata.name.as_str())
            {
                diagnostics.push(format!(
                    "SKILL.md name `{}` does not match directory",
                    metadata.name
                ));
            }
            if metadata.description.trim().is_empty() {
                diagnostics.push("SKILL.md description is empty".to_owned());
            }
            LibrarySkill {
                path,
                name: Some(metadata.name),
                description: Some(metadata.description),
                validity: if diagnostics.is_empty() {
                    SkillValidity::Valid
                } else {
                    SkillValidity::Invalid
                },
                available: true,
                diagnostics,
                absolute_path,
                fingerprint,
            }
        }
        Err(error) => LibrarySkill {
            path,
            name: None,
            description: None,
            validity: SkillValidity::Invalid,
            available: true,
            diagnostics: vec![error],
            absolute_path,
            fingerprint,
        },
    }
}

fn parse_frontmatter(bytes: &[u8]) -> Result<SkillFrontmatter, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|error| format!("SKILL.md is not UTF-8: {error}"))?;
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return Err("SKILL.md must start with YAML metadata between --- lines".to_owned());
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err("SKILL.md metadata is missing its closing --- line".to_owned());
    }
    serde_saphyr::from_str(&yaml).map_err(|error| error.to_string())
}

pub(crate) fn validated_skill_metadata_at(directory: &Path) -> Option<(String, String)> {
    let metadata = fs::read(directory.join("SKILL.md"))
        .ok()
        .and_then(|bytes| parse_frontmatter(&bytes).ok())?;
    (valid_skill_name(&metadata.name) && !metadata.description.trim().is_empty())
        .then_some((metadata.name, metadata.description))
}

pub(crate) fn validated_skill_name_at(directory: &Path) -> Option<String> {
    validated_skill_metadata_at(directory).map(|(name, _)| name)
}

fn valid_skill_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn path_text(path: &Path) -> String {
    if path.as_os_str().is_empty() || path == Path::new(".") {
        ".".to_owned()
    } else {
        path.components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn insert_discovered_source(
    snapshot: &mut LibrarySnapshot,
    location_index: usize,
    source: DiscoveredSource,
) {
    let suggested = suggest_source_key(&source);
    let key_text = suggested.as_str().to_owned();
    let relative_path = path_text(&source.relative);
    if snapshot.sources.contains_key(&key_text) {
        snapshot.diagnostics.push(LibraryDiagnostic {
            code: "source_key_collision",
            message: format!("More than one source uses the name `{suggested}`"),
            path: Some(source.root.clone()),
        });
        let inventory_key = format!(
            "collision:{location_index}:{}",
            source.root.to_string_lossy()
        );
        snapshot.sources.insert(
            inventory_key,
            LibrarySource {
                key: suggested.clone(),
                suggested_key: suggested,
                kind: source.kind,
                available: true,
                root: Some(source.root),
                origin: source.origin,
                skills: source
                    .skills
                    .into_iter()
                    .map(|skill| (skill.path.clone(), skill))
                    .collect(),
                location_index,
                relative_path,
                key_collision: true,
            },
        );
        return;
    }
    snapshot.sources.insert(
        key_text,
        LibrarySource {
            key: suggested.clone(),
            suggested_key: suggested,
            kind: source.kind,
            available: true,
            root: Some(source.root),
            origin: source.origin,
            skills: source
                .skills
                .into_iter()
                .map(|skill| (skill.path.clone(), skill))
                .collect(),
            location_index,
            relative_path,
            key_collision: false,
        },
    );
}

fn suggest_source_key(source: &DiscoveredSource) -> SourceKey {
    suggested_source_key(&source.root, source.origin.as_deref())
}

fn key_from_remote(remote: &str) -> Option<String> {
    let path = if let Some((_, path)) = remote.rsplit_once(':')
        && remote.contains('@')
    {
        path
    } else {
        remote.split("//").nth(1)?.split_once('/')?.1
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let segments: Vec<_> = path.split('/').filter(|part| !part.is_empty()).collect();
    (segments.len() >= 2).then(|| {
        format!(
            "{}/{}",
            segments[segments.len() - 2].to_ascii_lowercase(),
            segments[segments.len() - 1].to_ascii_lowercase()
        )
    })
}
