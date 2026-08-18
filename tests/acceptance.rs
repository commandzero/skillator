#![cfg(unix)]

mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use skillator::app::{AppPaths, CommandReport, ReportStatus, SyncMode, SyncWorkflow};
use skillator::cli::{ColorPolicy, render_text, render_yaml};
use skillator::config::{
    LibraryConfig, LibraryConfigCodec, LoadResult, RepositoryConfig, RepositoryConfigCodec,
};
use skillator::domain::MaterializationKind;
use skillator::library::{LibrarySnapshot, Registration, SkillValidity, scan_library};
use skillator::reconcile::{Authorization, Outcome, Safety, execute, plan, prepare_check};
use skillator::target::{Comparison, MaterializationState, ObservedState, Target, observe};
use skillator::tui::{Action as TuiAction, CheckState, Model, Overlay, Row, Workspace, reduce};
use std::collections::BTreeMap;
use std::path::PathBuf;

struct AcceptanceFixture {
    home: support::TestHome,
    target_path: PathBuf,
    target: Target,
    library_config: LibraryConfig,
    repository: RepositoryConfig,
    library: LibrarySnapshot,
    observed: ObservedState,
}

impl AcceptanceFixture {
    fn new() -> Self {
        let home = support::TestHome::new();
        let primary = home.path().join("catalog");
        let release = primary.join("release-checklist");
        write_skill(&release, "release-checklist", "Prepare releases");
        let copy_helper = primary.join("copy-helper");
        write_skill(&copy_helper, "copy-helper", "Copy safely");
        std::os::unix::fs::symlink("/tmp", copy_helper.join("absolute-link")).unwrap();
        let invalid = primary.join("legacy-helper");
        write_skill(&invalid, "wrong-name", "Invalid metadata");

        let nested = primary.join("tools");
        support::git_init(&nested);
        support::git(
            &nested,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/acme/tools.git",
            ],
        );
        let esql = nested.join("esql");
        write_skill(&esql, "esql", "Write ESQL");

        let missing = home.path().join("missing-catalog");
        let library_yaml = format!(
            "version: 1\nlocations:\n  - path: {}\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: local/catalog\n        path: .\n        skills:\n          - path: release-checklist\n          - path: copy-helper\n      - key: acme/tools\n        path: tools\n        skills:\n          - path: esql\n  - path: {}\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: missing/source\n        path: .\n        skills:\n          - path: ghost\n",
            serde_json::to_string(primary.to_str().unwrap()).unwrap(),
            serde_json::to_string(missing.to_str().unwrap()).unwrap(),
        );
        let LoadResult::Valid(library_loaded) = LibraryConfigCodec::parse(library_yaml.as_bytes())
        else {
            panic!("valid acceptance Library configuration")
        };
        let library_config = library_loaded.value().clone();
        let library = scan_library(
            &library_config,
            &home.library_config(),
            home.path(),
            &BTreeMap::new(),
        );

        let repository_yaml = b"version: 1\nskill_directories:\n  - key: agents\n    path: .agents/skills\n  - key: claude\n    path: .claude/skills\nenablements:\n  - directory: agents\n    skill:\n      source: local/catalog\n      path: release-checklist\n    materialization: linked\n  - directory: agents\n    skill:\n      source: acme/tools\n      path: esql\n    materialization: copied\n  - directory: claude\n    skill:\n      source: local/catalog\n      path: copy-helper\n    materialization: copied\n  - directory: claude\n    skill:\n      source: missing/source\n      path: ghost\n    materialization: linked\n";
        let LoadResult::Valid(repository_loaded) = RepositoryConfigCodec::parse(repository_yaml)
        else {
            panic!("valid acceptance Repository configuration")
        };
        let repository = repository_loaded.value().clone();

        let target_path = home.git_repo("target");
        let root = target_path.join(".agents/skills");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".gitignore"),
            "# Managed by skillator.\n*\n!.gitignore\n",
        )
        .unwrap();
        support::git(&target_path, &["add", "-f", ".agents/skills/.gitignore"]);
        std::os::unix::fs::symlink(
            release.canonicalize().unwrap(),
            root.join("release-checklist"),
        )
        .unwrap();
        let copied = root.join("esql");
        std::fs::create_dir_all(&copied).unwrap();
        std::fs::write(
            copied.join("SKILL.md"),
            "---\nname: esql\ndescription: locally edited\n---\n",
        )
        .unwrap();
        std::fs::write(root.join("notes"), "unmanaged").unwrap();
        let encoded = "orphan"
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        std::fs::write(
            root.join(format!(".skillator-backup-1-1-{encoded}")),
            "recovery",
        )
        .unwrap();

        std::fs::create_dir_all(home.library_config().parent().unwrap()).unwrap();
        std::fs::write(home.library_config(), library_yaml).unwrap();
        std::fs::create_dir_all(target_path.join(".agents")).unwrap();
        std::fs::write(target_path.join(".agents/skillator.yaml"), repository_yaml).unwrap();

        let target = Target::select(&target_path).unwrap();
        let observed = observe(&target, &repository, &library);
        Self {
            home,
            target_path,
            target,
            library_config,
            repository,
            library,
            observed,
        }
    }
}

#[test]
fn wayfinder_01_discovers_multiple_locations_and_sources() {
    let fixture = AcceptanceFixture::new();
    assert_eq!(fixture.library_config.locations().len(), 2);
    assert!(fixture.library.sources().len() >= 3);
}

#[test]
fn wayfinder_02_keeps_registered_valid_skills_available() {
    let fixture = AcceptanceFixture::new();
    let source = fixture.library.source("local/catalog").unwrap();
    assert!(source.skills().any(|skill| {
        skill.path() == "release-checklist"
            && skill.registration() == Registration::Registered
            && skill.validity() == SkillValidity::Valid
    }));
}

#[test]
fn wayfinder_03_surfaces_invalid_skills_without_registering_them() {
    let fixture = AcceptanceFixture::new();
    let source = fixture.library.source("local/catalog").unwrap();
    assert!(source.skills().any(|skill| {
        skill.path() == "legacy-helper" && skill.validity() == SkillValidity::Invalid
    }));
}

#[test]
fn wayfinder_04_preserves_unavailable_registered_sources() {
    let fixture = AcceptanceFixture::new();
    let source = fixture.library.source("missing/source").unwrap();
    assert_eq!(source.registration(), Registration::Registered);
    assert!(!source.available());
}

#[test]
fn wayfinder_05_recognizes_a_canonical_link() {
    let fixture = AcceptanceFixture::new();
    assert!(fixture.observed.enablements().any(|entry| {
        entry.state() == &MaterializationState::CanonicalLink
            && entry.comparison() == Comparison::InSync
    }));
}

#[test]
fn wayfinder_06_detects_a_diverged_copy() {
    let fixture = AcceptanceFixture::new();
    assert!(
        fixture
            .observed
            .enablements()
            .any(|entry| entry.state() == &MaterializationState::DivergedCopy)
    );
}

#[test]
fn wayfinder_07_blocks_copy_ineligible_source_content() {
    let fixture = AcceptanceFixture::new();
    assert!(
        fixture
            .observed
            .enablements()
            .any(|entry| entry.state() == &MaterializationState::CopyIneligible)
    );
}

#[test]
fn wayfinder_08_reports_unmanaged_entries() {
    let fixture = AcceptanceFixture::new();
    assert!(
        fixture.observed.directories()[0]
            .unmanaged_entries()
            .iter()
            .any(|path| path.ends_with("notes"))
    );
}

#[test]
fn wayfinder_09_reports_recovery_artifacts() {
    let fixture = AcceptanceFixture::new();
    assert!(
        !fixture.observed.directories()[0]
            .recovery_artifacts()
            .is_empty()
    );
}

#[test]
fn wayfinder_10_aggregates_directory_drift() {
    let fixture = AcceptanceFixture::new();
    assert_eq!(
        fixture.observed.directories()[0].comparison(),
        Comparison::Drifted
    );
}

#[test]
fn wayfinder_11_plans_blocked_and_guarded_work_independently() {
    let fixture = AcceptanceFixture::new();
    let result = plan(&fixture.repository, &fixture.library, &fixture.observed);
    assert!(
        result
            .items()
            .iter()
            .any(|item| item.safety() == Safety::Blocked)
    );
    assert!(
        result
            .items()
            .iter()
            .any(|item| item.safety() == Safety::Guarded)
    );
}

#[test]
fn wayfinder_12_check_holds_a_lock_without_mutating_entries() {
    let fixture = AcceptanceFixture::new();
    let before = std::fs::read(fixture.target_path.join(".agents/skills/notes")).unwrap();
    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    assert_eq!(
        prepared.plan().items().len(),
        plan(&fixture.repository, &fixture.library, &fixture.observed)
            .items()
            .len()
    );
    drop(prepared);
    assert_eq!(
        std::fs::read(fixture.target_path.join(".agents/skills/notes")).unwrap(),
        before
    );
}

#[test]
fn wayfinder_13_safe_only_apply_preserves_guarded_content() {
    let fixture = AcceptanceFixture::new();
    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
    let result = execute(
        prepared,
        Authorization::SafeOnly,
        &fixture.target,
        &fixture.repository,
        &fixture.library,
    );
    assert!(fixture.target_path.join(".agents/skills/notes").exists());
    assert!(
        result
            .outcomes()
            .iter()
            .any(|item| item.outcome == Outcome::NotAuthorized)
    );
}

#[test]
fn wayfinder_14_force_authorizes_viable_guarded_changes() {
    let fixture = AcceptanceFixture::new();
    let prepared = prepare_check(&fixture.target, &fixture.repository, &fixture.library).unwrap();
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
            .any(|item| item.safety == Safety::Guarded && item.outcome == Outcome::Applied)
    );
}

#[test]
fn wayfinder_15_does_not_modify_the_git_index() {
    let fixture = AcceptanceFixture::new();
    let before = support::git_output(&fixture.target_path, &["diff", "--cached", "--name-only"]);
    let _ = SyncWorkflow::run(
        &AppPaths::new(fixture.home.path().to_owned()),
        &fixture.target_path,
        SyncMode::Check,
    )
    .unwrap();
    let after = support::git_output(&fixture.target_path, &["diff", "--cached", "--name-only"]);
    assert_eq!(before, after);
}

#[test]
fn wayfinder_16_resolves_a_nested_input_to_the_target_root() {
    let fixture = AcceptanceFixture::new();
    let nested = fixture.target_path.join("src/deeper");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(
        Target::select(nested).unwrap().root(),
        fixture.target.root()
    );
}

#[test]
fn wayfinder_17_emits_machine_json_without_diagnostics_on_stderr() {
    let fixture = AcceptanceFixture::new();
    Command::cargo_bin("skillator")
        .unwrap()
        .current_dir(&fixture.target_path)
        .env("HOME", fixture.home.path())
        .args(["sync", "--check", "--format=json"])
        .assert()
        .code(1)
        .stdout(predicate::str::starts_with("{"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn wayfinder_18_json_and_yaml_share_one_logical_report() {
    let report = CommandReport {
        format_version: 1,
        status: ReportStatus::InSync,
        exit_status: 0,
        mode: "check".to_owned(),
        target: "yes/null\npath".to_owned(),
        changes: Vec::new(),
        diagnostics: Vec::new(),
    };
    let json = serde_json::to_value(&report).unwrap();
    let yaml: serde_json::Value = serde_saphyr::from_str(&render_yaml(&report).unwrap()).unwrap();
    assert_eq!(json, yaml);
}

#[test]
fn wayfinder_19_text_output_is_compact() {
    let report = CommandReport {
        format_version: 1,
        status: ReportStatus::InSync,
        exit_status: 0,
        mode: "check".to_owned(),
        target: "/target".to_owned(),
        changes: Vec::new(),
        diagnostics: Vec::new(),
    };
    assert_eq!(render_text(&report, ColorPolicy::Never), "In sync.\n");
}

#[test]
fn wayfinder_20_prompts_before_discarding_staged_tui_edits() {
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::skill(
            "local/catalog",
            "release-checklist",
            "Prepare releases",
            false,
            true,
            MaterializationKind::Linked,
            "Disabled",
        )],
    );
    reduce(&mut model, TuiAction::Toggle);
    reduce(&mut model, TuiAction::ToggleWorkspace);
    assert_eq!(model.overlay(), &Overlay::DiscardWorkspace);
    assert_eq!(model.rows()[0].check(), Some(CheckState::Checked));
}

fn write_skill(directory: &std::path::Path, name: &str, description: &str) {
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n"),
    )
    .unwrap();
}
