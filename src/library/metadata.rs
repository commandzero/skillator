//! Skill document parsing and metadata validation.

use super::*;

#[derive(Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
}

pub(crate) struct SkillMetadata {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) errors: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

pub(super) fn read_skill(directory: &Path, relative: &Path) -> LibrarySkill {
    let path = path_text(relative);
    let absolute_path = directory.canonicalize().ok();
    let fingerprint = absolute_path
        .as_deref()
        .map(skill_fingerprint)
        .unwrap_or(EntryFingerprint::Uninspectable);
    let bytes = fs::read(directory.join("SKILL.md"));
    let directory_name = (!relative.as_os_str().is_empty() && relative != Path::new("."))
        .then(|| directory.file_name().and_then(OsStr::to_str))
        .flatten();
    let metadata = match bytes {
        Ok(bytes) => inspect_skill_metadata(&bytes, directory_name),
        Err(error) => Err(format!("cannot read SKILL.md: {error}")),
    };
    match metadata {
        Ok(metadata) => {
            let diagnostics = metadata.errors;
            LibrarySkill {
                path,
                name: Some(metadata.name),
                description: Some(metadata.description),
                validity: if diagnostics.is_empty() {
                    SkillValidity::Valid
                } else {
                    SkillValidity::Invalid
                },
                available: true,
                diagnostics,
                warnings: metadata.warnings,
                absolute_path,
                fingerprint,
            }
        }
        Err(error) => LibrarySkill {
            path,
            name: None,
            description: None,
            validity: SkillValidity::Invalid,
            available: true,
            diagnostics: vec![error],
            warnings: Vec::new(),
            absolute_path,
            fingerprint,
        },
    }
}

fn parse_frontmatter(bytes: &[u8]) -> Result<SkillFrontmatter, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|error| format!("SKILL.md is not UTF-8: {error}"))?;
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return Err("SKILL.md must start with YAML metadata between --- lines".to_owned());
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err("SKILL.md metadata is missing its closing --- line".to_owned());
    }
    serde_saphyr::from_str(&yaml).map_err(|error| error.to_string())
}

pub(crate) fn inspect_skill_metadata(
    bytes: &[u8],
    directory_name: Option<&str>,
) -> Result<SkillMetadata, String> {
    let metadata = parse_frontmatter(bytes)?;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let safe_name = SkillName(&metadata.name).safe();
    if !safe_name {
        errors.push(
            "SKILL.md name must be a nonempty, safe directory name of at most 255 UTF-8 bytes"
                .to_owned(),
        );
    } else if !SkillName(&metadata.name).recommended() {
        warnings.push("SKILL.md name should be 1 to 64 lowercase letters, digits, or single hyphens, with no leading or trailing hyphen".to_owned());
    }
    if safe_name && SkillName(&metadata.name).differs_from_directory(directory_name) {
        warnings.push(format!(
            "SKILL.md name `{}` does not match directory",
            metadata.name
        ));
    }
    if metadata.description.trim().is_empty() {
        errors.push("SKILL.md description is empty".to_owned());
    }
    Ok(SkillMetadata {
        name: metadata.name,
        description: metadata.description,
        errors,
        warnings,
    })
}

pub(crate) fn validated_skill_metadata_at(directory: &Path) -> Option<(String, String)> {
    let bytes = fs::read(directory.join("SKILL.md")).ok()?;
    let metadata = parse_frontmatter(&bytes).ok()?;
    (SkillName(&metadata.name).safe() && !metadata.description.trim().is_empty())
        .then_some((metadata.name, metadata.description))
}

pub(crate) fn validated_skill_name_at(directory: &Path) -> Option<String> {
    validated_skill_metadata_at(directory).map(|(name, _)| name)
}

struct SkillName<'a>(&'a str);

impl SkillName<'_> {
    fn safe(&self) -> bool {
        self.0.len() <= 255
            && !self.0.trim().is_empty()
            && !matches!(self.0, "." | "..")
            && !self.has_unsafe_character()
    }

    fn recommended(&self) -> bool {
        (1..=64).contains(&self.0.len()) && self.valid_boundaries() && self.valid_characters()
    }
    fn valid_boundaries(&self) -> bool {
        !self.0.starts_with('-') && !self.0.ends_with('-') && !self.0.contains("--")
    }
    fn valid_characters(&self) -> bool {
        self.0
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    }
    fn differs_from_directory(&self, directory: Option<&str>) -> bool {
        directory.is_some_and(|directory| directory != self.0)
    }
    fn has_unsafe_character(&self) -> bool {
        self.0
            .chars()
            .any(|character| matches!(character, '/' | '\\') || character.is_control())
    }
}
