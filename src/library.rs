//! Library discovery and immutable snapshots.

mod metadata;
use metadata::read_skill;
pub(crate) use metadata::{inspect_skill_metadata, validated_skill_name_at};
mod discovery;
use discovery::scan_location;

use crate::config::{LibraryConfig, LibraryLocationConfig};
use crate::domain::{SkillKey, SourceKey};
use crate::git::GitRepository;
use crate::materialization::{EntryFingerprint, skill_fingerprint};
use crate::remote::{MARKER_CONTENT, MARKER_NAME};
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
    warnings: Vec<String>,
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

    pub fn warnings(&self) -> &[String] {
        &self.warnings
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
        if self
            .sources
            .values()
            .filter(|source| source.key() == key.source())
            .count()
            != 1
        {
            return None;
        }
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
    let context = LocationEnvironment {
        config_parent: config_path.parent().unwrap_or_else(|| Path::new(".")),
        home,
        environment,
    };
    let mut resolved_locations = Vec::new();
    for (location_index, location_config) in config.locations().iter().enumerate() {
        let canonical = match context.resolve(location_config) {
            Ok(path) => path,
            Err(failure) => {
                snapshot.locations.push(failure.location);
                snapshot.diagnostics.push(failure.diagnostic);
                continue;
            }
        };
        let candidate = ResolvedLocation {
            index: location_index,
            path: canonical.clone(),
            allow_overlap: location_config.allow_overlap(),
        };
        check_location_overlaps(&mut snapshot, &candidate, &resolved_locations);
        resolved_locations.push(candidate);
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
    if let Some(key) = origin
        .and_then(key_from_remote)
        .and_then(|candidate| SourceKey::parse(candidate).ok())
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

struct LocationEnvironment<'a> {
    config_parent: &'a Path,
    home: &'a Path,
    environment: &'a BTreeMap<String, String>,
}

struct LocationFailure {
    location: LibraryLocation,
    diagnostic: LibraryDiagnostic,
}

impl LocationEnvironment<'_> {
    fn resolve(&self, config: &LibraryLocationConfig) -> Result<PathBuf, LocationFailure> {
        let expanded = expand_location(
            config.path(),
            self.config_parent,
            self.home,
            self.environment,
        )
        .map_err(|message| LocationFailure {
            location: LibraryLocation {
                expression: config.path().to_owned(),
                resolved: None,
                available: false,
            },
            diagnostic: LibraryDiagnostic {
                code: "location_expansion_failed",
                message,
                path: None,
            },
        })?;
        match expanded.canonicalize() {
            Ok(path) if path.is_dir() => Ok(path),
            _ => Err(LocationFailure {
                location: LibraryLocation {
                    expression: config.path().to_owned(),
                    resolved: Some(expanded.clone()),
                    available: false,
                },
                diagnostic: LibraryDiagnostic {
                    code: "location_unavailable",
                    message: format!("library folder is unavailable: {}", expanded.display()),
                    path: Some(expanded),
                },
            }),
        }
    }
}

struct ResolvedLocation {
    index: usize,
    path: PathBuf,
    allow_overlap: bool,
}

fn check_location_overlaps(
    snapshot: &mut LibrarySnapshot,
    candidate: &ResolvedLocation,
    previous: &[ResolvedLocation],
) {
    for other in previous {
        if path_overlap(&candidate.path, &other.path) {
            let allowed = candidate.allow_overlap && other.allow_overlap;
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
                    other.path.display(),
                    candidate.path.display()
                ),
                path: Some(candidate.path.clone()),
            });
            if allowed {
                snapshot.overlap_advisory_locations.insert(candidate.index);
                snapshot.overlap_advisory_locations.insert(other.index);
            }
        }
    }
}
