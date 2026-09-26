use super::{
    Error, Result, process,
    snapshot::{self, Location, Snapshot, Source},
    state::{self, Baseline, Entry, History},
};
use crate::app::{AppPaths, CommandReport, UserScopeWorkflow};
#[cfg(test)]
use crate::config::save_library;
use crate::config::{Fingerprint, LibraryConfigCodec, LibraryLocationConfig};
#[cfg(test)]
use crate::fs_safety::rename_exchange;
use crate::fs_safety::{Directory, bind_directory};
use crate::reconcile::TargetLocks;
use crate::target::Target;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
#[cfg(test)]
use std::os::unix::fs::OpenOptionsExt;
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
    ValidateStage,
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
        device: u64,
        inode: u64,
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
    stage_identity: Option<(u64, u64)>,
    stage_parent: Option<Directory>,
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
            stage_identity: None,
            stage_parent: None,
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
            Request::ValidateStage => {
                self.validate_stage_root()?;
                Ok(Response::Ok)
            }
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
                self.validate_stage_root()?;
                self.authorize_path(&path)?;
                let source = state::contained(self.paths.home(), &path)?;
                if self.observed_content(&path, &source, Some(&expected))? != Some(expected.clone())
                {
                    return Err(Error::input("source changed after observation"));
                }
                let source_parent =
                    Directory::open_existing_parent(self.paths.home(), source.parent().unwrap())
                        .map_err(Error::input_display)?;
                let source_name = source.file_name().unwrap();
                let anchored = state::observe_at(&source_parent, source_name)?;
                if anchored != Some(expected.clone())
                    && !(matches!(expected, Entry::Directory)
                        && matches!(anchored, Some(Entry::Link { .. }))
                        && self.observed_content(&path, &source, Some(&expected))?
                            == Some(expected.clone()))
                {
                    return Err(Error::input("source changed after observation"));
                }
                let stage_dir = self.open_stage()?;
                let name = state::new_id()?;
                let stage = self.stage.as_ref().unwrap().join(&name);
                copy_entry_at(
                    &source_parent,
                    source_name,
                    &stage_dir,
                    std::ffi::OsStr::new(&name),
                    &expected,
                )?;
                if state::observe_at(&stage_dir, std::ffi::OsStr::new(&name))? != Some(expected) {
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
                self.validate_stage_root()?;
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
                let library_bytes =
                    state::read_contained(self.paths.home(), ".skillator/library.yaml")?;
                if library_bytes.as_ref().map(|bytes| state::digest(bytes))
                    != self
                        .last
                        .as_ref()
                        .ok_or_else(|| Error::input("inspect before reconciling user state"))?
                        .0
                        .library_hash
                {
                    return Err(Error::input(
                        "library configuration changed after observation",
                    ));
                }
                let library = snapshot::library_from_bytes(library_bytes.as_deref())?;
                let bytes = state::read_contained(self.paths.home(), ".agents/skillator.yaml")?;
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
                let report = UserScopeWorkflow::save_remote(
                    &self.paths,
                    desired,
                    fingerprint,
                    locks,
                    check,
                    &library,
                )
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
                self.history_expected =
                    state::fingerprint_contained(self.paths.home(), ".skillator/rsync/state.json")?;
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
            let parent =
                match Directory::open_existing_parent(self.paths.home(), root.parent().unwrap()) {
                    Ok(parent) => Some(parent),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(Error::input_display(error)),
                };
            if let Some(parent) = parent {
                match parent.open_file(root.file_name().unwrap()) {
                    Ok(lock) if lock.metadata().map_err(Error::input_display)?.is_file() => {
                        lock.try_lock().map_err(|_| Error::busy())?;
                        self.session_lock = Some(lock);
                    }
                    Ok(_) => return Err(Error::input("session lock must be a regular file")),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(Error::input_display(error)),
                }
            }
        }
        Ok(())
    }

    fn begin(&mut self, token: &str) -> Result<Response> {
        self.lock(token)?;
        let observed = self.last.as_ref().unwrap().0.clone();
        let root = state::contained(self.paths.home(), ".skillator/rsync/session.lock")?;
        let parent = Directory::open_parent(self.paths.home(), root.parent().unwrap())
            .map_err(Error::input_display)?;
        if self.session_lock.is_none() {
            let lock = parent
                .open_lock(root.file_name().unwrap())
                .map_err(Error::input_display)?;
            if !lock.metadata().map_err(Error::input_display)?.is_file() {
                return Err(Error::input("session lock must be a regular file"));
            }
            lock.try_lock().map_err(|_| Error::busy())?;
            self.session_lock = Some(lock);
        }
        let recovered = recover_pending(self.paths.home())?;
        self.history = observed.history;
        self.history_expected =
            state::fingerprint_contained(self.paths.home(), ".skillator/rsync/state.json")?;
        let new_identity = self.history.id.is_none();
        let id = self.history.id.clone().unwrap_or(state::new_id()?);
        self.history.id = Some(id.clone());
        if new_identity {
            self.history
                .save(self.paths.home(), &self.history_expected)?;
            self.history_expected =
                state::fingerprint_contained(self.paths.home(), ".skillator/rsync/state.json")?;
        }
        let stage_name = format!("stage-{}", state::new_id()?);
        let stage = root.parent().unwrap().join(&stage_name);
        parent
            .create_dir(std::ffi::OsStr::new(&stage_name))
            .map_err(Error::input_display)?;
        let opened_stage = parent
            .open_dir(std::ffi::OsStr::new(&stage_name))
            .map_err(Error::input_display)?;
        self.stage_identity = Some(opened_stage.identity().map_err(Error::input_display)?);
        self.stage = Some(stage.clone());
        self.stage_parent = Some(parent);
        self.validate_stage_root()?;
        Ok(Response::Begun {
            id,
            stage: stage.to_string_lossy().into_owned(),
            device: self.stage_identity.unwrap().0,
            inode: self.stage_identity.unwrap().1,
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

    fn validate_stage_root(&self) -> Result<&Path> {
        self.require_active()?;
        let stage = self.stage.as_deref().unwrap();
        let metadata = fs::symlink_metadata(stage).map_err(|_| {
            Error::input("synchronization stage changed after Begin; abort and recover")
        })?;
        if !metadata.file_type().is_dir()
            || self.stage_identity != Some((metadata.dev(), metadata.ino()))
            || stage.canonicalize().map_err(Error::input_display)? != stage
        {
            return Err(Error::input(
                "synchronization stage changed after Begin; abort and recover",
            ));
        }
        state::home_relative(self.paths.home(), stage)?;
        Ok(stage)
    }

    fn open_stage(&self) -> Result<Directory> {
        let stage = self.validate_stage_root()?;
        let directory = self
            .stage_parent
            .as_ref()
            .unwrap()
            .open_dir(stage.file_name().unwrap())
            .map_err(Error::input_display)?;
        if Some(directory.identity().map_err(Error::input_display)?) != self.stage_identity {
            return Err(Error::input(
                "synchronization stage changed after Begin; abort and recover",
            ));
        }
        Ok(directory)
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
                if snapshot.sources.iter().any(|nested| {
                    nested.root.len() > source.root.len()
                        && Path::new(&nested.root).starts_with(&source.root)
                        && Path::new(path).starts_with(&nested.root)
                }) {
                    return false;
                }
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
        let directory =
            Directory::open_parent(self.paths.home(), parent).map_err(Error::input_display)?;
        let stage_name = format!(".skillator-clone-{}", state::new_id()?);
        let stage_name = std::ffi::OsStr::new(&stage_name);
        let stage = parent.join(stage_name);
        directory
            .create_dir(stage_name)
            .map_err(Error::input_display)?;
        let stage_dir = directory
            .open_dir(stage_name)
            .map_err(Error::input_display)?;
        let result = (|| {
            validate_clone_stage(self.paths.home(), &stage, stage_dir.as_file())?;
            git_in_directory(stage_dir.as_file(), &["clone", "--no-checkout", "--", &git.origin, "."])
                .map_err(|error| Error::input(format!(
                "cannot clone Git source {}; verify the configured origin, repository access, credentials, and network connectivity: {error}", source.root
            )))?;
            validate_clone_stage(self.paths.home(), &stage, stage_dir.as_file())?;
            if git_in_directory(
                stage_dir.as_file(),
                &["cat-file", "-e", &format!("{}^{{commit}}", git.commit)],
            )
            .is_err()
            {
                git_in_directory(stage_dir.as_file(), &["fetch", "--", "origin", &git.commit])
                    .map_err(|error| Error::input(format!(
                        "cannot fetch exact commit {} for Git source {}; ensure the commit is published and accessible from this host: {error}", git.commit, source.root
                    )))?;
            }
            validate_clone_stage(self.paths.home(), &stage, stage_dir.as_file())?;
            git_in_directory(
                stage_dir.as_file(),
                &["checkout", "--detach", &git.commit, "--"],
            )?;
            let origin = String::from_utf8(git_in_directory(
                stage_dir.as_file(),
                &["remote", "get-url", "origin"],
            )?)
            .map_err(Error::input_display)?;
            let commit = String::from_utf8(git_in_directory(
                stage_dir.as_file(),
                &["rev-parse", "--verify", "HEAD^{commit}"],
            )?)
            .map_err(Error::input_display)?;
            if origin.trim() != git.origin || commit.trim() != git.commit {
                return Err(Error::input(
                    "clone did not produce the exact requested Git reference",
                ));
            }
            validate_clone_stage(self.paths.home(), &stage, stage_dir.as_file())?;
            if state::contained(self.paths.home(), &source.root)? != destination {
                return Err(Error::input("clone parent changed"));
            }
            directory
                .rename_noreplace(stage_name, destination.file_name().unwrap())
                .map_err(Error::input_display)
        })();
        if result.is_err()
            && validate_clone_stage(self.paths.home(), &stage, stage_dir.as_file()).is_ok()
        {
            let _ = directory.remove_tree(stage_name);
        }
        result
    }

    fn register(&mut self, locations: &[Location], expected: Option<String>) -> Result<()> {
        if let Some((desired, fingerprint)) = self.prepare_registration(locations, expected)? {
            let bytes = LibraryConfigCodec::render(&desired).map_err(Error::input_display)?;
            state::save_contained_bytes(
                self.paths.home(),
                ".skillator/library.yaml",
                bytes.as_bytes(),
                &fingerprint,
            )?;
            self.last.as_mut().unwrap().0.library_hash = Some(state::digest(bytes.as_bytes()));
        }
        Ok(())
    }

    fn prepare_registration(
        &self,
        locations: &[Location],
        expected: Option<String>,
    ) -> Result<Option<(crate::config::LibraryConfig, Fingerprint)>> {
        let bytes = state::read_contained(self.paths.home(), ".skillator/library.yaml")?;
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
        let directory = Directory::open_parent(self.paths.home(), destination.parent().unwrap())
            .map_err(Error::input_display)?;
        let destination_name = destination.file_name().unwrap();
        let unchanged = || {
            matches!(state::contained(self.paths.home(), path), Ok(current) if current == destination)
                && matches!(state::contained(self.paths.home(), target).and_then(|path| path.canonicalize().map_err(Error::input_display)), Ok(current) if current == real_target)
        };
        if !unchanged() || state::observe_at(&directory, destination_name)?.is_some() {
            return Err(Error::input(
                "alias destination or target changed before staging",
            ));
        }
        let sibling_name = format!(".skillator-alias-{}", state::new_id()?);
        let sibling_name = std::ffi::OsStr::new(&sibling_name);
        directory
            .symlink(sibling_name, physical_target.as_os_str())
            .map_err(Error::input_display)?;
        if !unchanged() || state::observe_at(&directory, destination_name)?.is_some() {
            let _ = directory.remove(sibling_name);
            return Err(Error::input(
                "alias destination or target changed during staging",
            ));
        }
        let result = directory
            .rename_noreplace(sibling_name, destination_name)
            .map_err(Error::input_display);
        if result.is_err() {
            let _ = directory.remove(sibling_name);
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
        let directory =
            Directory::open_parent(self.paths.home(), parent).map_err(Error::input_display)?;
        let destination_name = destination.file_name().unwrap();
        if state::observe_at(&directory, destination_name)?.as_ref() != expected {
            return Err(Error::input("destination changed after observation"));
        }
        let sibling_name = format!(".skillator-rsync-{}", state::new_id()?);
        let sibling_name = std::ffi::OsStr::new(&sibling_name);
        let sibling = parent.join(sibling_name);
        if let Some(desired) = desired {
            if matches!(desired, Entry::Directory) {
                directory
                    .create_dir(sibling_name)
                    .map_err(Error::input_display)?;
            } else {
                let stage = staged.ok_or_else(|| Error::input("missing staged content"))?;
                let stage_path = Path::new(stage);
                if stage_path.parent() != Some(self.validate_stage_root()?) {
                    return Err(Error::input("invalid staged path"));
                }
                let stage_dir = self.open_stage()?;
                let stage_name = stage_path.file_name().unwrap();
                if state::observe_at(&stage_dir, stage_name)?.as_ref() != Some(desired) {
                    return Err(Error::input("staged content does not match planned value"));
                }
                copy_entry_at(&stage_dir, stage_name, &directory, sibling_name, desired)?;
            }
        }
        if !matches!(state::contained(self.paths.home(), path), Ok(current) if current == destination)
            || state::observe_at(&directory, destination_name)?.as_ref() != expected
        {
            if desired.is_some() {
                let _ = directory.remove(sibling_name);
            }
            return Err(Error::input("destination changed during staging"));
        }
        let stage_dir = self.open_stage()?;
        let journal_name = format!("recovery-{}.json", state::new_id()?);
        let mut journal = stage_dir
            .create_file(std::ffi::OsStr::new(&journal_name))
            .map_err(Error::input_display)?;
        journal
            .write_all(
                &serde_json::to_vec(&Recovery {
                    path: path.into(),
                    expected: expected.cloned(),
                    desired: desired.cloned(),
                    sibling: sibling.clone(),
                })
                .map_err(Error::input_display)?,
            )
            .and_then(|()| journal.sync_all())
            .map_err(Error::input_display)?;
        let result = match (expected, desired) {
            (None, Some(_)) => directory.rename_noreplace(sibling_name, destination_name),
            (Some(_), Some(_)) => directory.rename_exchange(sibling_name, destination_name),
            (Some(_), None) => directory.rename_noreplace(destination_name, sibling_name),
            (None, None) => Ok(()),
        };
        if let Err(error) = result {
            return Err(Error::input(format!(
                "publication failed; inspect recovery journals under ~/.skillator/rsync: {error}"
            )));
        }
        let verified = (|| -> Result<bool> {
            let preserved = if expected.is_some() {
                state::observe_at(&directory, sibling_name)?
            } else {
                None
            };
            Ok(preserved.as_ref() == expected
                && (!matches!(expected, Some(Entry::Directory))
                    || directory.is_empty_dir(sibling_name).map_err(Error::input_display)?)
                && state::observe_at(&directory, destination_name)?.as_ref() == desired
                && matches!(state::contained(self.paths.home(), path), Ok(current) if current == destination))
        })()
        .unwrap_or(false);
        if !verified {
            let rollback = match (expected, desired) {
                (Some(_), Some(_)) => directory.rename_exchange(sibling_name, destination_name),
                (Some(_), None) => directory.rename_noreplace(sibling_name, destination_name),
                (None, Some(_)) => directory.rename_noreplace(destination_name, sibling_name),
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
        if state::observe_at(&directory, sibling_name)?.is_some() {
            directory
                .remove(sibling_name)
                .map_err(Error::input_display)?;
        }
        stage_dir
            .remove(std::ffi::OsStr::new(&journal_name))
            .map_err(Error::input_display)?;
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        let stage_valid = if self.stage.is_some() {
            self.validate_stage_root().map(|_| ())
        } else {
            Ok(())
        };
        let cleanup = if let Some(stage) = self.stage.as_ref()
            && stage_valid.is_ok()
        {
            (|| -> Result<()> {
                let parent = self.stage_parent.as_ref().unwrap();
                let name = stage.file_name().unwrap();
                let opened = parent.open_dir(name).map_err(Error::input_display)?;
                if Some(opened.identity().map_err(Error::input_display)?) != self.stage_identity {
                    return Err(Error::input("synchronization stage changed during cleanup"));
                }
                if !opened
                    .entries()
                    .map_err(Error::input_display)?
                    .iter()
                    .any(|name| name.to_string_lossy().starts_with("recovery-"))
                {
                    parent.remove_tree(name).map_err(Error::input_display)?;
                }
                Ok(())
            })()
        } else {
            Ok(())
        };
        self.stage = None;
        self.stage_parent = None;
        self.stage_identity = None;
        self.user_lock = None;
        self.session_lock = None;
        stage_valid.and(cleanup)
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
    let directory = match Directory::open_existing_parent(home, &root) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::input_display(error)),
    };
    let mut records = Vec::new();
    for name in directory.entries().map_err(Error::input_display)? {
        if !name.to_string_lossy().starts_with("stage-") {
            continue;
        }
        if directory
            .metadata(&name)
            .map_err(Error::input_display)?
            .st_mode
            & libc::S_IFMT
            != libc::S_IFDIR
        {
            continue;
        }
        let stage = directory.open_dir(&name).map_err(Error::input_display)?;
        for file in stage.entries().map_err(Error::input_display)? {
            if file.to_string_lossy().starts_with("recovery-") {
                records.push(root.join(&name).join(file));
            }
        }
    }
    records.sort();
    Ok(records)
}

fn recover_pending(home: &Path) -> Result<usize> {
    let records = pending_recovery(home)?;
    for record in &records {
        let root = record.parent().unwrap().parent().unwrap();
        let stage_parent =
            Directory::open_existing_parent(home, root).map_err(Error::input_display)?;
        let stage = stage_parent
            .open_dir(record.parent().unwrap().file_name().unwrap())
            .map_err(Error::input_display)?;
        let mut bytes = Vec::new();
        stage
            .open_file(record.file_name().unwrap())
            .map_err(Error::input_display)?
            .read_to_end(&mut bytes)
            .map_err(Error::input_display)?;
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
        let parent = Directory::open_existing_parent(home, destination.parent().unwrap())
            .map_err(Error::input_display)?;
        let destination_name = destination.file_name().unwrap();
        let sibling_name = recovery.sibling.file_name().unwrap();
        let actual = state::observe_at(&parent, destination_name)?;
        let backup = state::observe_at(&parent, sibling_name)?;
        // Published and removed directory entries are empty at the journal boundary.
        // A type-only observation cannot prove that later children are ours to move.
        for (path, name, entry) in [
            (&destination, destination_name, &actual),
            (&recovery.sibling, sibling_name, &backup),
        ] {
            if matches!(entry, Some(Entry::Directory))
                && !parent.is_empty_dir(name).map_err(Error::input_display)?
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
                (Some(_), Some(_)) => parent.rename_exchange(sibling_name, destination_name),
                (Some(_), None) => parent.rename_noreplace(sibling_name, destination_name),
                (None, Some(_)) => parent.rename_noreplace(destination_name, sibling_name),
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
        if state::observe_at(&parent, sibling_name)?.is_some() {
            parent.remove(sibling_name).map_err(Error::input_display)?;
        }
        stage
            .remove(record.file_name().unwrap())
            .map_err(Error::input_display)?;
    }
    Ok(records.len())
}

fn reserved_temporary(name: &str) -> bool {
    [".skillator-rsync-", ".skillator-clone-"]
        .iter()
        .any(|prefix| name.strip_prefix(prefix).is_some_and(state::valid_id))
}

fn validate_clone_stage(home: &Path, stage: &Path, directory: &File) -> Result<()> {
    let current = fs::symlink_metadata(stage)
        .map_err(|_| Error::input("clone staging directory changed; preserve it for recovery"))?;
    let opened = directory.metadata().map_err(Error::input_display)?;
    if !current.file_type().is_dir()
        || (current.dev(), current.ino()) != (opened.dev(), opened.ino())
    {
        return Err(Error::input(
            "clone staging directory changed; preserve it for recovery",
        ));
    }
    state::home_relative(home, stage)?;
    Ok(())
}

fn git_in_directory(directory: &File, args: &[&str]) -> Result<Vec<u8>> {
    let mut command = Command::new("git");
    command
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args);
    bind_directory(&mut command, directory);
    process::capture(&mut command)
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

fn copy_entry_at(
    source_parent: &Directory,
    source_name: &std::ffi::OsStr,
    parent: &Directory,
    name: &std::ffi::OsStr,
    entry: &Entry,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    match entry {
        Entry::File { executable, .. } => {
            let mut input = source_parent
                .open_file(source_name)
                .map_err(Error::input_display)?;
            let mut output = parent.create_file(name).map_err(Error::input_display)?;
            std::io::copy(&mut input, &mut output).map_err(Error::input_display)?;
            output
                .set_permissions(fs::Permissions::from_mode(if *executable {
                    0o755
                } else {
                    0o644
                }))
                .map_err(Error::input_display)?;
            output.sync_all().map_err(Error::input_display)?;
        }
        Entry::Link { target } => parent
            .symlink(name, std::ffi::OsStr::new(target))
            .map_err(Error::input_display)?,
        Entry::Directory => parent.create_dir(name).map_err(Error::input_display)?,
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
    fn replaced_stage_directory_is_rejected_before_export_or_publication() {
        for symlink in [true, false] {
            let (home, mut session, stage) = setup();
            let preserved = stage.with_extension("preserved");
            fs::rename(&stage, &preserved).unwrap();
            let outside = tempfile::tempdir().unwrap();
            if symlink {
                std::os::unix::fs::symlink(outside.path(), &stage).unwrap();
            } else {
                fs::create_dir(&stage).unwrap();
            }
            let error = session.handle(Request::ValidateStage).unwrap_err();
            assert!(error.message.contains("stage changed"), "{error}");
            let source = home.path().join(".skillator/library/demo/data");
            let expected = state::observe(&source).unwrap().unwrap();
            assert!(
                session
                    .handle(Request::Export {
                        path: ".skillator/library/demo/data".into(),
                        expected: expected.clone(),
                    })
                    .unwrap_err()
                    .message
                    .contains("stage changed")
            );
            assert!(
                session
                    .handle(Request::Publish {
                        path: ".skillator/library/demo/data".into(),
                        expected: Some(expected.clone()),
                        desired: Some(expected),
                        stage: None,
                    })
                    .unwrap_err()
                    .message
                    .contains("stage changed")
            );
            assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
            assert!(preserved.exists());
            assert!(session.handle(Request::Finish).is_err());
            assert!(preserved.exists());
        }
    }

    #[test]
    fn clone_process_keeps_its_opened_directory_after_parent_redirect() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let parent = home.path().join("checkout-parent");
        let stage = parent.join(".skillator-clone-0123456789abcdef0123456789abcdef");
        fs::create_dir_all(&stage).unwrap();
        let directory = File::options()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&stage)
            .unwrap();
        validate_clone_stage(home.path(), &stage, &directory).unwrap();
        let preserved = home.path().join("preserved-parent");
        fs::rename(&parent, &preserved).unwrap();
        std::os::unix::fs::symlink(outside.path(), &parent).unwrap();
        let mut command = Command::new("sh");
        command.args(["-c", "printf safe > marker"]);
        bind_directory(&mut command, &directory);
        assert!(command.status().unwrap().success());
        assert_eq!(
            fs::read_to_string(preserved.join(stage.file_name().unwrap()).join("marker")).unwrap(),
            "safe"
        );
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
        assert!(validate_clone_stage(home.path(), &stage, &directory).is_err());
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
            let error = UserScopeWorkflow::save_remote(
                &paths,
                session.config,
                expected,
                locks,
                false,
                &crate::config::LibraryConfig::empty(),
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("changed after observation"),
                "{error}"
            );
            assert_eq!(fs::read(paths.user_config()).unwrap(), edited);
        }
    }

    #[test]
    fn remote_user_reconciliation_rejects_a_replaced_library_configuration() {
        let (home, mut session, _) = setup();
        let outside = tempfile::tempdir().unwrap();
        let config = home.path().join(".skillator/library.yaml");
        fs::write(
            outside.path().join("library.yaml"),
            format!(
                "version: 1\nlocations: [{{path: '{}'}}]\n",
                outside.path().display()
            ),
        )
        .unwrap();
        fs::rename(&config, config.with_extension("saved")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("library.yaml"), &config).unwrap();
        let error = session
            .handle(Request::User {
                entries: BTreeMap::new(),
                expected: None,
                check: false,
            })
            .unwrap_err();
        assert!(
            error.message.contains("link") || error.message.contains("symbolic"),
            "{error}"
        );
        assert!(!home.path().join(".agents/skillator.yaml").exists());
        fs::remove_file(&config).unwrap();
        fs::write(&config, "version: 1\nlocations: []\n").unwrap();
        let error = session
            .handle(Request::User {
                entries: BTreeMap::new(),
                expected: None,
                check: false,
            })
            .unwrap_err();
        assert!(
            error.message.contains("changed after observation"),
            "{error}"
        );
        assert!(!home.path().join(".agents/skillator.yaml").exists());
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
        let (home, mut session, _) = setup();
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
    fn copied_file_rejects_a_replaced_source_symlink() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = home.path().join("source");
        fs::write(&source, "original").unwrap();
        let expected = state::observe(&source).unwrap().unwrap();
        fs::write(outside.path().join("secret"), "outside").unwrap();
        fs::remove_file(&source).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret"), &source).unwrap();
        let directory =
            Directory::open_existing_parent(home.path(), &home.path().canonicalize().unwrap())
                .unwrap();
        assert!(
            copy_entry_at(
                &directory,
                std::ffi::OsStr::new("source"),
                &directory,
                std::ffi::OsStr::new("copy"),
                &expected
            )
            .is_err()
        );
        assert!(!home.path().join("copy").exists());
        assert_eq!(
            fs::read_to_string(outside.path().join("secret")).unwrap(),
            "outside"
        );
    }

    #[test]
    fn existing_session_lock_rejects_a_final_symlink() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".skillator/rsync")).unwrap();
        fs::write(outside.path().join("lock"), "outside").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("lock"),
            home.path().join(".skillator/rsync/session.lock"),
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
        assert!(session.handle(Request::Begin { token }).is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("lock")).unwrap(),
            "outside"
        );
        assert!(session.stage.is_none());
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
