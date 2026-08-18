use skillator::domain::{
    Enablement, MaterializationKind, RepositoryRelativePath, SkillDirectoryKey, SkillKey,
    SkillPath, SourceKey,
};

#[test]
fn canonical_domain_values_form_an_enablement() {
    let source = SourceKey::parse("elastic/agent-skills").unwrap();
    let skill = SkillKey::new(source, SkillPath::parse("release/checklist").unwrap());
    let enablement = Enablement::new(
        SkillDirectoryKey::parse("agents").unwrap(),
        skill,
        MaterializationKind::Linked,
    );

    assert_eq!(enablement.directory().as_str(), "agents");
    assert_eq!(enablement.skill().source().as_str(), "elastic/agent-skills");
    assert_eq!(enablement.skill().path().as_str(), "release/checklist");
    assert_eq!(enablement.materialization(), MaterializationKind::Linked);
}

#[test]
fn noncanonical_and_escaping_values_are_rejected_with_canonical_suggestions() {
    let error = SourceKey::parse("Elastic/Agent-Skills").unwrap_err();
    assert_eq!(error.suggestion(), Some("elastic/agent-skills"));

    assert!(SourceKey::parse("single").is_err());
    assert!(SkillDirectoryKey::parse("Claude_Code").is_err());
    assert!(RepositoryRelativePath::parse("../outside").is_err());
    assert!(RepositoryRelativePath::parse("/absolute").is_err());
    assert!(RepositoryRelativePath::parse("a\\b").is_err());
}

#[test]
fn source_root_is_the_only_valid_dot_skill_path() {
    assert_eq!(SkillPath::parse(".").unwrap().as_str(), ".");
    assert!(RepositoryRelativePath::parse(".").is_err());
    assert!(SkillPath::parse("./child").is_err());
}
