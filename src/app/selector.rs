//! Parsing and display of canonical library skill selectors.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSelector {
    pub(super) key: SkillKey,
}

impl SkillSelector {
    pub fn parse(value: &str) -> Result<Self, WorkflowError> {
        let Some((source, path)) = value.rsplit_once(':') else {
            return Err(WorkflowError::InvalidInput {
                message: "Skill selector must use <source-key>:<skill-path>".to_owned(),
            });
        };
        let source = SourceKey::parse(source).map_err(|error| WorkflowError::InvalidInput {
            message: error.to_string(),
        })?;
        let path = SkillPath::parse(path).map_err(|error| WorkflowError::InvalidInput {
            message: error.to_string(),
        })?;
        Ok(Self {
            key: SkillKey::new(source, path),
        })
    }

    pub fn key(&self) -> &SkillKey {
        &self.key
    }
}

impl std::fmt::Display for SkillSelector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.key.source(), self.key.path())
    }
}
