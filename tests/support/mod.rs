#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct TestHome {
    root: tempfile::TempDir,
}

pub fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git fixture command");
    assert!(output.status.success(), "git {args:?} failed: {output:?}");
}

pub fn git_output(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git fixture command");
    assert!(output.status.success(), "git {args:?} failed: {output:?}");
    String::from_utf8(output.stdout).expect("Git output is UTF-8")
}

impl TestHome {
    pub fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("create isolated test home"),
        }
    }

    pub fn path(&self) -> &Path {
        self.root.path()
    }

    pub fn library_config(&self) -> PathBuf {
        self.path().join(".skillator/library.yaml")
    }

    pub fn git_repo(&self, name: &str) -> PathBuf {
        let path = self.path().join(name);
        std::fs::create_dir_all(&path).expect("create test repository");
        git_init(&path);
        path
    }
}

pub fn git_init(path: &Path) {
    let output = Command::new("git")
        .args(["init", "--quiet"])
        .arg(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git init");
    assert!(output.status.success(), "git init failed: {output:?}");
}
