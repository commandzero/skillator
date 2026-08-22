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
fn batched_git_facts_match_single_path_facts() {
    let home = support::TestHome::new();
    let repository = home.git_repo("project");
    std::fs::create_dir_all(repository.join("managed/child")).unwrap();
    std::fs::write(repository.join("managed/child/file"), "content").unwrap();
    std::fs::write(repository.join(".gitignore"), "ignored\n").unwrap();
    support::git(&repository, &["add", "managed", ".gitignore"]);
    let git = GitRepository::discover(&repository).unwrap();
    let paths = vec![
        std::path::PathBuf::from("managed"),
        std::path::PathBuf::from("ignored"),
        std::path::PathBuf::from("missing"),
    ];

    let batched = git.facts_for_many(&paths).unwrap();
    for path in paths {
        assert_eq!(batched.get(&path), Some(&git.facts_for(&path).unwrap()));
    }
    assert!(batched[&std::path::PathBuf::from("managed")].tracked);
    assert!(batched[&std::path::PathBuf::from("ignored")].ignored);
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
