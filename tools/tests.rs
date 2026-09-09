use super::*;

struct Repo(tempfile::TempDir);

impl Repo {
    fn new() -> Self {
        let repo = Self(tempfile::tempdir().unwrap());
        repo.git(&["init", "-q"]);
        repo.git(&["config", "user.name", "Gate test"]);
        repo.git(&["config", "user.email", "gate@example.invalid"]);
        repo.write("README.md", "fixture\n");
        repo.commit();
        repo
    }
    fn root(&self) -> &Path {
        self.0.path()
    }
    fn git(&self, args: &[&str]) -> String {
        git(self.root(), args).unwrap()
    }
    fn write(&self, path: &str, content: &str) {
        let path = self.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    fn commit(&self) -> String {
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "test: fixture"]);
        self.git(&["rev-parse", "HEAD"]).trim().to_owned()
    }
    fn active(&self, id: &str) {
        self.write(&format!("openspec/changes/{id}/tasks.md"), "- [ ] Work\n");
    }
    fn archived(&self, id: &str) {
        let prefix = format!("openspec/changes/archive/2026-09-06-{id}");
        for artifact in ["proposal.md", "design.md", "tasks.md"] {
            self.write(&format!("{prefix}/{artifact}"), "- [x] Complete\n");
        }
        self.write(
            &format!("{prefix}/specs/{id}/spec.md"),
            "## ADDED Requirements\n### Requirement: Result\nThe tool SHALL report the result.\n",
        );
        self.write(
            &format!("openspec/specs/{id}/spec.md"),
            "### Requirement: Result\nThe tool SHALL report the result.\n",
        );
    }
    fn review(&self, id: &str) {
        let review = make_review(
            self.root(),
            "HEAD",
            id,
            "test-reviewer",
            "Compared the requirement and scenarios with the main spec.",
            false,
            &[],
        )
        .unwrap();
        self.write(
            &format!("openspec/reviews/{id}.yaml"),
            &serde_saphyr::to_string(&review).unwrap(),
        );
        self.commit();
    }
}

const NONE: &str = "OpenSpec changes: none\nOpenSpec reason: Build tooling only.\n";

#[test]
fn conventional_titles_and_association_fields_are_checked() {
    for title in [
        "fix: preserve output",
        "ci(release): test archives",
        "feat(cli)!: change output",
    ] {
        title_check(title).unwrap();
    }
    for title in [
        "Add CLI workflows",
        "fix: ",
        "fix(): invalid",
        "spec: unsupported",
        "fix: line\nother",
    ] {
        assert!(title_check(title).is_err(), "{title}");
    }
    assert!(!valid_id("none"));
    let repo = Repo::new();
    for body in [
        "",
        "OpenSpec changes: ../escape",
        "OpenSpec changes: none, feature",
        "OpenSpec changes: feature, none",
        "OpenSpec changes: none",
        "OpenSpec changes: none\nOpenSpec changes: none",
        "OpenSpec changes: none\nOpenSpec reason: Replace this placeholder",
    ] {
        assert!(selected(repo.root(), "HEAD", "HEAD", body).is_err());
    }
}

#[test]
fn unrelated_active_changes_do_not_block_a_pr() {
    let repo = Repo::new();
    repo.active("unrelated");
    let base = repo.commit();
    repo.write("README.md", "changed\n");
    let head = repo.commit();
    assert!(
        selected(repo.root(), &base, &head, NONE)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn edited_deleted_and_renamed_active_changes_are_selected() {
    let repo = Repo::new();
    for id in ["edited", "deleted", "renamed"] {
        repo.active(id);
    }
    let base = repo.commit();
    repo.write("openspec/changes/edited/tasks.md", "- [x] Edited\n");
    fs::remove_dir_all(repo.root().join("openspec/changes/deleted")).unwrap();
    fs::rename(
        repo.root().join("openspec/changes/renamed"),
        repo.root().join("openspec/changes/new-name"),
    )
    .unwrap();
    let head = repo.commit();
    let selected = selected(repo.root(), &base, &head, NONE).unwrap();
    assert_eq!(
        selected,
        ["deleted", "edited", "new-name", "renamed"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    for id in selected {
        assert!(check_review(repo.root(), &head, &id).is_err());
    }
}

#[test]
fn new_archives_need_semantic_review_and_stale_reviews_fail() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.archived("feature");
    let head = repo.commit();
    assert!(
        selected(repo.root(), base.trim(), &head, NONE)
            .unwrap()
            .contains("feature")
    );
    assert!(check_review(repo.root(), &head, "feature").is_err());
    repo.review("feature");
    check_review(repo.root(), "HEAD", "feature").unwrap();
    repo.write(
        "openspec/specs/feature/spec.md",
        "Unsynchronized replacement\n",
    );
    repo.commit();
    assert!(check_review(repo.root(), "HEAD", "feature").is_err());
    repo.review("feature");
    repo.write(
        "openspec/changes/archive/2026-09-06-feature/tasks.md",
        "- [ ] Reopened\n",
    );
    repo.commit();
    assert!(check_review(repo.root(), "HEAD", "feature").is_err());
}

#[test]
fn already_synced_specs_can_pass_without_a_spec_diff() {
    let repo = Repo::new();
    repo.archived("synced");
    repo.commit();
    repo.review("synced");
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.write(
        "implementation.rs",
        "// already synchronized implementation\n",
    );
    let head = repo.commit();
    let ids = selected(repo.root(), base.trim(), &head, "OpenSpec changes: synced").unwrap();
    assert_eq!(ids, BTreeSet::from(["synced".to_owned()]));
    check_review(repo.root(), &head, "synced").unwrap();
}

#[test]
fn explicit_associations_include_work_without_artifact_diffs() {
    let repo = Repo::new();
    repo.archived("first");
    repo.active("second");
    repo.commit();
    repo.review("first");
    let ids = selected(
        repo.root(),
        "HEAD",
        "HEAD",
        "OpenSpec changes: first, second",
    )
    .unwrap();
    assert_eq!(ids.len(), 2);
    check_review(repo.root(), "HEAD", "first").unwrap();
    assert!(check_review(repo.root(), "HEAD", "second").is_err());
}

#[test]
fn removed_and_renamed_specs_are_bound_to_the_review() {
    let repo = Repo::new();
    repo.archived("old-name");
    fs::rename(
        repo.root().join("openspec/specs/old-name"),
        repo.root().join("openspec/specs/new-name"),
    )
    .unwrap();
    repo.commit();
    let review = make_review(
        repo.root(),
        "HEAD",
        "old-name",
        "reviewer",
        "Verified the removal and renamed destination.",
        false,
        &["openspec/specs/new-name/spec.md".to_owned()],
    )
    .unwrap();
    assert_eq!(review.specs["openspec/specs/old-name/spec.md"], "absent");
    repo.write(
        "openspec/reviews/old-name.yaml",
        &serde_saphyr::to_string(&review).unwrap(),
    );
    repo.commit();
    check_review(repo.root(), "HEAD", "old-name").unwrap();
    repo.write("openspec/specs/new-name/spec.md", "Changed after review\n");
    repo.commit();
    assert!(check_review(repo.root(), "HEAD", "old-name").is_err());
}

#[test]
fn no_delta_archives_require_an_explicit_explanation() {
    let repo = Repo::new();
    repo.archived("tooling");
    fs::remove_dir_all(
        repo.root()
            .join("openspec/changes/archive/2026-09-06-tooling/specs"),
    )
    .unwrap();
    repo.commit();
    assert!(
        make_review(
            repo.root(),
            "HEAD",
            "tooling",
            "reviewer",
            "Build configuration only.",
            false,
            &[]
        )
        .is_err()
    );
    let review = make_review(
        repo.root(),
        "HEAD",
        "tooling",
        "reviewer",
        "Build configuration only; no product requirements changed.",
        true,
        &[],
    )
    .unwrap();
    repo.write(
        "openspec/reviews/tooling.yaml",
        &serde_saphyr::to_string(&review).unwrap(),
    );
    repo.commit();
    check_review(repo.root(), "HEAD", "tooling").unwrap();
}

#[test]
fn changed_main_specs_require_an_association() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.write("openspec/specs/example/spec.md", "A changed requirement\n");
    let head = repo.commit();
    assert!(selected(repo.root(), base.trim(), &head, NONE).is_err());
}

#[test]
fn validation_uses_reviewed_config_and_schemas_without_unrelated_changes() {
    let repo = Repo::new();
    let config = "schema: custom\ncontext: Committed project settings\n";
    repo.write("openspec/config.yaml", config);
    repo.write("openspec/schemas/custom/schema.yaml", "name: custom\n");
    repo.archived("selected");
    repo.archived("unrelated");
    repo.active("active");
    let head = repo.commit();
    repo.write("openspec/config.yaml", "schema: spec-driven\n");
    let archive = "openspec/changes/archive/2026-09-06-selected".to_owned();
    let stage = stage_selected(repo.root(), &head, std::slice::from_ref(&archive)).unwrap();
    assert_eq!(
        fs::read_to_string(stage.path().join("openspec/config.yaml")).unwrap(),
        config
    );
    assert_eq!(
        fs::read_to_string(stage.path().join("openspec/schemas/custom/schema.yaml")).unwrap(),
        "name: custom\n"
    );
    assert!(stage.path().join(&archive).is_dir());
    assert!(
        stage
            .path()
            .join("openspec/specs/unrelated/spec.md")
            .is_file()
    );
    assert!(
        !stage
            .path()
            .join("openspec/changes/archive/2026-09-06-unrelated")
            .exists()
    );
    assert!(!stage.path().join("openspec/changes/active").exists());
}
