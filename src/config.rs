//! Strict Library and Repository configuration codecs.

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
use std::path::{Path, PathBuf};
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
    #[error("Skill name `{skill}` appears more than once in target `{target}")]
    DuplicateTargetSkill { target: String, skill: String },
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("configuration changed since it was loaded")]
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

pub struct RepositoryConfigCodec;

impl RepositoryConfigCodec {
    pub fn parse(bytes: &[u8]) -> LoadResult<RepositoryConfig> {
        parse_document(bytes, |text| {
            let value: Value = parse_yaml(text)?;
            if value
                .as_object()
                .is_some_and(|root| root.contains_key("skill_directories"))
            {
                let issues = validate_repository_shape(&value);
                if !issues.is_empty() {
                    return Err(issues);
                }
                return convert_repository(parse_yaml(text)?);
            }
            convert_target_map(parse_yaml(text)?)
        })
    }

    pub fn render(config: &RepositoryConfig) -> Result<String, RenderError> {
        let mut directories = config.skill_directories.clone();
        directories.sort_by(|left, right| left.key.cmp(&right.key));
        let mut enablements = config.enablements.clone();
        enablements.sort_by(|left, right| {
            (left.directory(), left.skill().source(), left.skill().path()).cmp(&(
                right.directory(),
                right.skill().source(),
                right.skill().path(),
            ))
        });

        let mut output = String::from("version: 1\n");
        for directory in directories {
            output.push_str(&format!("{}:\n", directory.key.as_str()));
            output.push_str(&format!("  path: {}\n", quote(directory.path.as_str())?));
            if let Some(label) = directory.label {
                output.push_str(&format!("  label: {}\n", quote(&label)?));
            }
            let entries = enablements
                .iter()
                .filter(|enablement| enablement.directory() == &directory.key)
                .collect::<Vec<_>>();
            if entries.is_empty() {
                output.push_str("  skills: {}\n");
            } else {
                output.push_str("  skills:\n");
                let mut names = BTreeSet::new();
                for enablement in entries {
                    let name = enablement
                        .skill()
                        .path()
                        .as_str()
                        .rsplit('/')
                        .next()
                        .expect("Skill path has a final segment");
                    if !names.insert(name) {
                        return Err(RenderError::DuplicateTargetSkill {
                            target: directory.key.as_str().to_owned(),
                            skill: name.to_owned(),
                        });
                    }
                    output.push_str(&format!("    {}:\n", quote(name)?));
                    output.push_str(&format!(
                        "      source: {}\n",
                        quote(enablement.skill().source().as_str())?
                    ));
                    if enablement.skill().path().as_str() != name {
                        output.push_str(&format!(
                            "      path: {}\n",
                            quote(enablement.skill().path().as_str())?
                        ));
                    }
                    if enablement.materialization() == MaterializationKind::Copied {
                        output.push_str("      type: \"copied\"\n");
                    }
                }
            }
        }
        Ok(output)
    }
}

pub struct LibraryConfigCodec;

impl LibraryConfigCodec {
    pub fn parse(bytes: &[u8]) -> LoadResult<LibraryConfig> {
        parse_document(bytes, |text| {
            let value: Value = parse_yaml(text)?;
            let issues = validate_library_shape(&value);
            if !issues.is_empty() {
                return Err(issues);
            }
            let raw: RawLibrary = parse_yaml(text)?;
            convert_library(raw)
        })
    }

    pub fn render(config: &LibraryConfig) -> Result<String, RenderError> {
        let mut output = String::from("version: 1\nlocations:");
        if config.locations.is_empty() {
            output.push_str(" []\n");
        } else {
            output.push('\n');
            for location in &config.locations {
                output.push_str(&format!("  - path: {}\n", quote(&location.path)?));
                if location.exclusions.is_empty() {
                    output.push_str("    exclusions: []\n");
                } else {
                    output.push_str("    exclusions:\n");
                    for exclusion in &location.exclusions {
                        output.push_str(&format!("      - {}\n", quote(exclusion)?));
                    }
                }
                output.push_str(&format!("    allow_overlap: {}\n", location.allow_overlap));
            }
        }
        if !config.hidden_skills.is_empty() {
            output.push_str("hidden_skills:\n");
            for skill in &config.hidden_skills {
                output.push_str(&format!(
                    "  - source: {}\n",
                    quote(skill.source().as_str())?
                ));
                output.push_str(&format!("    path: {}\n", quote(skill.path().as_str())?));
            }
        }
        Ok(output)
    }
}

fn quote(value: &str) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

fn parse_document<T>(
    bytes: &[u8],
    parser: impl FnOnce(&str) -> Result<T, Vec<ConfigIssue>>,
) -> LoadResult<T> {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            return LoadResult::Invalid {
                issues: vec![ConfigIssue {
                    path: "$".to_owned(),
                    message: format!("configuration is not UTF-8: {error}"),
                }],
            };
        }
    };
    if let Some(version) = top_level_version(text)
        && version != VERSION
    {
        return LoadResult::Unsupported {
            version,
            bytes: bytes.to_vec(),
        };
    }
    match parser(text) {
        Ok(value) => LoadResult::Valid(Loaded {
            value,
            fingerprint: Fingerprint::for_bytes(bytes),
        }),
        Err(issues) => LoadResult::Invalid { issues },
    }
}

fn top_level_version(text: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let line = line.trim_end();
        if line.starts_with(char::is_whitespace) || !line.starts_with("version:") {
            return None;
        }
        line["version:".len()..]
            .split('#')
            .next()
            .and_then(|value| value.trim().parse().ok())
    })
}

fn parse_yaml<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, Vec<ConfigIssue>> {
    if let Some(message) = forbidden_yaml_feature(text) {
        return Err(vec![ConfigIssue {
            path: "$".to_owned(),
            message,
        }]);
    }
    let options = serde_saphyr::options! {
        budget: serde_saphyr::budget! { max_documents: 1 },
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        merge_keys: serde_saphyr::MergeKeyPolicy::Error,
        strict_booleans: true,
        emit_comments: false,
    };
    serde_saphyr::from_str_with_options(text, options).map_err(|error| {
        vec![ConfigIssue {
            path: "$".to_owned(),
            message: error.to_string(),
        }]
    })
}

fn forbidden_yaml_feature(text: &str) -> Option<String> {
    let document_starts = text.lines().filter(|line| line.trim() == "---").count();
    if document_starts > 1
        || (document_starts == 1 && text.lines().position(|line| line.trim() == "---") != Some(0))
    {
        return Some("multiple YAML documents are not allowed".to_owned());
    }
    for line in text.lines() {
        let content = line.split('#').next().unwrap_or_default();
        let trimmed = content.trim_start();
        if trimmed.starts_with("<<:") {
            return Some("YAML merge keys are not allowed".to_owned());
        }
        if contains_yaml_indicator(content, '&') || contains_yaml_indicator(content, '*') {
            return Some("YAML anchors and aliases are not allowed".to_owned());
        }
        if contains_yaml_indicator(content, '!') {
            return Some("YAML tags are not allowed".to_owned());
        }
    }
    None
}

fn contains_yaml_indicator(line: &str, sought: char) -> bool {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if double && character == '\\' && !escaped {
            escaped = true;
            continue;
        }
        if character == '\'' && !double {
            single = !single;
        } else if character == '"' && !single && !escaped {
            double = !double;
        } else if character == sought && !single && !double {
            let prior = line[..index].chars().next_back();
            if prior.is_none_or(|value| value.is_whitespace() || value == ':') {
                return true;
            }
        }
        escaped = false;
    }
    false
}

fn validate_repository_shape(value: &Value) -> Vec<ConfigIssue> {
    let mut issues = Vec::new();
    let Some(root) = expect_object(value, "$", &mut issues) else {
        return issues;
    };
    validate_keys(
        root,
        "$",
        &["version", "skill_directories", "enablements"],
        &["version", "skill_directories", "enablements"],
        &mut issues,
    );
    expect_unsigned(root.get("version"), "version", &mut issues);
    if let Some(directories) = expect_array(
        root.get("skill_directories"),
        "skill_directories",
        &mut issues,
    ) {
        for (index, value) in directories.iter().enumerate() {
            let path = format!("skill_directories[{index}]");
            let Some(object) = expect_object(value, &path, &mut issues) else {
                continue;
            };
            validate_keys(
                object,
                &path,
                &["key", "path", "label"],
                &["key", "path"],
                &mut issues,
            );
            expect_string(object.get("key"), &format!("{path}.key"), &mut issues);
            expect_string(object.get("path"), &format!("{path}.path"), &mut issues);
            expect_optional_string(object.get("label"), &format!("{path}.label"), &mut issues);
        }
    }
    if let Some(enablements) = expect_array(root.get("enablements"), "enablements", &mut issues) {
        for (index, value) in enablements.iter().enumerate() {
            let path = format!("enablements[{index}]");
            let Some(object) = expect_object(value, &path, &mut issues) else {
                continue;
            };
            validate_keys(
                object,
                &path,
                &["directory", "skill", "materialization"],
                &["directory", "skill", "materialization"],
                &mut issues,
            );
            expect_string(
                object.get("directory"),
                &format!("{path}.directory"),
                &mut issues,
            );
            expect_string(
                object.get("materialization"),
                &format!("{path}.materialization"),
                &mut issues,
            );
            let skill_path = format!("{path}.skill");
            if let Some(skill) = expect_object_value(object.get("skill"), &skill_path, &mut issues)
            {
                validate_keys(
                    skill,
                    &skill_path,
                    &["source", "path"],
                    &["source", "path"],
                    &mut issues,
                );
                expect_string(
                    skill.get("source"),
                    &format!("{skill_path}.source"),
                    &mut issues,
                );
                expect_string(
                    skill.get("path"),
                    &format!("{skill_path}.path"),
                    &mut issues,
                );
            }
        }
    }
    issues
}

fn validate_library_shape(value: &Value) -> Vec<ConfigIssue> {
    let mut issues = Vec::new();
    let Some(root) = expect_object(value, "$", &mut issues) else {
        return issues;
    };
    validate_keys(
        root,
        "$",
        &["version", "locations", "hidden_skills"],
        &["version", "locations"],
        &mut issues,
    );
    expect_unsigned(root.get("version"), "version", &mut issues);
    if let Some(hidden) =
        expect_optional_array(root.get("hidden_skills"), "hidden_skills", &mut issues)
    {
        for (index, skill) in hidden.iter().enumerate() {
            let path = format!("hidden_skills[{index}]");
            if let Some(skill) = expect_object(skill, &path, &mut issues) {
                validate_keys(
                    skill,
                    &path,
                    &["source", "path"],
                    &["source", "path"],
                    &mut issues,
                );
                expect_string(skill.get("source"), &format!("{path}.source"), &mut issues);
                expect_string(skill.get("path"), &format!("{path}.path"), &mut issues);
            }
        }
    }
    if let Some(locations) = expect_array(root.get("locations"), "locations", &mut issues) {
        for (location_index, value) in locations.iter().enumerate() {
            let path = format!("locations[{location_index}]");
            let Some(location) = expect_object(value, &path, &mut issues) else {
                continue;
            };
            validate_keys(
                location,
                &path,
                &["path", "exclusions", "allow_overlap", "sources"],
                &["path"],
                &mut issues,
            );
            expect_string(location.get("path"), &format!("{path}.path"), &mut issues);
            expect_optional_string_array(
                location.get("exclusions"),
                &format!("{path}.exclusions"),
                &mut issues,
            );
            expect_optional_bool(
                location.get("allow_overlap"),
                &format!("{path}.allow_overlap"),
                &mut issues,
            );
            let Some(sources) = expect_optional_array(
                location.get("sources"),
                &format!("{path}.sources"),
                &mut issues,
            ) else {
                continue;
            };
            for (source_index, value) in sources.iter().enumerate() {
                let source_path = format!("{path}.sources[{source_index}]");
                let Some(source) = expect_object(value, &source_path, &mut issues) else {
                    continue;
                };
                validate_keys(
                    source,
                    &source_path,
                    &["key", "path", "skills"],
                    &["key", "path"],
                    &mut issues,
                );
                expect_string(
                    source.get("key"),
                    &format!("{source_path}.key"),
                    &mut issues,
                );
                expect_string(
                    source.get("path"),
                    &format!("{source_path}.path"),
                    &mut issues,
                );
                let Some(skills) = expect_optional_array(
                    source.get("skills"),
                    &format!("{source_path}.skills"),
                    &mut issues,
                ) else {
                    continue;
                };
                for (skill_index, value) in skills.iter().enumerate() {
                    let skill_path = format!("{source_path}.skills[{skill_index}]");
                    let Some(skill) = expect_object(value, &skill_path, &mut issues) else {
                        continue;
                    };
                    validate_keys(skill, &skill_path, &["path"], &["path"], &mut issues);
                    expect_string(
                        skill.get("path"),
                        &format!("{skill_path}.path"),
                        &mut issues,
                    );
                }
            }
        }
    }
    issues
}

fn validate_keys(
    object: &serde_json::Map<String, Value>,
    path: &str,
    allowed: &[&str],
    required: &[&str],
    issues: &mut Vec<ConfigIssue>,
) {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            issues.push(ConfigIssue {
                path: field_path(path, key),
                message: "unknown field".to_owned(),
            });
        }
    }
    for key in required {
        if !object.contains_key(*key) {
            issues.push(ConfigIssue {
                path: field_path(path, key),
                message: "missing required field".to_owned(),
            });
        }
    }
}

fn field_path(parent: &str, field: &str) -> String {
    if parent == "$" {
        field.to_owned()
    } else {
        format!("{parent}.{field}")
    }
}

fn expect_object<'a>(
    value: &'a Value,
    path: &str,
    issues: &mut Vec<ConfigIssue>,
) -> Option<&'a serde_json::Map<String, Value>> {
    match value.as_object() {
        Some(object) => Some(object),
        None => {
            issues.push(type_issue(path, "mapping"));
            None
        }
    }
}

fn expect_object_value<'a>(
    value: Option<&'a Value>,
    path: &str,
    issues: &mut Vec<ConfigIssue>,
) -> Option<&'a serde_json::Map<String, Value>> {
    value.and_then(|value| expect_object(value, path, issues))
}

fn expect_array<'a>(
    value: Option<&'a Value>,
    path: &str,
    issues: &mut Vec<ConfigIssue>,
) -> Option<&'a [Value]> {
    match value.and_then(Value::as_array) {
        Some(array) => Some(array),
        None if value.is_none() => None,
        None => {
            issues.push(type_issue(path, "sequence"));
            None
        }
    }
}

fn expect_optional_array<'a>(
    value: Option<&'a Value>,
    path: &str,
    issues: &mut Vec<ConfigIssue>,
) -> Option<&'a [Value]> {
    match value {
        None => Some(&[]),
        Some(value) => expect_array(Some(value), path, issues),
    }
}

fn expect_string(value: Option<&Value>, path: &str, issues: &mut Vec<ConfigIssue>) {
    if value.is_some_and(|value| !value.is_string()) {
        issues.push(type_issue(path, "string"));
    }
}

fn expect_optional_string(value: Option<&Value>, path: &str, issues: &mut Vec<ConfigIssue>) {
    if value.is_some_and(|value| !value.is_null() && !value.is_string()) {
        issues.push(type_issue(path, "string or null"));
    }
}

fn expect_unsigned(value: Option<&Value>, path: &str, issues: &mut Vec<ConfigIssue>) {
    if value.is_some_and(|value| value.as_u64().is_none()) {
        issues.push(type_issue(path, "non-negative integer"));
    }
}

fn expect_optional_bool(value: Option<&Value>, path: &str, issues: &mut Vec<ConfigIssue>) {
    if value.is_some_and(|value| !value.is_boolean()) {
        issues.push(type_issue(path, "boolean"));
    }
}

fn expect_optional_string_array(value: Option<&Value>, path: &str, issues: &mut Vec<ConfigIssue>) {
    let Some(values) = expect_optional_array(value, path, issues) else {
        return;
    };
    for (index, value) in values.iter().enumerate() {
        if !value.is_string() {
            issues.push(type_issue(&format!("{path}[{index}]"), "string"));
        }
    }
}

fn type_issue(path: &str, expected: &str) -> ConfigIssue {
    ConfigIssue {
        path: path.to_owned(),
        message: format!("expected {expected}"),
    }
}

fn convert_repository(raw: RawRepository) -> Result<RepositoryConfig, Vec<ConfigIssue>> {
    if raw.version != VERSION {
        return Err(vec![ConfigIssue {
            path: "version".to_owned(),
            message: "only version 1 is supported".to_owned(),
        }]);
    }
    let mut issues = Vec::new();
    let mut directories = Vec::new();
    for (index, raw) in raw.skill_directories.into_iter().enumerate() {
        let key = SkillDirectoryKey::parse(&raw.key).map_err(|error| {
            issue(
                &mut issues,
                format!("skill_directories[{index}].key"),
                error,
            )
        });
        let path = RepositoryRelativePath::parse(&raw.path).map_err(|error| {
            issue(
                &mut issues,
                format!("skill_directories[{index}].path"),
                error,
            )
        });
        if let (Ok(key), Ok(path)) = (key, path) {
            directories.push(SkillDirectoryConfig::new(key, path, raw.label));
        }
    }
    let mut enablements = Vec::new();
    for (index, raw) in raw.enablements.into_iter().enumerate() {
        let directory = SkillDirectoryKey::parse(&raw.directory).map_err(|error| {
            issue(
                &mut issues,
                format!("enablements[{index}].directory"),
                error,
            )
        });
        let source = SourceKey::parse(&raw.skill.source).map_err(|error| {
            issue(
                &mut issues,
                format!("enablements[{index}].skill.source"),
                error,
            )
        });
        let path = SkillPath::parse(&raw.skill.path).map_err(|error| {
            issue(
                &mut issues,
                format!("enablements[{index}].skill.path"),
                error,
            )
        });
        let materialization = match raw.materialization.as_str() {
            "linked" => Ok(MaterializationKind::Linked),
            "copied" => Ok(MaterializationKind::Copied),
            _ => Err(issue(
                &mut issues,
                format!("enablements[{index}].materialization"),
                "expected `linked` or `copied`",
            )),
        };
        if let (Ok(directory), Ok(source), Ok(path), Ok(materialization)) =
            (directory, source, path, materialization)
        {
            enablements.push(Enablement::new(
                directory,
                SkillKey::new(source, path),
                materialization,
            ));
        }
    }
    let candidate = RepositoryConfig {
        skill_directories: directories,
        enablements,
    };
    issues.extend(validate_repository(&candidate));
    if issues.is_empty() {
        Ok(candidate)
    } else {
        Err(issues)
    }
}

fn convert_target_map(raw: RawTargetMap) -> Result<RepositoryConfig, Vec<ConfigIssue>> {
    if raw.version != VERSION {
        return Err(vec![ConfigIssue {
            path: "version".to_owned(),
            message: "only version 1 is supported".to_owned(),
        }]);
    }
    let mut directories = Vec::new();
    let mut enablements = Vec::new();
    let mut issues = Vec::new();
    for (key_text, target) in raw.targets {
        let key = match SkillDirectoryKey::parse(&key_text) {
            Ok(key) => key,
            Err(error) => {
                issue(&mut issues, key_text, error);
                continue;
            }
        };
        let path = match RepositoryRelativePath::parse(&target.path) {
            Ok(path) => path,
            Err(error) => {
                issue(&mut issues, format!("{}.path", key.as_str()), error);
                continue;
            }
        };
        for (name, skill) in target.skills {
            let source = SourceKey::parse(&skill.source).map_err(|error| {
                issue(
                    &mut issues,
                    format!("{}.skills.{name}.source", key.as_str()),
                    error,
                )
            });
            let path_text = skill.path.as_deref().unwrap_or(&name);
            let skill_path = SkillPath::parse(path_text).map_err(|error| {
                issue(
                    &mut issues,
                    format!("{}.skills.{name}.path", key.as_str()),
                    error,
                )
            });
            let materialization = match skill.materialization.as_deref().unwrap_or("linked") {
                "linked" => Ok(MaterializationKind::Linked),
                "copied" => Ok(MaterializationKind::Copied),
                _ => Err(issue(
                    &mut issues,
                    format!("{}.skills.{name}.type", key.as_str()),
                    "expected `linked` or `copied`",
                )),
            };
            if let (Ok(source), Ok(skill_path), Ok(materialization)) =
                (source, skill_path, materialization)
            {
                enablements.push(Enablement::new(
                    key.clone(),
                    SkillKey::new(source, skill_path),
                    materialization,
                ));
            }
        }
        directories.push(SkillDirectoryConfig::new(key, path, target.label));
    }
    let candidate = RepositoryConfig {
        skill_directories: directories,
        enablements,
    };
    issues.extend(validate_repository(&candidate));
    if issues.is_empty() {
        Ok(candidate)
    } else {
        Err(issues)
    }
}

fn issue(
    issues: &mut Vec<ConfigIssue>,
    path: String,
    error: impl std::fmt::Display,
) -> ConfigIssue {
    let issue = ConfigIssue {
        path,
        message: error.to_string(),
    };
    issues.push(issue.clone());
    issue
}

fn validate_repository(config: &RepositoryConfig) -> Vec<ConfigIssue> {
    let mut issues = Vec::new();
    let mut keys = BTreeSet::new();
    for directory in &config.skill_directories {
        if !keys.insert(directory.key.as_str()) {
            issues.push(ConfigIssue {
                path: "skill_directories".to_owned(),
                message: format!("duplicate Skill Directory Key `{}`", directory.key),
            });
        }
        let path = directory.path.as_str();
        if path == ".git"
            || path.starts_with(".git/")
            || path == ".agents/skillator.yaml"
            || ".agents/skillator.yaml".starts_with(&format!("{path}/"))
            || path.starts_with(".agents/skillator.yaml/")
        {
            issues.push(ConfigIssue {
                path: format!("skill_directories.{}", directory.key),
                message: "Skill Directory overlaps a protected path".to_owned(),
            });
        }
    }
    for (index, left) in config.skill_directories.iter().enumerate() {
        for right in config.skill_directories.iter().skip(index + 1) {
            if paths_overlap(left.path.as_str(), right.path.as_str()) {
                issues.push(ConfigIssue {
                    path: "skill_directories".to_owned(),
                    message: format!("`{}` overlaps `{}`", left.path, right.path),
                });
            }
        }
    }
    let mut relationships = BTreeSet::new();
    for enablement in &config.enablements {
        if !keys.contains(enablement.directory().as_str()) {
            issues.push(ConfigIssue {
                path: "enablements.directory".to_owned(),
                message: format!("unknown Skill Directory `{}`", enablement.directory()),
            });
        }
        let identity = (
            enablement.directory().as_str(),
            enablement.skill().source().as_str(),
            enablement.skill().path().as_str(),
        );
        if !relationships.insert(identity) {
            issues.push(ConfigIssue {
                path: "enablements".to_owned(),
                message: "duplicate Enablement".to_owned(),
            });
        }
    }
    issues
}

fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

fn convert_library(raw: RawLibrary) -> Result<LibraryConfig, Vec<ConfigIssue>> {
    let RawLibrary {
        version,
        locations: raw_locations,
        hidden_skills: raw_hidden_skills,
    } = raw;
    if version != VERSION {
        return Err(vec![ConfigIssue {
            path: "version".to_owned(),
            message: "only version 1 is supported".to_owned(),
        }]);
    }
    let mut issues = Vec::new();
    let mut locations = Vec::new();
    for (location_index, raw) in raw_locations.into_iter().enumerate() {
        if raw.path.trim().is_empty() {
            issues.push(ConfigIssue {
                path: format!("locations[{location_index}].path"),
                message: "Location path cannot be empty".to_owned(),
            });
        }
        // `sources` is accepted as legacy input so development builds can open
        // previously written configuration.  Source and Skill inventory is now
        // discovered from the Location at runtime and is deliberately not
        // retained in the configuration model.
        let _legacy_sources = raw.sources;
        locations.push(LibraryLocationConfig::new(
            raw.path,
            raw.exclusions,
            raw.allow_overlap,
        ));
    }
    let hidden_skills = raw_hidden_skills
        .into_iter()
        .enumerate()
        .filter_map(|(index, raw)| {
            match (SourceKey::parse(&raw.source), SkillPath::parse(&raw.path)) {
                (Ok(source), Ok(path)) => Some(SkillKey::new(source, path)),
                (source, path) => {
                    if let Err(error) = source {
                        issue(&mut issues, format!("hidden_skills[{index}].source"), error);
                    }
                    if let Err(error) = path {
                        issue(&mut issues, format!("hidden_skills[{index}].path"), error);
                    }
                    None
                }
            }
        })
        .collect();
    let candidate = LibraryConfig {
        locations,
        hidden_skills,
    };
    issues.extend(validate_library(&candidate));
    if issues.is_empty() {
        Ok(candidate)
    } else {
        Err(issues)
    }
}

fn validate_library(config: &LibraryConfig) -> Vec<ConfigIssue> {
    let _ = config;
    Vec::new()
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

fn conditional_save(
    path: &Path,
    bytes: &[u8],
    expected: &Fingerprint,
) -> Result<Fingerprint, SaveError> {
    conditional_save_with(path, bytes, expected, || {})
}

fn conditional_save_with(
    path: &Path,
    bytes: &[u8],
    expected: &Fingerprint,
    before_publish: impl FnOnce(),
) -> Result<Fingerprint, SaveError> {
    let current = match fs::read(path) {
        Ok(bytes) => Fingerprint::for_bytes(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Fingerprint::Absent,
        Err(error) => return Err(error.into()),
    };
    if &current != expected {
        return Err(SaveError::Stale);
    }
    let desired = Fingerprint::for_bytes(bytes);
    if current == desired {
        return Ok(desired);
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = stage_path(parent);
    let mut stage_contains_prior = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        let latest = match fs::read(path) {
            Ok(bytes) => Fingerprint::for_bytes(&bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Fingerprint::Absent,
            Err(error) => return Err(SaveError::Io(error)),
        };
        if &latest != expected {
            return Err(SaveError::Stale);
        }
        before_publish();
        if *expected == Fingerprint::Absent {
            rename_noreplace(&stage, path).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    SaveError::Stale
                } else {
                    SaveError::Io(error)
                }
            })?;
            sync_directory(parent)?;
            return Ok(desired);
        }

        rename_exchange(&stage, path)?;
        stage_contains_prior = true;
        sync_directory(parent)?;
        let moved = fs::read(&stage)
            .map(|bytes| Fingerprint::for_bytes(&bytes))
            .map_err(SaveError::Io)?;
        if &moved != expected {
            return if rename_exchange(&stage, path).is_ok() {
                stage_contains_prior = false;
                let _ = sync_directory(parent);
                Err(SaveError::Stale)
            } else {
                Err(SaveError::Io(std::io::Error::other(format!(
                    "configuration changed during save and atomic rollback failed; preserve and recover {}",
                    stage.display()
                ))))
            };
        }
        if let Err(error) = fs::remove_file(&stage) {
            return Err(SaveError::Io(std::io::Error::other(format!(
                "configuration was saved but prior content remains at {}: {error}",
                stage.display()
            ))));
        }
        stage_contains_prior = false;
        sync_directory(parent)?;
        Ok(desired)
    })();
    if result.is_err() && !stage_contains_prior {
        let _ = fs::remove_file(&stage);
    }
    result
}

fn stage_path(parent: &Path) -> PathBuf {
    let sequence = STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".skillator-config-stage-{}-{sequence}",
        std::process::id()
    ))
}

fn sync_directory(path: &Path) -> Result<(), std::io::Error> {
    OpenOptions::new().read(true).open(path)?.sync_all()
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
}
