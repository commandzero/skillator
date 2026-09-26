use super::{
    Error, Result, process,
    snapshot::{self, Location, Snapshot, Source},
    state::{self, Baseline, Entry, History},
};
use crate::app::{AppPaths, CommandReport, UserScopeWorkflow};
use crate::config::{Fingerprint, LibraryLocationConfig, save_library};
use crate::fs_safety::{rename_exchange, rename_noreplace};
use crate::reconcile::TargetLocks;
use crate::target::Target;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Request {
    Inspect {
        sources: Vec<Source>,
    },
    Begin {
        token: String,
    },
    Lock {
        token: String,
    },
    Bootstrap {
        source: Source,
    },
    Export {
        path: String,
        expected: Entry,
    },
    Publish {
        path: String,
        expected: Option<Entry>,
        desired: Option<Entry>,
        stage: Option<String>,
    },
    Alias {
        path: String,
        target: String,
    },
    Register {
        locations: Vec<Location>,
        expected: Option<String>,
    },
    User {
        entries: BTreeMap<String, String>,
        expected: Option<String>,
        check: bool,
    },
    Acknowledge {
        peers: BTreeMap<String, Baseline>,
        aliases: BTreeMap<String, String>,
        #[serde(default)]
        provenance: BTreeMap<String, state::GitProvenance>,
    },
    Finish,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Response {
    Snapshot {
        snapshot: Box<Snapshot>,
        token: String,
    },
    Begun {
        id: String,
        stage: String,
        recovered: usize,
    },
    Exported {
        path: String,
    },
    User {
        report: CommandReport,
    },
    Ok,
    Error {
        code: u8,
        message: String,
    },
}

pub(super) struct Session {
    pub paths: AppPaths,
    last: Option<(Snapshot, Vec<Source>, String)>,
    stage: Option<PathBuf>,
    session_lock: Option<File>,
    user_lock: Option<TargetLocks>,
    history: History,
    history_expected: Fingerprint,
}

impl Session {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            last: None,
            stage: None,
            session_lock: None,
            user_lock: None,
            history: History::default(),
            history_expected: Fingerprint::Absent,
        }
    }

    pub fn handle(&mut self, request: Request) -> Result<Response> {
        match request {
            Request::Inspect { sources } => {
                let _observation_lock = if self.stage.is_none() {
                    let target = Target::user(self.paths.home()).map_err(Error::input_display)?;
                    Some(TargetLocks::acquire(&[&target]).map_err(|_| Error::busy())?)
                } else {
                    None
                };
                process::capture(Command::new("git").arg("--version"))
                    .map_err(|error| Error::input(format!("Git is not available: {error}")))?;
                process::capture(Command::new("rsync").arg("--version"))
                    .map_err(|error| Error::input(format!("rsync is not available: {error}")))?;
                let snapshot = snapshot::inspect(&self.paths, &sources)?;
                let token =
                    state::digest(&serde_json::to_vec(&snapshot).map_err(Error::input_display)?);
                self.last = Some((snapshot.clone(), sources, token.clone()));
                Ok(Response::Snapshot {
                    snapshot: Box::new(snapshot),
                    token,
                })
            }
            Request::Begin { token } => self.begin(&token),
            Request::Lock { token } => {
                self.lock(&token)?;
                Ok(Response::Ok)
            }
            Request::Bootstrap { source } => {
                self.require_active()?;
                self.bootstrap(&source)?;
                Ok(Response::Ok)
            }
            Request::Export { path, expected } => {
                self.require_active()?;
                self.authorize_path(&path)?;
                let source = state::contained(self.paths.home(), &path)?;
                if self.observed_content(&path, &source, Some(&expected))? != Some(expected.clone())
                {
                    return Err(Error::input("source changed after observation"));
                }
                let stage = self.stage.as_ref().unwrap().join(state::new_id()?);
                copy_entry(&source, &stage, &expected)?;
                if state::observe(&stage)? != Some(expected) {
                    return Err(Error::input("source changed during export"));
                }
                Ok(Response::Exported {
                    path: stage.to_string_lossy().into_owned(),
                })
            }
            Request::Publish {
                path,
                expected,
                desired,
                stage,
            } => {
                self.require_active()?;
                self.authorize_path(&path)?;
                self.publish(&path, expected.as_ref(), desired.as_ref(), stage.as_deref())?;
                Ok(Response::Ok)
            }
            Request::Alias { path, target } => {
                self.require_active()?;
                self.publish_alias(&path, &target)?;
                Ok(Response::Ok)
            }
            Request::Register {
                locations,
                expected,
            } => {
                self.require_active()?;
                self.register(&locations, expected)?;
                Ok(Response::Ok)
            }
            Request::User {
                entries,
                expected,
                check,
            } => {
                let path = state::contained(self.paths.home(), ".agents/skillator.yaml")?;
                let bytes = state::read_optional(&path)?;
                if bytes.as_ref().map(|bytes| state::digest(bytes)) != expected {
                    return Err(Error::input("user configuration changed after observation"));
                }
                let fingerprint = bytes
                    .as_deref()
                    .map(Fingerprint::for_bytes)
                    .unwrap_or(Fingerprint::Absent);
                let desired = snapshot::user_from_entries(&entries)?;
                let target = Target::user(self.paths.home()).map_err(Error::input_display)?;
                let locks = if check {
                    TargetLocks::acquire(&[&target]).map_err(|_| Error::busy())?
                } else {
                    self.require_active()?;
                    self.user_lock.take().ok_or_else(|| {
                        Error::input("user state already reconciled in this session")
                    })?
                };
                let report =
                    UserScopeWorkflow::save_remote(&self.paths, desired, fingerprint, locks, check)
                        .map_err(Error::input_display)?;
                Ok(Response::User { report })
            }
            Request::Acknowledge {
                peers,
                aliases,
                provenance,
            } => {
                self.require_active()?;
                for (peer, baseline) in &peers {
                    if !state::valid_id(peer) || Some(peer) == self.history.id.as_ref() {
                        return Err(Error::input("invalid peer identity"));
                    }
                    for (path, expected) in &baseline.files {
                        self.authorize_path(path)?;
                        if baseline.contexts.get(path)
                            != snapshot::file_context(&self.last.as_ref().unwrap().0, path).as_ref()
                        {
                            return Err(Error::input(
                                "source context changed before acknowledgement",
                            ));
                        }
                        let actual = match state::observation_path(self.paths.home(), path)? {
                            Some(actual) => {
                                self.observed_content(path, &actual, expected.as_ref())?
                            }
                            None => None,
                        };
                        if actual != *expected {
                            return Err(Error::input("content changed before acknowledgement"));
                        }
                    }
                    let user = snapshot::user_entries(&snapshot::user(&self.paths)?)?;
                    for (key, value) in &baseline.user {
                        if user.get(key) != value.as_ref() {
                            return Err(Error::input("user state changed before acknowledgement"));
                        }
                    }
                }
                for (root, reference) in &provenance {
                    let observed = self
                        .last
                        .as_ref()
                        .unwrap()
                        .0
                        .sources
                        .iter()
                        .find(|source| &source.root == root)
                        .ok_or_else(|| Error::input("unobserved provenance source"))?;
                    let actual = snapshot::git_ref(&state::contained(self.paths.home(), root)?)?;
                    if observed.key != reference.source
                        || actual.origin != reference.origin
                        || actual.commit != reference.commit
                    {
                        return Err(Error::input(
                            "Git source changed before recording provenance",
                        ));
                    }
                }
                for (peer, baseline) in peers {
                    let target = self.history.peers.entry(peer).or_default();
                    target.files.extend(baseline.files);
                    target.contexts.extend(baseline.contexts);
                    target.user.extend(baseline.user);
                }
                if aliases.values().any(|id| !state::valid_id(id)) {
                    return Err(Error::input("invalid alias identity"));
                }
                self.history.aliases.extend(aliases);
                self.history.provenance.extend(provenance);
                self.history
                    .save(self.paths.home(), &self.history_expected)?;
                self.history_expected = state::fingerprint(&state::contained(
                    self.paths.home(),
                    ".skillator/rsync/state.json",
                )?)?;
                Ok(Response::Ok)
            }
            Request::Finish => {
                self.finish()?;
                Ok(Response::Ok)
            }
        }
    }

    fn lock(&mut self, token: &str) -> Result<()> {
        if self.stage.is_some() {
            return Err(Error::input("session is already active"));
        }
        let (_, sources, expected) = self
            .last
            .as_ref()
            .ok_or_else(|| Error::input("inspect before beginning a session"))?;
        if expected != token {
            return Err(Error::input("invalid observation token"));
        }
        let target = Target::user(self.paths.home()).map_err(Error::input_display)?;
        if self.user_lock.is_none() {
            self.user_lock = Some(TargetLocks::acquire(&[&target]).map_err(|_| Error::busy())?);
        }
        let observed = snapshot::inspect(&self.paths, sources)?;
        if state::digest(&serde_json::to_vec(&observed).map_err(Error::input_display)?) != token {
            self.user_lock = None;
            return Err(Error::input("participant changed after preflight; retry"));
        }
        // An earlier session can still hold its history lock after user reconciliation.
        // Reserve an existing lock without creating persistent state during this phase.
        if self.session_lock.is_none() {
            let root = state::contained(self.paths.home(), ".skillator/rsync/session.lock")?;
            match fs::symlink_metadata(&root) {
                Ok(meta) if meta.is_file() => {
                    let lock = File::open(root).map_err(Error::input_display)?;
                    lock.try_lock().map_err(|_| Error::busy())?;
                    self.session_lock = Some(lock);
                }
                Ok(_) => return Err(Error::input("session lock must be a regular file")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(Error::input_display(error)),
            }
        }
        Ok(())
    }

    fn begin(&mut self, token: &str) -> Result<Response> {
        self.lock(token)?;
        let observed = self.last.as_ref().unwrap().0.clone();
        let root = state::contained(self.paths.home(), ".skillator/rsync/session.lock")?;
        fs::create_dir_all(root.parent().unwrap()).map_err(Error::input_display)?;
        if fs::symlink_metadata(&root).is_ok_and(|meta| !meta.is_file()) {
            return Err(Error::input("session lock must be a regular file"));
        }
        if self.session_lock.is_none() {
            let lock = File::options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&root)
                .map_err(Error::input_display)?;
            lock.try_lock().map_err(|_| Error::busy())?;
            self.session_lock = Some(lock);
        }
        let recovered = recover_pending(self.paths.home())?;
        self.history = observed.history;
        self.history_expected = state::fingerprint(&state::contained(
            self.paths.home(),
            ".skillator/rsync/state.json",
        )?)?;
        let new_identity = self.history.id.is_none();
        let id = self.history.id.clone().unwrap_or(state::new_id()?);
        self.history.id = Some(id.clone());
        if new_identity {
            self.history
                .save(self.paths.home(), &self.history_expected)?;
            self.history_expected = state::fingerprint(&state::contained(
                self.paths.home(),
                ".skillator/rsync/state.json",
            )?)?;
        }
        let stage = root
            .parent()
            .unwrap()
            .join(format!("stage-{}", state::new_id()?));
        fs::create_dir(&stage).map_err(Error::input_display)?;
        self.stage = Some(stage.clone());
        Ok(Response::Begun {
            id,
            stage: stage.to_string_lossy().into_owned(),
            recovered,
        })
    }

    fn require_active(&self) -> Result<()> {
        if self.stage.is_none() {
            return Err(Error::input(
                "begin after successful preflight before writing",
            ));
        }
        Ok(())
    }

    fn authorize_path(&self, path: &str) -> Result<()> {
        let directories =
            snapshot::user_directories(self.paths.home(), &snapshot::user(&self.paths)?)?;
        if !state::transferable(self.paths.home(), path)?
            || !snapshot::outside_user_directories(self.paths.home(), path, &directories)?
        {
            return Err(Error::input("administrative paths cannot be transferred"));
        }
        let snapshot = &self
            .last
            .as_ref()
            .ok_or_else(|| Error::input("no observation"))?
            .0;
        if let Some(source) = snapshot
            .sources
            .iter()
            .filter(|source| {
                source.skills.iter().any(|skill| {
                    let root = Path::new(&source.root).join(skill);
                    Path::new(path).starts_with(&root)
                }) || source.files.contains_key(path)
                    || source.committed.contains_key(path)
                    || (path.starts_with(&format!("{}/", source.root))
                        && self
                            .history
                            .peers
                            .values()
                            .any(|peer| peer.files.contains_key(path)))
            })
            .max_by_key(|source| source.root.len())
        {
            state::home_relative(
                self.paths.home(),
                &state::contained(self.paths.home(), &source.root)?,
            )?;
            let boundary = self.skill_boundary(snapshot, source, path)?;
            if let Some(actual) = state::observation_path(self.paths.home(), path)? {
                let actual = if actual.is_symlink()
                    && source
                        .skills
                        .iter()
                        .any(|skill| Path::new(&source.root).join(skill) == Path::new(path))
                {
                    actual.canonicalize().map_err(Error::input_display)?
                } else {
                    actual
                };
                if !actual.starts_with(&boundary) {
                    return Err(Error::input("path escapes its physical skill directory"));
                }
            }
            if let Some(reference) = &source.git {
                let root = state::contained(self.paths.home(), &source.root)?;
                if snapshot::git_ref(&root)? != *reference
                    || !process::git(&root, &["ls-files", "-u"])?.is_empty()
                {
                    return Err(Error::input("Git source changed after observation"));
                }
            }
            Ok(())
        } else {
            Err(Error::input("path is outside observed skill boundaries"))
        }
    }

    fn skill_boundary(&self, snapshot: &Snapshot, source: &Source, path: &str) -> Result<PathBuf> {
        let mut roots: Vec<String> = source
            .skills
            .iter()
            .map(|skill| {
                Path::new(&source.root)
                    .join(skill)
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        for candidate in source.files.keys().chain(source.committed.keys()).chain(
            self.history
                .peers
                .values()
                .flat_map(|peer| peer.files.keys()),
        ) {
            if candidate.ends_with("/SKILL.md")
                && let Some(root) = candidate.strip_suffix("/SKILL.md")
                && Path::new(root).starts_with(&source.root)
            {
                roots.push(root.to_owned());
            }
        }
        let root = roots
            .into_iter()
            .filter(|root| Path::new(path).starts_with(root))
            .max_by_key(|root| root.len())
            .ok_or_else(|| Error::input("path has no observed skill boundary"))?;
        let logical = state::contained(self.paths.home(), &root)?;
        if logical.is_symlink() {
            let expected = snapshot
                .physical_paths
                .get(&root)
                .ok_or_else(|| Error::input("skill root changed after observation"))?;
            let actual = logical.canonicalize().map_err(Error::input_display)?;
            if actual != Path::new(expected) {
                return Err(Error::input("skill root changed after observation"));
            }
            Ok(actual)
        } else {
            Ok(logical)
        }
    }

    fn bootstrap(&self, source: &Source) -> Result<()> {
        let git = source
            .git
            .as_ref()
            .ok_or_else(|| Error::input("Git bootstrap needs a reference commit"))?;
        if !self.last.as_ref().is_some_and(|(_, extra, _)| {
            extra.iter().any(|expected| {
                expected.root == source.root
                    && expected.key == source.key
                    && expected.git == source.git
            })
        }) {
            return Err(Error::input("Git source was not included in preflight"));
        }
        if !valid_origin(&git.origin)
            || !matches!(git.commit.len(), 40 | 64)
            || !git.commit.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::input("invalid Git bootstrap reference"));
        }
        let destination = state::contained(self.paths.home(), &source.root)?;
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(Error::input("Git bootstrap destination already exists"));
        }
        let parent = destination.parent().unwrap();
        fs::create_dir_all(parent).map_err(Error::input_display)?;
        let stage = parent.join(format!(".skillator-clone-{}", state::new_id()?));
        let result = (|| {
            process::capture(
                Command::new("git")
                    .env("GIT_TERMINAL_PROMPT", "0")
                    .args([
                        "-c",
                        "core.hooksPath=/dev/null",
                        "clone",
                        "--no-checkout",
                        "--",
                        &git.origin,
                    ])
                    .arg(&stage),
            ).map_err(|error| Error::input(format!(
                "cannot clone Git source {}; verify the configured origin, repository access, credentials, and network connectivity: {error}", source.root
            )))?;
            if process::git(
                &stage,
                &["cat-file", "-e", &format!("{}^{{commit}}", git.commit)],
            )
            .is_err()
            {
                process::git(&stage, &["fetch", "--", "origin", &git.commit])
                    .map_err(|error| Error::input(format!(
                        "cannot fetch exact commit {} for Git source {}; ensure the commit is published and accessible from this host: {error}", git.commit, source.root
                    )))?;
            }
            process::git(&stage, &["checkout", "--detach", &git.commit, "--"])?;
            if snapshot::git_ref(&stage)? != *git {
                return Err(Error::input(
                    "clone did not produce the exact requested Git reference",
                ));
            }
            if state::contained(self.paths.home(), &source.root)? != destination {
                return Err(Error::input("clone parent changed"));
            }
            rename_noreplace(&stage, &destination).map_err(Error::input_display)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result
    }

    fn register(&self, locations: &[Location], expected: Option<String>) -> Result<()> {
        if let Some((desired, fingerprint)) = self.prepare_registration(locations, expected)? {
            save_library(&self.paths.library_config(), &desired, &fingerprint)
                .map_err(Error::input_display)?;
        }
        Ok(())
    }

    fn prepare_registration(
        &self,
        locations: &[Location],
        expected: Option<String>,
    ) -> Result<Option<(crate::config::LibraryConfig, Fingerprint)>> {
        let bytes = state::read_optional(&self.paths.library_config())?;
        if bytes.as_ref().map(|bytes| state::digest(bytes)) != expected {
            return Err(Error::input(
                "library configuration changed after observation",
            ));
        }
        let fingerprint = bytes
            .as_deref()
            .map(Fingerprint::for_bytes)
            .unwrap_or(Fingerprint::Absent);
        let config = snapshot::library(&self.paths)?;
        let mut desired = config.locations().to_vec();
        let home = self
            .paths
            .home()
            .canonicalize()
            .map_err(Error::input_display)?;
        let mut destinations = Vec::new();
        for location in locations {
            let destination = state::contained(self.paths.home(), &location.path)?;
            if !destination.is_dir() {
                return Err(Error::input("incoming library location is unavailable"));
            }
            let physical = destination.canonicalize().map_err(Error::input_display)?;
            if physical == home || !physical.starts_with(&home) {
                return Err(Error::input(
                    "incoming library location must resolve below the user home, not to or outside user home",
                ));
            }
            destinations.push((destination, physical.clone()));
            let found = desired.iter().find(|existing| {
                crate::library::expand_location(
                    existing.path(),
                    self.paths.library_config().parent().unwrap(),
                    self.paths.home(),
                    self.paths.environment(),
                )
                .is_ok_and(|path| path.canonicalize().is_ok_and(|path| path == physical))
            });
            if let Some(found) = found {
                if found.exclusions() != location.exclusions
                    || found.allow_overlap() != location.allow_overlap
                {
                    return Err(Error::input("conflicting library location settings"));
                }
            } else {
                desired.push(LibraryLocationConfig::new(
                    format!("~/{}", location.path),
                    location.exclusions.clone(),
                    location.allow_overlap,
                ));
            }
        }
        let desired = config
            .with_locations(desired)
            .map_err(|issues| Error::input(format!("invalid locations: {issues:?}")))?;
        if desired == config {
            return Ok(None);
        }
        let scanned = crate::library::scan_library(
            &desired,
            &self.paths.library_config(),
            self.paths.home(),
            self.paths.environment(),
        );
        if scanned
            .diagnostics()
            .iter()
            .any(|d| d.code == "overlapping_locations")
        {
            return Err(Error::input(
                "library locations overlap; align registrations explicitly",
            ));
        }
        for (destination, expected) in destinations {
            if destination.canonicalize().map_err(Error::input_display)? != expected {
                return Err(Error::input(
                    "incoming library location changed before registration",
                ));
            }
        }
        fs::create_dir_all(self.paths.library_config().parent().unwrap())
            .map_err(Error::input_display)?;
        Ok(Some((desired, fingerprint)))
    }

    fn observed_content(
        &self,
        logical: &str,
        path: &Path,
        expected: Option<&Entry>,
    ) -> Result<Option<Entry>> {
        // Only aliases observed as skill-root directories can retain their local link.
        if matches!(expected, Some(Entry::Directory))
            && path.is_symlink()
            && path.is_dir()
            && let Some((snapshot, _, _)) = &self.last
            && snapshot.sources.iter().any(|source| {
                source.files.get(logical) == Some(&Entry::Directory)
                    && source.skills.iter().any(|skill| {
                        let root = if skill == "." {
                            source.root.clone()
                        } else {
                            format!("{}/{}", source.root, skill)
                        };
                        root == logical
                    })
            })
            && let Some(physical) = snapshot.physical_paths.get(logical)
            && path.canonicalize().map_err(Error::input_display)? == Path::new(physical)
        {
            return Ok(Some(Entry::Directory));
        }
        state::observe(path)
    }

    fn publish_alias(&self, path: &str, target: &str) -> Result<()> {
        if !state::transferable(self.paths.home(), path)?
            || Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(crate::library::reserved_temporary)
        {
            return Err(Error::input("administrative paths cannot be aliased"));
        }
        let snapshot = &self
            .last
            .as_ref()
            .ok_or_else(|| Error::input("no observation"))?
            .0;
        let parent = Path::new(path)
            .parent()
            .ok_or_else(|| Error::input("invalid alias path"))?;
        if let Some(local) = snapshot.locations.first() {
            if parent != Path::new(&local.path) {
                return Err(Error::input(
                    "alias is not a direct child of the local library",
                ));
            }
        } else if !snapshot.sources.iter().any(|source| {
            Path::new(&source.root) == parent
                && source.location == source.root
                && source.git.is_none()
        }) {
            return Err(Error::input(
                "alias is not a direct child of the local library",
            ));
        }
        let recognized = snapshot.sources.iter().any(|source| {
            Path::new(&source.root) != parent
                && source
                    .skills
                    .iter()
                    .any(|skill| Path::new(&source.root).join(skill) == Path::new(target))
        });
        if !recognized {
            return Err(Error::input("alias target is not a registered skill"));
        }
        let destination = state::contained(self.paths.home(), path)?;
        let physical_target = state::contained(self.paths.home(), target)?;
        let real_target = physical_target
            .canonicalize()
            .map_err(Error::input_display)?;
        state::home_relative(self.paths.home(), &real_target)?;
        if !real_target.join("SKILL.md").is_file() {
            return Err(Error::input("alias target skill is not available"));
        }
        match fs::symlink_metadata(&destination) {
            Ok(meta) => {
                if meta.file_type().is_symlink()
                    && destination.canonicalize().map_err(Error::input_display)? == real_target
                {
                    return Ok(());
                }
                return Err(Error::input(
                    "alias destination is occupied; preserve it for manual resolution",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::input_display(error)),
        }
        fs::create_dir_all(destination.parent().unwrap()).map_err(Error::input_display)?;
        let sibling = destination
            .parent()
            .unwrap()
            .join(format!(".skillator-alias-{}", state::new_id()?));
        std::os::unix::fs::symlink(&physical_target, &sibling).map_err(Error::input_display)?;
        let result = rename_noreplace(&sibling, &destination).map_err(Error::input_display);
        if result.is_err() {
            let _ = fs::remove_file(&sibling);
        }
        result
    }

    fn publish(
        &self,
        path: &str,
        expected: Option<&Entry>,
        desired: Option<&Entry>,
        staged: Option<&str>,
    ) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let destination = state::contained(self.paths.home(), path)?;
        if let Some(Entry::Link { target }) = desired {
            self.validate_link(path, target)?;
        }
        let actual = self.observed_content(path, &destination, desired.or(expected))?;
        if actual.as_ref() == desired {
            return Ok(());
        }
        if actual.as_ref() != expected {
            return Err(Error::input("destination changed after observation"));
        }
        if expected == desired {
            return Ok(());
        }
        if matches!(expected, Some(Entry::Directory))
            && (destination.is_symlink()
                || fs::read_dir(&destination)
                    .map_err(Error::input_display)?
                    .next()
                    .is_some())
        {
            return Err(Error::input(
                "directory still contains entries or is an acquisition link; preserve it and retry after resolving child entries",
            ));
        }
        let parent = destination.parent().unwrap();
        fs::create_dir_all(parent).map_err(Error::input_display)?;
        let sibling = parent.join(format!(".skillator-rsync-{}", state::new_id()?));
        if let Some(desired) = desired {
            if matches!(desired, Entry::Directory) {
                fs::create_dir(&sibling).map_err(Error::input_display)?;
            } else {
                let stage = staged.ok_or_else(|| Error::input("missing staged content"))?;
                let stage_path = Path::new(stage);
                if stage_path.parent() != self.stage.as_deref() {
                    return Err(Error::input("invalid staged path"));
                }
                if state::observe(stage_path)?.as_ref() != Some(desired) {
                    return Err(Error::input("staged content does not match planned value"));
                }
                copy_entry(stage_path, &sibling, desired)?;
            }
            if let Entry::File { executable, .. } = desired {
                fs::set_permissions(
                    &sibling,
                    fs::Permissions::from_mode(if *executable { 0o755 } else { 0o644 }),
                )
                .map_err(Error::input_display)?;
            }
        }
        if state::contained(self.paths.home(), path)? != destination
            || self
                .observed_content(path, &destination, expected)?
                .as_ref()
                != expected
        {
            let _ = remove_entry(&sibling);
            return Err(Error::input("destination changed during staging"));
        }
        let journal = self
            .stage
            .as_ref()
            .unwrap()
            .join(format!("recovery-{}.json", state::new_id()?));
        state::write_new(
            &journal,
            &serde_json::to_vec(&Recovery {
                path: path.into(),
                expected: expected.cloned(),
                desired: desired.cloned(),
                sibling: sibling.clone(),
            })
            .map_err(Error::input_display)?,
        )?;
        let result = match (expected, desired) {
            (None, Some(_)) => rename_noreplace(&sibling, &destination),
            (Some(_), Some(_)) => rename_exchange(&sibling, &destination),
            (Some(_), None) => rename_noreplace(&destination, &sibling),
            (None, None) => Ok(()),
        };
        if let Err(error) = result {
            return Err(Error::input(format!(
                "publication failed; inspect recovery journals under ~/.skillator/rsync: {error}"
            )));
        }
        let preserved = if expected.is_some() {
            state::observe(&sibling)?
        } else {
            None
        };
        if preserved.as_ref() != expected
            || (matches!(expected, Some(Entry::Directory))
                && fs::read_dir(&sibling)
                    .map_err(Error::input_display)?
                    .next()
                    .is_some())
            || self.observed_content(path, &destination, desired)?.as_ref() != desired
        {
            let rollback = match (expected, desired) {
                (Some(_), Some(_)) => rename_exchange(&sibling, &destination),
                (Some(_), None) => rename_noreplace(&sibling, &destination),
                (None, Some(_)) => rename_noreplace(&destination, &sibling),
                _ => Ok(()),
            };
            return Err(Error::input(format!(
                "publication verification failed; rollback {}; inspect recovery journals under ~/.skillator/rsync",
                if rollback.is_ok() {
                    "completed"
                } else {
                    "requires recovery"
                }
            )));
        }
        if sibling.exists() || fs::symlink_metadata(&sibling).is_ok() {
            remove_entry(&sibling)?;
        }
        fs::remove_file(journal).map_err(Error::input_display)?;
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        if let Some(stage) = self.stage.take() {
            let recovery = fs::read_dir(&stage)
                .map_err(Error::input_display)?
                .any(|entry| {
                    entry.is_ok_and(|entry| {
                        entry.file_name().to_string_lossy().starts_with("recovery-")
                    })
                });
            if !recovery {
                fs::remove_dir_all(stage).map_err(Error::input_display)?;
            }
        }
        self.user_lock = None;
        self.session_lock = None;
        Ok(())
    }

    fn validate_link(&self, path: &str, target: &str) -> Result<()> {
        if Path::new(target).is_absolute() || target.contains('\0') {
            return Err(Error::input("absolute or invalid skill link"));
        }
        let logical = Path::new(path);
        let mut resolved = logical.parent().unwrap().to_path_buf();
        for component in Path::new(target).components() {
            match component {
                std::path::Component::Normal(value) => resolved.push(value),
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if !resolved.pop() {
                        return Err(Error::input("skill link escapes home"));
                    }
                }
                _ => return Err(Error::input("invalid skill link")),
            }
        }
        let snapshot = &self.last.as_ref().unwrap().0;
        let source = snapshot
            .sources
            .iter()
            .filter(|source| Path::new(path).starts_with(&source.root))
            .max_by_key(|source| source.root.len())
            .ok_or_else(|| Error::input("link has no skill boundary"))?;
        let boundary = self.skill_boundary(snapshot, source, path)?;
        let target_path = state::contained(
            self.paths.home(),
            resolved
                .to_str()
                .ok_or_else(|| Error::input("invalid link path"))?,
        )?;
        if !target_path.starts_with(&boundary)
            || (fs::symlink_metadata(&target_path).is_ok()
                && !target_path
                    .canonicalize()
                    .map_err(Error::input_display)?
                    .starts_with(&boundary))
        {
            return Err(Error::input(
                "skill link escapes its physical skill directory",
            ));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recovery {
    path: String,
    expected: Option<Entry>,
    desired: Option<Entry>,
    sibling: PathBuf,
}

pub(super) fn pending_recovery(home: &Path) -> Result<Vec<PathBuf>> {
    let root = state::contained(home, ".skillator/rsync/state.json")?
        .parent()
        .unwrap()
        .to_path_buf();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(root).map_err(Error::input_display)? {
        let entry = entry.map_err(Error::input_display)?;
        if !entry.file_name().to_string_lossy().starts_with("stage-")
            || !entry.file_type().map_err(Error::input_display)?.is_dir()
        {
            continue;
        }
        for file in fs::read_dir(entry.path()).map_err(Error::input_display)? {
            let file = file.map_err(Error::input_display)?;
            if file.file_name().to_string_lossy().starts_with("recovery-") {
                records.push(file.path());
            }
        }
    }
    records.sort();
    Ok(records)
}

fn recover_pending(home: &Path) -> Result<usize> {
    let records = pending_recovery(home)?;
    for record in &records {
        let bytes = state::read_optional(record)?
            .ok_or_else(|| Error::input("recovery record disappeared"))?;
        let recovery: Recovery = serde_json::from_slice(&bytes).map_err(Error::input_display)?;
        let destination = state::contained(home, &recovery.path)?;
        if recovery.sibling.parent() != destination.parent()
            || !recovery
                .sibling
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(reserved_temporary)
        {
            return Err(Error::input(
                "invalid recovery backup path; manual recovery required",
            ));
        }
        let actual = state::observe(&destination)?;
        let backup = state::observe(&recovery.sibling)?;
        // Published and removed directory entries are empty at the journal boundary.
        // A type-only observation cannot prove that later children are ours to move.
        for (path, entry) in [(&destination, &actual), (&recovery.sibling, &backup)] {
            if matches!(entry, Some(Entry::Directory))
                && fs::read_dir(path)
                    .map_err(Error::input_display)?
                    .next()
                    .is_some()
            {
                return Err(Error::input(format!(
                    "{} contains entries added after interruption; preserve the directory and backup for manual recovery",
                    path.display()
                )));
            }
        }
        if actual == recovery.expected {
            // Publication never happened, or the earlier rollback completed.
            if backup.is_some() && backup != recovery.desired {
                return Err(Error::input(
                    "recovery backup changed; preserve it for manual recovery",
                ));
            }
        } else if actual == recovery.desired && backup == recovery.expected {
            match (&recovery.expected, &recovery.desired) {
                (Some(_), Some(_)) => rename_exchange(&recovery.sibling, &destination),
                (Some(_), None) => rename_noreplace(&recovery.sibling, &destination),
                (None, Some(_)) => rename_noreplace(&destination, &recovery.sibling),
                _ => Ok(()),
            }
            .map_err(Error::input_display)?;
        } else if actual == recovery.desired && backup.is_none() {
            // Verification and backup cleanup completed before the journal was removed.
        } else {
            return Err(Error::input(format!(
                "{} changed after interruption; manual recovery required; inspect recovery journals under ~/.skillator/rsync",
                recovery.path
            )));
        }
        if fs::symlink_metadata(&recovery.sibling).is_ok() {
            remove_entry(&recovery.sibling)?;
        }
        fs::remove_file(record).map_err(Error::input_display)?;
    }
    Ok(records.len())
}

fn reserved_temporary(name: &str) -> bool {
    [".skillator-rsync-", ".skillator-clone-"]
        .iter()
        .any(|prefix| name.strip_prefix(prefix).is_some_and(state::valid_id))
}

pub(super) fn valid_origin(origin: &str) -> bool {
    !origin.is_empty()
        && !origin.starts_with('-')
        && !origin.contains('\0')
        && !origin.contains(['?', '#'])
        && !origin.split_once('@').is_some_and(|(prefix, _)| {
            !origin.contains("://") && (prefix.contains(':') || prefix.contains('%'))
        })
        && !origin.split_once("://").is_some_and(|(_, rest)| {
            rest.split('/').next().is_some_and(|authority| {
                authority.split_once('@').is_some_and(|(userinfo, _)| {
                    !origin.starts_with("ssh://")
                        || userinfo.contains(':')
                        || userinfo.contains('%')
                })
            })
        })
        && !origin.split_once("::").is_some_and(|(transport, _)| {
            !transport.is_empty()
                && transport
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
        })
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(test)]
fn hash_optional(path: &Path) -> Result<Option<String>> {
    Ok(state::read_optional(path)?.map(|bytes| state::digest(&bytes)))
}

fn remove_entry(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(Error::input_display)?;
    if meta.is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(Error::input_display)
}

fn copy_entry(source: &Path, destination: &Path, entry: &Entry) -> Result<()> {
    match entry {
        Entry::File { .. } => {
            let mut input = File::open(source).map_err(Error::input_display)?;
            let mut output = File::options()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(Error::input_display)?;
            std::io::copy(&mut input, &mut output).map_err(Error::input_display)?;
            output
                .set_permissions(
                    input
                        .metadata()
                        .map_err(Error::input_display)?
                        .permissions(),
                )
                .map_err(Error::input_display)?;
            output.sync_all().map_err(Error::input_display)?;
        }
        Entry::Link { target } => {
            std::os::unix::fs::symlink(target, destination).map_err(Error::input_display)?;
        }
        Entry::Directory => {
            fs::create_dir(destination).map_err(Error::input_display)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_publication_rejects_administrative_and_staging_names() {
        let home = tempfile::tempdir().unwrap();
        let local = home.path().join(".skillator/library");
        let external = home.path().join("Development/skills/demo");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&external).unwrap();
        fs::write(
            external.join("SKILL.md"),
            "---\nname: demo\ndescription: External skill\n---\n",
        )
        .unwrap();
        fs::write(
            home.path().join(".skillator/library.yaml"),
            "version: 1\nlocations:\n - path: '~/.skillator/library'\n - path: '~/Development/skills'\n",
        )
        .unwrap();
        let mut session = Session::new(AppPaths::new(home.path().into()));
        session
            .handle(Request::Inspect {
                sources: Vec::new(),
            })
            .unwrap();
        for name in [
            ".git",
            ".skillator-rsync-0123456789abcdef0123456789abcdef",
            ".skillator-clone-0123456789abcdef0123456789abcdef",
            ".skillator-alias-0123456789abcdef0123456789abcdef",
        ] {
            let path = format!(".skillator/library/{name}");
            let error = session
                .publish_alias(&path, "Development/skills/demo")
                .unwrap_err();
            assert!(error.message.contains("administrative paths"), "{error}");
            assert!(fs::symlink_metadata(local.join(name)).is_err());
        }
    }

    fn setup() -> (tempfile::TempDir, Session, PathBuf) {
        let home = tempfile::tempdir().unwrap();
        let skill = home.path().join(".skillator/library/demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\ndescription: Session test\n---\n",
        )
        .unwrap();
        fs::write(skill.join("data"), "original").unwrap();
        fs::write(
            home.path().join(".skillator/library.yaml"),
            "version: 1\nlocations:\n - path: './library'\n",
        )
        .unwrap();
        let mut session = Session::new(AppPaths::new(home.path().into()));
        let Response::Snapshot { token, .. } = session
            .handle(Request::Inspect {
                sources: Vec::new(),
            })
            .unwrap()
        else {
            panic!()
        };
        let Response::Begun { stage, .. } = session.handle(Request::Begin { token }).unwrap()
        else {
            panic!()
        };
        (home, session, PathBuf::from(stage))
    }

    #[test]
    fn symlinked_parents_cannot_cross_a_skill_boundary() {
        let (home, mut session, _) = setup();
        let root = home.path().join(".skillator/library/demo");
        let unrelated = home.path().join("unrelated");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("private"), "keep").unwrap();
        std::os::unix::fs::symlink(&unrelated, root.join("sub")).unwrap();
        let path = ".skillator/library/demo/sub/private";
        assert!(
            session
                .validate_link(".skillator/library/demo/new", "sub/private")
                .is_err()
        );
        session.last.as_mut().unwrap().0.sources[0].skills.clear();
        let mut baseline = state::Baseline::default();
        baseline
            .files
            .insert(".skillator/library/demo/SKILL.md".into(), None);
        baseline.files.insert(path.into(), None);
        session
            .history
            .peers
            .insert(state::new_id().unwrap(), baseline);
        assert!(
            session
                .authorize_path(path)
                .unwrap_err()
                .message
                .contains("physical skill")
        );
        assert_eq!(
            fs::read_to_string(unrelated.join("private")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn ordinary_directory_links_are_replaced_instead_of_acknowledged_as_directories() {
        let (home, session, stage) = setup();
        let root = home.path().join(".skillator/library/demo");
        fs::create_dir(root.join("real")).unwrap();
        std::os::unix::fs::symlink("real", root.join("child")).unwrap();
        let staged = stage.join("directory");
        fs::create_dir(&staged).unwrap();
        session
            .publish(
                ".skillator/library/demo/child",
                Some(&Entry::Link {
                    target: "real".into(),
                }),
                Some(&Entry::Directory),
                staged.to_str(),
            )
            .unwrap();
        assert!(!root.join("child").is_symlink());
        assert!(root.join("child").is_dir());
        assert!(root.join("real").is_dir());
    }

    #[test]
    fn remote_user_save_rejects_edits_after_the_request_fingerprint_check() {
        for present in [false, true] {
            let home = tempfile::tempdir().unwrap();
            let paths = AppPaths::new(home.path().into());
            fs::create_dir_all(paths.user_config().parent().unwrap()).unwrap();
            let original = crate::config::RepositoryConfigCodec::render(
                &crate::config::RepositoryConfig::user_first_run(),
            )
            .unwrap();
            if present {
                fs::write(paths.user_config(), &original).unwrap();
            }
            let session = UserScopeWorkflow::load(&paths).unwrap();
            let expected = state::fingerprint(&paths.user_config()).unwrap();
            let edited = [b"# concurrent edit\n".as_slice(), original.as_bytes()].concat();
            fs::write(paths.user_config(), &edited).unwrap();
            let target = Target::user(home.path()).unwrap();
            let locks = TargetLocks::acquire(&[&target]).unwrap();
            let error =
                UserScopeWorkflow::save_remote(&paths, session.config, expected, locks, false)
                    .unwrap_err();
            assert!(
                error.to_string().contains("changed after observation"),
                "{error}"
            );
            assert_eq!(fs::read(paths.user_config()).unwrap(), edited);
        }
    }

    #[test]
    fn skill_control_files_are_neither_collected_nor_authorized() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".skillator/rsync")).unwrap();
        fs::create_dir_all(home.path().join(".skillator/nested/.agents")).unwrap();
        fs::write(
            home.path().join(".skillator/SKILL.md"),
            "---\nname: demo\ndescription: Broad skill root\n---\n",
        )
        .unwrap();
        fs::write(
            home.path().join(".skillator/library.yaml"),
            "version: 1\nlocations: [{path: '~/.skillator'}]\n",
        )
        .unwrap();
        let controls = [
            ".skillator/library.yaml",
            ".skillator/config.yaml",
            ".skillator/targets.yaml",
            ".skillator/rsync/private",
            ".skillator/nested/.agents/skillator.yaml",
        ];
        for path in &controls[1..] {
            fs::write(home.path().join(path), "machine-local bytes").unwrap();
        }
        std::os::unix::fs::symlink("config.yaml", home.path().join(".skillator/alias")).unwrap();
        let mut session = Session::new(AppPaths::new(home.path().into()));
        let Response::Snapshot { snapshot, .. } = session
            .handle(Request::Inspect { sources: vec![] })
            .unwrap()
        else {
            panic!()
        };
        assert!(
            snapshot
                .sources
                .iter()
                .any(|source| source.files.contains_key(".skillator/SKILL.md"))
        );
        for path in controls.into_iter().chain([".skillator/alias"]) {
            assert!(
                snapshot
                    .sources
                    .iter()
                    .all(|source| !source.files.contains_key(path)),
                "{path}"
            );
            assert!(session.authorize_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn configured_user_materializations_and_physical_aliases_are_excluded() {
        use crate::config::RepositoryConfigCodec;
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".skillator/library")).unwrap();
        fs::create_dir_all(home.path().join(".agents")).unwrap();
        let config = snapshot::user_from_entries(&BTreeMap::from([(
            "directory/custom".into(),
            r#"["Library/user-skills",null]"#.into(),
        )]))
        .unwrap();
        fs::write(
            home.path().join(".agents/skillator.yaml"),
            RepositoryConfigCodec::render(&config).unwrap(),
        )
        .unwrap();
        for root in [".agents/skills/demo", "Library/user-skills/demo"] {
            fs::create_dir_all(home.path().join(root)).unwrap();
            fs::write(
                home.path().join(root).join("SKILL.md"),
                "---\nname: demo\ndescription: User copy\n---\n",
            )
            .unwrap();
        }
        std::os::unix::fs::symlink(
            home.path().join("Library/user-skills/demo"),
            home.path().join(".skillator/library/alias"),
        )
        .unwrap();
        fs::write(home.path().join(".skillator/library.yaml"), "version: 1\nlocations: [{path: '~/.skillator/library'}, {path: '~/.agents'}, {path: '~/Library'}]\n").unwrap();
        let mut session = Session::new(AppPaths::new(home.path().into()));
        let Response::Snapshot { snapshot, .. } = session
            .handle(Request::Inspect { sources: vec![] })
            .unwrap()
        else {
            panic!()
        };
        for path in [
            ".agents/skills/demo/SKILL.md",
            "Library/user-skills/demo/SKILL.md",
            ".skillator/library/alias/SKILL.md",
        ] {
            assert!(
                snapshot
                    .sources
                    .iter()
                    .all(|source| !source.files.contains_key(path)),
                "{path}"
            );
            assert!(session.authorize_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn registration_rejects_escaping_final_links_and_preserves_equivalent_expressions() {
        let (home, session, _) = setup();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("escape")).unwrap();
        let config = home.path().join(".skillator/library.yaml");
        let original = fs::read(&config).unwrap();
        let incoming = |path: &str| Location {
            path: path.into(),
            exclusions: vec![],
            allow_overlap: false,
        };
        assert!(
            session
                .register(&[incoming("escape")], hash_optional(&config).unwrap())
                .unwrap_err()
                .message
                .contains("outside user home")
        );
        assert_eq!(fs::read(&config).unwrap(), original);
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
        std::os::unix::fs::symlink(home.path(), home.path().join("home-alias")).unwrap();
        assert!(
            session
                .register(&[incoming("home-alias")], hash_optional(&config).unwrap())
                .is_err()
        );
        assert_eq!(fs::read(&config).unwrap(), original);
        std::os::unix::fs::symlink(
            home.path().join(".skillator/library"),
            home.path().join("library-alias"),
        )
        .unwrap();
        for expression in ["~/library-alias", "~/.skillator/../.skillator/library"] {
            let bytes = format!(
                "# preserve this expression\nversion: 1\nlocations: [{{path: '{expression}'}}]\n"
            );
            fs::write(&config, &bytes).unwrap();
            session
                .register(
                    &[incoming(".skillator/library")],
                    hash_optional(&config).unwrap(),
                )
                .unwrap();
            assert_eq!(fs::read_to_string(&config).unwrap(), bytes);
        }
    }

    #[test]
    fn registration_keeps_the_observed_fingerprint_until_publication() {
        let (home, session, _) = setup();
        fs::create_dir(home.path().join("incoming")).unwrap();
        let config = home.path().join(".skillator/library.yaml");
        let (desired, fingerprint) = session
            .prepare_registration(
                &[Location {
                    path: "incoming".into(),
                    exclusions: vec![],
                    allow_overlap: false,
                }],
                hash_optional(&config).unwrap(),
            )
            .unwrap()
            .unwrap();
        let intervening = "# concurrent edit\nversion: 1\nlocations: [{path: './library'}]\n";
        fs::write(&config, intervening).unwrap();
        assert!(save_library(&config, &desired, &fingerprint).is_err());
        assert_eq!(fs::read_to_string(config).unwrap(), intervening);
    }

    #[test]
    fn an_observed_skill_cannot_be_redirected_to_the_home_root() {
        let (home, session, _) = setup();
        let skill = home.path().join(".skillator/library/demo");
        fs::rename(&skill, home.path().join("retained-skill")).unwrap();
        std::os::unix::fs::symlink(home.path(), &skill).unwrap();
        fs::write(home.path().join("data"), "original").unwrap();
        assert!(
            session
                .authorize_path(".skillator/library/demo/data")
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(home.path().join("data")).unwrap(),
            "original"
        );
    }

    #[test]
    fn git_origins_allow_ipv6_but_reject_external_helpers() {
        for origin in [
            "ssh://git@[2001:db8::1]/repo",
            "https://[::1]/repo.git",
            "git@[2001:db8::1]:org/repo",
            "/tmp/local-repo",
            "git@example.test:org/repo",
        ] {
            assert!(valid_origin(origin), "{origin}");
        }
        for origin in [
            "",
            "--upload-pack=evil",
            "ext::sh -c evil",
            "helper::address",
            "bad\0origin",
            "https://user:token@example.test/repo",
            "ssh://user:token@example.test/repo",
            "ssh://user%3Asecret@example.test/repo",
            "ssh://user%3asecret@example.test/repo",
            "https://example.test/repo?access_token=secret",
            "user:token@example.test:repo",
        ] {
            assert!(!valid_origin(origin), "{origin}");
        }
    }

    #[test]
    fn recovery_preserves_children_added_to_a_published_directory() {
        for backup_directory in [false, true] {
            let (home, session, stage) = setup();
            let path = ".skillator/library/demo/data";
            let destination = state::contained(home.path(), path).unwrap();
            let expected = state::observe(&destination).unwrap();
            let sibling = destination
                .parent()
                .unwrap()
                .join(format!(".skillator-rsync-{}", state::new_id().unwrap()));
            fs::create_dir(&sibling).unwrap();
            let desired = state::observe(&sibling).unwrap();
            rename_exchange(&sibling, &destination).unwrap();
            let journal = stage.join("recovery-directory.json");
            state::write_new(
                &journal,
                &serde_json::to_vec(&Recovery {
                    path: path.into(),
                    expected,
                    desired,
                    sibling: sibling.clone(),
                })
                .unwrap(),
            )
            .unwrap();
            let directory = if backup_directory {
                // The destination's type changed again; keep both the new backup
                // children and the live replacement rather than guessing ownership.
                fs::remove_file(&sibling).unwrap();
                fs::create_dir(&sibling).unwrap();
                &sibling
            } else {
                &destination
            };
            fs::write(directory.join("later.txt"), "later user edit").unwrap();
            drop(session);
            assert!(
                recover_pending(home.path())
                    .unwrap_err()
                    .message
                    .contains("manual recovery")
            );
            assert_eq!(
                fs::read_to_string(directory.join("later.txt")).unwrap(),
                "later user edit"
            );
            assert!(destination.is_dir());
            assert!(journal.exists());
            if !backup_directory {
                assert_eq!(fs::read_to_string(sibling).unwrap(), "original");
            }
        }
    }

    #[test]
    fn source_and_destination_changes_block_publication() {
        let (home, mut session, stage) = setup();
        let path = ".skillator/library/demo/data";
        let original = state::observe(&home.path().join(path)).unwrap().unwrap();
        fs::write(stage.join("incoming"), "incoming").unwrap();
        let desired = state::observe(&stage.join("incoming")).unwrap();
        fs::write(home.path().join(path), "intervening edit").unwrap();
        assert!(
            session
                .handle(Request::Export {
                    path: path.into(),
                    expected: original.clone()
                })
                .is_err()
        );
        assert!(
            session
                .handle(Request::Publish {
                    path: path.into(),
                    expected: Some(original),
                    desired,
                    stage: Some(stage.join("incoming").to_string_lossy().into_owned())
                })
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(home.path().join(path)).unwrap(),
            "intervening edit"
        );
        let mut other = Session::new(AppPaths::new(home.path().into()));
        assert_eq!(
            other
                .handle(Request::Inspect {
                    sources: Vec::new()
                })
                .unwrap_err()
                .code,
            4
        );
    }

    #[test]
    fn changed_parent_and_escaping_link_are_rejected() {
        let (home, mut session, stage) = setup();
        let outside = tempfile::tempdir().unwrap();
        fs::write(stage.join("incoming"), "incoming").unwrap();
        let desired = state::observe(&stage.join("incoming")).unwrap();
        let old = home.path().join(".skillator/library/demo");
        fs::rename(&old, home.path().join("retained")).unwrap();
        std::os::unix::fs::symlink(outside.path(), &old).unwrap();
        assert!(
            session
                .handle(Request::Publish {
                    path: ".skillator/library/demo/new".into(),
                    expected: None,
                    desired,
                    stage: Some(stage.join("incoming").to_string_lossy().into_owned())
                })
                .is_err()
        );
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
        assert!(
            session
                .validate_link(".skillator/library/demo/new", "/etc/passwd")
                .is_err()
        );
        assert!(
            session
                .validate_link(".skillator/library/demo/new", "../../../../outside")
                .is_err()
        );
    }

    #[test]
    fn interrupted_exchange_recovers_but_preserves_later_edits() {
        for intervening in [false, true] {
            let (home, session, stage) = setup();
            let path = ".skillator/library/demo/data";
            let destination = state::contained(home.path(), path).unwrap();
            let expected = state::observe(&destination).unwrap();
            let sibling = destination
                .parent()
                .unwrap()
                .join(format!(".skillator-rsync-{}", state::new_id().unwrap()));
            fs::write(&sibling, "replacement").unwrap();
            let desired = state::observe(&sibling).unwrap();
            rename_exchange(&sibling, &destination).unwrap();
            let journal = stage.join("recovery-test.json");
            state::write_new(
                &journal,
                &serde_json::to_vec(&Recovery {
                    path: path.into(),
                    expected,
                    desired,
                    sibling: sibling.clone(),
                })
                .unwrap(),
            )
            .unwrap();
            if intervening {
                fs::write(&destination, "later edit").unwrap();
            }
            drop(session);
            if intervening {
                assert!(recover_pending(home.path()).is_err());
                assert_eq!(fs::read_to_string(&destination).unwrap(), "later edit");
                assert_eq!(fs::read_to_string(&sibling).unwrap(), "original");
                assert!(journal.exists());
            } else {
                assert_eq!(recover_pending(home.path()).unwrap(), 1);
                assert_eq!(fs::read_to_string(&destination).unwrap(), "original");
                assert!(!journal.exists());
            }
        }
    }
    #[test]
    fn unobtainable_commit_never_publishes_the_origin_head() {
        let origin = tempfile::tempdir().unwrap();
        process::git(origin.path(), &["init", "-q"]).unwrap();
        process::git(
            origin.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "initial",
            ],
        )
        .unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut session = Session::new(AppPaths::new(home.path().into()));
        let source = Source {
            key: "acme/skills".into(),
            root: "Development/acme/skills".into(),
            location: "Development/acme/skills".into(),
            exclusions: Vec::new(),
            branch: None,
            git: Some(snapshot::GitRef {
                origin: origin.path().to_string_lossy().into_owned(),
                commit: "1234567890abcdef1234567890abcdef12345678".into(),
            }),
            skills: Default::default(),
            invalid_skills: Default::default(),
            files: Default::default(),
            committed: Default::default(),
            problems: Vec::new(),
        };
        let Response::Snapshot { token, .. } = session
            .handle(Request::Inspect {
                sources: vec![source.clone()],
            })
            .unwrap()
        else {
            panic!()
        };
        session.handle(Request::Begin { token }).unwrap();
        let error = session.handle(Request::Bootstrap { source }).unwrap_err();
        assert!(
            error
                .message
                .contains("cannot fetch exact commit 1234567890abcdef1234567890abcdef12345678"),
            "{error}"
        );
        assert!(
            error.message.contains("published and accessible"),
            "{error}"
        );
        assert!(!home.path().join("Development/acme/skills").exists());
        assert!(
            fs::read_dir(home.path().join("Development/acme"))
                .unwrap()
                .next()
                .is_none()
        );
    }
}
