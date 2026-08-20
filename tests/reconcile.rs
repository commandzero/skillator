mod support;

use skillator::config::{LibraryConfigCodec, LoadResult, RepositoryConfig, RepositoryConfigCodec};
use skillator::library::{LibrarySnapshot, scan_library};
use skillator::reconcile::{
    Action, Authorization, Outcome, Safety, TargetBusy, execute, plan, prepare_apply,
    prepare_check, prepare_transition,
};
use skillator::target::{Target, observe};
use std::collections::BTreeMap;

#[cfg(unix)]
#[test]
fn safe_plan_materializes_missing_link_and_control_file() {
    let fixture = Fixture::new("linked");
    let observed = observe(&fixture.target, &fixture.repository, &fixture.library);
    let plan = plan(&fixture.repository, &fixture.library, &observed);
    assert!(
        plan.items()
            .iter()
            .any(|item| item.safety() == Safety::Safe)
    );

    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    let result = execute(
        prepared,
        Authorization::SafeOnly,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );

    assert!(
        result
            .outcomes()
            .iter()
            .any(|item| item.outcome == Outcome::Applied)
    );
    let link = fixture
        .target
        .root()
        .join(".agents/skills/release-checklist");
    assert_eq!(
        std::fs::read_link(link).unwrap(),
        fixture.skill.canonicalize().unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(fixture.target.root().join(".agents/skills/.gitignore")).unwrap(),
        "# Managed by skillator.\n*\n!.gitignore\n"
    );
}

#[cfg(unix)]
#[test]
fn guarded_conflict_requires_all_guarded_authorization() {
    let fixture = Fixture::new("linked");
    let occupant = fixture
        .target
        .root()
        .join(".agents/skills/release-checklist");
    std::fs::create_dir_all(occupant.parent().unwrap()).unwrap();
    std::fs::write(&occupant, "user content").unwrap();
    let observed = observe(&fixture.target, &fixture.repository, &fixture.library);
    let first_plan = plan(&fixture.repository, &fixture.library, &observed);
    assert!(
        first_plan
            .items()
            .iter()
            .any(|item| item.safety() == Safety::Guarded)
    );

    let result = execute(
        prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap(),
        Authorization::SafeOnly,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );
    assert_eq!(std::fs::read_to_string(&occupant).unwrap(), "user content");
    assert!(
        result
            .outcomes()
            .iter()
            .any(|item| item.outcome == Outcome::NotAuthorized)
    );

    let result = execute(
        prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap(),
        Authorization::AllGuarded,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );
    assert!(
        result
            .outcomes()
            .iter()
            .any(|item| item.outcome == Outcome::Applied)
    );
    assert!(
        std::fs::symlink_metadata(&occupant)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn prepared_plan_holds_an_exclusive_noncreating_target_lock() {
    let fixture = Fixture::new("linked");
    let first = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    let second = prepare_check(&fixture.target, &fixture.repository, &fixture.library);
    std::assert_matches!(second, Err(TargetBusy));
    drop(first);
    assert!(prepare_check(&fixture.target, &fixture.repository, &fixture.library).is_ok());
}

#[test]
fn recovery_restores_one_backup_and_preserves_ambiguous_artifacts() {
    let fixture = Fixture::new("linked");
    let root = fixture.target.root().join(".agents/skills");
    std::fs::create_dir_all(&root).unwrap();
    let encoded = "release-checklist"
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let backup = root.join(format!(".skillator-backup-1-1-{encoded}"));
    std::fs::write(&backup, "recover me").unwrap();

    let prepared = prepare_apply(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    assert!(!root.join("release-checklist").exists());
    let result = execute(
        prepared,
        Authorization::SafeOnly,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );
    assert_eq!(
        std::fs::read_to_string(root.join("release-checklist")).unwrap(),
        "recover me"
    );
    assert!(!backup.exists());
    assert!(result.outcomes().iter().any(|outcome| {
        outcome.action == Action::Recover && outcome.outcome == Outcome::Applied
    }));

    let first = root.join(format!(".skillator-backup-1-2-{encoded}"));
    let second = root.join(format!(".skillator-backup-1-3-{encoded}"));
    std::fs::write(&first, "one").unwrap();
    std::fs::write(&second, "two").unwrap();
    let prepared = prepare_apply(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    assert!(first.exists() && second.exists());
    assert!(
        prepared
            .plan()
            .items()
            .iter()
            .any(|item| { item.action() == Action::Recover && item.safety() == Safety::Blocked })
    );
}

#[test]
fn recovery_blocks_when_destination_becomes_git_protected_after_planning() {
    let fixture = Fixture::new("linked");
    let root = fixture.target.root().join(".agents/skills");
    std::fs::create_dir_all(&root).unwrap();
    let destination = root.join("release-checklist");
    let encoded = "release-checklist"
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let backup = root.join(format!(".skillator-backup-1-1-{encoded}"));
    std::fs::write(&backup, "recover me").unwrap();
    let prepared = prepare_apply(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    std::fs::write(&destination, "index content").unwrap();
    support::git(
        fixture.target.root(),
        &["add", ".agents/skills/release-checklist"],
    );
    std::fs::remove_file(&destination).unwrap();

    let result = execute(
        prepared,
        Authorization::SafeOnly,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );

    assert!(result.outcomes().iter().any(|outcome| {
        outcome.action == Action::Recover && outcome.outcome == Outcome::Blocked
    }));
    assert!(backup.exists());
    assert!(!destination.exists());
}

#[cfg(unix)]
#[test]
fn copied_candidates_preserve_safe_links_and_exclude_git_metadata() {
    let mut fixture = Fixture::new("copied");
    std::fs::create_dir_all(fixture.skill.join("assets")).unwrap();
    std::fs::write(fixture.skill.join("assets/data"), "data").unwrap();
    std::os::unix::fs::symlink("assets/data", fixture.skill.join("data-link")).unwrap();
    std::fs::create_dir_all(fixture.skill.join("assets/.git")).unwrap();
    std::fs::write(fixture.skill.join("assets/.git/secret"), "excluded").unwrap();
    fixture.rescan();
    let result = execute(
        prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap(),
        Authorization::SafeOnly,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );
    assert!(
        result
            .outcomes()
            .iter()
            .any(|outcome| outcome.action == Action::Copy && outcome.outcome == Outcome::Applied),
        "{result:#?}"
    );
    let destination = fixture
        .target
        .root()
        .join(".agents/skills/release-checklist");
    assert_eq!(
        std::fs::read_link(destination.join("data-link")).unwrap(),
        std::path::PathBuf::from("assets/data")
    );
    assert!(!destination.join("assets/.git").exists());
}

#[cfg(unix)]
#[test]
fn escaping_or_absolute_internal_links_block_copy_without_fallback() {
    let fixture = Fixture::new("copied");
    std::os::unix::fs::symlink("/tmp", fixture.skill.join("escape")).unwrap();
    let observed = observe(&fixture.target, &fixture.repository, &fixture.library);
    let plan = plan(&fixture.repository, &fixture.library, &observed);
    assert!(
        plan.items()
            .iter()
            .any(|item| { item.action() == Action::Replace && item.safety() == Safety::Blocked })
    );
    assert!(
        !fixture
            .target
            .root()
            .join(".agents/skills/release-checklist")
            .exists()
    );
}

#[test]
fn apply_revalidates_destination_without_replanning() {
    let fixture = Fixture::new("linked");
    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    let destination = fixture
        .target
        .root()
        .join(".agents/skills/release-checklist");
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    std::fs::write(&destination, "arrived after planning").unwrap();

    let result = execute(
        prepared,
        Authorization::AllGuarded,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "arrived after planning"
    );
    assert!(
        result
            .outcomes()
            .iter()
            .any(|outcome| { outcome.path == destination && outcome.outcome == Outcome::Blocked })
    );
}

#[test]
fn copied_source_change_after_snapshot_blocks_staged_operation() {
    let fixture = Fixture::new("copied");
    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    std::fs::write(fixture.skill.join("new-content"), "changed").unwrap();

    let result = execute(
        prepared,
        Authorization::SafeOnly,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );
    assert!(
        result
            .outcomes()
            .iter()
            .any(|outcome| outcome.action == Action::Copy && outcome.outcome == Outcome::Blocked)
    );
    assert!(
        !fixture
            .target
            .root()
            .join(".agents/skills/release-checklist")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn symlinked_skill_directory_blocks_every_operation_even_with_force() {
    let fixture = Fixture::new("linked");
    let root = fixture.target.root().join(".agents/skills");
    let outside = fixture._home.path().join("outside");
    std::fs::create_dir_all(root.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, &root).unwrap();

    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    assert!(
        prepared
            .plan()
            .items()
            .iter()
            .all(|item| item.safety() == Safety::Blocked)
    );
    let result = execute(
        prepared,
        Authorization::AllGuarded,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );

    assert!(
        result
            .outcomes()
            .iter()
            .all(|item| item.outcome == Outcome::Blocked)
    );
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn apply_time_ancestor_symlink_change_cannot_redirect_writes_outside_target() {
    let fixture = Fixture::new("linked");
    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    let outside = fixture._home.path().join("outside-race");
    std::fs::create_dir_all(fixture.target.root().join(".agents")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, fixture.target.root().join(".agents/skills")).unwrap();

    let result = execute(
        prepared,
        Authorization::AllGuarded,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );

    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
    assert!(
        result
            .outcomes()
            .iter()
            .all(|item| item.outcome != Outcome::Applied)
    );
}

#[cfg(unix)]
#[test]
fn recovery_never_traverses_a_symlinked_skill_directory() {
    let fixture = Fixture::new("linked");
    let root = fixture.target.root().join(".agents/skills");
    let outside = fixture._home.path().join("outside-recovery");
    std::fs::create_dir_all(root.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let encoded = "release-checklist"
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let artifact = outside.join(format!(".skillator-stage-1-1-{encoded}"));
    std::fs::write(&artifact, "preserve").unwrap();
    std::os::unix::fs::symlink(&outside, &root).unwrap();

    let prepared = prepare_apply(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    drop(prepared);

    assert_eq!(std::fs::read_to_string(artifact).unwrap(), "preserve");
}

#[test]
fn apply_blocks_when_git_protection_changes_after_planning() {
    let fixture = Fixture::new("linked");
    let destination = fixture
        .target
        .root()
        .join(".agents/skills/release-checklist");
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    std::fs::write(&destination, "index placeholder").unwrap();
    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    support::git(
        fixture.target.root(),
        &["add", "-N", ".agents/skills/release-checklist"],
    );

    let result = execute(
        prepared,
        Authorization::AllGuarded,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );

    assert_eq!(
        std::fs::read_to_string(destination).unwrap(),
        "index placeholder"
    );
    assert!(result.outcomes().iter().any(|item| {
        item.outcome == Outcome::Blocked && item.message.contains("Git facts changed")
    }));
}

#[test]
fn staged_deletion_is_git_protected_before_planning_and_force_preserves_content() {
    let fixture = Fixture::new("linked");
    let destination = fixture
        .target
        .root()
        .join(".agents/skills/release-checklist");
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    std::fs::write(&destination, "user content").unwrap();
    support::git(
        fixture.target.root(),
        &["add", ".agents/skills/release-checklist"],
    );
    support::git(fixture.target.root(), &["config", "user.name", "Test User"]);
    support::git(
        fixture.target.root(),
        &["config", "user.email", "test@example.invalid"],
    );
    support::git(fixture.target.root(), &["commit", "-m", "track skill"]);
    support::git(
        fixture.target.root(),
        &["rm", "--cached", ".agents/skills/release-checklist"],
    );

    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    assert!(
        prepared
            .plan()
            .items()
            .iter()
            .any(|item| { item.action() == Action::Replace && item.safety() == Safety::Blocked })
    );
    let result = execute(
        prepared,
        Authorization::AllGuarded,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );

    assert_eq!(
        std::fs::read_to_string(destination).unwrap(),
        "user content"
    );
    assert!(result.outcomes().iter().any(|outcome| {
        outcome.action == Action::Replace && outcome.outcome == Outcome::Blocked
    }));
}

#[test]
fn staged_deleted_control_file_is_blocked_when_missing_or_modified() {
    for missing in [true, false] {
        let fixture = Fixture::new("linked");
        let root = fixture.target.root().join(".agents/skills");
        std::fs::create_dir_all(&root).unwrap();
        let control = root.join(".gitignore");
        std::fs::write(&control, "*\n!.gitignore\n").unwrap();
        support::git(
            fixture.target.root(),
            &["add", "-f", ".agents/skills/.gitignore"],
        );
        support::git(fixture.target.root(), &["config", "user.name", "Test User"]);
        support::git(
            fixture.target.root(),
            &["config", "user.email", "test@example.invalid"],
        );
        support::git(fixture.target.root(), &["commit", "-m", "track control"]);
        support::git(
            fixture.target.root(),
            &["rm", "--cached", ".agents/skills/.gitignore"],
        );
        if missing {
            std::fs::remove_file(&control).unwrap();
        } else {
            std::fs::write(&control, "user rules\n").unwrap();
        }

        let prepared =
            prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();

        assert!(prepared.plan().items().iter().any(|item| {
            item.action() == Action::WriteControlFile && item.safety() == Safety::Blocked
        }));
    }
}

#[cfg(unix)]
#[test]
fn linked_source_replacement_with_a_symlink_is_blocked() {
    let fixture = Fixture::new("linked");
    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    let replacement = fixture._home.path().join("replacement-source");
    std::fs::create_dir_all(&replacement).unwrap();
    std::fs::write(
        replacement.join("SKILL.md"),
        "---\nname: release-checklist\ndescription: replacement\n---\n",
    )
    .unwrap();
    std::fs::remove_dir_all(&fixture.skill).unwrap();
    std::os::unix::fs::symlink(&replacement, &fixture.skill).unwrap();

    let result = execute(
        prepared,
        Authorization::SafeOnly,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );

    assert!(
        result
            .outcomes()
            .iter()
            .any(|item| item.action == Action::Link && item.outcome == Outcome::Blocked)
    );
    assert!(
        !fixture
            .target
            .root()
            .join(".agents/skills/release-checklist")
            .exists()
    );
}

#[test]
fn linked_source_frontmatter_name_change_after_planning_is_blocked() {
    let fixture = Fixture::new("linked");
    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    std::fs::write(
        fixture.skill.join("SKILL.md"),
        "---\nname: renamed-skill\ndescription: Changed\n---\n",
    )
    .unwrap();

    let result = execute(
        prepared,
        Authorization::SafeOnly,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );

    assert!(
        result.outcomes().iter().any(|outcome| {
            outcome.action == Action::Link && outcome.outcome == Outcome::Blocked
        })
    );
    assert!(
        !fixture
            .target
            .root()
            .join(".agents/skills/release-checklist")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn disabling_an_in_sync_materialization_plans_safe_removal() {
    let fixture = Fixture::new("linked");
    execute(
        prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap(),
        Authorization::SafeOnly,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );
    support::git(
        fixture.target.root(),
        &["add", "-f", ".agents/skills/.gitignore"],
    );
    let staged = fixture.repository.with_enablements(Vec::new()).unwrap();

    let prepared = prepare_transition(
        &fixture.target,
        &fixture.repository,
        &staged,
        &fixture.library,
    )
    .unwrap();

    assert!(
        prepared.plan().items().iter().any(|item| {
            item.action() == Action::RemoveUnmanaged && item.safety() == Safety::Safe
        })
    );
}

struct Fixture {
    _home: support::TestHome,
    target: Target,
    skill: std::path::PathBuf,
    repository: RepositoryConfig,
    library: LibrarySnapshot,
}

impl Fixture {
    fn new(materialization: &str) -> Self {
        let home = support::TestHome::new();
        let library_root = home.path().join("library");
        let skill = library_root.join("release-checklist");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: release-checklist\ndescription: Prepare a release\n---\n",
        )
        .unwrap();
        let library_yaml = format!(
            "version: 1\nlocations:\n  - path: {}\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: local/library\n        path: .\n        skills:\n          - path: release-checklist\n",
            serde_json::to_string(library_root.to_str().unwrap()).unwrap()
        );
        let LoadResult::Valid(config) = LibraryConfigCodec::parse(library_yaml.as_bytes()) else {
            panic!("valid Library")
        };
        let library = scan_library(
            config.value(),
            &home.library_config(),
            home.path(),
            &BTreeMap::new(),
        );
        let repository_yaml = format!(
            "version: 1\nskill_directories:\n  - key: agents\n    path: .agents/skills\nenablements:\n  - directory: agents\n    skill:\n      source: local/library\n      path: release-checklist\n    materialization: {materialization}\n"
        );
        let LoadResult::Valid(repository) =
            RepositoryConfigCodec::parse(repository_yaml.as_bytes())
        else {
            panic!("valid Repository")
        };
        let target = Target::select(home.git_repo("target")).unwrap();
        Self {
            _home: home,
            target,
            skill,
            repository: repository.value().clone(),
            library,
        }
    }

    fn rescan(&mut self) {
        let library_root = self.skill.parent().unwrap();
        let library_yaml = format!(
            "version: 1\nlocations:\n  - path: {}\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: local/library\n        path: .\n        skills:\n          - path: release-checklist\n",
            serde_json::to_string(library_root.to_str().unwrap()).unwrap()
        );
        let LoadResult::Valid(config) = LibraryConfigCodec::parse(library_yaml.as_bytes()) else {
            panic!("valid Library")
        };
        self.library = scan_library(
            config.value(),
            &self._home.library_config(),
            self._home.path(),
            &BTreeMap::new(),
        );
    }
}
