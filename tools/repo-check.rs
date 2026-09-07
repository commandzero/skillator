//! Contributor checks. Built as an example so tooling adds no product commands.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.as_bytes()[0].is_ascii_lowercase()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn title_check(title: &str) -> Result<()> {
    let (prefix, summary) = title
        .split_once(": ")
        .ok_or("Use a Conventional Commit PR title")?;
    if summary.trim().is_empty() || title.contains(['\n', '\r']) {
        return Err("The PR title must have a one-line summary".into());
    }
    let prefix = prefix.strip_suffix('!').unwrap_or(prefix);
    let kind = if let Some((kind, scope)) = prefix.split_once('(') {
        let scope = scope.strip_suffix(')').ok_or("Invalid commit scope")?;
        if scope.is_empty()
            || !scope
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_/".contains(&b))
        {
            return Err("Invalid commit scope".into());
        }
        kind
    } else {
        prefix
    };
    if ![
        "feat", "fix", "docs", "refactor", "perf", "test", "build", "ci", "chore", "revert",
    ]
    .contains(&kind)
    {
        return Err(format!("Unsupported Conventional Commit type: {kind}").into());
    }
    Ok(())
}

fn field<'a>(body: &'a str, name: &str) -> Result<&'a str> {
    let values: Vec<_> = body
        .lines()
        .filter_map(|line| line.strip_prefix(name))
        .collect();
    if values.len() != 1 || values[0].trim().is_empty() {
        return Err(format!("Provide exactly one nonempty `{name}` field").into());
    }
    Ok(values[0].trim())
}

fn id_from_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("openspec/changes/")?;
    let name = if let Some(rest) = rest.strip_prefix("archive/") {
        let folder = rest.split('/').next()?;
        if folder.len() < 12 || folder.as_bytes()[10] != b'-' {
            return None;
        }
        &folder[11..]
    } else {
        rest.split('/').next()?
    };
    valid_id(name).then(|| name.to_owned())
}

fn files(root: &Path, head: &str, path: &str) -> Result<Vec<String>> {
    Ok(git(
        root,
        &["ls-tree", "-r", "--name-only", "-z", head, "--", path],
    )?
    .split('\0')
    .filter(|s| !s.is_empty())
    .map(str::to_owned)
    .collect())
}

fn selected(root: &Path, base: &str, head: &str, body: &str) -> Result<BTreeSet<String>> {
    let association = field(body, "OpenSpec changes:")?;
    let mut ids = BTreeSet::new();
    if association == "none" {
        let reason = field(body, "OpenSpec reason:")?;
        if reason.starts_with("Replace this") {
            return Err("Explain why this PR has no OpenSpec work".into());
        }
    } else {
        for id in association.split(',').map(str::trim) {
            if !valid_id(id) {
                return Err(format!("Invalid OpenSpec change ID: {id}").into());
            }
            ids.insert(id.to_owned());
        }
    }
    // Disabling rename detection exposes both old and new paths, including
    // deleted-only and renamed changes. The merge base scopes the comparison.
    let diff = git(
        root,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            &format!("{base}...{head}"),
            "--",
        ],
    )?;
    for path in diff.split('\0') {
        if let Some(id) = id_from_path(path) {
            ids.insert(id);
        }
        if let Some(name) = path
            .strip_prefix("openspec/reviews/")
            .and_then(|s| s.strip_suffix(".yaml"))
        {
            if !valid_id(name) {
                return Err("Invalid OpenSpec review filename".into());
            }
            ids.insert(name.to_owned());
        }
    }
    if ids.is_empty() && diff.split('\0').any(|p| p.starts_with("openspec/specs/")) {
        return Err("Main specs changed: associate their OpenSpec change IDs explicitly".into());
    }
    Ok(ids)
}

fn archive(root: &Path, head: &str, id: &str) -> Result<String> {
    if !files(root, head, &format!("openspec/changes/{id}"))?.is_empty() {
        return Err(format!("{id}: active change must be archived before merge").into());
    }
    let archives: BTreeSet<_> = files(root, head, "openspec/changes/archive")?
        .iter()
        .filter(|p| id_from_path(p).as_deref() == Some(id))
        .filter_map(|p| p.split('/').nth(3))
        .map(|folder| format!("openspec/changes/archive/{folder}"))
        .collect();
    if archives.len() != 1 {
        return Err(format!(
            "{id}: expected exactly one preserved archive, found {}",
            archives.len()
        )
        .into());
    }
    let path = archives.into_iter().next().unwrap();
    for artifact in ["proposal.md", "design.md", "tasks.md"] {
        git(root, &["show", &format!("{head}:{path}/{artifact}")])
            .map_err(|_| format!("{id}: archive is missing {artifact}"))?;
    }
    Ok(path)
}

fn object(root: &Path, head: &str, path: &str) -> Result<String> {
    // List first: a failed Git operation must not masquerade as an absent spec.
    if git(root, &["ls-tree", head, "--", path])?.is_empty() {
        return Ok("absent".to_owned());
    }
    Ok(git(root, &["rev-parse", &format!("{head}:{path}")])?
        .trim()
        .to_owned())
}

fn delta_paths(root: &Path, head: &str, archive: &str) -> Result<BTreeSet<String>> {
    Ok(files(root, head, &format!("{archive}/specs"))?
        .into_iter()
        .filter_map(|p| {
            p.strip_prefix(&format!("{archive}/"))
                .map(|p| format!("openspec/{p}"))
        })
        .collect())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    change: String,
    reviewer: String,
    reviewed_at: String,
    notes: String,
    no_spec_deltas: bool,
    archive_tree: String,
    specs: BTreeMap<String, String>,
}

fn make_review(
    root: &Path,
    head: &str,
    id: &str,
    reviewer: &str,
    notes: &str,
    no_deltas: bool,
    extra: &[String],
) -> Result<Review> {
    let archive = archive(root, head, id)?;
    let mut paths = delta_paths(root, head, &archive)?;
    if paths.is_empty() && !no_deltas {
        return Err("No deltas: use --no-spec-deltas and explain why in the review notes".into());
    }
    paths.extend(extra.iter().cloned());
    let specs = paths
        .into_iter()
        .map(|path| {
            if !path.starts_with("openspec/specs/")
                || !path.ends_with("/spec.md")
                || path.contains("..")
            {
                return Err("Review spec paths must be openspec/specs/<capability>/spec.md".into());
            }
            Ok((path.clone(), object(root, head, &path)?))
        })
        .collect::<Result<_>>()?;
    let date = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()?;
    if !date.status.success() {
        return Err("Cannot read review time".into());
    }
    Ok(Review {
        change: id.to_owned(),
        reviewer: reviewer.to_owned(),
        notes: notes.to_owned(),
        reviewed_at: String::from_utf8(date.stdout)?.trim().to_owned(),
        no_spec_deltas: no_deltas,
        archive_tree: object(root, head, &archive)?,
        specs,
    })
}

fn check_review(root: &Path, head: &str, id: &str) -> Result<String> {
    let archive = archive(root, head, id)?;
    let yaml = git(
        root,
        &["show", &format!("{head}:openspec/reviews/{id}.yaml")],
    )
    .map_err(|_| {
        format!("{id}: missing committed synchronization review; see docs/contributing.md")
    })?;
    let review: Review = serde_saphyr::from_str(&yaml)?;
    if review.change != id
        || review.reviewer.trim().is_empty()
        || review.notes.trim().is_empty()
        || review.reviewed_at.trim().is_empty()
    {
        return Err(format!(
            "{id}: review must identify the change, reviewer, time and semantic findings"
        )
        .into());
    }
    if review.archive_tree != object(root, head, &archive)? {
        return Err(format!("{id}: archive changed after synchronization review").into());
    }
    let required = delta_paths(root, head, &archive)?;
    if required.is_empty() && !review.no_spec_deltas {
        return Err(format!("{id}: missing explicit no-delta explanation").into());
    }
    for path in &required {
        if !review.specs.contains_key(path) {
            return Err(format!("{id}: review omits {path}").into());
        }
    }
    for (path, digest) in &review.specs {
        if !path.starts_with("openspec/specs/")
            || !path.ends_with("/spec.md")
            || path.contains("..")
        {
            return Err(format!("{id}: invalid reviewed spec path").into());
        }
        if object(root, head, path)? != *digest {
            return Err(format!("{id}: {path} changed after synchronization review").into());
        }
    }
    println!(
        "{id}: archive and reviewed spec revisions match, reviewer {}",
        review.reviewer
    );
    Ok(archive)
}

fn stage_selected(root: &Path, head: &str, archives: &[String]) -> Result<tempfile::TempDir> {
    let stage = tempfile::tempdir()?;
    let mut paths = files(root, head, "openspec/specs")?;
    // Preserve the reviewed revision's configuration and local schema definitions.
    paths.extend(files(root, head, "openspec/config.yaml")?);
    paths.extend(files(root, head, "openspec/schemas")?);
    for archive in archives {
        paths.extend(files(root, head, archive)?);
    }
    for path in paths {
        let destination = stage.path().join(&path);
        fs::create_dir_all(destination.parent().unwrap())?;
        fs::write(
            destination,
            git(root, &["show", &format!("{head}:{path}")])?,
        )?;
    }
    Ok(stage)
}

fn validate_selected(root: &Path, head: &str, archives: &[String]) -> Result<()> {
    if archives.is_empty() {
        return Ok(());
    }
    let stage = stage_selected(root, head, archives)?;
    for kind in ["--specs", "--archived"] {
        let status = Command::new("openspec")
            .current_dir(stage.path())
            .env("OPENSPEC_TELEMETRY", "0")
            .args(["validate", kind, "--strict", "--no-interactive"])
            .status()?;
        if !status.success() {
            return Err(format!("OpenSpec {kind} validation failed").into());
        }
    }
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = PathBuf::from(git(Path::new("."), &["rev-parse", "--show-toplevel"])?.trim());
    match args.first().map(String::as_str) {
        Some("pr") if args.len() == 5 => {
            title_check(&args[3])?;
            // Resolve revision inputs before using them in subsequent Git arguments.
            let base = git(&root, &["rev-parse", "--verify", "--end-of-options", &format!("{}^{{commit}}", args[1])])?;
            let head = git(&root, &["rev-parse", "--verify", "--end-of-options", &format!("{}^{{commit}}", args[2])])?;
            let ids = selected(&root, base.trim(), head.trim(), &fs::read_to_string(&args[4])?)?;
            if ids.is_empty() { println!("OpenSpec: not applicable to this PR"); }
            let mut archives = Vec::new();
            let mut failures = Vec::new();
            for id in &ids {
                match check_review(&root, head.trim(), id) {
                    Ok(archive) => archives.push(archive),
                    Err(error) => failures.push(format!("{id}: {error}")),
                }
            }
            if !failures.is_empty() { return Err(failures.join("\n").into()); }
            validate_selected(&root, head.trim(), &archives)?;
        }
        Some("review") if args.len() >= 5 && args[1] == "--reviewed" => {
            let id = &args[2];
            if !valid_id(id) { return Err("Invalid change ID".into()); }
            let notes = fs::read_to_string(&args[4])?;
            if args[3].trim().is_empty() || notes.trim().is_empty() { return Err("Reviewer and semantic review notes are required".into()); }
            let no_deltas = args[5..].iter().any(|s| s == "--no-spec-deltas");
            let extra: Vec<_> = args[5..].iter().filter(|s| s.as_str() != "--no-spec-deltas").cloned().collect();
            let review = make_review(&root, "HEAD", id, &args[3], &notes, no_deltas, &extra)?;
            fs::create_dir_all(root.join("openspec/reviews"))?;
            let path = root.join(format!("openspec/reviews/{id}.yaml"));
            fs::write(&path, serde_saphyr::to_string(&review)?)?;
            println!("Recorded review of committed HEAD at {}", path.display());
        }
        _ => return Err("Usage: repo-check pr BASE HEAD TITLE BODY_FILE | review --reviewed ID REVIEWER NOTES_FILE [--no-spec-deltas] [EXTRA_SPEC_PATH ...]".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests;
