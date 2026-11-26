//! Canonical registered repository paths and their configuration codec.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TargetRegistry {
    targets: Vec<PathBuf>,
}

impl TargetRegistry {
    pub fn new(mut targets: Vec<PathBuf>) -> Result<Self, Vec<ConfigIssue>> {
        let mut issues = Vec::new();
        if targets.iter().any(|path| !is_canonical_absolute(path)) {
            issues.push(ConfigIssue {
                path: "targets".to_owned(),
                message: "registered Target paths must be canonical and absolute".to_owned(),
            });
        }
        targets.sort();
        if targets.windows(2).any(|pair| pair[0] == pair[1]) {
            issues.push(ConfigIssue {
                path: "targets".to_owned(),
                message: "duplicate registered Target path".to_owned(),
            });
        }
        if issues.is_empty() {
            Ok(Self { targets })
        } else {
            Err(issues)
        }
    }

    pub fn targets(&self) -> &[PathBuf] {
        &self.targets
    }

    pub fn with_target(&self, target: PathBuf) -> Result<Self, Vec<ConfigIssue>> {
        let mut targets = self.targets.clone();
        if !targets.contains(&target) {
            targets.push(target);
        }
        Self::new(targets)
    }

    pub fn without_target(&self, target: &Path) -> Result<Self, Vec<ConfigIssue>> {
        Self::new(
            self.targets
                .iter()
                .filter(|candidate| candidate.as_path() != target)
                .cloned()
                .collect(),
        )
    }

    pub fn retaining(&self, keep: impl Fn(&Path) -> bool) -> Result<Self, Vec<ConfigIssue>> {
        Self::new(
            self.targets
                .iter()
                .filter(|target| keep(target))
                .cloned()
                .collect(),
        )
    }
}

fn is_canonical_absolute(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    let normalized: PathBuf = path.components().collect();
    normalized.as_os_str() == path.as_os_str()
        && path.components().all(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::Normal(_)
            )
        })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTargetRegistry {
    version: u64,
    targets: Vec<String>,
}

pub struct TargetRegistryCodec;

impl TargetRegistryCodec {
    pub fn parse(bytes: &[u8]) -> LoadResult<TargetRegistry> {
        parse_document(bytes, |text| {
            let raw: RawTargetRegistry = parse_yaml(text)?;
            if raw.version != VERSION {
                return Err(vec![ConfigIssue {
                    path: "version".to_owned(),
                    message: "only version 1 is supported".to_owned(),
                }]);
            }
            TargetRegistry::new(raw.targets.into_iter().map(PathBuf::from).collect())
        })
    }

    pub fn render(registry: &TargetRegistry) -> Result<String, RenderError> {
        let mut output = String::from("version: 1\ntargets:");
        if registry.targets.is_empty() {
            output.push_str(" []\n");
        } else {
            output.push('\n');
            for target in &registry.targets {
                output.push_str(&format!("  - {}\n", quote(&target.to_string_lossy())?));
            }
        }
        Ok(output)
    }
}

pub fn load_target_registry(path: &Path) -> Result<LoadResult<TargetRegistry>, LoadError> {
    match fs::read(path) {
        Ok(bytes) => Ok(TargetRegistryCodec::parse(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(LoadResult::Missing),
        Err(error) => Err(error.into()),
    }
}

pub fn save_target_registry(
    path: &Path,
    registry: &TargetRegistry,
    expected: &Fingerprint,
) -> Result<Fingerprint, SaveError> {
    conditional_save(
        path,
        TargetRegistryCodec::render(registry)?.as_bytes(),
        expected,
    )
}
