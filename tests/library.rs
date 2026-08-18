mod support;

use skillator::config::{LibraryConfigCodec, LoadResult};
use skillator::library::{Registration, SkillValidity, SourceKind, scan_library};
use std::collections::BTreeMap;

#[test]
fn library_scan_keeps_one_inventory_across_local_and_git_sources() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    std::fs::create_dir_all(library.join("local-skill")).unwrap();
    write_skill(&library.join("local-skill"), "local-skill", "Local skill");

    let git_source = library.join("agent-skills");
    std::fs::create_dir_all(&git_source).unwrap();
    support::git_init(&git_source);
    support::git(
        &git_source,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:elastic/agent-skills.git",
        ],
    );
    write_skill(&git_source, "agent-skills", "Repository-root skill");
    std::fs::create_dir_all(git_source.join("nested")).unwrap();
    write_skill(&git_source.join("nested"), "nested", "Nested skill");

    let invalid = library.join("legacy-helper");
    std::fs::create_dir_all(&invalid).unwrap();
    write_skill(&invalid, "different-name", "Invalid name");

    let yaml = format!(
        "version: 1\nlocations:\n  - path: {}\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: local/library\n        path: .\n        skills:\n          - path: local-skill\n      - key: elastic/agent-skills\n        path: agent-skills\n        skills:\n          - path: .\n",
        serde_json::to_string(library.to_str().unwrap()).unwrap()
    );
    let LoadResult::Valid(config) = LibraryConfigCodec::parse(yaml.as_bytes()) else {
        panic!("valid fixture config");
    };

    let snapshot = scan_library(
        config.value(),
        &home.library_config(),
        home.path(),
        &BTreeMap::new(),
    );

    assert_eq!(snapshot.sources().len(), 2);
    let local = snapshot.source("local/library").unwrap();
    assert_eq!(local.kind(), SourceKind::Local);
    assert_eq!(
        local.skill("local-skill").unwrap().registration(),
        Registration::Registered
    );
    assert_eq!(
        local.skill("legacy-helper").unwrap().validity(),
        SkillValidity::Invalid
    );
    let git = snapshot.source("elastic/agent-skills").unwrap();
    assert_eq!(git.kind(), SourceKind::Git);
    assert_eq!(
        git.skill(".").unwrap().registration(),
        Registration::Registered
    );
    assert_eq!(
        git.skill("nested").unwrap().registration(),
        Registration::Unregistered
    );
}

#[test]
fn unavailable_and_moved_registrations_remain_visible() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    std::fs::create_dir_all(library.join("new-place")).unwrap();
    write_skill(&library.join("new-place"), "new-place", "Moved skill");
    let yaml = format!(
        "version: 1\nlocations:\n  - path: {}\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: local/library\n        path: .\n        skills:\n          - path: old-place\n      - key: missing/source\n        path: missing\n        skills:\n          - path: absent\n",
        serde_json::to_string(library.to_str().unwrap()).unwrap()
    );
    let LoadResult::Valid(config) = LibraryConfigCodec::parse(yaml.as_bytes()) else {
        panic!("valid fixture config");
    };

    let snapshot = scan_library(
        config.value(),
        &home.library_config(),
        home.path(),
        &BTreeMap::new(),
    );

    assert!(!snapshot.source("missing/source").unwrap().available());
    assert!(
        !snapshot
            .source("local/library")
            .unwrap()
            .skill("old-place")
            .unwrap()
            .available()
    );
    assert_eq!(
        snapshot
            .source("local/library")
            .unwrap()
            .skill("new-place")
            .unwrap()
            .registration(),
        Registration::Unregistered
    );
}

#[test]
fn expressions_exclusions_and_directory_symlinks_are_respected() {
    let home = support::TestHome::new();
    let library = home.path().join("skills");
    std::fs::create_dir_all(library.join("kept")).unwrap();
    std::fs::create_dir_all(library.join("ignored/hidden")).unwrap();
    write_skill(&library.join("kept"), "kept", "Kept");
    write_skill(&library.join("ignored/hidden"), "hidden", "Hidden");
    #[cfg(unix)]
    std::os::unix::fs::symlink(library.join("kept"), library.join("linked")).unwrap();
    let yaml = b"version: 1\nlocations:\n  - path: ${SKILL_ROOT}\n    exclusions:\n      - ignored/\n    allow_overlap: false\n    sources:\n      - key: local/skills\n        path: .\n        skills: []\n";
    let LoadResult::Valid(config) = LibraryConfigCodec::parse(yaml) else {
        panic!("valid fixture config");
    };
    let environment = BTreeMap::from([(
        "SKILL_ROOT".to_owned(),
        library.to_string_lossy().into_owned(),
    )]);

    let snapshot = scan_library(
        config.value(),
        &home.library_config(),
        home.path(),
        &environment,
    );
    let source = snapshot.source("local/skills").unwrap();
    assert!(source.skill("kept").is_some());
    assert!(source.skill("ignored/hidden").is_none());
    assert!(source.skill("linked").is_none());
}

#[test]
fn repository_root_skill_uses_frontmatter_name_independently_of_source_directory() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    let source = library.join("repository-name");
    std::fs::create_dir_all(&source).unwrap();
    support::git_init(&source);
    write_skill(&source, "portable-skill", "Root skill");
    let yaml = format!(
        "version: 1\nlocations:\n  - path: {}\n    sources:\n      - key: local/repository\n        path: repository-name\n        skills:\n          - path: .\n",
        serde_json::to_string(library.to_str().unwrap()).unwrap()
    );
    let LoadResult::Valid(config) = LibraryConfigCodec::parse(yaml.as_bytes()) else {
        panic!("valid fixture config");
    };

    let snapshot = scan_library(
        config.value(),
        &home.library_config(),
        home.path(),
        &BTreeMap::new(),
    );

    let skill = snapshot
        .source("local/repository")
        .unwrap()
        .skill(".")
        .unwrap();
    assert_eq!(skill.name(), Some("portable-skill"));
    assert_eq!(skill.validity(), SkillValidity::Valid);
}

#[test]
fn allowed_overlaps_are_visible_as_advisories() {
    let home = support::TestHome::new();
    let root = home.path().join("library");
    std::fs::create_dir_all(root.join("nested")).unwrap();
    let yaml = format!(
        "version: 1\nlocations:\n  - path: {}\n    allow_overlap: true\n  - path: {}\n    allow_overlap: true\n",
        serde_json::to_string(root.to_str().unwrap()).unwrap(),
        serde_json::to_string(root.join("nested").to_str().unwrap()).unwrap()
    );
    let LoadResult::Valid(config) = LibraryConfigCodec::parse(yaml.as_bytes()) else {
        panic!("valid fixture config");
    };

    let snapshot = scan_library(
        config.value(),
        &home.library_config(),
        home.path(),
        &BTreeMap::new(),
    );

    assert!(
        snapshot
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "overlapping_locations_allowed")
    );
}

#[test]
fn discovered_source_key_collision_preserves_registered_and_discovered_rows() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    let discovered = library.join("different-path");
    std::fs::create_dir_all(&discovered).unwrap();
    support::git_init(&discovered);
    support::git(
        &discovered,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/skills.git",
        ],
    );
    write_skill(&discovered, "root-skill", "Root skill");
    let yaml = format!(
        "version: 1\nlocations:\n  - path: {}\n    sources:\n      - key: acme/skills\n        path: moved-away\n        skills: []\n",
        serde_json::to_string(library.to_str().unwrap()).unwrap()
    );
    let LoadResult::Valid(config) = LibraryConfigCodec::parse(yaml.as_bytes()) else {
        panic!("valid fixture config");
    };

    let snapshot = scan_library(
        config.value(),
        &home.library_config(),
        home.path(),
        &BTreeMap::new(),
    );

    assert_eq!(
        snapshot
            .sources()
            .filter(|source| source.key().as_str() == "acme/skills")
            .count(),
        2
    );
    assert!(!snapshot.source("acme/skills").unwrap().available());
    assert!(snapshot.sources().any(|source| {
        source.key().as_str() == "acme/skills" && source.available() && source.key_collision()
    }));
}

#[test]
fn invalid_registered_skill_keeps_its_registration_identity() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    let invalid = library.join("legacy-helper");
    std::fs::create_dir_all(&invalid).unwrap();
    write_skill(&invalid, "different-name", "Legacy helper");
    let yaml = format!(
        "version: 1\nlocations:\n  - path: {}\n    sources:\n      - key: local/library\n        path: .\n        skills:\n          - path: legacy-helper\n",
        serde_json::to_string(library.to_str().unwrap()).unwrap()
    );
    let LoadResult::Valid(config) = LibraryConfigCodec::parse(yaml.as_bytes()) else {
        panic!("valid fixture config");
    };

    let snapshot = scan_library(
        config.value(),
        &home.library_config(),
        home.path(),
        &BTreeMap::new(),
    );
    let skill = snapshot
        .source("local/library")
        .unwrap()
        .skill("legacy-helper")
        .unwrap();

    assert_eq!(skill.validity(), SkillValidity::Invalid);
    assert_eq!(skill.registration(), Registration::Registered);
}

fn write_skill(directory: &std::path::Path, name: &str, description: &str) {
    std::fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n"),
    )
    .unwrap();
}
