mod support;

use skillator::config::{LibraryConfigCodec, LoadResult, RepositoryConfigCodec};
use skillator::library::scan_library;
use skillator::target::{Comparison, MaterializationState, RootState, Target, observe};
use std::collections::BTreeMap;

#[cfg(unix)]
#[test]
fn linked_materializations_are_compared_to_canonical_absolute_sources() {
    let fixture = Fixture::new("linked");
    let observed = fixture.observe();
    let entry = observed.enablements().next().unwrap();
    assert_eq!(entry.comparison(), Comparison::Drifted);
    assert_eq!(entry.state(), &MaterializationState::Missing);

    let skill_directory = fixture.target.join(".agents/skills");
    std::fs::create_dir_all(&skill_directory).unwrap();
    std::fs::write(
        skill_directory.join(".gitignore"),
        "# Managed by skillator.\n*\n!.gitignore\n",
    )
    .unwrap();
    support::git(&fixture.target, &["add", "-f", ".agents/skills/.gitignore"]);
    std::os::unix::fs::symlink(
        fixture.skill.canonicalize().unwrap(),
        skill_directory.join("release-checklist"),
    )
    .unwrap();

    let observed = fixture.observe();
    let entry = observed.enablements().next().unwrap();
    assert_eq!(entry.comparison(), Comparison::InSync);
    assert_eq!(entry.state(), &MaterializationState::CanonicalLink);

    std::fs::remove_file(skill_directory.join("release-checklist")).unwrap();
    std::os::unix::fs::symlink(
        "../../../library/release-checklist",
        skill_directory.join("release-checklist"),
    )
    .unwrap();
    let entry = fixture.observe().enablements().next().unwrap().clone();
    assert_eq!(entry.comparison(), Comparison::Drifted);
    assert_eq!(entry.state(), &MaterializationState::NoncanonicalLink);
}

#[test]
fn copied_materializations_compare_content_and_executable_state_not_timestamps() {
    let fixture = Fixture::new("copied");
    std::fs::create_dir_all(fixture.skill.join("bin")).unwrap();
    std::fs::write(fixture.skill.join("bin/run"), "run\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            fixture.skill.join("bin/run"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let destination = fixture.target.join(".agents/skills/release-checklist");
    copy_fixture(&fixture.skill, &destination);
    std::fs::write(
        fixture.target.join(".agents/skills/.gitignore"),
        "# Managed by skillator.\n*\n!.gitignore\n",
    )
    .unwrap();
    support::git(&fixture.target, &["add", "-f", ".agents/skills/.gitignore"]);

    let entry = fixture.observe().enablements().next().unwrap().clone();
    assert_eq!(entry.comparison(), Comparison::InSync);
    assert_eq!(entry.state(), &MaterializationState::EquivalentCopy);

    std::fs::write(destination.join("bin/run"), "changed\n").unwrap();
    let entry = fixture.observe().enablements().next().unwrap().clone();
    assert_eq!(entry.comparison(), Comparison::Drifted);
    assert_eq!(entry.state(), &MaterializationState::DivergedCopy);
}

#[test]
fn unresolved_present_copy_is_unverifiable_and_unmanaged_entries_are_reported() {
    let fixture = Fixture::new("copied");
    let root = fixture.target.join(".agents/skills");
    std::fs::create_dir_all(root.join("release-checklist")).unwrap();
    std::fs::write(root.join("release-checklist/SKILL.md"), "unknown").unwrap();
    std::fs::write(root.join("notes"), "mine").unwrap();
    std::fs::remove_dir_all(fixture.home.path().join("library")).unwrap();

    let observed = fixture.observe();
    let entry = observed.enablements().next().unwrap();
    assert_eq!(entry.comparison(), Comparison::Unverifiable);
    assert!(entry.unresolved());
    assert!(
        observed.directories()[0]
            .unmanaged_entries()
            .iter()
            .any(|path| path.ends_with("notes"))
    );
}

#[cfg(unix)]
#[test]
fn broken_misdirected_wrong_kind_and_symlinked_roots_are_distinct_facts() {
    let fixture = Fixture::new("linked");
    let root = fixture.target.join(".agents/skills");
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("release-checklist");
    std::os::unix::fs::symlink("missing", &entry).unwrap();
    assert_eq!(
        fixture.observe().enablements().next().unwrap().state(),
        &MaterializationState::BrokenLink
    );

    std::fs::remove_file(&entry).unwrap();
    let elsewhere = fixture.home.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &entry).unwrap();
    assert_eq!(
        fixture.observe().enablements().next().unwrap().state(),
        &MaterializationState::MisdirectedLink
    );

    std::fs::remove_file(&entry).unwrap();
    std::fs::write(&entry, "not a link").unwrap();
    assert_eq!(
        fixture.observe().enablements().next().unwrap().state(),
        &MaterializationState::WrongKind
    );

    std::fs::remove_file(&entry).unwrap();
    std::fs::remove_dir(&root).unwrap();
    std::os::unix::fs::symlink(fixture.home.path(), &root).unwrap();
    assert_eq!(
        fixture.observe().directories()[0].root_state(),
        RootState::Symlink
    );
}

#[cfg(unix)]
#[test]
fn case_variants_and_links_to_known_skills_remain_unmanaged_duplicates() {
    let fixture = Fixture::new("linked");
    let root = fixture.target.join(".agents/skills");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(
        fixture.skill.canonicalize().unwrap(),
        root.join("Release-Checklist"),
    )
    .unwrap();
    std::fs::write(root.join(".gitignore"), "user rules\n").unwrap();

    let observed = fixture.observe();
    let directory = &observed.directories()[0];
    assert!(
        directory
            .duplicate_entries()
            .iter()
            .any(|path| path.ends_with("Release-Checklist"))
    );
    assert!(
        directory
            .diagnostics()
            .iter()
            .any(|message| message.contains("only by case"))
    );
    assert!(
        directory
            .diagnostics()
            .iter()
            .any(|message| message.contains("not canonical"))
    );
}

#[test]
fn expected_entry_collisions_and_agent_compatibility_overlap_are_reported() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    std::fs::create_dir_all(library.join("release-checklist")).unwrap();
    write_skill(
        &library.join("release-checklist"),
        "release-checklist",
        "First",
    );
    let second = library.join("second");
    support::git_init(&second);
    std::fs::create_dir_all(second.join("release-checklist")).unwrap();
    write_skill(
        &second.join("release-checklist"),
        "release-checklist",
        "Second",
    );
    let library_yaml = format!(
        "version: 1\nlocations:\n  - path: {}\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: local/library\n        path: .\n        skills:\n          - path: release-checklist\n      - key: local/second\n        path: second\n        skills:\n          - path: release-checklist\n",
        serde_json::to_string(library.to_str().unwrap()).unwrap()
    );
    let LoadResult::Valid(library_config) = LibraryConfigCodec::parse(library_yaml.as_bytes())
    else {
        panic!("valid Library config")
    };
    let snapshot = scan_library(
        library_config.value(),
        &home.library_config(),
        home.path(),
        &BTreeMap::new(),
    );
    let repository_yaml = b"version: 1\nskill_directories:\n  - key: agents\n    path: .agents/skills\n  - key: claude\n    path: .claude/skills\nenablements:\n  - directory: agents\n    skill:\n      source: local/library\n      path: release-checklist\n    materialization: linked\n  - directory: agents\n    skill:\n      source: local/second\n      path: release-checklist\n    materialization: linked\n  - directory: claude\n    skill:\n      source: local/library\n      path: release-checklist\n    materialization: linked\n";
    let LoadResult::Valid(repository) = RepositoryConfigCodec::parse(repository_yaml) else {
        panic!("valid Repository config")
    };
    let target = Target::select(home.git_repo("target")).unwrap();

    let observed = observe(&target, repository.value(), &snapshot);
    assert_eq!(
        observed.enablements().next().unwrap().state(),
        &MaterializationState::ExpectedEntryCollision
    );
    assert!(
        observed.directories()[0]
            .diagnostics()
            .iter()
            .any(|message| message.contains("compatibility overlaps"))
    );
    assert!(
        observed.directories()[0]
            .compatible_agents()
            .contains(&"Codex")
    );
}

struct Fixture {
    home: support::TestHome,
    target: std::path::PathBuf,
    skill: std::path::PathBuf,
    materialization: &'static str,
}

impl Fixture {
    fn new(materialization: &'static str) -> Self {
        let home = support::TestHome::new();
        let library = home.path().join("library");
        let skill = library.join("release-checklist");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: release-checklist\ndescription: Prepare a release\n---\n",
        )
        .unwrap();
        let target = home.git_repo("target");
        Self {
            home,
            target,
            skill,
            materialization,
        }
    }

    fn observe(&self) -> skillator::target::ObservedState {
        let library_yaml = format!(
            "version: 1\nlocations:\n  - path: {}\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: local/library\n        path: .\n        skills:\n          - path: release-checklist\n",
            serde_json::to_string(self.home.path().join("library").to_str().unwrap()).unwrap()
        );
        let LoadResult::Valid(library_config) = LibraryConfigCodec::parse(library_yaml.as_bytes())
        else {
            panic!("valid Library config")
        };
        let library = scan_library(
            library_config.value(),
            &self.home.library_config(),
            self.home.path(),
            &BTreeMap::new(),
        );
        let repository_yaml = format!(
            "version: 1\nskill_directories:\n  - key: agents\n    path: .agents/skills\nenablements:\n  - directory: agents\n    skill:\n      source: local/library\n      path: release-checklist\n    materialization: {}\n",
            self.materialization
        );
        let LoadResult::Valid(repository_config) =
            RepositoryConfigCodec::parse(repository_yaml.as_bytes())
        else {
            panic!("valid Repository config")
        };
        observe(
            &Target::select(&self.target).unwrap(),
            repository_config.value(),
            &library,
        )
    }
}

fn copy_fixture(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_fixture(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn write_skill(directory: &std::path::Path, name: &str, description: &str) {
    std::fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n"),
    )
    .unwrap();
}
