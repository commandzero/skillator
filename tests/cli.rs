mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

#[path = "cli_cases/library.rs"]
mod library;
#[path = "cli_cases/output.rs"]
mod output;
#[path = "cli_cases/registry.rs"]
mod registry;
#[path = "cli_cases/scope_mutations.rs"]
mod scope_mutations;
#[path = "cli_cases/sync.rs"]
mod sync;

struct Fixture {
    home: support::TestHome,
    target: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let home = support::TestHome::new();
        let library = home.path().join(".skillator/library/release-checklist");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(
            library.join("SKILL.md"),
            "---\nname: release-checklist\ndescription: Prepare a release\n---\n",
        )
        .unwrap();
        std::fs::write(
            home.library_config(),
            "version: 1\nlocations:\n  - path: \"./library\"\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: \"local/library\"\n        path: \".\"\n        skills:\n          - path: \"release-checklist\"\n",
        )
        .unwrap();
        let target = home.git_repo("target");
        std::fs::create_dir_all(target.join(".agents")).unwrap();
        std::fs::write(
            target.join(".agents/skillator.yaml"),
            "version: 1\nskill_directories:\n  - key: \"agents\"\n    path: \".agents/skills\"\nenablements:\n  - directory: \"agents\"\n    skill:\n      source: \"local/library\"\n      path: \"release-checklist\"\n    materialization: \"linked\"\n",
        )
        .unwrap();
        Self { home, target }
    }

    fn command(&self) -> Command {
        let mut command = Command::cargo_bin("skillator").unwrap();
        command
            .env("HOME", self.home.path())
            .current_dir(&self.target);
        command
    }
}
