//! Filesystem traversal respecting source and replica ownership boundaries.

use super::*;

pub(super) fn scan_location(
    location_index: usize,
    config: &LibraryLocationConfig,
    root: &Path,
    snapshot: &mut LibrarySnapshot,
) {
    let exclusions = build_exclusions(root, config.exclusions(), &mut snapshot.diagnostics);
    let mut discovered = Vec::new();
    {
        let mut discovery = Discovery {
            location_root: root,
            exclusions: &exclusions,
            sources: &mut discovered,
        };
        if discovery.discover_replica(root, &mut snapshot.diagnostics) {
            // An owned replica is a container, not a source of its own.
        } else if is_git_root(root) {
            discovery.discover_source(root, SourceKind::Git);
        } else {
            let mut local =
                DiscoveredSource::new(root.to_owned(), PathBuf::from("."), SourceKind::Local);
            let is_local_library = location_index == 0;
            let contains_replica = discovery.discover_tree(root, &mut local, is_local_library);
            if !contains_replica || !local.skills.is_empty() {
                discovery.sources.push(local);
            } else {
                snapshot.diagnostics.append(&mut local.diagnostics);
            }
        }
    }

    for mut discovered_source in discovered {
        snapshot
            .diagnostics
            .append(&mut discovered_source.diagnostics);
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
    source_key: Option<SourceKey>,
    skills: Vec<LibrarySkill>,
    diagnostics: Vec<LibraryDiagnostic>,
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
            source_key: None,
            skills: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

struct SourceTraversal<'a> {
    directory: &'a Path,
    current: &'a mut DiscoveredSource,
    follow_root_skill_links: bool,
}

struct Discovery<'a> {
    location_root: &'a Path,
    exclusions: &'a Gitignore,
    sources: &'a mut Vec<DiscoveredSource>,
}

impl Discovery<'_> {
    fn discover_source(&mut self, source_root: &Path, kind: SourceKind) -> bool {
        let relative = source_root
            .strip_prefix(self.location_root)
            .unwrap_or(Path::new("."))
            .to_owned();
        let relative = if relative.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            relative
        };
        let mut source = DiscoveredSource::new(source_root.to_owned(), relative, kind);
        let contains_replica = self.discover_tree(source_root, &mut source, false);
        self.sources.push(source);
        contains_replica
    }

    fn discover_tree(
        &mut self,
        directory: &Path,
        current: &mut DiscoveredSource,
        follow_root_skill_links: bool,
    ) -> bool {
        let relative_to_source = directory
            .strip_prefix(&current.root)
            .unwrap_or(Path::new("."));
        if directory.join("SKILL.md").is_file() {
            current
                .skills
                .push(read_skill(directory, relative_to_source));
        }
        let Some(entries) = read_entries(directory, &mut current.diagnostics) else {
            return false;
        };
        let mut contains_replica = false;
        for entry in entries {
            contains_replica |= self.discover_tree_entry(
                entry,
                SourceTraversal {
                    directory,
                    current,
                    follow_root_skill_links,
                },
            );
        }
        contains_replica
    }

    fn discover_tree_entry(&mut self, entry: fs::DirEntry, traversal: SourceTraversal<'_>) -> bool {
        let SourceTraversal {
            directory,
            current,
            follow_root_skill_links,
        } = traversal;
        let Some((path, file_type)) = inspect_entry(&entry, &mut current.diagnostics) else {
            return false;
        };
        let follows_links = follow_root_skill_links && directory == current.root;
        if follows_links && is_skill_symlink(&path, &file_type) {
            self.read_linked_skill(&path, current);
            return false;
        }
        if !is_regular_directory(&file_type) || self.excluded(&path) {
            return false;
        }
        self.discover_child(&path, current, follow_root_skill_links)
    }

    fn discover_child(
        &mut self,
        path: &Path,
        current: &mut DiscoveredSource,
        follow_root_skill_links: bool,
    ) -> bool {
        if current.source_key.is_none() && self.discover_replica(path, &mut current.diagnostics) {
            true
        } else if current.source_key.is_none() && is_git_root(path) {
            self.discover_source(path, SourceKind::Git)
        } else {
            self.discover_tree(path, current, follow_root_skill_links)
        }
    }

    fn excluded(&self, path: &Path) -> bool {
        let relative = path.strip_prefix(self.location_root).unwrap_or(path);
        self.exclusions
            .matched_path_or_any_parents(relative, true)
            .is_ignore()
    }

    fn read_linked_skill(&self, path: &Path, current: &mut DiscoveredSource) {
        if !self.excluded(path) {
            let relative = path.strip_prefix(self.location_root).unwrap_or(path);
            current.skills.push(read_skill(path, relative));
        }
    }

    /// Recognize ownership before ordinary discovery can flatten a replica.
    /// Invalid markers also establish a boundary, but never provide inventory.
    fn discover_replica(&mut self, root: &Path, diagnostics: &mut Vec<LibraryDiagnostic>) -> bool {
        let marker = root.join(MARKER_NAME);
        let valid = match fs::symlink_metadata(&marker) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                match fs::read(&marker) {
                    Ok(bytes) => bytes == MARKER_CONTENT.as_bytes(),
                    Err(error) => {
                        diagnostics.push(discovery_error(&marker, error));
                        false
                    }
                }
            }
            Ok(_) => false,
            Err(error) => {
                diagnostics.push(discovery_error(&marker, error));
                false
            }
        };
        if valid {
            self.discover_replica_sources(root, root, diagnostics);
        } else {
            diagnostics.push(LibraryDiagnostic {
                code: "invalid_replica_marker",
                message: format!("replica ownership marker is invalid: {}", marker.display()),
                path: Some(marker),
            });
        }
        true
    }

    fn discover_replica_sources(
        &mut self,
        directory: &Path,
        replica_root: &Path,
        diagnostics: &mut Vec<LibraryDiagnostic>,
    ) {
        let Some(entries) = read_entries(directory, diagnostics) else {
            return;
        };
        for entry in entries {
            let Some((path, file_type)) = inspect_entry(&entry, diagnostics) else {
                continue;
            };
            if !is_regular_directory(&file_type) || self.excluded(&path) {
                continue;
            }
            if entry.file_name() == OsStr::new("_skills") {
                let relative = directory.strip_prefix(replica_root).unwrap_or(directory);
                self.discover_replica_source(&path, relative, diagnostics);
            } else {
                self.discover_replica_sources(&path, replica_root, diagnostics);
            }
        }
    }

    fn discover_replica_source(
        &mut self,
        path: &Path,
        relative_to_replica: &Path,
        diagnostics: &mut Vec<LibraryDiagnostic>,
    ) {
        let key = match SourceKey::parse(path_text(relative_to_replica)) {
            Ok(key) => key,
            Err(error) => {
                diagnostics.push(LibraryDiagnostic {
                    code: "invalid_replica_source",
                    message: error.to_string(),
                    path: Some(path.to_owned()),
                });
                return;
            }
        };
        let relative_to_location = path.strip_prefix(self.location_root).unwrap_or(path);
        let mut source = DiscoveredSource::new(
            path.to_owned(),
            relative_to_location.to_owned(),
            SourceKind::Local,
        );
        source.source_key = Some(key);
        self.discover_tree(path, &mut source, false);
        self.sources.push(source);
    }
}

fn is_git_root(path: &Path) -> bool {
    let marker = path.join(".git");
    marker.is_dir() || marker.is_file()
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
    source
        .source_key
        .clone()
        .unwrap_or_else(|| suggested_source_key(&source.root, source.origin.as_deref()))
}

fn discovery_error(path: &Path, error: std::io::Error) -> LibraryDiagnostic {
    LibraryDiagnostic {
        code: "discovery_incomplete",
        message: format!("cannot inspect {}: {error}", path.display()),
        path: Some(path.to_owned()),
    }
}

fn read_entries(
    directory: &Path,
    diagnostics: &mut Vec<LibraryDiagnostic>,
) -> Option<Vec<fs::DirEntry>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(discovery_error(directory, error));
            return None;
        }
    };
    let mut entries: Vec<_> = entries
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(error) => {
                diagnostics.push(discovery_error(directory, error));
                None
            }
        })
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    Some(entries)
}

fn inspect_entry(
    entry: &fs::DirEntry,
    diagnostics: &mut Vec<LibraryDiagnostic>,
) -> Option<(PathBuf, fs::FileType)> {
    if entry.file_name() == OsStr::new(".git") {
        return None;
    }
    let path = entry.path();
    match entry.file_type() {
        Ok(kind) => Some((path, kind)),
        Err(error) => {
            diagnostics.push(discovery_error(&path, error));
            None
        }
    }
}

fn is_regular_directory(file_type: &fs::FileType) -> bool {
    file_type.is_dir() && !file_type.is_symlink()
}
fn is_skill_symlink(path: &Path, file_type: &fs::FileType) -> bool {
    file_type.is_symlink() && path.join("SKILL.md").is_file()
}
