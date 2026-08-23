use skillator::config::{
    Fingerprint, LibraryConfig, LibraryConfigCodec, LoadResult, RepositoryConfig,
    RepositoryConfigCodec, SaveError, load_repository, save_repository,
};

const REPOSITORY_YAML: &str = r#"version: 1
skill_directories:
  - key: "claude"
    path: ".claude/skills"
    label: "Claude Code"
  - key: "agents"
    path: ".agents/skills"
enablements:
  - directory: "claude"
    skill:
      source: "elastic/agent-skills"
      path: "release/checklist"
    materialization: "copied"
"#;

#[test]
fn repository_configuration_parses_and_renders_canonically() {
    let parsed = RepositoryConfigCodec::parse(REPOSITORY_YAML.as_bytes());
    let LoadResult::Valid(loaded) = parsed else {
        panic!("expected valid configuration: {parsed:?}");
    };

    assert_eq!(loaded.value().skill_directories().len(), 2);
    assert_eq!(
        RepositoryConfigCodec::render(loaded.value()).unwrap(),
        "version: 1\nagents:\n  path: \".agents/skills\"\n  skills: {}\nclaude:\n  path: \".claude/skills\"\n  label: \"Claude Code\"\n  skills:\n    \"checklist\":\n      source: \"elastic/agent-skills\"\n      path: \"release/checklist\"\n      type: \"copied\"\n"
    );
}

#[test]
fn target_keyed_repository_configuration_parses() {
    let input = br#"version: 1
agents:
  path: ".agents/skills"
  skills:
    unslop:
      source: "local/library"
"#;

    let LoadResult::Valid(loaded) = RepositoryConfigCodec::parse(input) else {
        panic!("expected target-keyed configuration to parse");
    };
    assert_eq!(loaded.value().skill_directories().len(), 1);
    assert_eq!(loaded.value().enablements().len(), 1);
}

#[test]
fn repository_validation_rejects_root_level_skill_directories() {
    let input = br#"version: 1
skill_directories:
  - key: skills
    path: "skills"
enablements: []
"#;

    let LoadResult::Invalid { issues } = RepositoryConfigCodec::parse(input) else {
        panic!("expected root-level Skill Directory to be rejected");
    };
    assert!(issues.iter().any(|issue| {
        issue.message.contains("must be nested") && issue.path == "skill_directories.skills"
    }));
}

#[test]
fn strict_yaml_constructs_and_unknown_fields_are_rejected() {
    for input in [
        "version: 1\nversion: 1\nskill_directories: []\nenablements: []\n",
        "version: 1\nskill_directories: &dirs []\nenablements: *dirs\n",
        "version: 1\nskill_directories: !!seq []\nenablements: []\n",
        "version: 1\n<<: {}\nskill_directories: []\nenablements: []\n",
        "version: 1\nskill_directories: []\nenablements: []\n---\nversion: 1\n",
        "version: 1\nskill_directories: []\nenablements: []\nsurprise: true\n",
    ] {
        assert!(
            matches!(
                RepositoryConfigCodec::parse(input.as_bytes()),
                LoadResult::Invalid { .. }
            ),
            "accepted {input:?}"
        );
    }
}

#[test]
fn repository_validation_collects_independent_field_errors() {
    let input = b"version: 1\nskill_directories:\n  - key: Bad_Key\n    path: ../outside\nenablements:\n  - directory: missing\n    skill:\n      source: only-one-segment\n      path: ./bad\n    materialization: magic\n";
    let LoadResult::<RepositoryConfig>::Invalid { issues } = RepositoryConfigCodec::parse(input)
    else {
        panic!("expected invalid configuration");
    };

    assert!(issues.len() >= 5, "issues: {issues:?}");
    assert!(
        issues
            .iter()
            .any(|issue| issue.path == "skill_directories[0].key")
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.path == "skill_directories[0].path")
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.path == "enablements[0].skill.source")
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.path == "enablements[0].skill.path")
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.path == "enablements[0].materialization")
    );
}

#[test]
fn library_configuration_accepts_legacy_inventory_but_renders_locations_only() {
    let input = b"version: 1\nlocations:\n  - path: ~/Development\n    exclusions:\n      - target/\n    allow_overlap: true\n    sources:\n      - key: elastic/agent-skills\n        path: agent-skills\n        skills:\n          - path: nested/release\n";
    let LoadResult::Valid(loaded) = LibraryConfigCodec::parse(input) else {
        panic!("expected valid Library configuration");
    };
    let location = &loaded.value().locations()[0];
    assert_eq!(location.path(), "~/Development");
    assert_eq!(location.exclusions(), ["target/"]);
    assert!(location.allow_overlap());
    assert_eq!(
        LibraryConfigCodec::render(loaded.value()).unwrap(),
        "version: 1\nlocations:\n  - path: \"~/Development\"\n    exclusions:\n      - \"target/\"\n    allow_overlap: true\n"
    );
}

#[test]
fn legacy_source_inventory_does_not_define_library_validity() {
    let parsed = LibraryConfigCodec::parse(
        b"version: 1\nlocations:\n  - path: ./library\n    sources:\n      - key: first/source\n        path: shared\n      - key: second/source\n        path: shared\n",
    );

    let LoadResult::Valid(_) = parsed else {
        panic!("legacy Source inventory must not define Library validity");
    };
}

#[test]
fn unsupported_version_preserves_original_bytes() {
    let input = b"# future\nversion: 17\nanything: goes\n";
    let LoadResult::<RepositoryConfig>::Unsupported { version, bytes } =
        RepositoryConfigCodec::parse(input)
    else {
        panic!("expected unsupported version");
    };
    assert_eq!(version, 17);
    assert_eq!(bytes, input);
}

#[test]
fn library_first_run_is_staged_and_canonical() {
    let config = LibraryConfig::first_run();
    assert_eq!(config.locations()[0].path(), "./library");
    assert_eq!(
        LibraryConfigCodec::render(&config).unwrap(),
        "version: 1\nlocations:\n  - path: \"./library\"\n    exclusions: []\n    allow_overlap: false\n"
    );
}

#[test]
fn conditional_save_refuses_stale_content_and_cleans_staging() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("skillator.yaml");
    let config = RepositoryConfig::empty();

    save_repository(&path, &config, &Fingerprint::Absent).unwrap();
    let LoadResult::Valid(loaded) = load_repository(&path).unwrap() else {
        panic!("expected saved configuration");
    };
    std::fs::write(&path, "externally changed\n").unwrap();

    let error = save_repository(&path, &config, loaded.fingerprint()).unwrap_err();
    std::assert_matches!(error, SaveError::Stale);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "externally changed\n"
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn conditional_save_does_not_replace_an_identical_document() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("skillator.yaml");
    let config = RepositoryConfig::empty();
    let fingerprint = save_repository(&path, &config, &Fingerprint::Absent).unwrap();
    let before = std::fs::metadata(&path).unwrap();

    let returned = save_repository(&path, &config, &fingerprint).unwrap();

    let after = std::fs::metadata(&path).unwrap();
    assert_eq!(returned, fingerprint);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(after.ino(), before.ino());
    }
}
