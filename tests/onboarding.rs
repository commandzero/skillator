mod support;

use skillator::acquisition::LibraryAcquisitionMode;
use skillator::app::{AppPaths, LibraryWorkflow, UserScopeWorkflow};
use skillator::config::{
    Fingerprint, LoadResult, RepositoryConfig, SkillDirectoryConfig, load_library, load_repository,
    save_repository,
};
use skillator::domain::{MaterializationKind, RepositoryRelativePath, SkillDirectoryKey};
use skillator::onboarding::{OnboardingEntryKind, OnboardingError, OnboardingWorkflow};
use skillator::target::{Comparison, observe};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(unix)]
#[test]
fn onboarding_inventories_imports_and_preserves_existing_links() {
    let home = support::TestHome::new();
    let paths = AppPaths::new(home.path().to_owned());
    let global = home.path().join(".agents/skills");
    let physical = global.join("physical-skill");
    std::fs::create_dir_all(&physical).unwrap();
    write_skill_with_description(&physical, "physical-skill", "Imported skill description");
    let linked = home.path().join("external/linked-skill");
    std::fs::create_dir_all(&linked).unwrap();
    write_skill(&linked, "linked-skill");
    let stored_link = std::path::Path::new("../../external/linked-skill");
    std::os::unix::fs::symlink(stored_link, global.join("linked-skill")).unwrap();
    std::fs::write(global.join("notes.txt"), "leave me").unwrap();

    let session = OnboardingWorkflow::load(&paths).unwrap();
    let physical_entry = session
        .entries()
        .iter()
        .find(|entry| entry.name() == "physical-skill")
        .unwrap();
    assert_eq!(physical_entry.kind(), OnboardingEntryKind::Physical);
    assert!(physical_entry.selected_by_default());
    assert_eq!(physical_entry.detail(), "Imported skill description");
    let linked_entry = session
        .entries()
        .iter()
        .find(|entry| entry.name() == "linked-skill")
        .unwrap();
    assert_eq!(linked_entry.kind(), OnboardingEntryKind::Symlink);
    assert!(!linked_entry.selected_by_default());
    assert!(session.entries().iter().any(|entry| {
        entry.name() == "notes.txt"
            && entry.kind() == OnboardingEntryKind::Invalid
            && !entry.selectable()
    }));

    let selected = BTreeSet::from(["physical-skill".to_owned(), "linked-skill".to_owned()]);
    let prepared = OnboardingWorkflow::prepare(&paths, &session, "./library", &selected).unwrap();
    let move_item = prepared
        .review()
        .iter()
        .find(|item| item.action == "Move to Library")
        .unwrap();
    assert_eq!(
        move_item.destination,
        home.path().join(".skillator/library/physical-skill")
    );
    assert!(!move_item.destination.to_string_lossy().contains("/./"));
    assert!(
        prepared
            .review()
            .iter()
            .any(|item| item.action == "Move to Library")
    );
    assert!(
        prepared
            .review()
            .iter()
            .any(|item| item.action == "Register Source")
    );

    OnboardingWorkflow::commit(prepared).unwrap();

    let imported = home.path().join(".skillator/library/physical-skill");
    assert!(imported.is_dir());
    assert_eq!(
        std::fs::read_link(global.join("physical-skill")).unwrap(),
        imported.canonicalize().unwrap()
    );
    assert_eq!(
        std::fs::read_link(global.join("linked-skill")).unwrap(),
        stored_link
    );
    assert_eq!(
        std::fs::read_to_string(global.join("notes.txt")).unwrap(),
        "leave me"
    );
    assert!(!global.join(".gitignore").exists());

    let LoadResult::Valid(library) = load_library(&paths.library_config()).unwrap() else {
        panic!("valid Library Configuration")
    };
    assert_eq!(
        library
            .value()
            .locations()
            .iter()
            .flat_map(|location| location.sources())
            .flat_map(|source| source.skills())
            .count(),
        2
    );
    let LoadResult::Valid(user) = load_repository(&paths.user_config()).unwrap() else {
        panic!("valid User Scope Configuration")
    };
    assert_eq!(user.value().enablements().len(), 2);
    let user_session = UserScopeWorkflow::load(&paths).unwrap();
    let snapshot = LibraryWorkflow::snapshot(&paths, library.value());
    let observed = observe(&user_session.target, &user_session.config, &snapshot);
    assert!(
        observed
            .enablements()
            .all(|enablement| enablement.comparison() == Comparison::InSync)
    );
}

#[test]
fn onboarding_blocks_an_existing_library_destination_without_writes() {
    let home = support::TestHome::new();
    let paths = AppPaths::new(home.path().to_owned());
    let physical = home.path().join(".agents/skills/demo");
    std::fs::create_dir_all(&physical).unwrap();
    write_skill(&physical, "demo");
    let collision = home.path().join(".skillator/library/demo");
    std::fs::create_dir_all(&collision).unwrap();
    std::fs::write(collision.join("mine"), "preserve").unwrap();
    let session = OnboardingWorkflow::load(&paths).unwrap();

    let error = OnboardingWorkflow::prepare(
        &paths,
        &session,
        "./library",
        &BTreeSet::from(["demo".to_owned()]),
    )
    .unwrap_err();

    std::assert_matches!(error, OnboardingError::Invalid(_));
    assert!(physical.is_dir());
    assert_eq!(
        std::fs::read_to_string(collision.join("mine")).unwrap(),
        "preserve"
    );
    assert!(!paths.library_config().exists());
    assert!(!paths.user_config().exists());
}

#[test]
fn onboarding_uses_the_configured_user_directory_key() {
    let home = support::TestHome::new();
    let paths = AppPaths::new(home.path().to_owned());
    let physical = home.path().join(".agents/skills/demo");
    std::fs::create_dir_all(&physical).unwrap();
    write_skill(&physical, "demo");
    let existing = RepositoryConfig::new(
        vec![SkillDirectoryConfig::new(
            SkillDirectoryKey::parse("personal").unwrap(),
            RepositoryRelativePath::parse(".agents/skills").unwrap(),
            Some("Personal".to_owned()),
        )],
        Vec::new(),
    )
    .unwrap();
    save_repository(&paths.user_config(), &existing, &Fingerprint::Absent).unwrap();
    let session = OnboardingWorkflow::load(&paths).unwrap();

    let prepared = OnboardingWorkflow::prepare(
        &paths,
        &session,
        "./library",
        &BTreeSet::from(["demo".to_owned()]),
    )
    .unwrap();

    assert_eq!(
        prepared.user_config().enablements()[0].directory().as_str(),
        "personal"
    );
}

#[test]
fn onboarding_copy_keeps_the_user_skill_and_records_a_copied_materialization() {
    let home = support::TestHome::new();
    let paths = AppPaths::new(home.path().to_owned());
    let physical = home.path().join(".agents/skills/demo");
    std::fs::create_dir_all(&physical).unwrap();
    write_skill(&physical, "demo");
    let session = OnboardingWorkflow::load(&paths).unwrap();
    let selected = BTreeMap::from([("demo".to_owned(), LibraryAcquisitionMode::Copy)]);

    let prepared =
        OnboardingWorkflow::prepare_with_modes(&paths, &session, "./library", &selected).unwrap();
    OnboardingWorkflow::commit(prepared).unwrap();

    assert!(physical.is_dir());
    assert!(
        !physical
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(home.path().join(".skillator/library/demo").is_dir());
    let LoadResult::Valid(user) = load_repository(&paths.user_config()).unwrap() else {
        panic!("valid User Scope Configuration")
    };
    assert_eq!(
        user.value().enablements()[0].materialization(),
        MaterializationKind::Copied
    );
}

#[cfg(unix)]
#[test]
fn onboarding_link_keeps_the_user_skill_and_links_it_into_the_library() {
    let home = support::TestHome::new();
    let paths = AppPaths::new(home.path().to_owned());
    let physical = home.path().join(".agents/skills/demo");
    std::fs::create_dir_all(&physical).unwrap();
    write_skill(&physical, "demo");
    let session = OnboardingWorkflow::load(&paths).unwrap();
    let selected = BTreeMap::from([("demo".to_owned(), LibraryAcquisitionMode::Link)]);

    let prepared =
        OnboardingWorkflow::prepare_with_modes(&paths, &session, "./library", &selected).unwrap();
    OnboardingWorkflow::commit(prepared).unwrap();

    let imported = home.path().join(".skillator/library/demo");
    assert!(physical.is_dir());
    assert!(
        !physical
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        imported
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        imported.canonicalize().unwrap(),
        physical.canonicalize().unwrap()
    );
    let LoadResult::Valid(user) = load_repository(&paths.user_config()).unwrap() else {
        panic!("valid User Scope Configuration")
    };
    assert_eq!(
        user.value().enablements()[0].materialization(),
        MaterializationKind::Copied
    );
}

fn write_skill(directory: &std::path::Path, name: &str) {
    write_skill_with_description(directory, name, name);
}

fn write_skill_with_description(directory: &std::path::Path, name: &str, description: &str) {
    std::fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n"),
    )
    .unwrap();
}
