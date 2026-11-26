//! Validated configuration conversion and domain invariants.

use super::*;

pub(super) fn convert_repository(raw: RawRepository) -> Result<RepositoryConfig, Vec<ConfigIssue>> {
    if raw.version != VERSION {
        return Err(vec![ConfigIssue {
            path: "version".to_owned(),
            message: "only version 1 is supported".to_owned(),
        }]);
    }
    let mut issues = Vec::new();
    let directories = raw
        .skill_directories
        .into_iter()
        .enumerate()
        .filter_map(|(index, raw)| convert_directory(index, raw, &mut issues))
        .collect();
    let enablements = raw
        .enablements
        .into_iter()
        .enumerate()
        .filter_map(|(index, raw)| convert_enablement(index, raw, &mut issues))
        .collect();
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

pub(super) fn convert_target_map(raw: RawTargetMap) -> Result<RepositoryConfig, Vec<ConfigIssue>> {
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
        enablements.extend(convert_target_skills(&key, target.skills, &mut issues));
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

pub(super) fn issue(
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

pub(super) fn validate_repository(config: &RepositoryConfig) -> Vec<ConfigIssue> {
    let mut issues = Vec::new();
    let mut keys = BTreeSet::new();
    for directory in &config.skill_directories {
        if !keys.insert(directory.key.as_str()) {
            issues.push(ConfigIssue {
                path: "skill_directories".to_owned(),
                message: format!("duplicate skill folder key `{}`", directory.key),
            });
        }
        validate_directory_location(directory, &mut issues);
    }
    validate_directory_overlaps(config, &mut issues);
    validate_enablement_relationships(config, &keys, &mut issues);
    issues
}

pub(super) fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

pub(super) fn convert_library(raw: RawLibrary) -> Result<LibraryConfig, Vec<ConfigIssue>> {
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
                message: "Folder path cannot be empty".to_owned(),
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
        .filter_map(|(index, raw)| convert_hidden_skill(index, raw, &mut issues))
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

pub(super) fn validate_library(config: &LibraryConfig) -> Vec<ConfigIssue> {
    let _ = config;
    Vec::new()
}

fn convert_directory(
    index: usize,
    raw: RawSkillDirectory,
    issues: &mut Vec<ConfigIssue>,
) -> Option<SkillDirectoryConfig> {
    let key = SkillDirectoryKey::parse(&raw.key)
        .map_err(|error| issue(issues, format!("skill_directories[{index}].key"), error));
    let path = RepositoryRelativePath::parse(&raw.path)
        .map_err(|error| issue(issues, format!("skill_directories[{index}].path"), error));
    if let (Ok(key), Ok(path)) = (key, path) {
        return Some(SkillDirectoryConfig::new(key, path, raw.label));
    }
    None
}

fn convert_enablement(
    index: usize,
    raw: RawEnablement,
    issues: &mut Vec<ConfigIssue>,
) -> Option<Enablement> {
    let directory = SkillDirectoryKey::parse(&raw.directory)
        .map_err(|error| issue(issues, format!("enablements[{index}].directory"), error));
    let source = SourceKey::parse(&raw.skill.source)
        .map_err(|error| issue(issues, format!("enablements[{index}].skill.source"), error));
    let path = SkillPath::parse(&raw.skill.path)
        .map_err(|error| issue(issues, format!("enablements[{index}].skill.path"), error));
    let materialization = match raw.materialization.as_str() {
        "linked" => Ok(MaterializationKind::Linked),
        "copied" => Ok(MaterializationKind::Copied),
        _ => Err(issue(
            issues,
            format!("enablements[{index}].materialization"),
            "expected `linked` or `copied`",
        )),
    };
    if let (Ok(directory), Ok(source), Ok(path), Ok(materialization)) =
        (directory, source, path, materialization)
    {
        return Some(Enablement::new(
            directory,
            SkillKey::new(source, path),
            materialization,
        ));
    }
    None
}

fn convert_target_skill(
    key: &SkillDirectoryKey,
    name: String,
    skill: RawTargetSkill,
    issues: &mut Vec<ConfigIssue>,
) -> Option<Enablement> {
    let source = SourceKey::parse(&skill.source).map_err(|error| {
        issue(
            issues,
            format!("{}.skills.{name}.source", key.as_str()),
            error,
        )
    });
    let path_text = skill.path.as_deref().unwrap_or(&name);
    let skill_path = SkillPath::parse(path_text).map_err(|error| {
        issue(
            issues,
            format!("{}.skills.{name}.path", key.as_str()),
            error,
        )
    });
    let materialization = match skill.materialization.as_deref().unwrap_or("linked") {
        "linked" => Ok(MaterializationKind::Linked),
        "copied" => Ok(MaterializationKind::Copied),
        _ => Err(issue(
            issues,
            format!("{}.skills.{name}.type", key.as_str()),
            "expected `linked` or `copied`",
        )),
    };
    if let (Ok(source), Ok(skill_path), Ok(materialization)) = (source, skill_path, materialization)
    {
        return Some(Enablement::new(
            key.clone(),
            SkillKey::new(source, skill_path),
            materialization,
        ));
    }
    None
}

fn validate_directory_location(directory: &SkillDirectoryConfig, issues: &mut Vec<ConfigIssue>) {
    let path = directory.path.as_str();
    if !path.contains('/') {
        issues.push(ConfigIssue {
                path: format!("skill_directories.{}", directory.key),
                message:
                    "Use a nested skill folder such as .agents/skills so Skillator does not replace the repository root .gitignore"
                        .to_owned(),
            });
    }
    if overlaps_protected_path(path) {
        issues.push(ConfigIssue {
            path: format!("skill_directories.{}", directory.key),
            message: "skill folder overlaps a protected path".to_owned(),
        });
    }
}

fn validate_directory_overlaps(config: &RepositoryConfig, issues: &mut Vec<ConfigIssue>) {
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
}

fn validate_enablement_relationships(
    config: &RepositoryConfig,
    keys: &BTreeSet<&str>,
    issues: &mut Vec<ConfigIssue>,
) {
    let mut relationships = BTreeSet::new();
    for enablement in &config.enablements {
        if !keys.contains(enablement.directory().as_str()) {
            issues.push(ConfigIssue {
                path: "enablements.directory".to_owned(),
                message: format!("unknown skill folder `{}`", enablement.directory()),
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
                message: "skill is enabled more than once in the same folder".to_owned(),
            });
        }
    }
}

fn overlaps_protected_path(path: &str) -> bool {
    path == ".git"
        || path.starts_with(".git/")
        || path == ".agents/skillator.yaml"
        || ".agents/skillator.yaml".starts_with(&format!("{path}/"))
        || path.starts_with(".agents/skillator.yaml/")
}

fn convert_hidden_skill(
    index: usize,
    raw: RawSkill,
    issues: &mut Vec<ConfigIssue>,
) -> Option<SkillKey> {
    match (SourceKey::parse(&raw.source), SkillPath::parse(&raw.path)) {
        (Ok(source), Ok(path)) => Some(SkillKey::new(source, path)),
        (source, path) => {
            if let Err(error) = source {
                issue(issues, format!("hidden_skills[{index}].source"), error);
            }
            if let Err(error) = path {
                issue(issues, format!("hidden_skills[{index}].path"), error);
            }
            None
        }
    }
}

fn convert_target_skills(
    key: &SkillDirectoryKey,
    skills: BTreeMap<String, RawTargetSkill>,
    issues: &mut Vec<ConfigIssue>,
) -> Vec<Enablement> {
    let mut enablements = Vec::new();
    for (name, skill) in skills {
        if let Some(enablement) = convert_target_skill(key, name, skill, issues) {
            enablements.push(enablement);
        }
    }
    enablements
}
