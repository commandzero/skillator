//! Validated values shared across module boundaries.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {kind} `{value}`{suggestion_text}")]
pub struct ValidationError {
    kind: &'static str,
    value: String,
    suggestion: Option<String>,
    suggestion_text: String,
}

impl ValidationError {
    fn new(kind: &'static str, value: &str, suggestion: Option<String>) -> Self {
        let suggestion_text = suggestion
            .as_ref()
            .map(|candidate| format!("; try `{candidate}`"))
            .unwrap_or_default();
        Self {
            kind,
            value: value.to_owned(),
            suggestion,
            suggestion_text,
        }
    }

    pub fn suggestion(&self) -> Option<&str> {
        self.suggestion.as_deref()
    }
}

fn canonical_segment(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

macro_rules! string_value {
    ($name:ident) => {
        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceKey(String);

impl SourceKey {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ValidationError> {
        let value = value.as_ref();
        let valid = value.split('/').count() >= 2 && value.split('/').all(canonical_segment);
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            let lower = value.to_ascii_lowercase().replace('_', "-");
            let suggestion = (lower != value
                && lower.split('/').count() >= 2
                && lower.split('/').all(canonical_segment))
            .then_some(lower);
            Err(ValidationError::new("Source Key", value, suggestion))
        }
    }
}

string_value!(SourceKey);

impl<'de> Deserialize<'de> for SourceKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SkillDirectoryKey(String);

impl SkillDirectoryKey {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ValidationError> {
        let value = value.as_ref();
        if canonical_segment(value) {
            Ok(Self(value.to_owned()))
        } else {
            let lower = value.to_ascii_lowercase().replace('_', "-");
            let suggestion = (lower != value && canonical_segment(&lower)).then_some(lower);
            Err(ValidationError::new(
                "Skill Directory Key",
                value,
                suggestion,
            ))
        }
    }
}

string_value!(SkillDirectoryKey);

impl<'de> Deserialize<'de> for SkillDirectoryKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepositoryRelativePath(String);

impl RepositoryRelativePath {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ValidationError> {
        parse_relative_path(value.as_ref(), false).map(Self)
    }
}

string_value!(RepositoryRelativePath);

impl<'de> Deserialize<'de> for RepositoryRelativePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SkillPath(String);

impl SkillPath {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ValidationError> {
        parse_relative_path(value.as_ref(), true).map(Self)
    }
}

string_value!(SkillPath);

impl<'de> Deserialize<'de> for SkillPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

fn parse_relative_path(value: &str, allow_root: bool) -> Result<String, ValidationError> {
    if allow_root && value == "." {
        return Ok(value.to_owned());
    }
    let valid = !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    valid
        .then(|| value.to_owned())
        .ok_or_else(|| ValidationError::new("repository-relative path", value, None))
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SkillKey {
    source: SourceKey,
    path: SkillPath,
}

impl SkillKey {
    pub fn new(source: SourceKey, path: SkillPath) -> Self {
        Self { source, path }
    }

    pub fn source(&self) -> &SourceKey {
        &self.source
    }

    pub fn path(&self) -> &SkillPath {
        &self.path
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaterializationKind {
    Linked,
    Copied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enablement {
    directory: SkillDirectoryKey,
    skill: SkillKey,
    materialization: MaterializationKind,
}

impl Enablement {
    pub fn new(
        directory: SkillDirectoryKey,
        skill: SkillKey,
        materialization: MaterializationKind,
    ) -> Self {
        Self {
            directory,
            skill,
            materialization,
        }
    }

    pub fn directory(&self) -> &SkillDirectoryKey {
        &self.directory
    }

    pub fn skill(&self) -> &SkillKey {
        &self.skill
    }

    pub fn materialization(&self) -> MaterializationKind {
        self.materialization
    }
}
