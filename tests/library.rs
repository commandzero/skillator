mod support;

use skillator::config::{LibraryConfigCodec, LoadResult};
use skillator::domain::{SkillKey, SkillPath, SourceKey};
use skillator::library::{SkillValidity, SourceKind, scan_library};
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

    let divergent = library.join("legacy-helper");
    std::fs::create_dir_all(&divergent).unwrap();
    write_skill(&divergent, "different-name", "Legacy helper");

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
    let legacy = local.skill("legacy-helper").unwrap();
    assert_eq!(legacy.validity(), SkillValidity::Valid);
    assert!(legacy.diagnostics().is_empty());
    assert!(
        legacy
            .warnings()
            .iter()
            .any(|warning| warning.contains("does not match directory"))
    );
    let git = snapshot.source("elastic/agent-skills").unwrap();
    assert_eq!(git.kind(), SourceKind::Git);
    assert!(git.skill(".").is_some());
    assert!(git.skill("nested").is_some());
}

#[test]
fn manually_added_skill_is_discovered_without_config_refresh() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let yaml = format!(
        "version: 1\nlocations:\n  - path: {}\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: local/library\n        path: .\n        skills:\n          - path: old-place\n      - key: missing/source\n        path: missing\n        skills:\n          - path: absent\n",
        serde_json::to_string(library.to_str().unwrap()).unwrap()
    );
    let LoadResult::Valid(config) = LibraryConfigCodec::parse(yaml.as_bytes()) else {
        panic!("valid fixture config");
    };

    let before = scan_library(
        config.value(),
        &home.library_config(),
        home.path(),
        &BTreeMap::new(),
    );

    assert!(before.source("missing/source").is_none());
    assert!(
        before
            .source("local/library")
            .unwrap()
            .skill("unslop")
            .is_none()
    );

    std::fs::create_dir_all(library.join("unslop")).unwrap();
    write_skill(&library.join("unslop"), "unslop", "Remove AI writing tells");

    let after = scan_library(
        config.value(),
        &home.library_config(),
        home.path(),
        &BTreeMap::new(),
    );
    let skill = after
        .source("local/library")
        .unwrap()
        .skill("unslop")
        .unwrap();
    assert_eq!(skill.name(), Some("unslop"));
    assert_eq!(skill.description(), Some("Remove AI writing tells"));
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
    assert!(source.skill("linked").is_some());
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
        .source("local/repository-name")
        .unwrap()
        .skill(".")
        .unwrap();
    assert_eq!(skill.name(), Some("portable-skill"));
    assert!(skill.warnings().is_empty());
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
fn legacy_source_inventory_does_not_create_phantom_sources() {
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

    assert_eq!(snapshot.sources().len(), 2);
    assert!(snapshot.source("acme/skills").unwrap().available());
}

#[test]
fn differently_named_registered_skill_keeps_its_registration_identity() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    let divergent = library.join("legacy-helper");
    std::fs::create_dir_all(&divergent).unwrap();
    write_skill(&divergent, "different-name", "Legacy helper");
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

    assert_eq!(skill.validity(), SkillValidity::Valid);
    assert_eq!(skill.path(), "legacy-helper");
    assert_eq!(skill.name(), Some("different-name"));
    assert!(skill.diagnostics().is_empty());
    assert_eq!(skill.warnings().len(), 1);
}

#[test]
fn colliding_source_keys_cannot_resolve_a_skill() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    for name in ["first", "second"] {
        let source = library.join(name);
        std::fs::create_dir_all(&source).unwrap();
        support::git_init(&source);
        support::git(
            &source,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/acme/skills.git",
            ],
        );
        write_skill(&source, "demo", "Demo skill");
    }
    let yaml = format!(
        "version: 1\nlocations:\n  - path: {}\n",
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
    let key = SkillKey::new(
        SourceKey::parse("acme/skills").unwrap(),
        SkillPath::parse(".").unwrap(),
    );

    assert_eq!(
        snapshot
            .sources()
            .filter(|source| source.key() == key.source())
            .count(),
        2
    );
    assert!(snapshot.resolve(&key).is_none());
}

#[test]
fn human_readable_names_are_usable_and_unsafe_or_incomplete_metadata_is_not() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let readable = library.join("bot-ui");
    std::fs::create_dir_all(&readable).unwrap();
    write_skill(&readable, "Make Bot UI", "Build bot interfaces");
    for (directory, name) in [
        ("empty", ""),
        ("dot", "."),
        ("parent", ".."),
        ("slash", "../outside"),
        ("backslash", "some\\path"),
        ("control", "some\u{0007}name"),
    ] {
        let path = library.join(directory);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("SKILL.md"),
            format!(
                "---\nname: {}\ndescription: Example\n---\n",
                serde_json::to_string(name).unwrap()
            ),
        )
        .unwrap();
    }
    let incomplete = library.join("incomplete");
    std::fs::create_dir_all(&incomplete).unwrap();
    write_skill(&incomplete, "Incomplete Skill", "\"\"");
    let malformed = library.join("malformed");
    std::fs::create_dir_all(&malformed).unwrap();
    std::fs::write(malformed.join("SKILL.md"), "not YAML frontmatter").unwrap();
    let missing_name = library.join("missing-name");
    std::fs::create_dir_all(&missing_name).unwrap();
    std::fs::write(
        missing_name.join("SKILL.md"),
        "---\ndescription: No name provided\n---\n",
    )
    .unwrap();
    let yaml = format!(
        "version: 1\nlocations:\n  - path: {}\n",
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
    let source = snapshot.source("local/library").unwrap();
    let readable = source.skill("bot-ui").unwrap();
    assert_eq!(readable.name(), Some("Make Bot UI"));
    assert_eq!(readable.validity(), SkillValidity::Valid);
    assert!(readable.diagnostics().is_empty());
    assert_eq!(readable.warnings().len(), 2);
    for directory in [
        "empty",
        "dot",
        "parent",
        "slash",
        "backslash",
        "control",
        "incomplete",
        "malformed",
        "missing-name",
    ] {
        let skill = source.skill(directory).unwrap();
        assert_eq!(skill.validity(), SkillValidity::Invalid, "{directory}");
        assert!(!skill.diagnostics().is_empty(), "{directory}");
    }
}

#[test]
fn owned_replica_preserves_sources_and_resolves_original_paths_from_either_location() {
    let home = support::TestHome::new();
    let library = home.path().join(".skillator/library");
    let replica = library.join("replica");
    mark_replica(&replica);
    write_replica_skill(&replica, "local/library", "demo", "demo");
    write_replica_skill(&replica, "alpha/tools", "nested/helper", "helper");
    write_replica_skill(&replica, "beta/tools", ".", "portable-root");
    write_replica_skill(
        &replica,
        "company/team/collection",
        "tools/_skills/helper",
        "helper",
    );

    for location in [&library, &replica] {
        let snapshot = scan_fixture_library(&home, location, &[]);
        assert_eq!(snapshot.sources().len(), 4);
        assert!(snapshot.diagnostics().is_empty());
        for (source_key, skill_path) in [
            ("local/library", "demo"),
            ("alpha/tools", "nested/helper"),
            ("beta/tools", "."),
            ("company/team/collection", "tools/_skills/helper"),
        ] {
            let source = snapshot.source(source_key).unwrap();
            assert_eq!(source.key().as_str(), source_key);
            assert_eq!(source.suggested_key().as_str(), source_key);
            assert_eq!(source.kind(), SourceKind::Local);
            assert!(!source.key_collision());
            let source_root = replica
                .join(source_key)
                .join("_skills")
                .canonicalize()
                .unwrap();
            assert_eq!(source.root(), Some(source_root.as_path()));
            let key = SkillKey::new(
                SourceKey::parse(source_key).unwrap(),
                SkillPath::parse(skill_path).unwrap(),
            );
            let skill = snapshot.resolve(&key).unwrap();
            assert_eq!(skill.path(), skill_path);
            let expected = if skill_path == "." {
                source_root
            } else {
                source_root.join(skill_path)
            };
            assert_eq!(skill.absolute_path(), Some(expected.as_path()));
            assert!(skill.absolute_path().unwrap().join("SKILL.md").is_file());
        }
    }
}

#[test]
fn replica_skills_are_not_flattened_into_an_enclosing_local_source() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    let replica = library.join("nested/replica");
    mark_replica(&replica);
    std::fs::create_dir_all(library.join("own")).unwrap();
    write_skill(&library.join("own"), "own", "Local skill");
    write_replica_skill(&replica, "acme/tools", "demo", "demo");

    let snapshot = scan_fixture_library(&home, &library, &[]);
    assert_eq!(snapshot.sources().len(), 2);
    let local = snapshot.source("local/library").unwrap();
    assert_eq!(local.skills().count(), 1);
    assert!(local.skill("own").is_some());
    assert!(
        snapshot
            .source("acme/tools")
            .unwrap()
            .skill("demo")
            .is_some()
    );
}

#[test]
fn unmarked_replica_shaped_folders_remain_ordinary_local_content() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    write_replica_skill(&library.join("replica"), "acme/tools", "demo", "demo");

    let snapshot = scan_fixture_library(&home, &library, &[]);
    assert_eq!(snapshot.sources().len(), 1);
    assert!(snapshot.source("acme/tools").is_none());
    assert!(
        snapshot
            .source("local/library")
            .unwrap()
            .skill("replica/acme/tools/_skills/demo")
            .is_some()
    );
}

#[test]
fn replica_sources_respect_location_exclusions() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    let replica = library.join("replica");
    mark_replica(&replica);
    write_replica_skill(&replica, "acme/tools", "kept", "kept");
    write_replica_skill(&replica, "acme/tools", "private/hidden", "hidden");
    write_replica_skill(&replica, "excluded/tools", "demo", "demo");

    for (location, prefix) in [(&library, "replica/"), (&replica, "")] {
        let exclusions = [
            format!("{prefix}excluded/"),
            format!("{prefix}acme/tools/_skills/private/"),
        ];
        let snapshot = scan_fixture_library(&home, location, &exclusions);
        assert_eq!(snapshot.sources().len(), 1);
        let source = snapshot.source("acme/tools").unwrap();
        assert_eq!(source.skills().count(), 1);
        assert!(source.skill("kept").is_some());
        assert!(source.skill("private/hidden").is_none());
        assert!(snapshot.source("excluded/tools").is_none());
    }
    let snapshot = scan_fixture_library(&home, &library, &["replica/".to_owned()]);
    assert!(snapshot.source("acme/tools").is_none());
    assert!(
        snapshot
            .sources()
            .all(|source| source.skills().count() == 0)
    );
}

#[test]
fn replica_collisions_with_nonempty_local_or_git_sources_cannot_resolve() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    let replica = library.join("replica");
    mark_replica(&replica);
    std::fs::create_dir_all(library.join("own")).unwrap();
    write_skill(&library.join("own"), "own", "Local skill");
    write_replica_skill(&replica, "local/library", "demo", "demo");
    write_replica_skill(&replica, "acme/tools", "demo", "demo");
    let git = library.join("checkout");
    std::fs::create_dir_all(git.join("demo")).unwrap();
    support::git_init(&git);
    support::git(
        &git,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/tools.git",
        ],
    );
    write_skill(&git.join("demo"), "demo", "Git skill");

    let snapshot = scan_fixture_library(&home, &library, &[]);
    for source_key in ["local/library", "acme/tools"] {
        assert_eq!(
            snapshot
                .sources()
                .filter(|source| source.key().as_str() == source_key)
                .count(),
            2
        );
        let key = SkillKey::new(
            SourceKey::parse(source_key).unwrap(),
            SkillPath::parse("demo").unwrap(),
        );
        assert!(snapshot.resolve(&key).is_none());
    }
    assert_eq!(
        snapshot
            .diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code == "source_key_collision")
            .count(),
        2
    );
}

#[test]
fn invalid_replica_markers_report_diagnostics_without_flattening_skills() {
    let home = support::TestHome::new();
    let library = home.path().join("library");
    for name in ["wrong-content", "marker-directory"] {
        let replica = library.join(name);
        write_replica_skill(&replica, "acme/tools", "demo", "demo");
        let marker = replica.join(".skillator-rsync-owned");
        if name == "wrong-content" {
            std::fs::write(marker, "skillator library rsync replica v1").unwrap();
        } else {
            std::fs::create_dir(marker).unwrap();
        }
        for location in [&library, &replica] {
            let snapshot = scan_fixture_library(&home, location, &[]);
            assert!(
                snapshot
                    .sources()
                    .all(|source| source.skills().count() == 0)
            );
            assert!(
                snapshot
                    .diagnostics()
                    .iter()
                    .any(|diagnostic| { diagnostic.code == "invalid_replica_marker" })
            );
        }
    }
}

#[test]
fn replica_container_discovery_prunes_git_metadata() {
    let home = support::TestHome::new();
    let replica = home.path().join("replica");
    mark_replica(&replica);
    write_replica_skill(&replica, "acme/tools", "kept", "kept");
    write_replica_skill(&replica.join(".git"), "Invalid/tools", "hidden", "hidden");
    write_replica_skill(
        &replica.join("nested/.git"),
        "other/tools",
        "hidden",
        "hidden",
    );
    let snapshot = scan_fixture_library(&home, &replica, &[]);
    assert!(snapshot.diagnostics().is_empty());
    assert!(
        snapshot
            .source("acme/tools")
            .unwrap()
            .skill("kept")
            .is_some()
    );
    assert!(snapshot.source("other/tools").is_none());
}

#[test]
fn invalid_replica_source_keys_are_diagnostic() {
    let home = support::TestHome::new();
    let replica = home.path().join("replica");
    mark_replica(&replica);
    write_replica_skill(&replica, "Invalid/tools", "demo", "demo");

    let snapshot = scan_fixture_library(&home, &replica, &[]);
    assert_eq!(snapshot.sources().len(), 0);
    assert!(
        snapshot
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code == "invalid_replica_source" })
    );
}

#[cfg(unix)]
#[test]
fn replica_discovery_does_not_follow_symlinked_containers_or_markers() {
    use std::os::unix::fs::symlink;

    let home = support::TestHome::new();
    let library = home.path().join("library");
    let replica = library.join("replica");
    mark_replica(&replica);
    write_replica_skill(&replica, "acme/tools", "kept", "kept");
    let outside = home.path().join("outside");
    write_replica_skill(&outside, "other/tools", "demo", "demo");
    symlink(outside.join("other"), replica.join("linked")).unwrap();
    std::fs::create_dir_all(replica.join("other/tools")).unwrap();
    symlink(
        outside.join("other/tools/_skills"),
        replica.join("other/tools/_skills"),
    )
    .unwrap();
    symlink(
        outside.join("other/tools/_skills/demo"),
        replica.join("acme/tools/_skills/linked"),
    )
    .unwrap();
    let invalid = library.join("invalid");
    write_replica_skill(&invalid, "unsafe/tools", "demo", "demo");
    symlink(
        replica.join(".skillator-rsync-owned"),
        invalid.join(".skillator-rsync-owned"),
    )
    .unwrap();
    symlink(&replica, library.join("replica-link")).unwrap();

    let snapshot = scan_fixture_library(&home, &library, &[]);
    assert_eq!(snapshot.sources().len(), 1);
    let source = snapshot.source("acme/tools").unwrap();
    assert_eq!(source.skills().count(), 1);
    assert!(source.skill("kept").is_some());
    assert!(
        snapshot
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code == "invalid_replica_marker" })
    );
}

fn mark_replica(root: &std::path::Path) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(
        root.join(".skillator-rsync-owned"),
        "skillator library rsync replica v1\n",
    )
    .unwrap();
}

fn write_replica_skill(root: &std::path::Path, source_key: &str, path: &str, name: &str) {
    let source_root = root.join(source_key).join("_skills");
    let directory = if path == "." {
        source_root
    } else {
        source_root.join(path)
    };
    std::fs::create_dir_all(&directory).unwrap();
    write_skill(&directory, name, "Replica skill");
}

fn scan_fixture_library(
    home: &support::TestHome,
    location: &std::path::Path,
    exclusions: &[String],
) -> skillator::library::LibrarySnapshot {
    let yaml = format!(
        "version: 1\nlocations:\n  - path: {}\n    exclusions: {}\n",
        serde_json::to_string(location.to_str().unwrap()).unwrap(),
        serde_json::to_string(exclusions).unwrap(),
    );
    let LoadResult::Valid(config) = LibraryConfigCodec::parse(yaml.as_bytes()) else {
        panic!("valid fixture config");
    };
    scan_library(
        config.value(),
        &home.library_config(),
        home.path(),
        &BTreeMap::new(),
    )
}

fn write_skill(directory: &std::path::Path, name: &str, description: &str) {
    std::fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n"),
    )
    .unwrap();
}
