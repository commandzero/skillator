//! Strict Library and Repository configuration codecs.

mod codecs;
pub use codecs::{LibraryConfigCodec, RepositoryConfigCodec};
mod registry;
pub use registry::{
    TargetRegistry, TargetRegistryCodec, load_target_registry, save_target_registry,
};
mod persistence;
use persistence::*;
mod conversion;
use conversion::*;
mod schema;
use schema::*;
mod syntax;
pub(crate) use syntax::parse_yaml;
use syntax::*;

use crate::domain::{
    Enablement, MaterializationKind, RepositoryRelativePath, SkillDirectoryKey, SkillKey,
    SkillPath, SourceKey,
};
use crate::fs_safety::{rename_exchange, rename_noreplace};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const VERSION: u64 = 1;
static STAGE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fingerprint {
    Absent,
    Sha256([u8; 32]),
}

impl Fingerprint {
    pub(crate) fn for_bytes(bytes: &[u8]) -> Self {
        Self::Sha256(Sha256::digest(bytes).into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded<T> {
    value: T,
    fingerprint: Fingerprint,
}

impl<T> Loaded<T> {
    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigIssue {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadResult<T> {
    Missing,
    Valid(Loaded<T>),
    Unsupported { version: u64, bytes: Vec<u8> },
    Invalid { issues: Vec<ConfigIssue> },
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cannot read configuration: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("cannot encode YAML string: {0}")]
    String(#[from] serde_json::Error),
    #[error("Skill name `{skill}` appears more than once in skill folder `{target}`")]
    DuplicateTargetSkill { target: String, skill: String },
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("Configuration changed since it was loaded; reopen it and try again")]
    Stale,
    #[error("cannot save configuration: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Render(#[from] RenderError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDirectoryConfig {
    key: SkillDirectoryKey,
    path: RepositoryRelativePath,
    label: Option<String>,
}

impl SkillDirectoryConfig {
    pub fn new(
        key: SkillDirectoryKey,
        path: RepositoryRelativePath,
        label: Option<String>,
    ) -> Self {
        Self { key, path, label }
    }

    pub fn key(&self) -> &SkillDirectoryKey {
        &self.key
    }

    pub fn path(&self) -> &RepositoryRelativePath {
        &self.path
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn agents_preset() -> Self {
        Self::new(
            SkillDirectoryKey::parse("agents").expect("built-in key is valid"),
            RepositoryRelativePath::parse(".agents/skills").expect("built-in path is valid"),
            Some(".agents".to_owned()),
        )
    }

    pub fn user_preset() -> Self {
        Self::new(
            SkillDirectoryKey::parse("agents").expect("built-in key is valid"),
            RepositoryRelativePath::parse(".agents/skills").expect("built-in path is valid"),
            Some("User".to_owned()),
        )
    }

    pub fn claude_preset() -> Self {
        Self::new(
            SkillDirectoryKey::parse("claude").expect("built-in key is valid"),
            RepositoryRelativePath::parse(".claude/skills").expect("built-in path is valid"),
            Some("Claude Code".to_owned()),
        )
    }

    pub fn github_preset() -> Self {
        Self::new(
            SkillDirectoryKey::parse("github").expect("built-in key is valid"),
            RepositoryRelativePath::parse(".github/skills").expect("built-in path is valid"),
            Some("GitHub Copilot".to_owned()),
        )
    }

    pub fn cursor_preset() -> Self {
        Self::new(
            SkillDirectoryKey::parse("cursor").expect("built-in key is valid"),
            RepositoryRelativePath::parse(".cursor/skills").expect("built-in path is valid"),
            Some("Cursor".to_owned()),
        )
    }

    pub fn gemini_preset() -> Self {
        Self::new(
            SkillDirectoryKey::parse("gemini").expect("built-in key is valid"),
            RepositoryRelativePath::parse(".gemini/skills").expect("built-in path is valid"),
            Some("Gemini CLI".to_owned()),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryConfig {
    skill_directories: Vec<SkillDirectoryConfig>,
    enablements: Vec<Enablement>,
}

impl RepositoryConfig {
    pub fn new(
        skill_directories: Vec<SkillDirectoryConfig>,
        enablements: Vec<Enablement>,
    ) -> Result<Self, Vec<ConfigIssue>> {
        let value = Self {
            skill_directories,
            enablements,
        };
        let issues = validate_repository(&value);
        if issues.is_empty() {
            Ok(value)
        } else {
            Err(issues)
        }
    }

    pub fn empty() -> Self {
        Self {
            skill_directories: Vec::new(),
            enablements: Vec::new(),
        }
    }

    pub fn first_run() -> Self {
        Self {
            skill_directories: vec![SkillDirectoryConfig::agents_preset()],
            enablements: Vec::new(),
        }
    }

    pub fn user_first_run() -> Self {
        Self {
            skill_directories: vec![SkillDirectoryConfig::user_preset()],
            enablements: Vec::new(),
        }
    }

    pub fn skill_directories(&self) -> &[SkillDirectoryConfig] {
        &self.skill_directories
    }

    pub fn enablements(&self) -> &[Enablement] {
        &self.enablements
    }

    pub fn with_enablements(&self, enablements: Vec<Enablement>) -> Result<Self, Vec<ConfigIssue>> {
        Self::new(self.skill_directories.clone(), enablements)
    }

    pub fn with_skill_directories(
        &self,
        skill_directories: Vec<SkillDirectoryConfig>,
    ) -> Result<Self, Vec<ConfigIssue>> {
        Self::new(skill_directories, self.enablements.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryLocationConfig {
    path: String,
    exclusions: Vec<String>,
    allow_overlap: bool,
}

impl LibraryLocationConfig {
    pub fn new(path: String, exclusions: Vec<String>, allow_overlap: bool) -> Self {
        Self {
            path,
            exclusions,
            allow_overlap,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn exclusions(&self) -> &[String] {
        &self.exclusions
    }

    pub fn allow_overlap(&self) -> bool {
        self.allow_overlap
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryConfig {
    locations: Vec<LibraryLocationConfig>,
    hidden_skills: BTreeSet<SkillKey>,
}

impl LibraryConfig {
    pub fn new(locations: Vec<LibraryLocationConfig>) -> Result<Self, Vec<ConfigIssue>> {
        let value = Self {
            locations,
            hidden_skills: BTreeSet::new(),
        };
        let issues = validate_library(&value);
        if issues.is_empty() {
            Ok(value)
        } else {
            Err(issues)
        }
    }

    pub fn empty() -> Self {
        Self {
            locations: Vec::new(),
            hidden_skills: BTreeSet::new(),
        }
    }

    pub fn first_run() -> Self {
        Self {
            locations: vec![LibraryLocationConfig::new(
                "./library".to_owned(),
                Vec::new(),
                false,
            )],
            hidden_skills: BTreeSet::new(),
        }
    }

    pub fn locations(&self) -> &[LibraryLocationConfig] {
        &self.locations
    }

    pub fn is_visible(&self, skill: &SkillKey) -> bool {
        !self.hidden_skills.contains(skill)
    }

    pub fn hidden_skills(&self) -> &BTreeSet<SkillKey> {
        &self.hidden_skills
    }

    pub fn with_hidden_skills(
        &self,
        hidden_skills: BTreeSet<SkillKey>,
    ) -> Result<Self, Vec<ConfigIssue>> {
        let value = Self {
            locations: self.locations.clone(),
            hidden_skills,
        };
        let issues = validate_library(&value);
        if issues.is_empty() {
            Ok(value)
        } else {
            Err(issues)
        }
    }

    pub fn with_locations(
        &self,
        locations: Vec<LibraryLocationConfig>,
    ) -> Result<Self, Vec<ConfigIssue>> {
        Self::new(locations)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRepository {
    version: u64,
    skill_directories: Vec<RawSkillDirectory>,
    enablements: Vec<RawEnablement>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSkillDirectory {
    key: String,
    path: String,
    label: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnablement {
    directory: String,
    skill: RawSkill,
    materialization: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSkill {
    source: String,
    path: String,
}

#[derive(Deserialize)]
struct RawTargetMap {
    version: u64,
    #[serde(flatten)]
    targets: BTreeMap<String, RawTargetDirectory>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTargetDirectory {
    path: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    skills: BTreeMap<String, RawTargetSkill>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTargetSkill {
    source: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default, rename = "type")]
    materialization: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLibrary {
    version: u64,
    locations: Vec<RawLocation>,
    #[serde(default)]
    hidden_skills: Vec<RawSkill>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLocation {
    path: String,
    #[serde(default)]
    exclusions: Vec<String>,
    #[serde(default)]
    allow_overlap: bool,
    #[serde(default)]
    sources: Vec<Value>,
}

fn quote(value: &str) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

pub fn load_repository(path: &Path) -> Result<LoadResult<RepositoryConfig>, LoadError> {
    match fs::read(path) {
        Ok(bytes) => Ok(RepositoryConfigCodec::parse(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(LoadResult::Missing),
        Err(error) => Err(error.into()),
    }
}

pub fn load_library(path: &Path) -> Result<LoadResult<LibraryConfig>, LoadError> {
    match fs::read(path) {
        Ok(bytes) => Ok(LibraryConfigCodec::parse(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(LoadResult::Missing),
        Err(error) => Err(error.into()),
    }
}

pub fn save_repository(
    path: &Path,
    config: &RepositoryConfig,
    expected: &Fingerprint,
) -> Result<Fingerprint, SaveError> {
    conditional_save(
        path,
        RepositoryConfigCodec::render(config)?.as_bytes(),
        expected,
    )
}

/// Conditionally publish opaque configuration-adjacent bytes with the same
/// sibling staging and stale-write protection as the YAML codecs.
pub(crate) fn save_bytes(
    path: &Path,
    bytes: &[u8],
    expected: &Fingerprint,
) -> Result<Fingerprint, SaveError> {
    conditional_save(path, bytes, expected)
}

pub fn save_library(
    path: &Path,
    config: &LibraryConfig,
    expected: &Fingerprint,
) -> Result<Fingerprint, SaveError> {
    conditional_save(
        path,
        LibraryConfigCodec::render(config)?.as_bytes(),
        expected,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn structural_validation_collects_independent_repository_issues() {
        let parsed = RepositoryConfigCodec::parse(
            br#"version: "one"
extra: true
skill_directories:
  - key: 4
    surprise: true
enablements:
  - directory: agents
    skill:
      source: demo/source
    materialization: false
"#,
        );
        let LoadResult::Invalid { issues } = parsed else {
            panic!("expected invalid configuration");
        };
        let paths = issues
            .iter()
            .map(|issue| issue.path.as_str())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"extra"));
        assert!(paths.contains(&"version"));
        assert!(paths.contains(&"skill_directories[0].key"));
        assert!(paths.contains(&"skill_directories[0].path"));
        assert!(paths.contains(&"skill_directories[0].surprise"));
        assert!(paths.contains(&"enablements[0].skill.path"));
        assert!(paths.contains(&"enablements[0].materialization"));
    }

    #[test]
    fn absent_save_never_overwrites_a_concurrently_created_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("skillator.yaml");
        let result = conditional_save_with(&path, b"desired", &Fingerprint::Absent, || {
            fs::write(&path, b"external").unwrap();
        });
        std::assert_matches!(result, Err(SaveError::Stale));
        assert_eq!(fs::read(&path).unwrap(), b"external");
    }

    #[test]
    fn existing_save_restores_a_concurrent_change() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("skillator.yaml");
        fs::write(&path, b"original").unwrap();
        let expected = Fingerprint::for_bytes(b"original");
        let result = conditional_save_with(&path, b"desired", &expected, || {
            fs::write(&path, b"external").unwrap();
        });
        std::assert_matches!(result, Err(SaveError::Stale));
        assert_eq!(fs::read(&path).unwrap(), b"external");
    }

    #[test]
    fn target_registry_is_strict_sorted_and_duplicate_free() {
        let registry = TargetRegistry::new(vec![
            PathBuf::from("/work/zeta"),
            PathBuf::from("/work/alpha"),
        ])
        .unwrap();
        let rendered = TargetRegistryCodec::render(&registry).unwrap();
        assert!(rendered.find("/work/alpha").unwrap() < rendered.find("/work/zeta").unwrap());
        assert!(matches!(
            TargetRegistryCodec::parse(b"version: 1\ntargets: [/work/a, /work/a]\n"),
            LoadResult::Invalid { .. }
        ));
        assert!(matches!(
            TargetRegistryCodec::parse(b"version: 1\ntargets: []\nextra: true\n"),
            LoadResult::Invalid { .. }
        ));
        assert!(TargetRegistry::new(vec![PathBuf::from("relative")]).is_err());
        assert!(matches!(
            TargetRegistryCodec::parse(b"version: 1\ntargets: [/work/../a]\n"),
            LoadResult::Invalid { .. }
        ));
        assert!(matches!(
            TargetRegistryCodec::parse(b"version: 1\ntargets: [/work/./a]\n"),
            LoadResult::Invalid { .. }
        ));
    }
}
