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
            Request::Bootstrap { source } => {
                self.require_active()?;
                self.bootstrap(&source)?;
                Ok(Response::Ok)
            }
            Request::Export { path, expected } => {
                self.require_active()?;
                self.authorize_path(&path)?;
                let source = state::contained(self.paths.home(), &path)?;
                if observed_content(&source, Some(&expected))? != Some(expected.clone()) {
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
                if hash_optional(&path)? != expected {
                    return Err(Error::input("user configuration changed after observation"));
                }
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
                let report = UserScopeWorkflow::save_remote(&self.paths, desired, locks, check)
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
                            Some(path) => observed_content(&path, expected.as_ref())?,
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

    fn begin(&mut self, token: &str) -> Result<Response> {
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
        self.user_lock = Some(TargetLocks::acquire(&[&target]).map_err(|_| Error::busy())?);
        let observed = snapshot::inspect(&self.paths, sources)?;
        if state::digest(&serde_json::to_vec(&observed).map_err(Error::input_display)?) != token {
            self.user_lock = None;
            return Err(Error::input("participant changed after preflight; retry"));
        }
        let root = state::contained(self.paths.home(), ".skillator/rsync/session.lock")?;
        fs::create_dir_all(root.parent().unwrap()).map_err(Error::input_display)?;
        if fs::symlink_metadata(&root).is_ok_and(|meta| !meta.is_file()) {
            return Err(Error::input("session lock must be a regular file"));
        }
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&root)
            .map_err(Error::input_display)?;
        lock.try_lock().map_err(|_| Error::busy())?;
        self.session_lock = Some(lock);
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
        state::relative(path)?;
        if Path::new(path)
            .components()
            .any(|part| part.as_os_str() == ".git")
            || path.starts_with(".skillator/rsync/")
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
                    let root = if skill.is_empty() {
                        source.root.clone()
                    } else {
                        format!("{}/{skill}", source.root)
                    };
                    path == root || path.starts_with(&format!("{root}/"))
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
        if git.origin.is_empty()
            || git.origin.starts_with('-')
            || git.origin.contains('\0')
            || git.origin.contains("::")
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
            )?;
            if process::git(
                &stage,
                &["cat-file", "-e", &format!("{}^{{commit}}", git.commit)],
            )
            .is_err()
            {
                process::git(&stage, &["fetch", "--", "origin", &git.commit])?;
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
        if hash_optional(&self.paths.library_config())? != expected {
            return Err(Error::input(
                "library configuration changed after observation",
            ));
        }
        let config = snapshot::library(&self.paths)?;
        let mut desired = config.locations().to_vec();
        for location in locations {
            let destination = state::contained(self.paths.home(), &location.path)?;
            if !destination.is_dir() {
                return Err(Error::input("incoming library location is unavailable"));
            }
            let found = desired.iter().find(|existing| {
                crate::library::expand_location(
                    existing.path(),
                    self.paths.library_config().parent().unwrap(),
                    self.paths.home(),
                    self.paths.environment(),
                )
                .is_ok_and(|path| path == self.paths.home().join(&location.path))
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
            return Ok(());
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
        let fingerprint = state::fingerprint(&self.paths.library_config())?;
        fs::create_dir_all(self.paths.library_config().parent().unwrap())
            .map_err(Error::input_display)?;
        save_library(&self.paths.library_config(), &desired, &fingerprint)
            .map_err(Error::input_display)?;
        Ok(())
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
        let actual = observed_content(&destination, desired.or(expected))?;
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
            || observed_content(&destination, expected)?.as_ref() != expected
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
            || observed_content(&destination, desired)?.as_ref() != desired
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
        let boundary = snapshot
            .sources
            .iter()
            .flat_map(|s| {
                s.skills
                    .iter()
                    .map(move |skill| Path::new(&s.root).join(skill))
            })
            .filter(|root| logical.starts_with(root))
            .max_by_key(|root| root.components().count())
            .ok_or_else(|| Error::input("link has no skill boundary"))?;
        if !resolved.starts_with(boundary) {
            return Err(Error::input("skill link escapes its skill directory"));
        }
        state::contained(
            self.paths.home(),
            resolved
                .to_str()
                .ok_or_else(|| Error::input("invalid link path"))?,
        )?;
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

pub(super) fn reserved_temporary(name: &str) -> bool {
    [".skillator-rsync-", ".skillator-clone-"]
        .iter()
        .any(|prefix| name.strip_prefix(prefix).is_some_and(state::valid_id))
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

pub(super) fn observed_content(path: &Path, expected: Option<&Entry>) -> Result<Option<Entry>> {
    // A library acquisition root link denotes the skill directory, not a transferred link.
    if matches!(expected, Some(Entry::Directory)) && path.is_symlink() && path.is_dir() {
        return Ok(Some(Entry::Directory));
    }
    state::observe(path)
}

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
        assert!(session.handle(Request::Bootstrap { source }).is_err());
        assert!(!home.path().join("Development/acme/skills").exists());
        assert!(
            fs::read_dir(home.path().join("Development/acme"))
                .unwrap()
                .next()
                .is_none()
        );
    }
}
