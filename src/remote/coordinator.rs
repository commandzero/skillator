use super::{
    Error, Result,
    config::Config,
    planner::{self, ConflictPolicy, Decision, MissingPolicy, Observation},
    session::{Request, Response},
    snapshot::{Location, Snapshot, Source},
    stage::Stage,
    state::{self, Baseline, Entry},
    transport::Endpoint,
};
use crate::app::{AppPaths, ReportDiagnostic};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) struct Options {
    pub hosts: Option<String>,
    pub conflict: ConflictPolicy,
    pub missing: MissingPolicy,
    pub check: bool,
    pub interactive: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    format_version: u8,
    status: &'static str,
    pub exit_status: u8,
    mode: &'static str,
    target: String,
    changes: Vec<Change>,
    diagnostics: Vec<ReportDiagnostic>,
}

#[derive(Debug, Serialize)]
struct Change {
    host: String,
    path: String,
    action: String,
    safety: String,
    outcome: crate::app::ReportOutcome,
}

impl Report {
    fn new(paths: &AppPaths) -> Self {
        Self {
            format_version: 1,
            status: "in_sync",
            exit_status: 0,
            mode: "library_rsync",
            target: paths.home().to_string_lossy().into_owned(),
            changes: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
    fn problem(&mut self, host: &str, path: &str, code: &str, message: impl Into<String>) {
        self.status = "not_converged";
        self.exit_status = 1;
        self.diagnostics.push(ReportDiagnostic {
            code: code.into(),
            severity: "error".into(),
            message: message.into(),
            data: Some(BTreeMap::from([
                ("host".into(), host.into()),
                ("path".into(), path.into()),
            ])),
        });
    }
    fn changed(&mut self, host: &str, path: &str, action: &'static str, check: bool) {
        self.changes.push(Change {
            host: host.into(),
            path: path.into(),
            action: action.into(),
            safety: "safe".into(),
            outcome: if check {
                crate::app::ReportOutcome::WouldApply
            } else {
                crate::app::ReportOutcome::Applied
            },
        });
        if check {
            self.status = "not_converged";
            self.exit_status = 1;
        }
    }
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.text_with_color(false)
    }

    pub fn text_with_color(&self, color: bool) -> String {
        if self.changes.is_empty() && self.diagnostics.is_empty() {
            return "In sync.\n".into();
        }
        let mut text = String::new();
        for change in &self.changes {
            let action = if color {
                format!("\x1b[36m{}\x1b[0m", change.action)
            } else {
                change.action.clone()
            };
            text.push_str(&format!(
                "{}: {} {} ({:?})\n",
                change.host, action, change.path, change.outcome
            ));
        }
        for diagnostic in &self.diagnostics {
            let message = if color {
                format!(
                    "\x1b[{}m{}\x1b[0m",
                    if diagnostic.severity == "error" {
                        "31"
                    } else {
                        "33"
                    },
                    diagnostic.message
                )
            } else {
                diagnostic.message.clone()
            };
            let data = diagnostic.data.as_ref();
            text.push_str(&format!(
                "{}: {}: {}\n",
                data.and_then(|d| d.get("host"))
                    .map(String::as_str)
                    .unwrap_or("local"),
                data.and_then(|d| d.get("path"))
                    .map(String::as_str)
                    .unwrap_or(""),
                message
            ));
        }
        text
    }
}

struct Participant {
    alias: String,
    endpoint: Endpoint,
    snapshot: Snapshot,
    token: String,
    id: Option<String>,
    stage: Option<Stage>,
}

impl Participant {
    fn observe(alias: String, mut endpoint: Endpoint, sources: Vec<Source>) -> Result<Self> {
        let Response::Snapshot { snapshot, token } = endpoint
            .request(Request::Inspect { sources })
            .map_err(|e| Error {
                code: e.code,
                message: format!("{} {alias}", e.message),
            })?
        else {
            return Err(Error::input("remote did not return an observation"));
        };
        if snapshot.protocol != 5 {
            return Err(Error::input(format!(
                "incompatible Skillator {} on remote host {alias}; protocol 5 is required",
                snapshot.version
            )));
        }
        if !snapshot.home.is_absolute() {
            return Err(Error::input("remote home is not absolute"));
        }
        Ok(Self {
            alias,
            endpoint,
            id: snapshot.history.id.clone(),
            snapshot: *snapshot,
            token,
            stage: None,
        })
    }
    fn inspect(&mut self, sources: &[Source]) -> Result<()> {
        let Response::Snapshot { snapshot, token } = self.endpoint.request(Request::Inspect {
            sources: sources.to_vec(),
        })?
        else {
            return Err(Error::input("invalid inspection response"));
        };
        if snapshot.history.id != self.id {
            return Err(Error::input(format!(
                "participant identity changed after observation: {}",
                self.alias
            )));
        }
        self.snapshot = *snapshot;
        self.token = token;
        Ok(())
    }
    fn request_ok(&mut self, request: Request) -> Result<()> {
        if matches!(self.endpoint.request(request)?, Response::Ok) {
            Ok(())
        } else {
            Err(Error::input("unexpected remote protocol response"))
        }
    }
}

pub(crate) fn run(paths: &AppPaths, options: Options) -> Result<Report> {
    let hosts = Config::load(paths.home())?.select(options.hosts.as_deref())?;
    let local = Participant::observe("local".into(), Endpoint::local(paths.clone()), Vec::new())?;
    let mut participants = vec![local];
    for (alias, host) in hosts {
        let endpoint = Endpoint::ssh(&host.destination).map_err(|_| {
            Error::input(format!(
                "cannot connect to remote host {alias}; verify SSH configuration and trust"
            ))
        })?;
        participants.push(Participant::observe(alias, endpoint, Vec::new())?);
    }
    synchronize(paths, participants, options)
}

fn synchronize(paths: &AppPaths, mut peers: Vec<Participant>, options: Options) -> Result<Report> {
    let mut result = synchronize_inner(paths, &mut peers, options);
    for peer in &mut peers {
        if let Err(error) = peer.request_ok(Request::Finish)
            && peer.stage.is_some()
            && let Ok(report) = &mut result
        {
            report.problem(&peer.alias, "", "recovery_required", error.to_string());
        }
    }
    if let Ok(report) = &mut result {
        report
            .changes
            .sort_by(|a, b| (&a.host, &a.path, &a.action).cmp(&(&b.host, &b.path, &b.action)));
        report
            .diagnostics
            .sort_by(|a, b| (&a.data, &a.code, &a.message).cmp(&(&b.data, &b.code, &b.message)));
    }
    result
}

fn synchronize_inner(
    paths: &AppPaths,
    peers: &mut [Participant],
    options: Options,
) -> Result<Report> {
    let mut report = Report::new(paths);
    let mut ids = BTreeSet::new();
    for peer in peers.iter() {
        if let Some(id) = &peer.id
            && !ids.insert(id)
        {
            return Err(Error::input(
                "duplicate participant identity; check host aliases and copied state directories",
            ));
        }
        if let Some(previous) = peers[0].snapshot.history.aliases.get(&peer.alias)
            && peer.id.as_ref() != Some(previous)
        {
            return Err(Error::input(format!(
                "participant identity changed for {}; restore its history or explicitly reset the saved alias association before first-contact synchronization",
                peer.alias
            )));
        }
    }
    // A receiving host may later initiate with different local SSH aliases. Its
    // acknowledged peer identities still protect an established cohort from a
    // replacement whose new identity has no matching alias on this machine.
    let history = &peers[0].snapshot.history;
    if !history.peers.is_empty()
        && peers.len() - 1 <= history.peers.len()
        && peers.iter().skip(1).any(|peer| {
            peer.id
                .as_ref()
                .is_none_or(|id| !history.peers.contains_key(id))
        })
    {
        return Err(Error::input(
            "participant identity changed in the selected host set; restore its history or explicitly reset the saved peer association before first-contact synchronization",
        ));
    }
    let mut sources = BTreeMap::<String, Source>::new();
    let mut blocked = BTreeSet::new();
    let local_roots: BTreeSet<_> = peers[0]
        .snapshot
        .sources
        .iter()
        .map(|s| s.root.clone())
        .collect();
    for peer in peers.iter() {
        for source in &peer.snapshot.sources {
            if source.git.is_some() && !local_roots.contains(&source.root) {
                blocked.insert(source.root.clone());
                report.problem(
                    &peer.alias,
                    &source.root,
                    "missing_git_reference",
                    "Git source has no initiating checkout; set it up locally before synchronizing",
                );
            }
            if let Some(reference) = sources.get(&source.root) {
                if reference.key != source.key || reference.git != source.git {
                    blocked.insert(source.root.clone());
                    report.problem(
                        &peer.alias,
                        &source.root,
                        "git_mismatch",
                        "source identity or Git commit differs; align it separately",
                    );
                }
            } else {
                sources.insert(source.root.clone(), source.clone());
            }
            if let Some(reference) = sources.get_mut(&source.root) {
                reference.skills.extend(source.skills.iter().cloned());
                reference
                    .invalid_skills
                    .extend(source.invalid_skills.iter().cloned());
            }
        }
    }
    let sources: Vec<Source> = sources.into_values().collect();
    let mut identities = BTreeMap::<&str, BTreeSet<&str>>::new();
    for source in &sources {
        identities
            .entry(&source.key)
            .or_default()
            .insert(&source.root);
    }
    for (identity, roots) in identities {
        if roots.len() > 1 {
            for root in roots {
                blocked.insert(root.to_owned());
                report.problem("local", root, "source_identity_collision", format!("source identity {identity} maps to multiple paths; align registrations before synchronization"));
            }
        }
    }

    let mut locations = BTreeMap::<String, Location>::new();
    for peer in peers.iter() {
        for location in &peer.snapshot.locations {
            if locations
                .get(&location.path)
                .is_some_and(|existing| existing != location)
            {
                for source in &sources {
                    if source.location == location.path {
                        blocked.insert(source.root.clone());
                    }
                }
                report.problem(
                    &peer.alias,
                    &location.path,
                    "location_conflict",
                    "location exclusions or overlap settings differ; align them separately",
                );
            } else {
                locations.insert(location.path.clone(), location.clone());
            }
        }
    }
    // Inspect incoming paths on every host before beginning any mutation.
    for peer in peers.iter_mut() {
        peer.inspect(&sources).map_err(|e| Error {
            code: e.code,
            message: format!("{}: {e}", peer.alias),
        })?;
        for message in &peer.snapshot.problems {
            report.diagnostics.push(ReportDiagnostic {
                code: "library_advisory".into(),
                severity: "warning".into(),
                message: message.clone(),
                data: Some(BTreeMap::from([("host".into(), peer.alias.clone())])),
            });
        }
        for source in &peer.snapshot.sources {
            if !source.problems.is_empty() {
                blocked.insert(source.root.clone());
                for message in &source.problems {
                    report.problem(&peer.alias, &source.root, "source_blocked", message);
                }
            }
            if let Some(reference) = sources.iter().find(|s| s.root == source.root)
                && let Some(actual) = &source.git
                && reference
                    .git
                    .as_ref()
                    .is_none_or(|expected| expected != actual)
            {
                blocked.insert(source.root.clone());
                report.problem(
                    &peer.alias,
                    &source.root,
                    "git_mismatch",
                    "Git commit differs; no checkout changes are authorized",
                );
            }
        }
    }
    if !options.check {
        // Reserve every participant before Begin creates identities or stages.
        for peer in peers.iter_mut() {
            let token = peer.token.clone();
            if let Err(error) = peer.request_ok(Request::Lock { token }) {
                let alias = peer.alias.clone();
                return Err(Error {
                    code: error.code,
                    message: format!("{alias}: {error}"),
                });
            }
        }
        for peer in peers.iter_mut() {
            match peer.endpoint.request(Request::Begin {
                token: peer.token.clone(),
            }) {
                Ok(Response::Begun {
                    id,
                    stage,
                    device,
                    inode,
                    recovered,
                }) => {
                    let descriptor = match Stage::new(
                        &peer.snapshot.home,
                        Path::new(&stage),
                        (device, inode),
                    ) {
                        Ok(descriptor) => descriptor,
                        Err(_) => {
                            report.problem(
                                &peer.alias,
                                "",
                                "begin_failed",
                                "remote returned a staging path outside the observed synchronization directory",
                            );
                            return Ok(report);
                        }
                    };
                    peer.id = Some(id);
                    peer.stage = Some(descriptor);
                    if recovered != 0 {
                        report.changed(&peer.alias, "", "recover_publication", false);
                    }
                }
                Ok(_) => return Err(Error::input("invalid begin response")),
                Err(error) => {
                    if error.code == 4 {
                        return Err(error);
                    }
                    report.problem(&peer.alias, "", "begin_failed", error.to_string());
                    return Ok(report);
                }
            }
        }
    }
    if !options.check {
        // Begin can take long enough for external Git changes; inspect again
        // before creating any receiving clone.
        if !inspect_git_checkpoint(peers, &sources, &mut blocked, &mut report, true) {
            return Ok(report);
        }
    }
    for source in &sources {
        if blocked.contains(&source.root) || source.git.is_none() {
            continue;
        }
        for peer in peers.iter_mut() {
            let actual = peer.snapshot.sources.iter().find(|s| s.root == source.root);
            if actual.is_some_and(|s| s.git.is_some()) {
                continue;
            }
            if options.check {
                report.changed(&peer.alias, &source.root, "clone_exact_commit", true);
                report.problem(
                    &peer.alias,
                    &source.root,
                    "bootstrap_unverified",
                    "checkout is absent; follow-on reconciliation needs inspection after bootstrap",
                );
                blocked.insert(source.root.clone());
            } else {
                match peer.request_ok(Request::Bootstrap {
                    source: source.clone(),
                }) {
                    Ok(()) => {
                        report.changed(&peer.alias, &source.root, "clone_exact_commit", false)
                    }
                    Err(error) => {
                        report.problem(
                            &peer.alias,
                            &source.root,
                            "bootstrap_failed",
                            error.to_string(),
                        );
                        blocked.insert(source.root.clone());
                    }
                }
            }
        }
    }
    if !options.check && !inspect_git_checkpoint(peers, &sources, &mut blocked, &mut report, false)
    {
        return Ok(report);
    }
    let mut acquisition_aliases = BTreeMap::<String, String>::new();
    let mut conflicting_aliases = BTreeSet::new();
    for peer in peers.iter() {
        for (path, target) in &peer.snapshot.acquisition_aliases {
            if acquisition_aliases
                .get(path)
                .is_some_and(|existing| existing != target)
            {
                report.problem(
                    &peer.alias,
                    path,
                    "alias_conflict",
                    "acquisition alias targets differ; align the links before synchronization",
                );
                conflicting_aliases.insert(path.clone());
                continue;
            }
            acquisition_aliases.insert(path.clone(), target.clone());
        }
    }
    let all_acquisition_aliases = acquisition_aliases.clone();
    acquisition_aliases.retain(|path, _| !conflicting_aliases.contains(path));
    acquisition_aliases.retain(|path, target| {
        let recognized = sources.iter().any(|source| {
            !blocked.contains(&source.root)
                && source.skills.iter().any(|skill| {
                    PathBuf::from(&source.root).join(skill).as_path()
                        == std::path::Path::new(target.as_str())
                })
        });
        if !recognized {
            report.problem(
                "local",
                path,
                "alias_target_unavailable",
                "alias target must be a registered, available skill location",
            );
        }
        recognized
    });
    let mut acknowledgements = vec![Baseline::default(); peers.len()];
    let failed_sources = sync_files(
        &sources,
        &blocked,
        &all_acquisition_aliases,
        peers,
        &options,
        &mut report,
        &mut acknowledgements,
    )?;
    blocked.extend(failed_sources);
    for (path, target) in &acquisition_aliases {
        if sources.iter().any(|source| {
            blocked.contains(&source.root)
                && source.skills.iter().any(|skill| {
                    PathBuf::from(&source.root).join(skill).as_path()
                        == std::path::Path::new(target)
                })
        }) {
            report.problem(
                "local",
                path,
                "alias_target_unverified",
                "alias target did not finish synchronization",
            );
            continue;
        }
        for peer in peers.iter_mut() {
            if peer.snapshot.acquisition_aliases.get(path) == Some(target) {
                continue;
            }
            if options.check {
                report.changed(&peer.alias, path, "create_acquisition_alias", true);
                continue;
            }
            match peer.request_ok(Request::Alias {
                path: path.clone(),
                target: target.clone(),
            }) {
                Ok(()) => report.changed(&peer.alias, path, "create_acquisition_alias", false),
                Err(error) => report.problem(&peer.alias, path, "alias_failed", error.to_string()),
            }
        }
    }
    let allowed_locations: Vec<_> = locations
        .into_values()
        .filter(|location| {
            sources
                .iter()
                .any(|s| s.location == location.path && !blocked.contains(&s.root))
        })
        .collect();
    for (index, peer) in peers.iter_mut().enumerate() {
        let incoming: Vec<_> = allowed_locations
            .iter()
            .filter(|location| !peer.snapshot.locations.contains(location))
            .filter(|location| {
                let within = |path: &str| {
                    path == location.path || path.starts_with(&format!("{}/", location.path))
                };
                peer.snapshot.available_locations.contains(&location.path)
                    || acknowledgements[index]
                        .files
                        .iter()
                        .any(|(path, value)| value.is_some() && within(path))
                    || acquisition_aliases.keys().any(|path| within(path))
                    || (options.check
                        && report.changes.iter().any(|change| {
                            change.host == peer.alias
                                && matches!(
                                    change.action.as_str(),
                                    "copy_skill_entry" | "clone_exact_commit"
                                )
                                && within(&change.path)
                        }))
            })
            .cloned()
            .collect();
        if incoming.is_empty() {
            continue;
        }
        if options.check {
            for location in &incoming {
                report.changed(&peer.alias, &location.path, "register_location", true);
            }
        } else {
            match peer.request_ok(Request::Register {
                locations: incoming.clone(),
                expected: peer.snapshot.library_hash.clone(),
            }) {
                Ok(()) => {
                    for location in &incoming {
                        report.changed(&peer.alias, &location.path, "register_location", false);
                    }
                }
                Err(error) => {
                    for location in &incoming {
                        blocked.extend(
                            sources
                                .iter()
                                .filter(|source| source.location == location.path)
                                .map(|source| source.root.clone()),
                        );
                        report.problem(
                            &peer.alias,
                            &location.path,
                            "registration_failed",
                            error.to_string(),
                        );
                    }
                }
            }
        }
    }
    if !options.check {
        for index in 0..peers.len() {
            let mut accepted = BTreeMap::new();
            for other in 0..peers.len() {
                if index == other {
                    continue;
                }
                let mut common = Baseline::default();
                for (path, value) in &acknowledgements[index].files {
                    if acknowledgements[other].files.get(path) == Some(value) {
                        common.files.insert(path.clone(), value.clone());
                        if let Some(context) =
                            super::snapshot::file_context(&peers[index].snapshot, path)
                        {
                            common.contexts.insert(path.clone(), context);
                        }
                    }
                }
                accepted.insert(peers[other].id.clone().unwrap(), common);
            }
            let aliases = if index == 0 {
                peers
                    .iter()
                    .skip(1)
                    .map(|peer| (peer.alias.clone(), peer.id.clone().unwrap()))
                    .collect()
            } else {
                BTreeMap::new()
            };
            let provenance = sources
                .iter()
                .filter(|source| !blocked.contains(&source.root))
                .filter_map(|source| {
                    source.git.as_ref().map(|git| {
                        (
                            source.root.clone(),
                            state::GitProvenance {
                                source: source.key.clone(),
                                origin: git.origin.clone(),
                                commit: git.commit.clone(),
                                branch: source.branch.clone(),
                            },
                        )
                    })
                })
                .collect();
            if let Err(error) = peers[index].request_ok(Request::Acknowledge {
                peers: accepted,
                aliases,
                provenance,
            }) {
                report.problem(
                    &peers[index].alias,
                    "",
                    "acknowledgement_failed",
                    error.to_string(),
                );
            }
        }
    }
    report
        .changes
        .sort_by(|a, b| (&a.host, &a.path, &a.action).cmp(&(&b.host, &b.path, &b.action)));
    report
        .diagnostics
        .sort_by(|a, b| (&a.data, &a.code, &a.message).cmp(&(&b.data, &b.code, &b.message)));
    Ok(report)
}

fn file_base(peers: &[Participant], index: usize, path: &str) -> Option<Option<Entry>> {
    if index == 0 {
        let bases: Option<BTreeSet<_>> = (1..peers.len())
            .map(|i| file_base(peers, i, path))
            .collect();
        let bases = bases?;
        (bases.len() == 1).then(|| bases.into_iter().next().unwrap())
    } else {
        let local_id = peers[0].snapshot.history.id.as_ref()?;
        let remote_id = peers[index].snapshot.history.id.as_ref()?;
        let remote_base = peers[index].snapshot.history.peers.get(local_id)?;
        let local_base = peers[0].snapshot.history.peers.get(remote_id)?;
        let context = super::snapshot::file_context(&peers[index].snapshot, path)?;
        if remote_base.contexts.get(path) != Some(&context)
            || local_base.contexts.get(path) != Some(&context)
            || super::snapshot::file_context(&peers[0].snapshot, path).as_ref() != Some(&context)
        {
            return None;
        }
        let remote = remote_base.files.get(path)?;
        let local = local_base.files.get(path)?;
        (remote == local).then(|| remote.clone())
    }
}

fn file_bases_disagree(peers: &[Participant], path: &str) -> bool {
    (1..peers.len())
        .filter_map(|index| file_base(peers, index, path))
        .collect::<BTreeSet<_>>()
        .len()
        > 1
}

fn inspect_git_checkpoint(
    peers: &mut [Participant],
    sources: &[Source],
    blocked: &mut BTreeSet<String>,
    report: &mut Report,
    before_bootstrap: bool,
) -> bool {
    for (index, peer) in peers.iter_mut().enumerate() {
        if let Err(error) = peer.inspect(sources) {
            report.problem(&peer.alias, "", "inspection_failed", error.to_string());
            return false;
        }
        for reference in sources {
            let actual = peer
                .snapshot
                .sources
                .iter()
                .find(|source| source.root == reference.root);
            let occupied_git = actual.is_some_and(|source| source.git.is_some());
            if (!before_bootstrap || index == 0 || occupied_git)
                && (reference.git.is_some() || occupied_git)
                && actual
                    .is_none_or(|source| source.key != reference.key || source.git != reference.git)
            {
                blocked.insert(reference.root.clone());
                report.problem(
                    &peer.alias,
                    &reference.root,
                    "git_mismatch",
                    "Git source changed since preflight; align it separately before synchronizing",
                );
            }
        }
    }
    true
}

fn sync_files(
    sources: &[Source],
    blocked: &BTreeSet<String>,
    acquisition_aliases: &BTreeMap<String, String>,
    peers: &mut [Participant],
    options: &Options,
    report: &mut Report,
    acknowledgements: &mut [Baseline],
) -> Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    for peer in peers.iter() {
        for source in &peer.snapshot.sources {
            paths.extend(source.files.keys().cloned());
            paths.extend(source.committed.keys().cloned());
        }
        for base in peer.snapshot.history.peers.values() {
            paths.extend(base.files.keys().cloned());
        }
    }
    paths.retain(|path| {
        !acquisition_aliases
            .keys()
            .any(|alias| path == alias || path.starts_with(&format!("{alias}/")))
    });
    let owner = |path: &str| {
        sources
            .iter()
            .filter(|s| path == s.root || path.starts_with(&format!("{}/", s.root)))
            .max_by_key(|s| s.root.len())
    };
    let mut exclusions = BTreeMap::new();
    for source in sources {
        let mut builder =
            ignore::gitignore::GitignoreBuilder::new(peers[0].snapshot.home.join(&source.location));
        for pattern in &source.exclusions {
            builder
                .add_line(None, pattern)
                .map_err(Error::input_display)?;
        }
        exclusions.insert(
            source.root.clone(),
            builder.build().map_err(Error::input_display)?,
        );
    }
    let mut filter_error = None;
    paths.retain(|path| {
        if filter_error.is_some() {
            return true;
        }
        match state::transferable(&peers[0].snapshot.home, path) {
            Ok(transferable) => {
                transferable
                    && owner(path).is_some_and(|s| {
                        !exclusions[&s.root]
                            .matched_path_or_any_parents(peers[0].snapshot.home.join(path), false)
                            .is_ignore()
                    })
            }
            Err(error) => {
                filter_error = Some(Error::input(format!(
                    "cannot inspect synchronization path {path}: {error}"
                )));
                true
            }
        }
    });
    if let Some(error) = filter_error {
        return Err(error);
    }
    let mut type_choices = BTreeMap::new();
    let mut type_conflicts = BTreeSet::new();
    for root in &paths {
        if type_choices.contains_key(root) || type_conflicts.contains(root) {
            continue;
        }
        let current = |peer: &Participant, path: &str| {
            peer.snapshot
                .sources
                .iter()
                .find_map(|source| source.files.get(path))
                .cloned()
        };
        let values: Vec<_> = peers
            .iter()
            .filter_map(|peer| current(peer, root))
            .collect();
        if !values.iter().any(|value| matches!(value, Entry::Directory))
            || !values
                .iter()
                .any(|value| !matches!(value, Entry::Directory))
        {
            continue;
        }
        let members: Vec<_> = paths
            .iter()
            .filter(|path| *path == root || path.starts_with(&format!("{root}/")))
            .cloned()
            .collect();
        let observations: Vec<_> = peers
            .iter()
            .enumerate()
            .map(|(index, peer)| {
                let value = Some(
                    members
                        .iter()
                        .filter_map(|path| current(peer, path).map(|value| (path.clone(), value)))
                        .collect::<BTreeMap<_, _>>(),
                );
                let known = file_base(peers, index, root).is_some();
                let base = known.then(|| {
                    Some(
                        members
                            .iter()
                            .filter_map(|path| {
                                file_base(peers, index, path)
                                    .flatten()
                                    .map(|value| (path.clone(), value))
                            })
                            .collect(),
                    )
                });
                Observation {
                    value,
                    base,
                    prior_presence: false,
                }
            })
            .collect();
        let decision = planner::resolve(&observations, options.conflict, 1);
        let labels: Vec<_> = peers.iter().map(|peer| peer.alias.clone()).collect();
        match choose(decision, &observations, &labels, root, options, report)? {
            Decision::Use(value) => {
                for path in members {
                    type_choices.insert(
                        path.clone(),
                        value.as_ref().and_then(|tree| tree.get(&path)).cloned(),
                    );
                }
            }
            _ => {
                type_conflicts.extend(members);
            }
        }
    }
    let mut parents: BTreeMap<_, _> = paths
        .iter()
        .map(|path| (path.clone(), path.clone()))
        .collect();
    // A library acquisition link and its original refer to the same content. Connect
    // their logical paths across the cohort before choosing any file outcomes.
    for peer in peers.iter() {
        let mut physical = BTreeMap::<&str, &str>::new();
        for (path, target) in &peer.snapshot.physical_paths {
            if !paths.contains(path) {
                continue;
            }
            // Internal links are independent entries, even when they resolve to a
            // file elsewhere in the same skill. Only their target content may alias.
            if peer
                .snapshot
                .sources
                .iter()
                .any(|source| matches!(source.files.get(path), Some(Entry::Link { .. })))
            {
                continue;
            }
            if let Some(previous) = physical.insert(target, path) {
                let left = group_root(&parents, previous);
                let right = group_root(&parents, path);
                if left != right {
                    let (first, second) = if left < right {
                        (left, right)
                    } else {
                        (right, left)
                    };
                    parents.insert(second, first);
                }
            }
        }
    }
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for path in paths {
        groups
            .entry(group_root(&parents, &path))
            .or_default()
            .push(path);
    }
    let mut planned = Vec::new();
    let mut failed = BTreeSet::new();
    for group in groups.into_values() {
        let roots: BTreeSet<_> = group
            .iter()
            .filter_map(|path| owner(path).map(|s| s.root.clone()))
            .collect();
        if roots.iter().any(|root| blocked.contains(root))
            || group.iter().any(|path| type_conflicts.contains(path))
        {
            failed.extend(roots);
            continue;
        }
        if let Some(path) = group.iter().find(|path| file_bases_disagree(peers, path)) {
            report.problem("local", path, "conflict", "acknowledged file baselines disagree across hosts; resolve the history before synchronizing this entry");
            failed.extend(roots);
            continue;
        }
        let mut observations = Vec::new();
        let mut labels = Vec::new();
        let mut addresses = Vec::new();
        for (index, peer) in peers.iter().enumerate() {
            for path in &group {
                let observed = peer
                    .snapshot
                    .sources
                    .iter()
                    .filter(|s| path == &s.root || path.starts_with(&format!("{}/", s.root)))
                    .max_by_key(|s| s.root.len());
                let value = observed.and_then(|s| s.files.get(path)).cloned();
                let base = file_base(peers, index, path).or_else(|| {
                    observed
                        .and_then(|s| s.committed.get(path))
                        .cloned()
                        .map(Some)
                });
                observations.push(Observation {
                    value,
                    base,
                    // Only matched peer history proves prior presence. The Git tree
                    // above is a comparison base, not synchronization history.
                    prior_presence: matches!(file_base(peers, index, path), Some(Some(_))),
                });
                labels.push(format!("{}:{}", peer.alias, path));
                addresses.push((index, path.clone()));
            }
        }
        // Each group names one physical entry across aliases. Distinct values
        // here mean two subtree choices disagree about that same entry; root
        // and child entries of one chosen tree belong to different groups.
        let overrides: BTreeSet<_> = group
            .iter()
            .filter_map(|path| type_choices.get(path).cloned())
            .collect();
        let decision = if overrides.len() == 1 {
            Decision::Use(overrides.into_iter().next().unwrap())
        } else if overrides.len() > 1 {
            Decision::Conflict
        } else {
            planner::decide_group(
                &observations,
                options.conflict,
                options.missing,
                group.len(),
            )
        };
        let decision = choose(decision, &observations, &labels, &group[0], options, report)?;
        match decision {
            Decision::Use(value) => {
                planned.push((group[0].clone(), addresses, observations, value, roots))
            }
            Decision::Ignore => {
                for ((index, path), observation) in addresses.iter().zip(&observations) {
                    if observation.value.is_some() {
                        continue;
                    }
                    report.diagnostics.push(ReportDiagnostic {
                        code: "missing_ignored".into(),
                        severity: "info".into(),
                        message: "one-sided absence intentionally ignored".into(),
                        data: Some(BTreeMap::from([
                            ("host".into(), peers[*index].alias.clone()),
                            ("path".into(), path.clone()),
                        ])),
                    });
                }
            }
            Decision::Unmatched => {
                for ((index, path), observation) in addresses.iter().zip(&observations) {
                    if observation.value.is_none() {
                        continue;
                    }
                    report.problem(
                        &peers[*index].alias,
                        path,
                        "missing_history",
                        "remove needs verified prior presence; unmatched entry preserved",
                    );
                }
                failed.extend(roots);
            }
            Decision::Conflict => {
                failed.extend(roots);
            }
        }
    }
    planned.sort_by(|a, b| {
        (
            a.3.is_some(),
            if a.3.is_none() {
                usize::MAX - a.0.len()
            } else {
                a.0.len()
            },
            &a.0,
        )
            .cmp(&(
                b.3.is_some(),
                if b.3.is_none() {
                    usize::MAX - b.0.len()
                } else {
                    b.0.len()
                },
                &b.0,
            ))
    });
    // Exports are individually validated by their owners. Only successful exports
    // enter the flat transfer manifests; a batch failure invalidates every member.
    let mut payloads = vec![None::<(usize, String)>; planned.len()];
    let mut unavailable = BTreeSet::new();
    let mut failed_pushes = BTreeSet::<(usize, usize)>::new();
    let mut pulls = BTreeMap::<usize, Vec<usize>>::new();
    if !options.check {
        for (group, (_, addresses, observations, desired, roots)) in planned.iter().enumerate() {
            let Some(entry) = desired
                .as_ref()
                .filter(|entry| !matches!(entry, Entry::Directory))
            else {
                continue;
            };
            if observations
                .iter()
                .all(|observation| observation.value == *desired)
            {
                continue;
            }
            let source = observations
                .iter()
                .position(|observation| observation.value.as_ref() == Some(entry))
                .ok_or_else(|| Error::input("planned value has no content source"))?;
            let (index, path) = &addresses[source];
            match peers[*index].endpoint.request(Request::Export {
                path: path.clone(),
                expected: entry.clone(),
            }) {
                Ok(Response::Exported { path: export }) => {
                    let export = Path::new(&export);
                    let name = export.file_name().and_then(|name| name.to_str());
                    if export.parent() != Some(peers[*index].stage.as_ref().unwrap().path())
                        || name.is_none_or(|name| !state::valid_id(name))
                    {
                        report.problem(
                            &peers[*index].alias,
                            path,
                            "protocol_error",
                            "invalid export stage path",
                        );
                        failed.extend(roots.iter().cloned());
                        unavailable.insert(group);
                        continue;
                    }
                    payloads[group] = Some((*index, name.unwrap().to_owned()));
                    if *index != 0 {
                        pulls.entry(*index).or_default().push(group);
                    }
                }
                Ok(_) => {
                    report.problem(
                        &peers[*index].alias,
                        path,
                        "protocol_error",
                        "unexpected export response",
                    );
                    failed.extend(roots.iter().cloned());
                    unavailable.insert(group);
                }
                Err(error) => {
                    report.problem(
                        &peers[*index].alias,
                        path,
                        "export_failed",
                        error.to_string(),
                    );
                    failed.extend(roots.iter().cloned());
                    unavailable.insert(group);
                }
            }
        }
        for (source, groups) in pulls {
            for chunk in groups.chunks(64) {
                let names: Vec<_> = chunk
                    .iter()
                    .map(|group| payloads[*group].as_ref().unwrap().1.clone())
                    .collect();
                let (initiator, others) = peers.split_at_mut(source);
                let local = &mut initiator[0];
                let remote = &mut others[0];
                let result = remote.endpoint.pull_batch(
                    &names,
                    remote.stage.as_ref().unwrap(),
                    local.stage.as_ref().unwrap(),
                );
                if let Err(error) = result {
                    for group in chunk {
                        let (_, addresses, _, _, roots) = &planned[*group];
                        let path = &addresses[0].1;
                        report.problem(&remote.alias, path, "transfer_failed", error.to_string());
                        failed.extend(roots.iter().cloned());
                        unavailable.insert(*group);
                    }
                }
            }
        }
    }
    let mut pushes = BTreeMap::<usize, Vec<usize>>::new();
    if !options.check {
        for (group, (_, addresses, observations, desired, _)) in planned.iter().enumerate() {
            if unavailable.contains(&group) || payloads[group].is_none() {
                continue;
            }
            let source = payloads[group].as_ref().unwrap().0;
            let mut destinations = BTreeSet::new();
            for ((index, _path), observation) in addresses.iter().zip(observations) {
                if observation.value == *desired || *index == source || *index == 0 {
                    continue;
                }
                if destinations.insert(*index) {
                    pushes.entry(*index).or_default().push(group);
                }
            }
        }
        for (destination, groups) in pushes {
            for chunk in groups.chunks(64) {
                let names: Vec<_> = chunk
                    .iter()
                    .map(|group| payloads[*group].as_ref().unwrap().1.clone())
                    .collect();
                let (initiator, others) = peers.split_at_mut(destination);
                let local = &mut initiator[0];
                let remote = &mut others[0];
                let result = remote.endpoint.push_batch(
                    &names,
                    local.stage.as_ref().unwrap(),
                    remote.stage.as_ref().unwrap(),
                );
                if let Err(error) = result {
                    for group in chunk {
                        let (_, addresses, _, _, roots) = &planned[*group];
                        failed_pushes.insert((*group, destination));
                        report.problem(
                            &remote.alias,
                            &addresses[0].1,
                            "transfer_failed",
                            error.to_string(),
                        );
                        failed.extend(roots.iter().cloned());
                    }
                }
            }
        }
    }
    for (group, (_, addresses, observations, desired, roots)) in planned.into_iter().enumerate() {
        let changed: Vec<_> = observations
            .iter()
            .enumerate()
            .filter(|(_, observation)| observation.value != desired)
            .map(|(address, _)| address)
            .collect();
        if changed.is_empty() {
            for (index, path) in &addresses {
                acknowledgements[*index]
                    .files
                    .insert(path.clone(), desired.clone());
            }
            continue;
        }
        if options.check {
            let mut destinations = BTreeSet::new();
            for address in changed {
                let (index, path) = &addresses[address];
                let physical = peers[*index]
                    .snapshot
                    .physical_paths
                    .get(path)
                    .unwrap_or(path);
                if destinations.insert((*index, physical.clone())) {
                    report.changed(
                        &peers[*index].alias,
                        path,
                        if desired.is_some() {
                            "copy_skill_entry"
                        } else {
                            "remove_skill_entry"
                        },
                        true,
                    );
                }
            }
            continue;
        }
        if unavailable.contains(&group) {
            failed.extend(roots);
            continue;
        }
        let mut published = BTreeMap::new();
        for ((index, path), observation) in addresses.iter().zip(observations) {
            if observation.value == desired {
                acknowledgements[*index]
                    .files
                    .insert(path.clone(), desired.clone());
                continue;
            }
            let physical = peers[*index]
                .snapshot
                .physical_paths
                .get(path)
                .unwrap_or(path)
                .clone();
            if let Some(success) = published.get(&(*index, physical.clone())) {
                if *success {
                    acknowledgements[*index]
                        .files
                        .insert(path.clone(), desired.clone());
                }
                continue;
            }
            published.insert((*index, physical.clone()), false);
            let stage = if let Some((_, name)) = &payloads[group] {
                if failed_pushes.contains(&(group, *index)) {
                    failed.extend(roots.iter().cloned());
                    continue;
                }
                // A source's validated export and a completed batch retain the
                // same generated filename on each receiving participant.
                let target_path = peers[*index].stage.as_ref().unwrap().path().to_path_buf();
                let local_check = peers[0].stage.as_ref().unwrap().open().map(|_| ());
                let target_check = if *index == 0 {
                    local_check
                } else {
                    local_check.and_then(|()| peers[*index].request_ok(Request::ValidateStage))
                };
                if let Err(error) = target_check {
                    report.problem(
                        &peers[*index].alias,
                        path,
                        "transfer_failed",
                        error.to_string(),
                    );
                    failed.extend(roots.iter().cloned());
                    continue;
                }
                Some(target_path.join(name).to_string_lossy().into_owned())
            } else {
                None
            };
            match peers[*index].request_ok(Request::Publish {
                path: path.clone(),
                expected: observation.value,
                desired: desired.clone(),
                stage,
            }) {
                Ok(()) => {
                    published.insert((*index, physical), true);
                    report.changed(
                        &peers[*index].alias,
                        path,
                        if desired.is_some() {
                            "copy_skill_entry"
                        } else {
                            "remove_skill_entry"
                        },
                        false,
                    );
                    acknowledgements[*index]
                        .files
                        .insert(path.clone(), desired.clone());
                }
                Err(error) => {
                    report.problem(
                        &peers[*index].alias,
                        path,
                        "publication_failed",
                        error.to_string(),
                    );
                    failed.extend(roots.iter().cloned());
                }
            }
        }
    }
    Ok(failed)
}

fn group_root(parents: &BTreeMap<String, String>, path: &str) -> String {
    let mut root = path;
    while let Some(parent) = parents.get(root) {
        if parent == root {
            break;
        }
        root = parent;
    }
    root.to_owned()
}

fn choose<T: Clone + Ord + std::fmt::Debug>(
    decision: Decision<T>,
    observations: &[Observation<T>],
    labels: &[String],
    path: &str,
    options: &Options,
    report: &mut Report,
) -> Result<Decision<T>> {
    if decision != Decision::Conflict {
        return Ok(decision);
    }
    if options.interactive && !options.check && matches!(options.conflict, ConflictPolicy::Ask) {
        let mut stderr = std::io::stderr().lock();
        writeln!(stderr, "Conflict: {path}").map_err(Error::input_display)?;
        for (index, observed) in observations.iter().enumerate() {
            writeln!(
                stderr,
                "{}. {}: {:?}",
                index + 1,
                labels[index],
                observed.value
            )
            .map_err(Error::input_display)?;
        }
        write!(
            stderr,
            "Choose a version [1-{}], or Enter to skip: ",
            observations.len()
        )
        .map_err(Error::input_display)?;
        stderr.flush().map_err(Error::input_display)?;
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(Error::input_display)?;
        if let Ok(index) = answer.trim().parse::<usize>()
            && index > 0
            && index <= observations.len()
        {
            return Ok(Decision::Use(observations[index - 1].value.clone()));
        }
    }
    let candidates = labels
        .iter()
        .zip(observations)
        .map(|(label, o)| format!("{label}={:?}", o.value))
        .collect::<Vec<_>>()
        .join(", ");
    report.problem(
        "local",
        path,
        "conflict",
        format!("unresolved conflict: {candidates}"),
    );
    Ok(Decision::Conflict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::test_support::{configure_library as configure, write_skill as skill};
    use std::fs;

    fn participants(homes: &[tempfile::TempDir]) -> Vec<Participant> {
        homes
            .iter()
            .enumerate()
            .map(|(index, home)| {
                Participant::observe(
                    if index == 0 {
                        "local".into()
                    } else {
                        format!("host{index}")
                    },
                    Endpoint::local(AppPaths::new(home.path().to_path_buf())),
                    Vec::new(),
                )
                .unwrap()
            })
            .collect()
    }
    fn options(check: bool) -> Options {
        Options {
            hosts: None,
            conflict: ConflictPolicy::Ask,
            missing: MissingPolicy::Copy,
            check,
            interactive: false,
        }
    }

    #[test]
    fn redirected_historical_path_is_not_silently_omitted() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        let old = ".skillator/library/demo/old";
        fs::create_dir(homes[0].path().join(old)).unwrap();
        fs::write(homes[0].path().join(old).join("payload"), "original").unwrap();
        let first = sync(&homes, options(false));
        assert_eq!(first.exit_status, 0, "{}", first.text());
        for home in &homes {
            fs::remove_dir_all(home.path().join(old)).unwrap();
        }
        let mut peers = participants(&homes);
        let sources = peers[0].snapshot.sources.clone();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("payload"), "outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), homes[0].path().join(old)).unwrap();
        let paths = AppPaths::new(homes[0].path().into());
        let mut report = Report::new(&paths);
        let mut acknowledgements = vec![Baseline::default(); peers.len()];
        let error = sync_files(
            &sources,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut peers,
            &options(true),
            &mut report,
            &mut acknowledgements,
        )
        .unwrap_err();
        assert!(error.message.contains(old), "{error}");
        assert!(report.changes.is_empty(), "{}", report.text());
        assert_eq!(
            fs::read_to_string(outside.path().join("payload")).unwrap(),
            "outside"
        );
        assert!(acknowledgements.iter().all(|base| base.files.is_empty()));
    }

    #[test]
    fn unmatched_remote_content_is_attributed_to_its_host() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        for home in &homes {
            configure(home.path(), ".skillator/library");
        }
        skill(homes[1].path(), ".skillator/library/demo", "remote only");
        let mut policy = options(false);
        policy.missing = MissingPolicy::Remove;
        let report = sync(&homes, policy);
        let diagnostics: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|d| d.code == "missing_history")
            .collect();
        assert!(!diagnostics.is_empty());
        assert!(
            diagnostics
                .iter()
                .all(|d| d.data.as_ref().unwrap()["host"] == "host1")
        );
        assert!(
            homes[1]
                .path()
                .join(".skillator/library/demo/SKILL.md")
                .exists()
        );
    }

    #[test]
    fn refreshed_identity_cannot_replace_the_observed_participant() {
        for initialized in [false, true] {
            let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
            configure(homes[0].path(), ".skillator/library");
            skill(homes[0].path(), ".skillator/library/demo", "original");
            if initialized {
                sync(&homes, options(false));
            }
            let peers = participants(&homes);
            let home = homes[1].path();
            let path = home.join(".skillator/rsync/state.json");
            let mut history = state::History::load(home).unwrap();
            let expected = state::fingerprint(&path).unwrap();
            history.id = Some(state::new_id().unwrap());
            history.save(home, &expected).unwrap();
            let replacement = fs::read(&path).unwrap();
            let error = synchronize(
                &AppPaths::new(homes[0].path().into()),
                peers,
                options(false),
            )
            .unwrap_err();
            assert!(
                error.message.contains("participant identity changed"),
                "{error}"
            );
            assert_eq!(fs::read(path).unwrap(), replacement);
            if !initialized {
                assert!(!homes[0].path().join(".skillator/rsync").exists());
            }
        }
    }

    #[test]
    fn late_busy_participant_returns_busy_without_starting_sessions() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        let peers = participants(&homes);
        let target = crate::target::Target::user(homes[1].path()).unwrap();
        let _lock = crate::reconcile::TargetLocks::acquire(&[&target]).unwrap();
        let error = synchronize(
            &AppPaths::new(homes[0].path().into()),
            peers,
            options(false),
        )
        .unwrap_err();
        assert_eq!(error.code, 4);
        for home in &homes {
            assert!(!home.path().join(".skillator/rsync").exists());
        }
        let target = crate::target::Target::user(homes[0].path()).unwrap();
        assert!(crate::reconcile::TargetLocks::acquire(&[&target]).is_ok());
    }

    #[test]
    fn early_session_failures_send_finish_to_every_participant() {
        use super::super::transport::Fault;
        for fault in [Fault::Begin, Fault::InspectActive] {
            let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
            configure(homes[0].path(), ".skillator/library");
            skill(homes[0].path(), ".skillator/library/demo", "original");
            let mut peers = participants(&homes);
            let inner = std::mem::replace(
                &mut peers[1].endpoint,
                Endpoint::local(AppPaths::new(homes[1].path().into())),
            );
            peers[1].endpoint = Endpoint::Fault {
                inner: Box::new(inner),
                fault,
            };
            let report = synchronize(
                &AppPaths::new(homes[0].path().into()),
                peers,
                options(false),
            )
            .unwrap();
            assert_eq!(report.exit_status, 1);
            assert!(homes[1].path().join("finish-observed").exists());
            for home in &homes {
                let root = home.path().join(".skillator/rsync");
                if root.exists() {
                    assert!(fs::read_dir(root).unwrap().all(|entry| {
                        !entry
                            .unwrap()
                            .file_name()
                            .to_string_lossy()
                            .starts_with("stage-")
                    }));
                }
            }
        }
    }

    #[test]
    fn invalid_remote_stage_is_rejected_before_any_transfer() {
        use super::super::transport::Fault;
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        let mut peers = participants(&homes);
        let inner = std::mem::replace(
            &mut peers[1].endpoint,
            Endpoint::local(AppPaths::new(homes[1].path().into())),
        );
        peers[1].endpoint = Endpoint::Fault {
            inner: Box::new(inner),
            fault: Fault::StageOutside,
        };
        let report = synchronize(
            &AppPaths::new(homes[0].path().into()),
            peers,
            options(false),
        )
        .unwrap();
        assert_eq!(report.exit_status, 1, "{}", report.text());
        assert!(report.diagnostics.iter().any(|d| d.code == "begin_failed"));
        assert!(!homes[1].path().join(".skillator/library/demo").exists());
    }

    #[test]
    fn changed_git_revision_after_begin_blocks_publication() {
        use super::super::process;
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        let origin_home = tempfile::tempdir().unwrap();
        let origin = origin_home.path().join("acme/skills");
        skill(origin_home.path(), "acme/skills/demo", "commit A");
        process::git(&origin, &["init", "-q"]).unwrap();
        process::git(&origin, &["add", "."]).unwrap();
        process::git(
            &origin,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "A",
            ],
        )
        .unwrap();
        let root = homes[0].path().join("Development/acme/skills");
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        process::capture(
            std::process::Command::new("git")
                .args(["clone", "--"])
                .arg(&origin)
                .arg(&root),
        )
        .unwrap();
        let reference = super::super::snapshot::git_ref(&root).unwrap();
        process::git(&root, &["checkout", "-b", "next"]).unwrap();
        skill(homes[0].path(), "Development/acme/skills/demo", "commit B");
        process::git(&root, &["add", "."]).unwrap();
        process::git(
            &root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "B",
            ],
        )
        .unwrap();
        process::git(&root, &["checkout", "--detach", &reference.commit]).unwrap();
        configure(homes[0].path(), "Development/acme/skills");
        let mut peers = participants(&homes);
        let inner = std::mem::replace(
            &mut peers[0].endpoint,
            Endpoint::local(AppPaths::new(homes[0].path().into())),
        );
        peers[0].endpoint = Endpoint::Fault {
            inner: Box::new(inner),
            fault: super::super::transport::Fault::AdvanceGitOnBegin,
        };
        let report = synchronize(
            &AppPaths::new(homes[0].path().into()),
            peers,
            options(false),
        )
        .unwrap();
        assert_eq!(report.exit_status, 1, "{}", report.text());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "git_mismatch")
        );
        let remote = homes[1].path().join("Development/acme/skills");
        assert!(!remote.exists(), "stale Git revision must not be cloned");
        assert!(
            fs::read_to_string(root.join("demo/SKILL.md"))
                .unwrap()
                .contains("commit B")
        );
    }

    #[test]
    fn lock_reservation_failure_releases_earlier_participants_without_writes() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        let mut peers = participants(&homes);
        let endpoint = std::mem::replace(
            &mut peers[1].endpoint,
            Endpoint::local(AppPaths::new(homes[1].path().into())),
        );
        peers[1].endpoint = Endpoint::Fault {
            inner: Box::new(endpoint),
            fault: super::super::transport::Fault::LockBusy,
        };
        let error = synchronize(
            &AppPaths::new(homes[0].path().into()),
            peers,
            options(false),
        )
        .unwrap_err();
        assert_eq!(error.code, 4);
        for home in &homes {
            assert!(!home.path().join(".skillator/rsync").exists());
            let target = crate::target::Target::user(home.path()).unwrap();
            assert!(crate::reconcile::TargetLocks::acquire(&[&target]).is_ok());
        }
    }

    #[test]
    fn recovered_exchange_is_reobserved_before_planning_and_acknowledgement() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        assert_eq!(sync(&homes, options(false)).exit_status, 0);
        let path = ".skillator/library/demo/SKILL.md";
        let destination = state::contained(homes[1].path(), path).unwrap();
        let expected = state::observe(&destination).unwrap();
        let backup = destination
            .parent()
            .unwrap()
            .join(format!(".skillator-rsync-{}", state::new_id().unwrap()));
        fs::copy(&destination, &backup).unwrap();
        skill(
            homes[1].path(),
            ".skillator/library/demo",
            "interrupted replacement",
        );
        let desired = state::observe(&destination).unwrap();
        let stage = homes[1].path().join(".skillator/rsync/stage-interrupted");
        fs::create_dir(&stage).unwrap();
        fs::write(
            stage.join("recovery-test.json"),
            serde_json::to_vec(&serde_json::json!({
                "path": path, "expected": expected, "desired": desired, "sibling": backup,
            }))
            .unwrap(),
        )
        .unwrap();
        let recovered = sync(&homes, options(false));
        assert_eq!(recovered.exit_status, 0, "{}", recovered.text());
        assert!(
            recovered
                .changes
                .iter()
                .any(|change| change.action == "recover_publication")
        );
        assert_eq!(
            fs::read(&destination).unwrap(),
            fs::read(homes[0].path().join(path)).unwrap()
        );
        let again = sync(&homes, options(false));
        assert_eq!(again.exit_status, 0, "{}", again.text());
        assert!(again.changes.is_empty(), "{}", again.text());
    }

    #[test]
    fn malformed_host_local_user_configuration_does_not_block_content_sync() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "first");
        let user_bytes: [&[u8]; 2] = [b"invalid:\n  - [\n", b"other:\n  - {\n"];
        for (home, bytes) in homes.iter().zip(user_bytes) {
            fs::create_dir_all(home.path().join(".agents")).unwrap();
            fs::write(home.path().join(".agents/skillator.yaml"), bytes).unwrap();
        }
        let report = sync(&homes, options(false));
        assert_eq!(report.exit_status, 0, "{}", report.text());
        assert!(
            homes[1]
                .path()
                .join(".skillator/library/demo/SKILL.md")
                .exists()
        );
        for (home, bytes) in homes.iter().zip(user_bytes) {
            assert_eq!(
                fs::read(home.path().join(".agents/skillator.yaml")).unwrap(),
                bytes
            );
            assert!(!home.path().join(".agents/skills/demo").exists());
        }
    }

    #[test]
    fn multiple_files_flow_in_both_directions_without_unrelated_content() {
        let homes: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        assert_eq!(sync(&homes, options(false)).exit_status, 0);
        let root = ".skillator/library/demo";
        for (peer, names) in [(1, ["alpha", "beta"]), (2, ["gamma", "delta"])] {
            for name in names {
                fs::write(
                    homes[peer].path().join(root).join(name),
                    format!("{peer}-{name}"),
                )
                .unwrap();
            }
        }
        fs::write(
            homes[1].path().join(".skillator/library/unrelated.txt"),
            "private",
        )
        .unwrap();
        let result = sync(&homes, options(false));
        assert_eq!(result.exit_status, 0, "{}", result.text());
        for home in &homes {
            for (peer, names) in [(1, ["alpha", "beta"]), (2, ["gamma", "delta"])] {
                for name in names {
                    assert_eq!(
                        fs::read_to_string(home.path().join(root).join(name)).unwrap(),
                        format!("{peer}-{name}")
                    );
                }
            }
        }
        assert!(
            !homes[0]
                .path()
                .join(".skillator/library/unrelated.txt")
                .exists()
        );
        assert!(
            !homes[2]
                .path()
                .join(".skillator/library/unrelated.txt")
                .exists()
        );
    }
    fn sync(homes: &[tempfile::TempDir], options: Options) -> Report {
        synchronize(
            &AppPaths::new(homes[0].path().into()),
            participants(homes),
            options,
        )
        .unwrap()
    }

    #[test]
    fn three_homes_sync_edit_delete_and_retry_without_host_order_winners() {
        let homes: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        let first = sync(&homes, options(false));
        assert_eq!(first.exit_status, 0, "{}", first.text());
        for home in &homes {
            assert!(
                home.path()
                    .join(".skillator/library/demo/SKILL.md")
                    .exists()
            );
        }
        let again = sync(&homes, options(false));
        assert_eq!(again.exit_status, 0, "{}", again.text());
        assert!(again.changes.is_empty(), "{}", again.text());
        skill(homes[1].path(), ".skillator/library/demo", "remote edit");
        let edited = sync(&homes, options(false));
        assert_eq!(edited.exit_status, 0, "{}", edited.text());
        for home in &homes {
            assert!(
                fs::read_to_string(home.path().join(".skillator/library/demo/SKILL.md"))
                    .unwrap()
                    .contains("remote edit")
            );
        }
        let file = ".skillator/library/demo/SKILL.md";
        fs::remove_file(homes[2].path().join(file)).unwrap();
        let copied = sync(&homes, options(false));
        assert_eq!(copied.exit_status, 0, "{}", copied.text());
        assert!(homes[2].path().join(file).exists());
        fs::remove_file(homes[2].path().join(file)).unwrap();
        let mut remove = options(false);
        remove.missing = MissingPolicy::Remove;
        let removed = sync(&homes, remove);
        assert_eq!(removed.exit_status, 0, "{}", removed.text());
        for home in &homes {
            assert!(!home.path().join(file).exists());
        }
    }

    #[test]
    fn registering_an_incoming_location_preserves_hidden_skills() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        fs::create_dir_all(homes[1].path().join(".skillator")).unwrap();
        fs::write(
            homes[1].path().join(".skillator/library.yaml"),
            "version: 1\nlocations: []\nhidden_skills:\n  - source: local/library\n    path: demo\n",
        )
        .unwrap();
        let report = sync(&homes, options(false));
        assert_eq!(report.exit_status, 0, "{}", report.text());
        let config = fs::read_to_string(homes[1].path().join(".skillator/library.yaml")).unwrap();
        assert!(config.contains("hidden_skills:"), "{config}");
        assert!(config.contains("local/library"), "{config}");
        assert!(
            homes[1]
                .path()
                .join(".skillator/library/demo/SKILL.md")
                .exists()
        );
    }

    #[test]
    fn divergent_acknowledged_bases_block_file_planning() {
        let homes: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        let first = sync(&homes, options(false));
        assert_eq!(first.exit_status, 0, "{}", first.text());
        let mut peers = participants(&homes);
        let local_id = peers[0].snapshot.history.id.clone().unwrap();
        let remote_id = peers[1].snapshot.history.id.clone().unwrap();
        let path = ".skillator/library/demo/SKILL.md";
        for (index, peer_id) in [(0, remote_id.as_str()), (1, local_id.as_str())] {
            let baseline = peers[index]
                .snapshot
                .history
                .peers
                .get_mut(peer_id)
                .unwrap();
            baseline.files.insert(
                path.into(),
                Some(Entry::File {
                    hash: "0".repeat(64),
                    executable: false,
                }),
            );
            fs::write(
                homes[index].path().join(".skillator/rsync/state.json"),
                serde_json::to_vec(&peers[index].snapshot.history).unwrap(),
            )
            .unwrap();
        }
        let observed = participants(&homes);
        assert!(file_bases_disagree(&observed, path));
        assert_eq!(file_base(&observed, 0, path), None);
        let report = sync(&homes, options(false));
        assert_eq!(report.exit_status, 1, "{}", report.text());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.message.contains("acknowledged file baselines disagree"))
        );
        assert!(report.changes.is_empty(), "{}", report.text());
    }

    #[test]
    fn new_peer_with_different_content_conflicts_with_known_cohort() {
        let homes: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
        for home in &homes {
            configure(home.path(), ".skillator/library");
        }
        skill(homes[0].path(), ".skillator/library/demo", "known content");
        let established = sync(&homes[..2], options(false));
        assert_eq!(established.exit_status, 0, "{}", established.text());
        skill(
            homes[2].path(),
            ".skillator/library/demo",
            "new peer content",
        );
        let peers = participants(&homes);
        let path = ".skillator/library/demo/SKILL.md";
        assert!(file_base(&peers, 1, path).is_some());
        assert!(file_base(&peers, 2, path).is_none());
        assert_eq!(file_base(&peers, 0, path), None);
        let report = sync(&homes, options(false));
        assert_eq!(report.exit_status, 1, "{}", report.text());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "conflict" && d.message.contains(path)),
            "{}",
            report.text()
        );
        for home in &homes[..2] {
            assert!(
                fs::read_to_string(home.path().join(path))
                    .unwrap()
                    .contains("known content")
            );
        }
        assert!(
            fs::read_to_string(homes[2].path().join(path))
                .unwrap()
                .contains("new peer content")
        );
    }

    #[test]
    fn check_does_not_create_remote_configuration_or_history() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        let checked = sync(&homes, options(true));
        assert_eq!(checked.exit_status, 1);
        assert!(fs::read_dir(homes[1].path()).unwrap().next().is_none());
        assert!(!homes[0].path().join(".skillator/rsync").exists());
    }

    #[test]
    fn exact_commit_bootstrap_does_not_take_new_upstream_content() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        let origins = tempfile::tempdir().unwrap();
        let origin = origins.path().join("acme/skills");
        fs::create_dir_all(&origin).unwrap();
        super::super::process::git(&origin, &["init", "-q"]).unwrap();
        skill(origins.path(), "acme/skills/demo", "commit A");
        super::super::process::git(&origin, &["add", "."]).unwrap();
        super::super::process::git(
            &origin,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "A",
            ],
        )
        .unwrap();
        let local = homes[0].path().join("Development/acme/skills");
        fs::create_dir_all(local.parent().unwrap()).unwrap();
        super::super::process::capture(
            std::process::Command::new("git")
                .args(["clone", "--"])
                .arg(&origin)
                .arg(&local),
        )
        .unwrap();
        let reference = super::super::snapshot::git_ref(&local).unwrap();
        configure(homes[0].path(), "Development/acme/skills");
        skill(origins.path(), "acme/skills/demo", "commit B");
        super::super::process::git(&origin, &["add", "."]).unwrap();
        super::super::process::git(
            &origin,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "B",
            ],
        )
        .unwrap();
        skill(
            homes[0].path(),
            "Development/acme/skills/demo",
            "local dirty edit",
        );
        let report = sync(&homes, options(false));
        assert_eq!(report.exit_status, 0, "{}", report.text());
        let remote = homes[1].path().join("Development/acme/skills");
        assert_eq!(super::super::snapshot::git_ref(&remote).unwrap(), reference);
        assert!(
            fs::read_to_string(remote.join("demo/SKILL.md"))
                .unwrap()
                .contains("local dirty edit")
        );
        assert!(
            !super::super::process::git(&remote, &["diff", "--name-only"])
                .unwrap()
                .is_empty()
        );
        assert!(
            super::super::process::git(&remote, &["diff", "--cached", "--name-only"])
                .unwrap()
                .is_empty()
        );
        // A separately advanced checkout blocks this source, without touching either index.
        super::super::process::git(&remote, &["fetch", "origin"]).unwrap();
        super::super::process::git(&remote, &["reset", "--hard", "origin/HEAD"]).unwrap();
        let advanced = super::super::snapshot::git_ref(&remote).unwrap();
        fs::write(local.join("unrelated.txt"), "unrelated dirty content").unwrap();
        let mismatch = sync(&homes, options(false));
        assert_eq!(mismatch.exit_status, 1, "{}", mismatch.text());
        assert_eq!(super::super::snapshot::git_ref(&remote).unwrap(), advanced);
        assert_eq!(super::super::snapshot::git_ref(&local).unwrap(), reference);
        assert_eq!(
            fs::read_to_string(local.join("unrelated.txt")).unwrap(),
            "unrelated dirty content"
        );
    }

    #[test]
    fn competing_remote_edits_are_preserved_with_independent_files_applied() {
        let homes: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        assert_eq!(sync(&homes, options(false)).exit_status, 0);
        skill(homes[1].path(), ".skillator/library/demo", "host1");
        skill(homes[2].path(), ".skillator/library/demo", "host2");
        fs::write(
            homes[2].path().join(".skillator/library/demo/new.txt"),
            "new",
        )
        .unwrap();
        let mut remote = options(false);
        remote.conflict = ConflictPolicy::Remote;
        let report = sync(&homes, remote);
        assert_eq!(report.exit_status, 1, "{}", report.text());
        for (index, home) in homes.iter().enumerate() {
            assert!(
                fs::read_to_string(home.path().join(".skillator/library/demo/SKILL.md"))
                    .unwrap()
                    .contains(["original", "host1", "host2"][index])
            );
            assert_eq!(
                fs::read_to_string(home.path().join(".skillator/library/demo/new.txt")).unwrap(),
                "new"
            );
        }
    }

    #[test]
    fn unregistered_acquisition_target_is_blocked_without_copying_an_alias_directory() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), "Development/skills/demo", "original");
        fs::create_dir_all(homes[0].path().join(".skillator/library")).unwrap();
        std::os::unix::fs::symlink(
            homes[0].path().join("Development/skills/demo"),
            homes[0].path().join(".skillator/library/demo"),
        )
        .unwrap();
        let report = sync(&homes, options(false));
        assert_eq!(report.exit_status, 1, "{}", report.text());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "alias_target_unavailable")
        );
        assert!(!homes[1].path().join(".skillator/library/demo").exists());
    }

    #[test]
    fn failed_registration_reports_error_without_losing_content() {
        use super::super::transport::Fault;
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        let mut peers = participants(&homes);
        let inner = std::mem::replace(
            &mut peers[1].endpoint,
            Endpoint::local(AppPaths::new(homes[1].path().into())),
        );
        peers[1].endpoint = Endpoint::Fault {
            inner: Box::new(inner),
            fault: Fault::Register,
        };
        let report = synchronize(
            &AppPaths::new(homes[0].path().into()),
            peers,
            options(false),
        )
        .unwrap();
        assert_eq!(report.exit_status, 1, "{}", report.text());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "registration_failed")
        );
        assert!(
            homes[1]
                .path()
                .join(".skillator/library/demo/SKILL.md")
                .exists()
        );
    }

    #[test]
    fn internal_file_link_is_independent_of_its_target() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        let root = homes[0].path().join(".skillator/library/demo");
        fs::write(root.join("data.txt"), "payload").unwrap();
        std::os::unix::fs::symlink("data.txt", root.join("shortcut")).unwrap();
        let first = sync(&homes, options(false));
        assert_eq!(first.exit_status, 0, "{}", first.text());
        let received = homes[1].path().join(".skillator/library/demo");
        assert_eq!(
            fs::read_link(received.join("shortcut")).unwrap(),
            PathBuf::from("data.txt")
        );
        fs::write(received.join("data.txt"), "remote edit").unwrap();
        let second = sync(&homes, options(false));
        assert_eq!(second.exit_status, 0, "{}", second.text());
        assert_eq!(
            fs::read_to_string(root.join("data.txt")).unwrap(),
            "remote edit"
        );
        assert_eq!(
            fs::read_link(root.join("shortcut")).unwrap(),
            PathBuf::from("data.txt")
        );
    }

    #[test]
    fn conflicting_acquisition_targets_do_not_publish_an_arbitrary_alias() {
        let homes: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
        for (index, target) in ["Development/skills/demo", "Development/other/demo"]
            .iter()
            .enumerate()
        {
            configure(homes[index].path(), ".skillator/library");
            fs::write(
                homes[index].path().join(".skillator/library.yaml"),
                format!(
                    "version: 1\nlocations:\n - path: '~/.skillator/library'\n - path: '~/{}'\n",
                    target.trim_end_matches("/demo")
                ),
            )
            .unwrap();
            skill(homes[index].path(), target, "original");
            fs::create_dir_all(homes[index].path().join(".skillator/library")).unwrap();
            std::os::unix::fs::symlink(
                homes[index].path().join(target),
                homes[index].path().join(".skillator/library/demo"),
            )
            .unwrap();
        }
        let report = sync(&homes, options(false));
        assert_eq!(report.exit_status, 1, "{}", report.text());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "alias_conflict")
        );
        assert!(!homes[2].path().join(".skillator/library/demo").exists());
        for (index, target) in ["Development/skills/demo", "Development/other/demo"]
            .iter()
            .enumerate()
        {
            assert_eq!(
                homes[index]
                    .path()
                    .join(".skillator/library/demo")
                    .canonicalize()
                    .unwrap(),
                homes[index].path().join(target).canonicalize().unwrap()
            );
        }
    }

    #[test]
    fn library_alias_and_original_converge_in_the_same_run() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        fs::write(homes[0].path().join(".skillator/library.yaml"), "version: 1\nlocations:\n - path: '~/.skillator/library'\n - path: '~/Development/skills'\n").unwrap();
        skill(homes[0].path(), "Development/skills/demo", "original");
        fs::create_dir_all(homes[0].path().join(".skillator/library")).unwrap();
        std::os::unix::fs::symlink(
            homes[0].path().join("Development/skills/demo"),
            homes[0].path().join(".skillator/library/demo"),
        )
        .unwrap();
        let preview = sync(&homes, options(true));
        assert!(preview.changes.iter().any(|change| {
            change.host == "host1" && change.action == "create_acquisition_alias"
        }));
        assert!(!homes[1].path().join(".skillator/library/demo").exists());
        let first = sync(&homes, options(false));
        assert_eq!(first.exit_status, 0, "{}", first.text());
        assert!(homes[1].path().join(".skillator/library/demo").is_symlink());
        assert_eq!(
            homes[1]
                .path()
                .join(".skillator/library/demo")
                .canonicalize()
                .unwrap(),
            homes[1]
                .path()
                .join("Development/skills/demo")
                .canonicalize()
                .unwrap()
        );
        skill(
            homes[1].path(),
            ".skillator/library/demo",
            "remote alias edit",
        );
        let changed = sync(&homes, options(false));
        assert_eq!(changed.exit_status, 0, "{}", changed.text());
        for home in &homes {
            for file in [
                ".skillator/library/demo/SKILL.md",
                "Development/skills/demo/SKILL.md",
            ] {
                assert!(
                    fs::read_to_string(home.path().join(file))
                        .unwrap()
                        .contains("remote alias edit")
                );
            }
        }
        assert!(homes[0].path().join(".skillator/library/demo").is_symlink());
        assert_eq!(
            changed
                .changes
                .iter()
                .filter(|change| change.host == "local" && change.action == "copy_skill_entry")
                .count(),
            1
        );
        assert!(sync(&homes, options(false)).changes.is_empty());
    }

    #[test]
    fn complete_skill_content_respects_exclusions_and_internal_links() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        fs::write(homes[0].path().join(".skillator/library.yaml"), "version: 1\nlocations:\n - path: '~/.skillator/library'\n   exclusions: ['demo/private.txt']\n").unwrap();
        skill(homes[0].path(), ".skillator/library/demo", "original");
        let source = homes[0].path().join(".skillator/library/demo");
        fs::create_dir(source.join("assets")).unwrap();
        fs::write(source.join("assets/example.txt"), "payload").unwrap();
        fs::write(source.join("private.txt"), "excluded").unwrap();
        std::os::unix::fs::symlink("assets", source.join("shortcut")).unwrap();
        let report = sync(&homes, options(false));
        assert_eq!(report.exit_status, 0, "{}", report.text());
        let received = homes[1].path().join(".skillator/library/demo");
        assert_eq!(
            fs::read_to_string(received.join("assets/example.txt")).unwrap(),
            "payload"
        );
        assert!(!received.join("private.txt").exists());
        assert_eq!(
            fs::read_link(received.join("shortcut")).unwrap(),
            PathBuf::from("assets")
        );
    }
    #[test]
    fn partial_failures_keep_identity_and_retry_safely() {
        use super::super::transport::Fault;
        for fault in [
            Fault::Transfer,
            Fault::Publish,
            Fault::Verification,
            Fault::Acknowledge,
        ] {
            let homes: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
            configure(homes[0].path(), ".skillator/library");
            skill(homes[0].path(), ".skillator/library/demo", "original");
            let mut peers = participants(&homes);
            let endpoint = std::mem::replace(
                &mut peers[1].endpoint,
                Endpoint::local(AppPaths::new(homes[1].path().into())),
            );
            peers[1].endpoint = Endpoint::Fault {
                inner: Box::new(endpoint),
                fault,
            };
            let failed = synchronize(
                &AppPaths::new(homes[0].path().into()),
                peers,
                options(false),
            )
            .unwrap();
            assert_eq!(failed.exit_status, 1, "{}", failed.text());
            let identity = state::History::load(homes[1].path()).unwrap().id.unwrap();
            let retried = sync(&homes, options(false));
            assert_eq!(retried.exit_status, 0, "{}", retried.text());
            assert_eq!(
                state::History::load(homes[1].path()).unwrap().id.as_deref(),
                Some(identity.as_str())
            );
            for home in &homes {
                assert!(
                    fs::read_to_string(home.path().join(".skillator/library/demo/SKILL.md"))
                        .unwrap()
                        .contains("original")
                );
            }
            assert!(sync(&homes, options(false)).changes.is_empty());
        }
    }

    #[test]
    fn failed_peer_batch_does_not_publish_or_ack_its_files() {
        use super::super::transport::Fault;
        let homes: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        let source = homes[0].path().join(".skillator/library/demo");
        for name in ["alpha", "beta"] {
            fs::write(source.join(name), name).unwrap();
        }
        let mut peers = participants(&homes);
        let endpoint = std::mem::replace(
            &mut peers[1].endpoint,
            Endpoint::local(AppPaths::new(homes[1].path().into())),
        );
        peers[1].endpoint = Endpoint::Fault {
            inner: Box::new(endpoint),
            fault: Fault::Transfer,
        };
        let report = synchronize(
            &AppPaths::new(homes[0].path().into()),
            peers,
            options(false),
        )
        .unwrap();
        assert_eq!(report.exit_status, 1, "{}", report.text());
        for name in ["alpha", "beta"] {
            let path = format!(".skillator/library/demo/{name}");
            assert!(!homes[1].path().join(&path).exists());
            assert_eq!(
                fs::read_to_string(homes[2].path().join(&path)).unwrap(),
                name
            );
            let first = state::History::load(homes[1].path()).unwrap();
            assert!(
                first
                    .peers
                    .values()
                    .all(|baseline| !matches!(baseline.files.get(&path), Some(Some(_))))
            );
        }
        let retry = sync(&homes, options(false));
        assert_eq!(retry.exit_status, 0, "{}", retry.text());
        for name in ["alpha", "beta"] {
            assert_eq!(
                fs::read_to_string(homes[1].path().join(".skillator/library/demo").join(name))
                    .unwrap(),
                name
            );
        }
    }

    #[test]
    fn replaced_and_duplicate_participant_identities_abort_before_mutation() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        assert_eq!(sync(&homes, options(false)).exit_status, 0);
        let local_state = fs::read(homes[0].path().join(".skillator/rsync/state.json")).unwrap();
        let remote_path = homes[1].path().join(".skillator/rsync/state.json");
        let remote_state = fs::read(&remote_path).unwrap();
        fs::write(&remote_path, &local_state).unwrap();
        let duplicate = synchronize(
            &AppPaths::new(homes[0].path().into()),
            participants(&homes),
            options(false),
        )
        .unwrap_err();
        assert!(duplicate.message.contains("duplicate"));
        fs::remove_file(&remote_path).unwrap();
        let replaced = synchronize(
            &AppPaths::new(homes[0].path().into()),
            participants(&homes),
            options(false),
        )
        .unwrap_err();
        assert!(replaced.message.contains("identity changed"));
        assert_eq!(
            fs::read(homes[0].path().join(".skillator/rsync/state.json")).unwrap(),
            local_state
        );
        assert!(!remote_path.exists());
        fs::write(remote_path, remote_state).unwrap();
    }

    #[test]
    fn former_receiver_rejects_replaced_initiator_before_mutation() {
        let mut homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        assert_eq!(sync(&homes, options(false)).exit_status, 0);
        let previous = state::History::load(homes[0].path()).unwrap().id.unwrap();
        let receiver = homes.remove(1);
        let replacement = tempfile::tempdir().unwrap();
        let rotated = vec![receiver, replacement];
        let receiver_state =
            fs::read(rotated[0].path().join(".skillator/rsync/state.json")).unwrap();
        let error = synchronize(
            &AppPaths::new(rotated[0].path().into()),
            participants(&rotated),
            options(false),
        )
        .unwrap_err();
        assert!(error.message.contains("identity changed"), "{error}");
        assert_eq!(
            fs::read(rotated[0].path().join(".skillator/rsync/state.json")).unwrap(),
            receiver_state
        );
        assert!(!rotated[1].path().join(".skillator/rsync").exists());
        assert!(
            state::History::load(rotated[0].path())
                .unwrap()
                .peers
                .contains_key(&previous)
        );
    }

    #[test]
    fn former_receiver_can_initiate_with_the_same_peer() {
        let mut homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        assert_eq!(sync(&homes, options(false)).exit_status, 0);
        homes.swap(0, 1);
        let report = sync(&homes, options(false));
        assert_eq!(report.exit_status, 0, "{}", report.text());
    }

    #[test]
    fn directory_type_conflicts_choose_a_complete_subtree() {
        for local_directory in [false, true] {
            let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
            for home in &homes {
                configure(home.path(), ".skillator/library");
                skill(home.path(), ".skillator/library/demo", "original");
                fs::write(
                    home.path().join(".skillator/library/demo/unrelated"),
                    "keep",
                )
                .unwrap();
            }
            for (index, home) in homes.iter().enumerate() {
                let path = home.path().join(".skillator/library/demo/data");
                if (index == 0) == local_directory {
                    fs::create_dir(&path).unwrap();
                    fs::write(path.join("child"), "nested").unwrap();
                } else {
                    fs::write(path, "flat").unwrap();
                }
            }
            let ask = sync(&homes, options(false));
            assert_eq!(ask.exit_status, 1, "{}", ask.text());
            let mut opts = options(false);
            opts.conflict = ConflictPolicy::Local;
            let chosen = sync(&homes, opts);
            assert_eq!(chosen.exit_status, 0, "{}", chosen.text());
            for home in &homes {
                let path = home.path().join(".skillator/library/demo/data");
                assert_eq!(path.is_dir(), local_directory);
                assert_eq!(
                    fs::read_to_string(if local_directory {
                        path.join("child")
                    } else {
                        path
                    })
                    .unwrap(),
                    if local_directory { "nested" } else { "flat" }
                );
                assert_eq!(
                    fs::read_to_string(home.path().join(".skillator/library/demo/unrelated"))
                        .unwrap(),
                    "keep"
                );
            }
        }
    }
    #[test]
    fn conflicting_location_settings_block_files_and_preserve_registration_bytes() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        for (index, home) in homes.iter().enumerate() {
            configure(home.path(), ".skillator/library");
            skill(
                home.path(),
                ".skillator/library/demo",
                if index == 0 { "local" } else { "remote" },
            );
        }
        let remote_config = homes[1].path().join(".skillator/library.yaml");
        fs::write(
            &remote_config,
            "version: 1\nlocations: [{path: '~/.skillator/library', exclusions: ['secret']}]\n",
        )
        .unwrap();
        let before = fs::read(&remote_config).unwrap();
        let mut opts = options(false);
        opts.conflict = ConflictPolicy::Local;
        let report = sync(&homes, opts);
        assert_eq!(report.exit_status, 1, "{}", report.text());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "location_conflict")
        );
        assert!(
            fs::read_to_string(homes[1].path().join(".skillator/library/demo/SKILL.md"))
                .unwrap()
                .contains("remote")
        );
        assert_eq!(fs::read(&remote_config).unwrap(), before);
    }

    #[test]
    fn source_identity_collisions_block_all_conflicting_paths() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), "first/skills");
        configure(homes[1].path(), "second/skills");
        skill(homes[0].path(), "first/skills/demo", "local");
        skill(homes[1].path(), "second/skills/demo", "remote");
        let report = sync(&homes, options(false));
        assert_eq!(report.exit_status, 1, "{}", report.text());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "source_identity_collision")
        );
        assert!(!homes[0].path().join("second").exists());
        assert!(!homes[1].path().join("first").exists());
    }
    #[test]
    fn ignored_absent_location_is_successful_in_check_and_apply() {
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        configure(homes[0].path(), ".skillator/library");
        skill(homes[0].path(), ".skillator/library/demo", "original");
        for check in [true, false, true] {
            let mut opts = options(check);
            opts.missing = MissingPolicy::Ignore;
            let report = sync(&homes, opts);
            assert_eq!(report.exit_status, 0, "{}", report.text());
            assert!(report.changes.is_empty(), "{}", report.text());
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|d| d.code == "missing_ignored")
            );
            assert!(
                report
                    .diagnostics
                    .iter()
                    .filter(|d| d.code == "missing_ignored")
                    .all(|d| d.data.as_ref().unwrap().get("host").unwrap() == "host1")
            );
            assert!(!homes[1].path().join(".skillator/library").exists());
            assert!(!homes[1].path().join(".skillator/library.yaml").exists());
        }
    }

    #[test]
    fn nested_directory_replacement_resolves_and_retries_with_independent_work() {
        for policy in [ConflictPolicy::Local, ConflictPolicy::Remote] {
            let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
            configure(homes[0].path(), ".skillator/library");
            skill(homes[0].path(), ".skillator/library/demo", "original");
            let data = homes[0].path().join(".skillator/library/demo/data");
            fs::create_dir_all(data.join("sub/deeper")).unwrap();
            fs::write(data.join("sub/deeper/child"), "nested").unwrap();
            assert_eq!(sync(&homes, options(false)).exit_status, 0);
            fs::remove_dir_all(&data).unwrap();
            fs::write(&data, "flat").unwrap();
            fs::write(
                homes[1]
                    .path()
                    .join(".skillator/library/demo/data/sub/deeper/child"),
                "remote nested edit",
            )
            .unwrap();
            fs::write(
                homes[1].path().join(".skillator/library/demo/independent"),
                "new",
            )
            .unwrap();
            let mut opts = options(false);
            opts.conflict = policy;
            let report = sync(&homes, opts);
            assert_eq!(report.exit_status, 0, "{}", report.text());
            for home in &homes {
                let data = home.path().join(".skillator/library/demo/data");
                assert_eq!(
                    fs::read_to_string(if matches!(policy, ConflictPolicy::Local) {
                        data
                    } else {
                        data.join("sub/deeper/child")
                    })
                    .unwrap(),
                    if matches!(policy, ConflictPolicy::Local) {
                        "flat"
                    } else {
                        "remote nested edit"
                    }
                );
                assert_eq!(
                    fs::read_to_string(home.path().join(".skillator/library/demo/independent"))
                        .unwrap(),
                    "new"
                );
            }
            let retry = sync(&homes, options(false));
            assert_eq!(retry.exit_status, 0, "{}", retry.text());
            assert!(retry.changes.is_empty(), "{}", retry.text());
        }
    }

    #[test]
    fn first_contact_git_base_does_not_authorize_removing_a_tracked_file() {
        use super::super::process;
        let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
        let origin = tempfile::tempdir().unwrap();
        skill(origin.path(), "demo", "base");
        fs::write(origin.path().join("demo/tracked"), "tracked").unwrap();
        process::git(origin.path(), &["init", "-q"]).unwrap();
        process::git(origin.path(), &["add", "."]).unwrap();
        process::git(
            origin.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "base",
            ],
        )
        .unwrap();
        for home in &homes {
            let checkout = home.path().join("skills");
            process::capture(
                std::process::Command::new("git")
                    .arg("clone")
                    .arg(origin.path())
                    .arg(&checkout),
            )
            .unwrap();
            configure(home.path(), "skills");
        }
        fs::remove_file(homes[0].path().join("skills/demo/tracked")).unwrap();
        let mut opts = options(false);
        opts.missing = MissingPolicy::Remove;
        let report = sync(&homes, opts);
        assert_eq!(report.exit_status, 1, "{}", report.text());
        assert!(homes[1].path().join("skills/demo/tracked").exists());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "missing_history")
        );
    }

    #[test]
    fn first_contact_tracked_conflicts_preserve_indexes_under_each_policy() {
        use super::super::process;
        for policy in [
            ConflictPolicy::Ask,
            ConflictPolicy::Local,
            ConflictPolicy::Remote,
        ] {
            let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
            let origin = tempfile::tempdir().unwrap();
            skill(origin.path(), "demo", "base");
            process::git(origin.path(), &["init", "-q"]).unwrap();
            process::git(origin.path(), &["add", "."]).unwrap();
            process::git(
                origin.path(),
                &[
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "commit",
                    "-qm",
                    "base",
                ],
            )
            .unwrap();
            let mut indexes = Vec::new();
            let mut references = Vec::new();
            for (index, home) in homes.iter().enumerate() {
                let checkout = home.path().join("skills");
                process::capture(
                    std::process::Command::new("git")
                        .arg("clone")
                        .arg(origin.path())
                        .arg(&checkout),
                )
                .unwrap();
                configure(home.path(), "skills");
                skill(
                    home.path(),
                    "skills/demo",
                    if index == 0 {
                        "local edit"
                    } else {
                        "remote edit"
                    },
                );
                indexes.push(fs::read(checkout.join(".git/index")).unwrap());
                references.push(super::super::snapshot::git_ref(&checkout).unwrap());
            }
            let mut opts = options(false);
            opts.conflict = policy;
            let report = sync(&homes, opts);
            assert_eq!(
                report.exit_status,
                if matches!(policy, ConflictPolicy::Ask) {
                    1
                } else {
                    0
                },
                "{}",
                report.text()
            );
            for (index, home) in homes.iter().enumerate() {
                let checkout = home.path().join("skills");
                let expected = match policy {
                    ConflictPolicy::Local => "local edit",
                    ConflictPolicy::Remote => "remote edit",
                    ConflictPolicy::Ask => {
                        if index == 0 {
                            "local edit"
                        } else {
                            "remote edit"
                        }
                    }
                };
                assert!(
                    fs::read_to_string(checkout.join("demo/SKILL.md"))
                        .unwrap()
                        .contains(expected)
                );
                assert_eq!(
                    fs::read(checkout.join(".git/index")).unwrap(),
                    indexes[index]
                );
                assert_eq!(
                    super::super::snapshot::git_ref(&checkout).unwrap(),
                    references[index]
                );
            }
        }
    }

    #[test]
    fn bootstrap_retains_initiating_branch_provenance_without_branch_alignment() {
        use super::super::process;
        for detached in [false, true] {
            let homes: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
            let origin = tempfile::tempdir().unwrap();
            skill(origin.path(), "demo", "base");
            process::git(origin.path(), &["init", "-q"]).unwrap();
            process::git(origin.path(), &["add", "."]).unwrap();
            process::git(
                origin.path(),
                &[
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "commit",
                    "-qm",
                    "base",
                ],
            )
            .unwrap();
            let local = homes[0].path().join("skills");
            process::capture(
                std::process::Command::new("git")
                    .arg("clone")
                    .arg(origin.path())
                    .arg(&local),
            )
            .unwrap();
            process::git(&local, &["checkout", "-b", "skills/reference"]).unwrap();
            if detached {
                process::git(&local, &["checkout", "--detach"]).unwrap();
            }
            configure(homes[0].path(), "skills");
            let reference = super::super::snapshot::git_ref(&local).unwrap();
            let report = sync(&homes, options(false));
            assert_eq!(report.exit_status, 0, "{}", report.text());
            for home in &homes {
                let history = state::History::load(home.path()).unwrap();
                let provenance = &history.provenance["skills"];
                assert_eq!(provenance.origin, reference.origin);
                assert_eq!(provenance.commit, reference.commit);
                assert_eq!(
                    provenance.branch.as_deref(),
                    if detached {
                        None
                    } else {
                        Some("skills/reference")
                    }
                );
            }
            assert_eq!(
                process::git(
                    &homes[1].path().join("skills"),
                    &["rev-parse", "--abbrev-ref", "HEAD"]
                )
                .unwrap(),
                b"HEAD\n"
            );
            let repeat = sync(&homes, options(false));
            assert_eq!(repeat.exit_status, 0, "{}", repeat.text());
            assert!(repeat.changes.is_empty());
        }
    }
}
