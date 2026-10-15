mod support;

use skillator::git::GitRepository;
use skillator::target::{Target, TargetError};

#[test]
fn nested_directory_resolves_to_worktree_root_and_reports_supplied_path() {
    let home = support::TestHome::new();
    let repository = home.git_repo("project");
    let nested = repository.join("src/nested");
    std::fs::create_dir_all(&nested).unwrap();

    let target = Target::select(&nested).unwrap();

    assert_eq!(target.supplied_path(), nested.canonicalize().unwrap());
    assert_eq!(target.root(), repository.canonicalize().unwrap());
    assert!(!target.repository().is_bare());
}

#[test]
fn git_facts_cover_origin_tracking_staging_and_ignore_rules() {
    let home = support::TestHome::new();
    let repository = home.git_repo("project");
    std::fs::write(repository.join("tracked"), "one").unwrap();
    std::fs::write(repository.join(".gitignore"), "ignored\n").unwrap();
    let git = GitRepository::discover(&repository).unwrap();
    support::git(&repository, &["add", "tracked", ".gitignore"]);
    support::git(&repository, &["config", "user.name", "Test User"]);
    support::git(
        &repository,
        &["config", "user.email", "test@example.invalid"],
    );
    support::git(&repository, &["commit", "-m", "fixture"]);
    support::git(
        &repository,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:elastic/agent-skills.git",
        ],
    );
    std::fs::write(repository.join("tracked"), "two").unwrap();
    support::git(&repository, &["add", "tracked"]);
    std::fs::write(repository.join("ignored"), "ignored").unwrap();

    let facts = git.facts_for("tracked").unwrap();
    assert!(facts.tracked);
    assert!(facts.staged);
    assert!(!facts.unmerged);
    assert!(git.facts_for("ignored").unwrap().ignored);
    assert_eq!(
        git.origin().unwrap().as_deref(),
        Some("git@github.com:elastic/agent-skills.git")
    );
}

#[test]
fn quoted_and_maximum_byte_paths_keep_tracking_staging_and_ignore_facts() {
    let home = support::TestHome::new();
    let repository = home.git_repo("project");
    std::fs::create_dir(repository.join("tracked")).unwrap();
    std::fs::create_dir(repository.join("ignored")).unwrap();
    std::fs::write(
        repository.join(".gitignore"),
        "ignored/*\n!ignored/keep:me\n",
    )
    .unwrap();
    support::git(&repository, &["config", "core.quotepath", "true"]);
    let names = [format!("{}x", "é".repeat(127)), "quote\"\ttab".to_owned()];
    let mut paths = vec![std::path::PathBuf::from("tracked")];
    for name in &names {
        let tracked = format!("tracked/{name}");
        let ignored = format!("ignored/{name}");
        std::fs::write(repository.join(&tracked), "tracked bytes").unwrap();
        std::fs::write(repository.join(&ignored), "ignored bytes").unwrap();
        support::git(&repository, &["add", "--", &tracked]);
        paths.push(tracked.into());
        paths.push(ignored.into());
    }
    std::fs::write(repository.join("ignored/keep:me"), "not ignored").unwrap();
    paths.push("ignored/keep:me".into());
    let git = GitRepository::discover(&repository).unwrap();
    let facts = git.facts_for_many(&paths).unwrap();
    assert!(facts[&std::path::PathBuf::from("tracked")].tracked);
    assert!(facts[&std::path::PathBuf::from("tracked")].staged);
    for name in names {
        let tracked = &facts[&std::path::PathBuf::from(format!("tracked/{name}"))];
        assert!(tracked.tracked && tracked.staged);
        assert!(facts[&std::path::PathBuf::from(format!("ignored/{name}"))].ignored);
    }
    assert!(!facts[&std::path::PathBuf::from("ignored/keep:me")].ignored);
}

#[test]
fn quoted_unmerged_paths_remain_git_protected() {
    let home = support::TestHome::new();
    let repository = home.git_repo("project");
    let name = "é-conflict\"\ttab";
    support::git(&repository, &["config", "user.name", "Test User"]);
    support::git(
        &repository,
        &["config", "user.email", "test@example.invalid"],
    );
    support::git(&repository, &["config", "core.quotepath", "true"]);
    support::git(&repository, &["symbolic-ref", "HEAD", "refs/heads/base"]);
    std::fs::write(repository.join(name), "base\n").unwrap();
    support::git(&repository, &["add", "--", name]);
    support::git(&repository, &["commit", "-m", "base"]);
    support::git(&repository, &["checkout", "-b", "side"]);
    std::fs::write(repository.join(name), "side\n").unwrap();
    support::git(&repository, &["add", "--", name]);
    support::git(&repository, &["commit", "-m", "side"]);
    support::git(&repository, &["checkout", "base"]);
    std::fs::write(repository.join(name), "main\n").unwrap();
    support::git(&repository, &["add", "--", name]);
    support::git(&repository, &["commit", "-m", "main"]);
    let merge = std::process::Command::new("git")
        .arg("-C")
        .arg(&repository)
        .args(["merge", "--no-edit", "side"])
        .output()
        .unwrap();
    assert_eq!(merge.status.code(), Some(1));
    let facts = GitRepository::discover(&repository)
        .unwrap()
        .facts_for(name)
        .unwrap();
    assert!(facts.tracked && facts.staged && facts.unmerged);
}

#[test]
fn invalid_target_inputs_are_rejected_without_writes() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("file");
    std::fs::write(&file, "content").unwrap();

    std::assert_matches!(Target::select(&file), Err(TargetError::NotDirectory(_)));
    std::assert_matches!(
        Target::select(directory.path()),
        Err(TargetError::NotGit(_))
    );
    std::assert_matches!(
        Target::select(directory.path().join("missing")),
        Err(TargetError::Missing(_))
    );
}

#[test]
fn git_fact_failures_are_errors_instead_of_untracked_facts() {
    let home = support::TestHome::new();
    let repository = home.git_repo("project");
    std::fs::write(repository.join("tracked"), "content").unwrap();
    support::git(&repository, &["add", "tracked"]);
    std::fs::write(repository.join(".git/index"), "not a git index").unwrap();
    let git = GitRepository::discover(&repository).unwrap();

    assert!(git.facts_for("tracked").is_err());
}
