//! Configuration YAML encoding and version-specific decoding.

use super::*;

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
            render_directory_skills(directory.key.as_str(), entries, &mut output)?;
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
                render_library_location(location, &mut output)?;
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

fn render_directory_skills(
    directory_key: &str,
    entries: Vec<&Enablement>,
    output: &mut String,
) -> Result<(), RenderError> {
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
                    target: directory_key.to_owned(),
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
    Ok(())
}

fn render_library_location(
    location: &LibraryLocationConfig,
    output: &mut String,
) -> Result<(), RenderError> {
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
    Ok(())
}
